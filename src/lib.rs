#![feature(proc_macro_hygiene, decl_macro)]
#![recursion_limit="128"]

#[macro_use]
extern crate handlebars;
#[macro_use]
extern crate if_chain;
extern crate rand;
#[macro_use]
extern crate rocket;
extern crate rocket_contrib;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate uuid;

use std::collections::HashMap;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::fs::{self, File};
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;

use rand::Rng;
use rocket::http::{Cookie, Cookies, Status, RawStr};
use rocket::request::{FromParam, LenientForm};
use rocket::response::Redirect;
use rocket::Rocket;
use rocket::State;
use rocket_contrib::templates::Template;
use rocket_contrib::json::Json;
use uuid::Uuid;

use self::models::{IndexTemplateArgs, IndexTemplateScore, RollForm, ScorecardTemplateArgs,
                   ScorecardTemplateScore, ErrorTemplateArgs};

mod models;

pub fn launch_server() {
    Rocket::ignite()
        .mount("/", routes![index, roll, mark, scorecard, scorecard_json])
        .register(catchers![e404, e500])
        .attach(Template::custom(|engines| {
            engines.handlebars.register_helper("inc", Box::new(inc));
            engines.handlebars.register_helper("eq", Box::new(eq));
        }))
        .manage(Mutex::new(GamesInProgress::default()))
        .launch();
}

handlebars_helper!(inc: |x: u64| x + 1);
handlebars_helper!(eq: |x: u64, y: u64| x == y);

#[derive(Default)]
struct GamesInProgress {
    games: HashMap<Uuid, GameInProgress>,
}

#[derive(Default)]
struct GameInProgress {
    fields: HashMap<CardField, u16>,
    rolls: u8,
    dice: [Die; 5],
}

#[derive(Copy, Clone, Default, Serialize)]
pub struct Die {
    pub value: u16,
    pub held: bool,
}

#[catch(404)]
fn e404() -> Template {
    Template::render("error", ErrorTemplateArgs { errorcode: 404 })
}

