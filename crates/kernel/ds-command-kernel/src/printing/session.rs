//! The print editor's workflow, decided once for every host.
//!
//! May this layout be changed right now? Is it dirty? What does Cancel
//! restore, and what do Undo and Redo mean for a print document? The Printing
//! setup page answered all of that with a handful of component flags and two
//! arrays, and `ds` could not answer at all. This module is the reducer both
//! hosts run: the host reports the state it holds and the event that just
//! happened, and receives the next state plus the operations to perform on
//! the documents it keeps (the current layout, the baseline, the undo and redo
//! stacks). Documents never cross this boundary — a keystroke must not carry
//! forty layouts into the kernel and back — only what is known about them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::lifecycle::{Library, Refusal};

/// How many undo entries the host keeps. Older entries fall off the bottom.
pub const HISTORY_CAP: usize = 40;

/// What the host knows about the documents it holds. Every field is a fact;
/// absent fields read as their zero value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct State {
    /// Which catalogue this page publishes to; a project page may read and
    /// copy a global template but never edit it in place.
    #[serde(default)]
    pub library: Library,
    /// A document is loaded (draft or published).
    #[serde(default)]
    pub has_layout: bool,
    /// The editor is in edit mode.
    #[serde(default)]
    pub editing: bool,
    /// Unsaved edits exist.
    #[serde(default)]
    pub dirty: bool,
    /// The loaded document came from the global library.
    #[serde(default)]
    pub loaded_global: bool,
    /// A baseline (the last loaded or saved document) is held for Cancel.
    #[serde(default)]
    pub has_baseline: bool,
    /// The id the catalogue knows the loaded document under; empty for a draft.
    #[serde(default)]
    pub saved_id: String,
    /// The revision held for the loaded document; empty for a draft.
    #[serde(default)]
    pub revision: String,
    /// How many documents the undo stack holds.
    #[serde(default)]
    pub undo_depth: usize,
    /// How many documents the redo stack holds.
    #[serde(default)]
    pub redo_depth: usize,
}

/// What just happened on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// Start a new unsaved document: create, duplicate or import.
    Draft {
        /// The operator has confirmed discarding unsaved edits.
        #[serde(default)]
        confirmed: bool,
    },
    /// Load a published setup from the catalogue.
    Load {
        id: String,
        #[serde(default)]
        revision: String,
        /// The setup comes from the global library.
        #[serde(default)]
        global: bool,
        #[serde(default)]
        confirmed: bool,
        /// The host already shows exactly this document from this library.
        #[serde(default)]
        same_document: bool,
    },
    /// The operator chose to edit the loaded document.
    BeginEdit,
    /// The operator opened the context picker, which edits the document.
    OpenContext,
    /// The document changed by an edit that belongs in history.
    Edit,
    Undo,
    Redo,
    /// Cancel: return to the baseline, or to no document.
    Discard {
        #[serde(default)]
        confirmed: bool,
    },
    /// The store accepted the document under this identity.
    Saved {
        id: String,
        #[serde(default)]
        revision: String,
    },
    /// The store deleted the loaded setup.
    Deleted,
}

/// What the host must do to the documents it holds, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Install the incoming document as the current one, without history.
    InstallDocument,
    /// Push the current document onto undo (keeping the newest
    /// `HISTORY_CAP`) and empty redo, before the edit replaces it.
    PushUndo,
    /// Push the current document onto redo, then install the top of undo.
    PopUndo,
    /// Push the current document onto undo, then install the top of redo.
    PopRedo,
    ClearHistory,
    /// Remember the current document as the baseline Cancel returns to.
    SnapshotBaseline,
    /// Install the baseline as the current document, without history.
    RestoreBaseline,
    DropBaseline,
    /// Forget the current document entirely.
    ClearDocument,
}

/// The outcome of one event: the next state and the operations that take the
/// host there, or a refusal that leaves everything as it was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Transition {
    pub state: State,
    pub ops: Vec<Op>,
    /// The event changed something.
    pub applied: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

fn refuse(state: &State, code: &str) -> Transition {
    Transition {
        state: state.clone(),
        ops: Vec::new(),
        applied: false,
        refusal: Some(Refusal {
            code: code.into(),
            message_key: code.into(),
        }),
    }
}

fn noop(state: &State) -> Transition {
    Transition {
        state: state.clone(),
        ops: Vec::new(),
        applied: false,
        refusal: None,
    }
}

fn apply(state: State, ops: Vec<Op>) -> Transition {
    Transition {
        state,
        ops,
        applied: true,
        refusal: None,
    }
}

fn bounded(value: &str, what: &str) -> Result<(), String> {
    if value.len() > 256 || value.chars().any(char::is_control) {
        return Err(format!("{what} must be one bounded token"));
    }
    Ok(())
}

