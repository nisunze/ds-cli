//! One page-role vocabulary shared by all governed printing consumers.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PageRole {
    Cover,
    Naming,
    SheetIndex,
    KeyMap,
    KeyPlan,
    Legend,
    GeneralNotes,
    LegendNotes,
    Revisions,
    Network,
    Schedule,
    PlanProfile,
    Context,
}
