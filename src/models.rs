#[derive(Serialize, Default)]
pub struct IndexTemplateArgs {
    pub scores: Vec<IndexTemplateScore>,
    pub total: u16,
    pub dice: [super::Die; 5],
    pub rolls_remaining: u8,
}

#[derive(Serialize)]
pub struct IndexTemplateScore {
    pub kind: String,
    pub value: Option<u16>,
    pub markable: bool,
    pub potential: u16,
}

#[derive(Deserialize, Default, Copy, Clone, FromForm)]
#[serde(default)]
pub struct RollForm {
    pub die1: bool,
    pub die2: bool,
    pub die3: bool,
    pub die4: bool,
    pub die5: bool,
}

#[derive(Serialize, Deserialize)]
pub struct ScorecardTemplateArgs {
    pub scores: Vec<ScorecardTemplateScore>,
    pub total: u16,
}

#[derive(Serialize, Deserialize)]
pub struct ScorecardTemplateScore {
    pub kind: String,
    pub value: u16,
}

#[derive(Serialize)]
pub struct ErrorTemplateArgs {
    pub errorcode: u16,
}
