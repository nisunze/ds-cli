//! Page composition for the native voltage-drop report set. Engineering verdicts
//! are retained producer facts; this policy never calculates or changes them.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceScope {
    #[default]
    Transformer,
    SetFrontMatter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub include_reinforcement: bool,
    pub references_scope: &'static str,
}

pub fn evaluate(verdict: &str, scope: ReferenceScope) -> Result<Plan, String> {
    let include_reinforcement = match verdict {
        "complies" => false,
        "complies_with_reinforcement" | "does_not_comply" => true,
        _ => {
            return Err(
                "voltage_drop_page_plan_invalid: a retained calculated verdict is required".into(),
            );
        }
    };
    Ok(Plan {
        include_reinforcement,
        references_scope: match scope {
            ReferenceScope::Transformer => "transformer",
            ReferenceScope::SetFrontMatter => "set_front_matter",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn passing_network_keeps_no_reinforcement_page() {
        assert_eq!(
            evaluate("complies", ReferenceScope::SetFrontMatter).unwrap(),
            Plan {
                include_reinforcement: false,
                references_scope: "set_front_matter"
            }
        );
    }
    #[test]
    fn required_and_unsolved_networks_keep_their_informative_plan_pages() {
        assert!(
            evaluate(
                "complies_with_reinforcement",
                ReferenceScope::SetFrontMatter
            )
            .unwrap()
            .include_reinforcement
        );
        assert!(
            evaluate("does_not_comply", ReferenceScope::SetFrontMatter)
                .unwrap()
                .include_reinforcement
        );
    }
    #[test]
    fn standalone_retains_its_own_references() {
        assert_eq!(
            evaluate("complies", ReferenceScope::Transformer)
                .unwrap()
                .references_scope,
            "transformer"
        );
    }
    #[test]
    fn incomplete_or_unknown_results_are_never_treated_as_passing() {
        for verdict in ["", "incomplete", "reserved", "pass"] {
            assert!(evaluate(verdict, ReferenceScope::SetFrontMatter).is_err());
        }
    }
}