#[catch(500)]
fn e500() -> Template {
    Template::render("error", ErrorTemplateArgs { errorcode: 500 })
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
enum CardField {
    Aces, Twos, Threes, Fours, Fives, Sixes, ThreeOfAKind, FourOfAKind, FullHouse, SmallStraight,
    LargeStraight, Yahtzee, Chance,
}

impl CardField {
    fn values() -> [CardField; 13] {
        [CardField::Aces, CardField::Twos, CardField::Threes, CardField::Fours, CardField::Fives,
            CardField::Sixes, CardField::ThreeOfAKind, CardField::FourOfAKind, CardField::FullHouse,
            CardField::SmallStraight, CardField::LargeStraight, CardField::Yahtzee,
            CardField::Chance]
    }
}

impl Display for CardField {
    fn fmt(&self, formatter: &mut Formatter) -> FmtResult {
        match self {
            CardField::Aces => write!(formatter, "Aces"),
            CardField::Twos => write!(formatter, "Twos"),
            CardField::Threes => write!(formatter, "Threes"),
            CardField::Fours => write!(formatter, "Fours"),
            CardField::Fives => write!(formatter, "Fives"),
            CardField::Sixes => write!(formatter, "Sixes"),
            CardField::ThreeOfAKind => write!(formatter, "Three of a kind"),
            CardField::FourOfAKind => write!(formatter, "Four of a kind"),
            CardField::FullHouse => write!(formatter, "Full house"),
            CardField::SmallStraight => write!(formatter, "Small straight"),
            CardField::LargeStraight => write!(formatter, "Large straight"),
            CardField::Yahtzee => write!(formatter, "Yahtzee"),
            CardField::Chance => write!(formatter, "Chance"),
        }
    }
}

#[get("/")]
fn index(mut cookies: Cookies, games: State<Mutex<GamesInProgress>>) -> Result<Template, Status> {
    let games = &mut *games.lock().map_err(|_| Status::InternalServerError)?;
    if_chain! {
        if let Some(cookie) = cookies.get("id");
        if let Ok(id) = Uuid::from_str(cookie.value());
        if let Some(game) = games.games.get(&id);
        then {
            let values = &CardField::values();
            let mut vec = Vec::with_capacity(values.len());
            let mut total = 0_u16;
            for card in values.iter() {
                let score = game.fields.get(card).cloned();
                let potential;
                if let Some(x) = score {
                    total += x as u16;
                    potential = 0;
                } else {
                    let yahtzee_field = game.fields.get(&CardField::Yahtzee);
                    potential = calculate_score(*card, game.dice,
                            yahtzee_field.map(|x| *x != 0).unwrap_or(true));
                }
                vec.push(IndexTemplateScore {
                    kind: format!("{}", card),
                    value: score,
                    markable: score.is_none() && game.rolls > 0,
                    potential,
                })
            }
            Ok(Template::render("index", IndexTemplateArgs {
                scores: vec,
                total,
                dice: game.dice,
                rolls_remaining: 3 - game.rolls,
            }))
        } else {
            let values = CardField::values().iter().map(|x| IndexTemplateScore {
                kind: format!("{}", x),
                value: None,
                markable: false,
                potential: 0,
            }).collect();
            let uuid = Uuid::new_v4();
            let game = GameInProgress::default();
            games.games.insert(uuid, game);
            let cookie = Cookie::new("id", uuid.to_string());
            cookies.add(cookie);
            let mut args = IndexTemplateArgs::default();
            args.scores = values;
            args.rolls_remaining = 3;
            Ok(Template::render("index", args))
        }
    }
}

#[post("/roll", data = "<form>")]
fn roll(cookies: Cookies, games: State<Mutex<GamesInProgress>>, form: LenientForm<RollForm>)
        -> Result<Redirect, Status>
{
    let games = &mut *games.lock().map_err(|_| Status::InternalServerError)?;
    if_chain! {
        if let Some(cookie) = cookies.get("id");
        if let Ok(id) = Uuid::from_str(cookie.value());
        if let Some(game) = games.games.get_mut(&id);
        if game.rolls < 3;
        then {
            game.rolls += 1;
            if game.rolls == 1 {
                game.dice = [roll_die(), roll_die(), roll_die(), roll_die(), roll_die()];
            } else {
                if !form.die1 {
                    game.dice[0] = roll_die();
                } else {
                    game.dice[0].held = true;
                }
                if !form.die2 {
                    game.dice[1] = roll_die();
                } else {
                    game.dice[1].held = true;
                }
                if !form.die3 {
                    game.dice[2] = roll_die();
                } else {
                    game.dice[2].held = true;
                }
                if !form.die4 {
                    game.dice[3] = roll_die();
                } else {
                    game.dice[3].held = true;
                }
                if !form.die5 {
                    game.dice[4] = roll_die();
                } else {
                    game.dice[4].held = true;
                }
            }
        }
    }
    Ok(Redirect::to("/"))
}

fn roll_die() -> Die {
    Die { value: rand::thread_rng().gen_range(1, 7), held: false }
}

#[post("/mark/<index>")]
fn mark(cookies: Cookies, games: State<Mutex<GamesInProgress>>, index: usize)
        -> Result<Redirect, Status>
{
    let games = &mut *games.lock().map_err(|_| Status::InternalServerError)?;
    if_chain! {
        if let Some(cookie) = cookies.get("id");
        if let Ok(id) = Uuid::from_str(cookie.value());
        if let Some(game) = games.games.get_mut(&id);
        if game.rolls > 0;
        if let Some(field) = CardField::values().get(index);
        if let None = game.fields.get(&field);
        then {
            let yahtzee_field = game.fields.get(&CardField::Yahtzee);
            let score = calculate_score(*field, game.dice,
                    yahtzee_field.map(|x| *x != 0).unwrap_or(true));
            game.fields.insert(*field, score);
            if game.fields.keys().len() == 13 {
                let mut vec = Vec::with_capacity(13);
                let mut total = 0;
                for field in CardField::values().iter() {
                    let score = game.fields[field];
                    total += score;
                    vec.push(ScorecardTemplateScore {
                        kind: format!("{}", field),
                        value: score,
                    });
                }
                fs::create_dir("scorecards").ok();
                let file = File::create("scorecards/".to_string() + &id.to_string() + ".json")
                        .map_err(|_| Status::InternalServerError)?;
                serde_json::to_writer(file, &ScorecardTemplateArgs {
                    scores: vec,
                    total,
                }).map_err(|_| Status::InternalServerError)?;
                games.games.remove(&id);
                return Ok(Redirect::to("/scorecard/".to_string() + &id.to_string()));
            } else {
                game.rolls = 0;
            }
        }
    }
    Ok(Redirect::to("/"))
}

#[get("/scorecard/<uuid>")]
fn scorecard(uuid: UuidReq) -> Result<Template, Status> {
    let file = File::open("scorecards/".to_string() + &uuid.to_string() + ".json")
        .map_err(|_| Status::NotFound)?;
    let params: ScorecardTemplateArgs = serde_json::from_reader(file)
        .map_err(|_| Status::InternalServerError)?;
    Ok(Template::render("scorecard", params))
}

#[get("/api/<uuid>")]
fn scorecard_json(uuid: UuidReq) -> Result<Json<ScorecardTemplateArgs>, Status> {
    let file = File::open("scorecards/".to_string() + &uuid.to_string() + ".json")
        .map_err(|_| Status::NotFound)?;
    let params = serde_json::from_reader(file)
        .map_err(|_| Status::InternalServerError)?;
    Ok(Json(params))
}

fn calculate_score(field: CardField, dice: [Die; 5], can_yahtzee: bool) -> u16 {
    let dice = [dice[0].value, dice[1].value, dice[2].value, dice[3].value, dice[4].value];
    match field {
        CardField::Aces => dice.iter().filter(|x| **x == 1).sum(),
        CardField::Twos => dice.iter().filter(|x| **x == 2).sum(),
        CardField::Threes => dice.iter().filter(|x| **x == 3).sum(),
        CardField::Fours => dice.iter().filter(|x| **x == 4).sum(),
        CardField::Fives => dice.iter().filter(|x| **x == 5).sum(),
        CardField::Sixes => dice.iter().filter(|x| **x == 6).sum(),
        CardField::ThreeOfAKind => {
            if can_yahtzee && dice[1..].iter().all(|x| *x == dice[0]) {
                100
            } else if dice[1..].iter().filter(|x| **x == dice[0]).count() >= 2 ||
                dice[2..].iter().filter(|x| **x == dice[1]).count() >= 2 ||
                dice[3..].iter().filter(|x| **x == dice[2]).count() == 2
            {
                dice.iter().sum()
            } else {
                0
            }
        },
        CardField::FourOfAKind => {
            if can_yahtzee && dice[1..].iter().all(|x| *x == dice[0]) {
                100
            } else if dice[1..].iter().filter(|x| **x == dice[0]).count() >= 3 ||
                dice[2..].iter().filter(|x| **x == dice[1]).count() == 3 {
                dice.iter().sum()
            } else {
                0
            }
        },
        CardField::FullHouse => {
            if can_yahtzee && dice[1..].iter().all(|x| *x == dice[0]) {
                100
            } else {
                let a = dice[0];
                let b = dice.iter().find(|x| **x != a);
                match b {
                    None => 0,
                    Some(b) => {
                        let ax = dice.iter().filter(|x| **x == a).count();
                        let bx = dice.iter().filter(|x| *x == b).count();
                        if (ax == 3 && bx == 2) || (ax == 2 && bx == 3) {
                            25
                        } else {
                            0
                        }
                    }
                }
            }
        },
        CardField::SmallStraight => {
            if can_yahtzee && dice[1..].iter().all(|x| *x == dice[0]) {
                100
            } else if (dice.contains(&1) && dice.contains(&2) &&
                dice.contains(&3) && dice.contains(&4)) ||

                (dice.contains(&2) && dice.contains(&3) &&
                    dice.contains(&4) && dice.contains(&5)) ||

                (dice.contains(&3) && dice.contains(&4) &&
                    dice.contains(&5) && dice.contains(&6))
            {
                30
            } else {
                0
            }
        },
        CardField::LargeStraight => {
            if can_yahtzee && dice[1..].iter().all(|x| *x == dice[0]) {
                100
            } else if dice.contains(&2) && dice.contains(&3) && dice.contains(&4) &&
                dice.contains(&5) && (dice.contains(&1) || dice.contains(&6))
            {
                40
            } else {
                0
            }
        },
        CardField::Yahtzee => {
            if dice[1..].iter().all(|x| *x == dice[0]) {
                50
            } else {
                0
            }
        },
        CardField::Chance => {
            if can_yahtzee && dice[1..].iter().all(|x| *x == dice[0]) {
                100
            } else {
                dice.iter().sum()
            }
        }
    }
}

struct UuidReq(Uuid);

impl<'a> FromParam<'a> for UuidReq {
    type Error = Status;
    fn from_param(param: &'a RawStr) -> Result<Self, Self::Error> {
        Ok(UuidReq(Uuid::parse_str(&param.percent_decode().map_err(|_| Status::BadRequest)?)
            .map_err(|_| Status::NotFound)?))
    }
}

impl Deref for UuidReq {
    type Target = Uuid;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[get("/static/<rest..>")]
fn static_content(rest: PathBuf) -> Result<File, ()> {
    Ok(File::open(rest).map_err(|_| ())?)
}
