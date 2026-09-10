//! Setup lifecycle decisions, decided once for every host.
//!
//! The Printing setup page and `ds report layout` both keep a catalogue of
//! named setups in ds-brain. What they may do to one right now — whether a
//! read may be answered from a local copy, whether a save is a create or an
//! update, whether a copy or a delete is admissible — used to be decided in
//! the browser (`shared.ts`) and again, differently, in the native client.
//! This module answers from facts the host reports. It performs no IO and
//! reads no clock; the host performs the read, the write, the cache.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What the host is about to do.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    List,
    #[default]
    Read,
    Save,
    Delete,
    Copy,
}

/// Which catalogue the setup lives in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Library {
    Global,
    #[default]
    Project,
}

/// How a live read failed, classified by the host from its transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemoteFailure {
    /// The device reports no network.
    Offline,
    /// The service could not be reached or answered 502/503/504.
    Unreachable,
    /// The service answered and refused (authorization, conflict, invalid).
    Refused,
}

/// The facts a host reports. Every field is a fact about the host's state,
/// never a decision; absent booleans read as `false`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub intent: Intent,
    pub library: Library,
    /// The device believes it has a network.
    #[serde(default)]
    pub online: bool,
    /// A local copy of the answer exists (reads only).
    #[serde(default)]
    pub has_cache: bool,
    /// The live read was attempted and failed this way (reads only).
    #[serde(default)]
    pub remote_failure: Option<RemoteFailure>,
    /// The revision the host holds for the setup it is acting on; empty when
    /// it holds none.
    #[serde(default)]
    pub held_revision: String,
    /// A saved setup is loaded (its id came back from the catalogue).
    #[serde(default)]
    pub saved: bool,
    /// The document being saved is the loaded saved setup, not a new draft.
    #[serde(default)]
    pub loaded_matches: bool,
    /// The editor is in edit mode.
    #[serde(default)]
    pub editing: bool,
    /// The loaded setup came from the global library, which a project page
    /// may read and copy but never overwrite.
    #[serde(default)]
    pub loaded_global: bool,
    /// Unsaved edits exist.
    #[serde(default)]
    pub dirty: bool,
}

/// What the host does next. Exactly one, never a fallback chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    ReadLive,
    ReadCached,
    Create,
    Update,
    Delete,
    Copy,
    Refuse,
}

