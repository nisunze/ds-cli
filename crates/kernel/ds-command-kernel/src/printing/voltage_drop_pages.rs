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

/// Required retained producer facts for voltage-table eligibility. A loading-only
/// global verdict cannot establish whether the voltage criterion passed.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VoltageDropFacts {
    pub customers_failing: i64,
    pub worst_vd_pct: f64,
    pub voltage_limit_pct: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Plan {
    pub include_reinforcement: bool,
    pub include_worst_voltage_drop: bool,
    pub references_scope: &'static str,
}

pub fn evaluate(
    verdict: &str,
    scope: ReferenceScope,
    voltage: VoltageDropFacts,
) -> Result<Plan, String> {
    if voltage.customers_failing < 0
        || !voltage.worst_vd_pct.is_finite()
        || voltage.worst_vd_pct < 0.0
        || !voltage.voltage_limit_pct.is_finite()
        || voltage.voltage_limit_pct <= 0.0
    {
        return Err(
            "voltage_drop_page_plan_invalid: complete nonnegative retained voltage facts and a positive voltage limit are required".into(),
        );
    }
    let include_worst_voltage_drop =
        voltage.customers_failing > 0 || voltage.worst_vd_pct > voltage.voltage_limit_pct;
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
        include_worst_voltage_drop,
        references_scope: match scope {
            ReferenceScope::Transformer => "transformer",
            ReferenceScope::SetFrontMatter => "set_front_matter",
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn passing_voltage() -> VoltageDropFacts {
        VoltageDropFacts {
            customers_failing: 0,
            worst_vd_pct: 9.0,
            voltage_limit_pct: 10.0,
        }
    }

    #[test]
    fn passing_voltage_omits_voltage_table_and_keeps_reference_scope() {
        assert_eq!(
            evaluate(
                "complies",
                ReferenceScope::SetFrontMatter,
                passing_voltage()
            )
            .unwrap(),
            Plan {
                include_reinforcement: false,
                include_worst_voltage_drop: false,
                references_scope: "set_front_matter",
            }
        );
        assert_eq!(
            evaluate("complies", ReferenceScope::Transformer, passing_voltage())
                .unwrap()
                .references_scope,
            "transformer"
        );
    }

    #[test]
    fn loading_only_failure_does_not_imply_voltage_failure() {
        for verdict in ["does_not_comply", "complies_with_reinforcement"] {
            let plan =
                evaluate(verdict, ReferenceScope::SetFrontMatter, passing_voltage()).unwrap();
            assert!(plan.include_reinforcement);
            assert!(!plan.include_worst_voltage_drop);
        }
    }

    #[test]
    fn either_retained_voltage_failure_fact_keeps_the_voltage_table() {
        for voltage in [
            VoltageDropFacts {
                customers_failing: 1,
                ..passing_voltage()
            },
            VoltageDropFacts {
                worst_vd_pct: 10.01,
                ..passing_voltage()
            },
            VoltageDropFacts {
                customers_failing: 8,
                worst_vd_pct: 12.0,
                ..passing_voltage()
            },
        ] {
            assert!(
                evaluate("does_not_comply", ReferenceScope::Transformer, voltage)
                    .unwrap()
                    .include_worst_voltage_drop
            );
        }
    }

    #[test]
    fn equal_limit_without_failing_customers_is_passing() {
        let voltage = VoltageDropFacts {
            worst_vd_pct: 10.0,
            ..passing_voltage()
        };
        assert!(
            !evaluate("complies", ReferenceScope::Transformer, voltage)
                .unwrap()
                .include_worst_voltage_drop
        );
    }

    #[test]
    fn invalid_voltage_evidence_is_refused() {
        for voltage in [
            VoltageDropFacts {
                customers_failing: -1,
                ..passing_voltage()
            },
            VoltageDropFacts {
                worst_vd_pct: -0.01,
                ..passing_voltage()
            },
            VoltageDropFacts {
                worst_vd_pct: f64::NAN,
                ..passing_voltage()
            },
            VoltageDropFacts {
                worst_vd_pct: f64::INFINITY,
                ..passing_voltage()
            },
            VoltageDropFacts {
                voltage_limit_pct: 0.0,
                ..passing_voltage()
            },
            VoltageDropFacts {
                voltage_limit_pct: -1.0,
                ..passing_voltage()
            },
            VoltageDropFacts {
                voltage_limit_pct: f64::NAN,
                ..passing_voltage()
            },
            VoltageDropFacts {
                voltage_limit_pct: f64::INFINITY,
                ..passing_voltage()
            },
        ] {
            assert!(evaluate("complies", ReferenceScope::Transformer, voltage).is_err());
        }
    }

    #[test]
    fn missing_or_malformed_voltage_facts_cannot_become_passing() {
        for facts in [
            serde_json::json!({}),
            serde_json::json!({"worst_vd_pct": 9, "voltage_limit_pct": 10}),
            serde_json::json!({"customers_failing": 0, "voltage_limit_pct": 10}),
            serde_json::json!({"customers_failing": 0, "worst_vd_pct": 9}),
            serde_json::json!({"customers_failing": 0.5, "worst_vd_pct": 9, "voltage_limit_pct": 10}),
            serde_json::json!({"customers_failing": 0, "worst_vd_pct": null, "voltage_limit_pct": 10}),
        ] {
            assert!(serde_json::from_value::<VoltageDropFacts>(facts).is_err());
        }
    }

    #[test]
    fn incomplete_or_unknown_verdicts_are_never_treated_as_passing() {
        for verdict in ["", "incomplete", "reserved", "pass"] {
            assert!(evaluate(verdict, ReferenceScope::SetFrontMatter, passing_voltage()).is_err());
        }
    }
}