/// Decide what one event does to this state.
pub fn step(state: &State, event: &Event) -> Result<Transition, String> {
    bounded(&state.saved_id, "saved_id")?;
    bounded(&state.revision, "revision")?;
    let global_read_only = state.loaded_global && state.library == Library::Project;
    Ok(match event {
        Event::Draft { confirmed } => {
            if state.dirty && !confirmed {
                return Ok(refuse(state, "printing_discard_unconfirmed"));
            }
            apply(
                State {
                    library: state.library,
                    has_layout: true,
                    editing: true,
                    dirty: true,
                    ..State::default()
                },
                vec![Op::InstallDocument, Op::DropBaseline, Op::ClearHistory],
            )
        }
        Event::Load {
            id,
            revision,
            global,
            confirmed,
            same_document,
        } => {
            bounded(id, "id")?;
            bounded(revision, "revision")?;
            if *same_document {
                return Ok(noop(state));
            }
            if state.dirty && !confirmed {
                return Ok(refuse(state, "printing_discard_unconfirmed"));
            }
            apply(
                State {
                    library: state.library,
                    has_layout: true,
                    editing: false,
                    dirty: false,
                    loaded_global: *global,
                    has_baseline: true,
                    saved_id: id.clone(),
                    revision: revision.clone(),
                    undo_depth: 0,
                    redo_depth: 0,
                },
                vec![Op::InstallDocument, Op::SnapshotBaseline, Op::ClearHistory],
            )
        }
        Event::BeginEdit => {
            if !state.has_layout {
                refuse(state, "printing_no_document")
            } else if global_read_only {
                refuse(state, "printing_global_read_only")
            } else if state.editing {
                noop(state)
            } else {
                apply(
                    State {
                        editing: true,
                        ..state.clone()
                    },
                    Vec::new(),
                )
            }
        }
        Event::OpenContext => {
            if !state.has_layout {
                refuse(state, "printing_no_document")
            } else if state.editing || state.loaded_global {
                // A global template on a project page opens its context
                // read-only; an editor already editing stays as it is.
                noop(state)
            } else {
                apply(
                    State {
                        editing: true,
                        ..state.clone()
                    },
                    Vec::new(),
                )
            }
        }
        Event::Edit => {
            if !state.editing {
                refuse(state, "printing_not_editing")
            } else if !state.has_layout {
                refuse(state, "printing_no_document")
            } else {
                apply(
                    State {
                        dirty: true,
                        undo_depth: (state.undo_depth + 1).min(HISTORY_CAP),
                        redo_depth: 0,
                        ..state.clone()
                    },
                    vec![Op::PushUndo],
                )
            }
        }
        Event::Undo | Event::Redo => {
            let back = matches!(event, Event::Undo);
            let from = if back {
                state.undo_depth
            } else {
                state.redo_depth
            };
            if !state.editing {
                refuse(state, "printing_not_editing")
            } else if !state.has_layout {
                refuse(state, "printing_no_document")
            } else if from == 0 {
                refuse(
                    state,
                    if back {
                        "printing_nothing_to_undo"
                    } else {
                        "printing_nothing_to_redo"
                    },
                )
            } else {
                // The stack the current document moves onto is not trimmed
                // here: only an edit trims undo. This is the page's own rule,
                // kept so a replay of its history is exact.
                let (undo_depth, redo_depth) = if back {
                    (state.undo_depth - 1, state.redo_depth + 1)
                } else {
                    (state.undo_depth + 1, state.redo_depth - 1)
                };
                apply(
                    State {
                        dirty: true,
                        undo_depth,
                        redo_depth,
                        ..state.clone()
                    },
                    vec![if back { Op::PopUndo } else { Op::PopRedo }],
                )
            }
        }
        Event::Discard { confirmed } => {
            if state.dirty && !confirmed {
                return Ok(refuse(state, "printing_discard_unconfirmed"));
            }
            let ops = if state.has_baseline {
                vec![Op::RestoreBaseline, Op::ClearHistory]
            } else {
                vec![Op::ClearDocument, Op::ClearHistory]
            };
            apply(
                State {
                    has_layout: state.has_baseline,
                    editing: false,
                    dirty: false,
                    undo_depth: 0,
                    redo_depth: 0,
                    ..state.clone()
                },
                ops,
            )
        }
        Event::Saved { id, revision } => {
            bounded(id, "id")?;
            bounded(revision, "revision")?;
            if !state.has_layout {
                return Ok(refuse(state, "printing_no_document"));
            }
            // History survives a save, as it did: the operator may still undo
            // past the point they published.
            apply(
                State {
                    saved_id: id.clone(),
                    revision: revision.clone(),
                    has_baseline: true,
                    dirty: false,
                    editing: false,
                    ..state.clone()
                },
                vec![Op::SnapshotBaseline],
            )
        }
        Event::Deleted => apply(
            State {
                library: state.library,
                loaded_global: state.loaded_global,
                ..State::default()
            },
            vec![Op::ClearDocument, Op::DropBaseline, Op::ClearHistory],
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s() -> State {
        State {
            library: Library::Project,
            ..State::default()
        }
    }

    fn go(state: &State, event: Event) -> Transition {
        step(state, &event).unwrap()
    }

    fn code(t: &Transition) -> Option<&str> {
        t.refusal.as_ref().map(|r| r.code.as_str())
    }

    #[test]
    fn a_draft_is_editable_dirty_and_unsaved_with_no_history() {
        let t = go(&s(), Event::Draft { confirmed: false });
        assert!(t.applied);
        assert_eq!(
            t.ops,
            vec![Op::InstallDocument, Op::DropBaseline, Op::ClearHistory]
        );
        assert!(t.state.has_layout && t.state.editing && t.state.dirty);
        assert!(!t.state.has_baseline && !t.state.loaded_global);
        assert_eq!((t.state.undo_depth, t.state.redo_depth), (0, 0));
        assert!(t.state.saved_id.is_empty() && t.state.revision.is_empty());
    }

    #[test]
    fn a_load_is_read_only_clean_and_baselined() {
        let t = go(
            &s(),
            Event::Load {
                id: "print-1".into(),
                revision: "r1".into(),
                global: true,
                confirmed: false,
                same_document: false,
            },
        );
        assert_eq!(
            t.ops,
            vec![Op::InstallDocument, Op::SnapshotBaseline, Op::ClearHistory]
        );
        assert!(!t.state.editing && !t.state.dirty && t.state.has_baseline);
        assert!(t.state.loaded_global);
        assert_eq!(t.state.saved_id, "print-1");
        assert_eq!(t.state.revision, "r1");
    }

    #[test]
    fn dirty_edits_block_draft_load_and_discard_until_confirmed() {
        let mut d = go(&s(), Event::Draft { confirmed: false }).state;
        d = go(&d, Event::Edit).state;
        assert!(d.dirty);
        for event in [
            Event::Draft { confirmed: false },
            Event::Load {
                id: "x".into(),
                revision: String::new(),
                global: false,
                confirmed: false,
                same_document: false,
            },
            Event::Discard { confirmed: false },
        ] {
            let t = go(&d, event);
            assert_eq!(code(&t), Some("printing_discard_unconfirmed"));
            assert_eq!(t.state, d);
            assert!(t.ops.is_empty() && !t.applied);
        }
        assert!(go(&d, Event::Discard { confirmed: true }).applied);
    }

    #[test]
    fn loading_the_document_already_shown_does_nothing() {
        let t = go(
            &s(),
            Event::Load {
                id: "x".into(),
                revision: String::new(),
                global: false,
                confirmed: false,
                same_document: true,
            },
        );
        assert!(!t.applied && t.refusal.is_none() && t.ops.is_empty());
    }

    #[test]
    fn an_edit_needs_edit_mode_pushes_undo_and_empties_redo() {
        let loaded = go(
            &s(),
            Event::Load {
                id: "x".into(),
                revision: "r".into(),
                global: false,
                confirmed: false,
                same_document: false,
            },
        )
        .state;
        assert_eq!(
            code(&go(&loaded, Event::Edit)),
            Some("printing_not_editing")
        );
        let editing = go(&loaded, Event::BeginEdit).state;
        assert!(editing.editing && !editing.dirty);
        let edited = go(&editing, Event::Edit);
        assert_eq!(edited.ops, vec![Op::PushUndo]);
        assert!(edited.state.dirty);
        assert_eq!((edited.state.undo_depth, edited.state.redo_depth), (1, 0));
    }

    #[test]
    fn undo_is_capped_at_forty_by_edits_only() {
        let mut st = go(&s(), Event::Draft { confirmed: false }).state;
        for _ in 0..45 {
            st = go(&st, Event::Edit).state;
        }
        assert_eq!(st.undo_depth, HISTORY_CAP);
        let undone = go(&st, Event::Undo);
        assert_eq!(undone.ops, vec![Op::PopUndo]);
        assert_eq!(
            (undone.state.undo_depth, undone.state.redo_depth),
            (HISTORY_CAP - 1, 1)
        );
        let redone = go(&undone.state, Event::Redo);
        assert_eq!(redone.ops, vec![Op::PopRedo]);
        assert_eq!(
            (redone.state.undo_depth, redone.state.redo_depth),
            (HISTORY_CAP, 0)
        );
        assert_eq!(
            code(&go(&redone.state, Event::Redo)),
            Some("printing_nothing_to_redo")
        );
    }

    #[test]
    fn undo_and_redo_need_edit_mode_and_something_to_move() {
        let draft = go(&s(), Event::Draft { confirmed: false }).state;
        assert_eq!(
            code(&go(&draft, Event::Undo)),
            Some("printing_nothing_to_undo")
        );
        let saved = go(
            &go(&draft, Event::Edit).state,
            Event::Saved {
                id: "x".into(),
                revision: "r".into(),
            },
        )
        .state;
        assert!(!saved.editing && saved.undo_depth == 1);
        assert_eq!(code(&go(&saved, Event::Undo)), Some("printing_not_editing"));
    }

    #[test]
    fn cancel_restores_the_baseline_or_clears_the_document() {
        let loaded = go(
            &s(),
            Event::Load {
                id: "x".into(),
                revision: "r".into(),
                global: false,
                confirmed: false,
                same_document: false,
            },
        )
        .state;
        let edited = go(&go(&loaded, Event::BeginEdit).state, Event::Edit).state;
        let cancelled = go(&edited, Event::Discard { confirmed: true });
        assert_eq!(cancelled.ops, vec![Op::RestoreBaseline, Op::ClearHistory]);
        assert!(cancelled.state.has_layout && !cancelled.state.dirty && !cancelled.state.editing);
        assert_eq!(cancelled.state.saved_id, "x");
        let draft = go(
            &go(&s(), Event::Draft { confirmed: false }).state,
            Event::Edit,
        )
        .state;
        let cleared = go(&draft, Event::Discard { confirmed: true });
        assert_eq!(cleared.ops, vec![Op::ClearDocument, Op::ClearHistory]);
        assert!(!cleared.state.has_layout);
    }

    #[test]
    fn a_save_records_identity_and_baseline_and_leaves_edit_mode() {
        let draft = go(
            &go(&s(), Event::Draft { confirmed: false }).state,
            Event::Edit,
        )
        .state;
        let t = go(
            &draft,
            Event::Saved {
                id: "print-9".into(),
                revision: "r9".into(),
            },
        );
        assert_eq!(t.ops, vec![Op::SnapshotBaseline]);
        assert_eq!(t.state.saved_id, "print-9");
        assert_eq!(t.state.revision, "r9");
        assert!(t.state.has_baseline && !t.state.dirty && !t.state.editing);
        assert_eq!(t.state.undo_depth, 1);
    }

    #[test]
    fn a_global_template_on_a_project_page_is_read_only_but_its_context_opens() {
        let loaded = go(
            &s(),
            Event::Load {
                id: "g".into(),
                revision: "r".into(),
                global: true,
                confirmed: false,
                same_document: false,
            },
        )
        .state;
        assert_eq!(
            code(&go(&loaded, Event::BeginEdit)),
            Some("printing_global_read_only")
        );
        let ctx = go(&loaded, Event::OpenContext);
        assert!(!ctx.applied && ctx.refusal.is_none() && !ctx.state.editing);
        let global_page = State {
            library: Library::Global,
            ..loaded.clone()
        };
        assert!(go(&global_page, Event::BeginEdit).state.editing);
    }

    #[test]
    fn opening_the_context_picker_enters_edit_mode_for_a_project_template() {
        let loaded = go(
            &s(),
            Event::Load {
                id: "p".into(),
                revision: "r".into(),
                global: false,
                confirmed: false,
                same_document: false,
            },
        )
        .state;
        let t = go(&loaded, Event::OpenContext);
        assert!(t.applied && t.state.editing && t.ops.is_empty());
        assert!(!go(&t.state, Event::OpenContext).applied);
    }

    #[test]
    fn a_delete_forgets_the_document_its_identity_and_its_history() {
        let loaded = go(
            &s(),
            Event::Load {
                id: "p".into(),
                revision: "r".into(),
                global: false,
                confirmed: false,
                same_document: false,
            },
        )
        .state;
        let t = go(&loaded, Event::Deleted);
        assert_eq!(
            t.ops,
            vec![Op::ClearDocument, Op::DropBaseline, Op::ClearHistory]
        );
        assert!(!t.state.has_layout && t.state.saved_id.is_empty());
        assert_eq!(t.state.library, Library::Project);
        assert_eq!(
            code(&go(&t.state, Event::BeginEdit)),
            Some("printing_no_document")
        );
    }

    #[test]
    fn a_malformed_identity_is_an_error_before_any_decision() {
        let mut st = s();
        st.saved_id = "x".repeat(257);
        assert!(step(&st, &Event::BeginEdit).is_err());
        assert!(step(
            &s(),
            &Event::Saved {
                id: "a\u{7}".into(),
                revision: String::new()
            }
        )
        .is_err());
    }
}