/// A refusal names its code and the message key the host renders; the host
/// may substitute `{detail}` with the remote service's own message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Refusal {
    pub code: String,
    pub message_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Plan {
    pub decision: Decision,
    /// The optimistic revision the store action must carry, when it must.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

fn refuse(code: &str) -> Plan {
    Plan {
        decision: Decision::Refuse,
        expected_revision: None,
        refusal: Some(Refusal {
            code: code.into(),
            message_key: code.into(),
        }),
    }
}

fn act(decision: Decision, expected_revision: Option<String>) -> Plan {
    Plan {
        decision,
        expected_revision,
        refusal: None,
    }
}

/// Decide the one action for these facts.
pub fn plan(facts: &Facts) -> Result<Plan, String> {
    let revision = facts.held_revision.trim();
    if revision.len() > 128 || revision.chars().any(char::is_control) {
        return Err("held_revision must be one bounded revision token or empty".into());
    }
    let held = (!revision.is_empty()).then(|| revision.to_owned());
    Ok(match facts.intent {
        Intent::List | Intent::Read => match (facts.online, facts.remote_failure) {
            // A live answer arrived, or is about to be asked for.
            (true, None) => act(Decision::ReadLive, None),
            // The service answered and said no: a cached copy is not an
            // answer to a refusal, it is a way of hiding one.
            (_, Some(RemoteFailure::Refused)) => refuse("printing_remote_refused"),
            // Offline, or the service could not be reached: a held copy may
            // answer, and its absence is named for what it is.
            (false, _) | (true, Some(_)) => {
                if facts.has_cache {
                    act(Decision::ReadCached, None)
                } else if facts.remote_failure.is_some() {
                    refuse("printing_setup_unavailable_live")
                } else {
                    refuse("printing_no_cached_copy")
                }
            }
        },
        Intent::Save => {
            if !facts.editing {
                refuse("printing_not_editing")
            } else if facts.loaded_global && facts.library == Library::Project {
                refuse("printing_global_read_only")
            } else if !facts.online {
                refuse("printing_offline_publish")
            } else if facts.loaded_matches && held.is_some() {
                act(Decision::Update, held)
            } else {
                act(Decision::Create, None)
            }
        }
        Intent::Delete | Intent::Copy => {
            if !facts.saved || held.is_none() {
                refuse("printing_nothing_saved")
            } else if facts.dirty {
                refuse("printing_dirty_edits")
            } else if !facts.online {
                refuse(if facts.intent == Intent::Delete {
                    "printing_offline_delete"
                } else {
                    "printing_offline_copy"
                })
            } else if facts.intent == Intent::Delete {
                act(Decision::Delete, held)
            } else {
                act(Decision::Copy, held)
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(intent: Intent) -> Facts {
        Facts {
            intent,
            library: Library::Project,
            online: true,
            ..Facts::default()
        }
    }

    fn decision(f: &Facts) -> (Decision, Option<String>, Option<String>) {
        let p = plan(f).unwrap();
        (p.decision, p.expected_revision, p.refusal.map(|r| r.code))
    }

    #[test]
    fn a_read_is_live_when_online_and_the_service_has_not_failed() {
        let f = facts(Intent::Read);
        assert_eq!(decision(&f), (Decision::ReadLive, None, None));
        let f = facts(Intent::List);
        assert_eq!(decision(&f).0, Decision::ReadLive);
    }

    #[test]
    fn an_unreachable_service_yields_the_held_copy_or_names_its_absence() {
        let mut f = facts(Intent::Read);
        f.remote_failure = Some(RemoteFailure::Unreachable);
        f.has_cache = true;
        assert_eq!(decision(&f).0, Decision::ReadCached);
        f.has_cache = false;
        assert_eq!(
            decision(&f).2.as_deref(),
            Some("printing_setup_unavailable_live")
        );
    }

    #[test]
    fn offline_reads_use_the_copy_and_refuse_without_one() {
        let mut f = facts(Intent::List);
        f.online = false;
        f.has_cache = true;
        assert_eq!(decision(&f).0, Decision::ReadCached);
        f.has_cache = false;
        assert_eq!(decision(&f).2.as_deref(), Some("printing_no_cached_copy"));
    }

    #[test]
    fn a_refusal_from_the_service_is_never_answered_from_the_cache() {
        let mut f = facts(Intent::Read);
        f.remote_failure = Some(RemoteFailure::Refused);
        f.has_cache = true;
        assert_eq!(decision(&f).2.as_deref(), Some("printing_remote_refused"));
    }

    #[test]
    fn a_save_is_an_update_only_for_the_loaded_saved_setup_with_its_revision() {
        let mut f = facts(Intent::Save);
        f.editing = true;
        assert_eq!(decision(&f), (Decision::Create, None, None));
        f.loaded_matches = true;
        assert_eq!(decision(&f).0, Decision::Create);
        f.held_revision = "a".repeat(64);
        assert_eq!(decision(&f), (Decision::Update, Some("a".repeat(64)), None));
        f.loaded_matches = false;
        assert_eq!(decision(&f).0, Decision::Create);
    }

    #[test]
    fn a_save_refuses_outside_edit_mode_on_a_global_setup_and_offline() {
        let mut f = facts(Intent::Save);
        assert_eq!(decision(&f).2.as_deref(), Some("printing_not_editing"));
        f.editing = true;
        f.loaded_global = true;
        assert_eq!(decision(&f).2.as_deref(), Some("printing_global_read_only"));
        f.library = Library::Global;
        assert_eq!(decision(&f).0, Decision::Create);
        f.library = Library::Project;
        f.loaded_global = false;
        f.online = false;
        assert_eq!(decision(&f).2.as_deref(), Some("printing_offline_publish"));
    }

    #[test]
    fn copy_and_delete_need_a_saved_clean_setup_and_a_network() {
        for intent in [Intent::Delete, Intent::Copy] {
            let mut f = facts(intent);
            assert_eq!(decision(&f).2.as_deref(), Some("printing_nothing_saved"));
            f.saved = true;
            assert_eq!(decision(&f).2.as_deref(), Some("printing_nothing_saved"));
            f.held_revision = "b".repeat(64);
            f.dirty = true;
            assert_eq!(decision(&f).2.as_deref(), Some("printing_dirty_edits"));
            f.dirty = false;
            f.online = false;
            let code = decision(&f).2.unwrap();
            assert_eq!(
                code,
                if intent == Intent::Delete {
                    "printing_offline_delete"
                } else {
                    "printing_offline_copy"
                }
            );
            f.online = true;
            let (d, rev, _) = decision(&f);
            assert_eq!(rev.as_deref(), Some("b".repeat(64).as_str()));
            assert_eq!(
                d,
                if intent == Intent::Delete {
                    Decision::Delete
                } else {
                    Decision::Copy
                }
            );
        }
    }

    #[test]
    fn a_malformed_held_revision_is_refused_before_any_decision() {
        let mut f = facts(Intent::Save);
        f.editing = true;
        f.held_revision = "x".repeat(129);
        assert!(plan(&f).is_err());
        f.held_revision = "ab\u{7}".into();
        assert!(plan(&f).is_err());
    }

    #[test]
    fn a_refusal_carries_its_code_as_the_message_key() {
        let mut f = facts(Intent::Delete);
        f.saved = false;
        let p = plan(&f).unwrap();
        let r = p.refusal.unwrap();
        assert_eq!(r.code, r.message_key);
        assert_eq!(p.decision, Decision::Refuse);
    }
}
