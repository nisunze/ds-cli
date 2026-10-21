//! Admission of host-observed project configuration. Never reads a cache or
//! clock. Every asynchronous host boundary must supply fresh scope/generation
//! facts before returning bytes or starting a durable write.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub cache_present: bool,
    pub cached_generation: u64,
    pub current_generation: u64,
    pub scope_matches: bool,
    pub invalidation_pending: bool,
    pub response_status: ResponseStatus,
    pub fetched_present: bool,
    pub wait_expired: bool,
    pub online: bool,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Idle,
    Pending,
    Success,
    Failed,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    ServeCached,
    ServeFetched,
    Superseded,
    StalePermitted,
    Unavailable,
}
#[derive(Clone, Debug, Serialize)]
pub struct Decision {
    pub verdict: Verdict,
    pub reason_key: &'static str,
}
pub fn admit(facts: &Facts) -> Decision {
    use ResponseStatus::*;
    use Verdict::*;
    let (verdict, reason_key) = if !facts.scope_matches {
        (Superseded, "config.scope_changed")
    } else if facts.cached_generation != facts.current_generation || facts.invalidation_pending {
        (Superseded, "config.generation_changed")
    } else if matches!(facts.response_status, Success) && facts.fetched_present {
        (ServeFetched, "config.fetched")
    } else if facts.cache_present {
        if matches!(facts.response_status, Idle) {
            (ServeCached, "config.cached")
        } else {
            (StalePermitted, "config.stale_permitted")
        }
    } else if facts.wait_expired || matches!(facts.response_status, Pending) && facts.online {
        (Unavailable, "config.loading")
    } else {
        (Unavailable, "config.unavailable")
    };
    Decision {
        verdict,
        reason_key,
    }
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 4096 {
        return Err("config_input_too_large".into());
    }
    let facts: Facts = serde_json::from_slice(input).map_err(|_| "config_request_invalid")?;
    serde_json::to_string(&admit(&facts)).map_err(|_| "config_request_invalid".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_admission_is_scope_and_generation_fenced() {
        for cache_present in [false, true] {
            for response_status in [
                ResponseStatus::Idle,
                ResponseStatus::Pending,
                ResponseStatus::Success,
                ResponseStatus::Failed,
            ] {
                for wait_expired in [false, true] {
                    for online in [false, true] {
                        let mut facts = Facts {
                            cache_present,
                            cached_generation: 1,
                            current_generation: 1,
                            scope_matches: true,
                            invalidation_pending: false,
                            response_status,
                            fetched_present: true,
                            wait_expired,
                            online,
                        };
                        facts.scope_matches = false;
                        assert_eq!(admit(&facts).verdict, Verdict::Superseded);
                        facts.scope_matches = true;
                        facts.current_generation = 2;
                        assert_eq!(admit(&facts).verdict, Verdict::Superseded);
                        facts.current_generation = 1;
                        facts.invalidation_pending = true;
                        assert_eq!(admit(&facts).verdict, Verdict::Superseded);
                    }
                }
            }
        }
    }
    #[test]
    fn failed_refresh_retains_only_a_usable_cache() {
        let mut facts = Facts {
            cache_present: true,
            cached_generation: 1,
            current_generation: 1,
            scope_matches: true,
            invalidation_pending: false,
            response_status: ResponseStatus::Failed,
            fetched_present: false,
            wait_expired: false,
            online: false,
        };
        assert_eq!(admit(&facts).verdict, Verdict::StalePermitted);
        facts.cache_present = false;
        assert_eq!(admit(&facts).verdict, Verdict::Unavailable);
    }
}
