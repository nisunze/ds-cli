//! Closed Profile control/query inputs without a retained runtime or solver.
//! History stays a typed generic: Native uses its owner revision type; transport
//! uses the same Network history shape with opaque revision strings.
use crate::profile_display::DisplayCase;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalculationTransfer {
    Full {},
    ExactJson {},
    Delta { base_signature: String },
}
impl Default for CalculationTransfer {
    fn default() -> Self {
        Self::Full {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalculationArguments {
    Start {
        job_id: String,
        #[serde(default)]
        display_case: Option<DisplayCase>,
        #[serde(default)]
        analyze: bool,
        #[serde(default)]
        scene_transfer: CalculationTransfer,
    },
    PrepareSceneBase {},
    Status {
        job_id: String,
    },
    Cancel {
        job_id: String,
    },
    Admit {
        job_id: String,
    },
}

impl CalculationArguments {
    pub fn validate(&self) -> Result<(), String> {
        if let Self::PrepareSceneBase {} = self {
            return Ok(());
        }
        if let Self::Start {
            scene_transfer: CalculationTransfer::Delta { base_signature },
            ..
        } = self
        {
            if base_signature.len() != 71
                || !base_signature.starts_with("sha256:")
                || !base_signature[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("invalid_profile_view: invalid calculated scene base signature".into());
            }
        }
        let job_id = match self {
            Self::Start { job_id, .. }
            | Self::Status { job_id }
            | Self::Cancel { job_id }
            | Self::Admit { job_id } => job_id,
            Self::PrepareSceneBase {} => unreachable!(),
        };
        validate_calculation_identity(job_id)
    }
}

pub fn validate_calculation_identity(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        return Err("invalid_profile_view: invalid calculation identity".into());
    }
    Ok(())
}

pub use ds_profile_policy::profile_queries::{
    QueryArguments, QueryCursor, QueryKind, QuerySource, DEFAULT_QUERY_LIMIT,
};
