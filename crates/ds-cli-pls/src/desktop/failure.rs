//! A finished run, read into a result or a typed refusal.
//!
//! An entry writes one document: `status: ok` with what the verb reads, or
//! `status: failed` with the driver's own message, the script that threw and
//! the PLS-CADD processes still running. The drivers already refuse
//! precisely — they stop on an unknown dialog, a digest that moved, a restore
//! that skipped a file — so the work here is to name those refusals with
//! stable codes, not to judge anything again.
//!
//! The cause is the driver's own message, matched against phrases the
//! drivers are known to throw and the watcher outcome they quote
//! (`did not return to ready: unknown`). Every phrase is held to the embedded
//! scripts by this module's tests, so a reworded driver fails the build here
//! rather than degrading a code to `driver_failed` on the desktop.
//!
//! The journals only enrich a dialog refusal: the latest `unknown_dialog` (or
//! `stop`) event the watcher wrote in the run's folders carries the dialog's
//! title and text. They never decide the cause. A step can journal a dialog
//! event and still succeed — `pls-report-any.ps1` runs the watcher once per
//! pass while it saves, and reads nothing from it — so the latest event is
//! evidence for a dialog refusal, not proof that a dialog caused a later
//! timeout.

use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use serde_json::{Value, json};

use super::run::Finished;
use super::{
    BACKUP_DIGEST_MISMATCH, BACKUP_INVALID, DIALOG_STOP, DRIVER_FAILED, OUTPUT_EXISTS,
    PLS_CADD_MISMATCH, PLS_CADD_RUNNING, PLS_CADD_TIMEOUT, RESTORED_TREE_MISMATCH,
    RESULT_UNREADABLE, SYSTEM_DRIVE_REFUSED, UNKNOWN_DIALOG, WORD_NOT_FOUND,
};

/// What went wrong, before it is checked against what the verb documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    PlsCaddRunning,
    PlsCaddMismatch,
    BackupDigestMismatch,
    BackupInvalid,
    RestoredTreeMismatch,
    WordNotFound,
    OutputExists,
    SystemDrive,
    UnknownDialog,
    DialogStop,
    Timeout,
    Other,
}

impl Kind {
    pub(crate) fn refusal(self) -> &'static Refusal {
        match self {
            Self::PlsCaddRunning => &PLS_CADD_RUNNING,
            Self::PlsCaddMismatch => &PLS_CADD_MISMATCH,
            Self::BackupDigestMismatch => &BACKUP_DIGEST_MISMATCH,
            Self::BackupInvalid => &BACKUP_INVALID,
            Self::RestoredTreeMismatch => &RESTORED_TREE_MISMATCH,
            Self::WordNotFound => &WORD_NOT_FOUND,
            Self::OutputExists => &OUTPUT_EXISTS,
            Self::SystemDrive => &SYSTEM_DRIVE_REFUSED,
            Self::UnknownDialog => &UNKNOWN_DIALOG,
            Self::DialogStop => &DIALOG_STOP,
            Self::Timeout => &PLS_CADD_TIMEOUT,
            Self::Other => &DRIVER_FAILED,
        }
    }
}

/// Phrases the drivers throw, and what each one means. Order matters: the
/// first match wins, and the specific causes come before the dialog and
/// timeout families that a later step can also report.
pub(crate) const MESSAGE_RULES: &[(Kind, &[&str])] = &[
    (
        Kind::PlsCaddRunning,
        &[
            "PLS-CADD is already running",
            "while PLS-CADD is already running",
        ],
    ),
    (
        Kind::PlsCaddMismatch,
        &[
            "executable digest does not match",
            "executable digest mismatch",
            "PLS-CADD version mismatch",
            "PLS-CADD executable version mismatch",
            "Unexpected executable",
        ],
    ),
    (
        Kind::BackupDigestMismatch,
        &[
            "Candidate digest mismatch",
            "Candidate backup digest mismatch",
        ],
    ),
    (
        Kind::BackupInvalid,
        &[
            "neither a native PLSBACKUPFILE nor a ZIP",
            "Invalid PLSBACKUPFILE",
            "ZIP-wrapped PLS backup must contain",
            "ZIP backup entry must be",
            "ZIP backup payload",
            "is not a native PLSBACKUPFILE",
            "Truncated PLS backup",
            "Native PLS backup contains no records",
            "Expected exactly one selected .xyz project in backup",
            "Candidate backup must contain exactly one project",
            "no recognized engineering-library members",
            "SourceRoot must be one absolute native Windows path",
            "cannot be mapped below an empty project root",
            "is outside project root",
            "ProjectFileName must be a leaf name",
        ],
    ),
    (
        Kind::RestoredTreeMismatch,
        &[
            "Restored member length mismatch",
            "Restored member digest mismatch",
            "Restored directory missing",
            "Restored tree verification failed",
            "Pre-open restore count mismatch",
            "Unexpected file(s) in fresh restore",
            "changed protected candidate content",
            "Native member payload digest mismatch",
        ],
    ),
    (Kind::WordNotFound, &["Microsoft Word is not registered"]),
    (
        Kind::SystemDrive,
        &["Refusing a run directory on C:", "Refusing a backup on C:"],
    ),
    (
        Kind::OutputExists,
        &[
            "Run directory already exists",
            "Output already exists",
            "already exists:",
        ],
    ),
    (Kind::DialogStop, &["requires repair and is not eligible"]),
    (
        Kind::UnknownDialog,
        &[
            "Unexpected PLS-CADD startup window",
            "Unexpected PLS-CADD top-level window",
            "Unexpected PLS-CADD window while exiting",
            "Unexpected PLS-CADD main-window title",
            "Unexpected restore/open dialog",
            "Unexpected backup dialog",
            "Unexpected exit dialog",
            "Unexpected window after backup",
            "Unexpected save-before-backup prompt",
            "Multiple PLS-CADD startup windows",
            "Multiple PLS-CADD top-level windows",
            "Multiple windows after backup",
            "PLS-CADD started with unexpected top-level window",
        ],
    ),
    (
        Kind::Timeout,
        &[
            "Timed out waiting for dialog title",
            "did not complete within",
            "did not settle within",
            "did not become stable within",
            "did not expose a main window within",
            "did not exit within",
            "did not appear within",
            "PDF not settled within",
        ],
    ),
];

/// The watcher's outcomes, as a driver quotes them in its message:
/// `did not return to ready: unknown`, `(r1, unknown)`, `ended 'unknown'`.
const OUTCOMES: &[(&str, Kind)] = &[
    ("unknown", Kind::UnknownDialog),
    ("stop", Kind::DialogStop),
    ("flow", Kind::DialogStop),
    ("timeout", Kind::Timeout),
];

/// The watcher events that mean a run stopped on a dialog.
const UNKNOWN_EVENTS: &[&str] = &[
    "unknown_dialog",
    "no_visible_ok",
    "button_not_visible",
    "no_ok_button",
];
const STOP_EVENTS: &[&str] = &["stop", "flow_dialog"];

const MAX_JOURNAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_JOURNALS: usize = 256;

/// The latest dialog decision the watcher journaled in a run's folders.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DialogEvent {
    pub(crate) kind: Kind,
    pub(crate) event: String,
    pub(crate) dialog: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) text: Option<String>,
    pub(crate) at: String,
    pub(crate) journal: String,
}

impl DialogEvent {
    fn detail(&self) -> Value {
        json!({
            "event": self.event,
            "dialog": self.dialog,
            "title": self.title,
            "text": self.text,
            "at": self.at,
            "journal": self.journal,
        })
    }
}

/// Read a finished run: its result, or the refusal it amounts to.
///
/// `documented` is the verb's own refusal list. A cause the verb does not
/// document is reported as `driver_failed`, so no code reaches a caller that
/// the command's help did not name.
pub(crate) fn outcome(
    finished: Finished,
    documented: &[Refusal],
    folders: &[&str],
) -> Result<Value, Failure> {
    let Some(document) = finished.document else {
        return Err(Failure::failed(
            RESULT_UNREADABLE.code,
            "the run ended without its result document",
        )
        .remedy(RESULT_UNREADABLE.remedy)
        .detail(json!({
            "exit_code": finished.exit_code,
            "stderr": finished.stderr_tail,
            "stdout": finished.stdout_tail,
            "command_line": finished.command_line,
        })));
    };
    match document["status"].as_str() {
        Some("ok") => Ok(document["result"].clone()),
        Some("failed") => Err(refusal(&document, documented, folders)),
        _ => Err(
            Failure::failed(RESULT_UNREADABLE.code, "the result document has no status")
                .remedy(RESULT_UNREADABLE.remedy)
                .detail(json!({ "exit_code": finished.exit_code })),
        ),
    }
}

fn refusal(document: &Value, documented: &[Refusal], folders: &[&str]) -> Failure {
    let message = document["message"].as_str().unwrap_or_default();
    let mut kind = classify(message);
    if !documented
        .iter()
        .any(|refusal| refusal.code == kind.refusal().code)
    {
        kind = Kind::Other;
    }
    let running = document["pls_cadd_running"].clone();
    let left_open = running.as_array().is_some_and(|pids| !pids.is_empty());
    let mut detail = json!({
        "message": message.chars().take(1_000).collect::<String>(),
        "script": document["script"],
        "line": document["line"],
        "pls_cadd_running": running,
        "folders": folders,
    });
    if matches!(kind, Kind::UnknownDialog | Kind::DialogStop)
        && let Some(dialog) = latest_dialog_event(folders, kind)
    {
        detail["dialog"] = dialog.detail();
    }
    if let Some(left) = qualify_failure(folders) {
        detail["process_left_for_operator"] = left;
    }
    let text = if left_open {
        "the desktop run stopped; PLS-CADD is still open for inspection"
    } else {
        "the desktop run stopped"
    };
    constructed(kind, text).detail(detail)
}

/// The refusal for `kind`, constructed with its own declared code and remedy.
fn constructed(kind: Kind, message: &str) -> Failure {
    let failure = match kind {
        Kind::PlsCaddRunning => Failure::conflict(PLS_CADD_RUNNING.code, message),
        Kind::PlsCaddMismatch => Failure::unavailable(PLS_CADD_MISMATCH.code, message),
        Kind::BackupDigestMismatch => Failure::conflict(BACKUP_DIGEST_MISMATCH.code, message),
        Kind::BackupInvalid => Failure::invalid(BACKUP_INVALID.code, message),
        Kind::RestoredTreeMismatch => Failure::failed(RESTORED_TREE_MISMATCH.code, message),
        Kind::WordNotFound => Failure::unavailable(WORD_NOT_FOUND.code, message),
        Kind::OutputExists => Failure::conflict(OUTPUT_EXISTS.code, message),
        Kind::SystemDrive => Failure::invalid(SYSTEM_DRIVE_REFUSED.code, message),
        Kind::UnknownDialog => Failure::failed(UNKNOWN_DIALOG.code, message),
        Kind::DialogStop => Failure::failed(DIALOG_STOP.code, message),
        Kind::Timeout => Failure::failed(PLS_CADD_TIMEOUT.code, message),
        Kind::Other => Failure::failed(DRIVER_FAILED.code, message),
    };
    failure.remedy(kind.refusal().remedy)
}

/// The cause of a failed run, from the driver's message: a phrase it is
/// known to throw, else the watcher outcome it quotes.
pub(crate) fn classify(message: &str) -> Kind {
    for (kind, phrases) in MESSAGE_RULES {
        if phrases.iter().any(|phrase| message.contains(phrase)) {
            return *kind;
        }
    }
    watcher_outcome(message).unwrap_or(Kind::Other)
}

/// The watcher outcome a driver quoted: the first outcome word in its
/// message, which the drivers put before any evidence they append. What
/// follows `; journal:` or `; see` is evidence and is not read.
fn watcher_outcome(message: &str) -> Option<Kind> {
    let head = ["; journal:", "; see "]
        .iter()
        .filter_map(|cut| message.find(cut))
        .min()
        .map_or(message, |at| &message[..at]);
    head.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .find_map(|token| {
            OUTCOMES
                .iter()
                .find(|(outcome, _)| *outcome == token)
                .map(|(_, kind)| *kind)
        })
}

/// The latest dialog event of `kind` in any journal below the run's folders.
/// Bounded: journals only, a fixed number of them, a fixed size each.
pub(crate) fn latest_dialog_event(folders: &[&str], kind: Kind) -> Option<DialogEvent> {
    let mut journals = Vec::new();
    for folder in folders {
        collect_journals(Path::new(folder), 0, &mut journals);
    }
    journals
        .iter()
        .flat_map(|journal| dialog_events(journal))
        .filter(|event| event.kind == kind)
        .max_by(|left, right| left.at.cmp(&right.at))
}

fn collect_journals(folder: &Path, depth: usize, into: &mut Vec<PathBuf>) {
    if depth > 3 || into.len() >= MAX_JOURNALS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect_journals(&path, depth + 1, into);
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|extension| extension == "jsonl")
            && into.len() < MAX_JOURNALS
        {
            into.push(path);
        }
    }
}

pub(crate) fn dialog_events(journal: &Path) -> Vec<DialogEvent> {
    let too_large = std::fs::metadata(journal).map_or(true, |meta| meta.len() > MAX_JOURNAL_BYTES);
    if too_large {
        return Vec::new();
    }
    let Ok(bytes) = std::fs::read(journal) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    text.lines()
        .map(|line| line.trim_start_matches('\u{feff}'))
        .filter(|line| line.contains("\"event\""))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|event| {
            let name = event["event"].as_str()?;
            let kind = if UNKNOWN_EVENTS.contains(&name) {
                Kind::UnknownDialog
            } else if STOP_EVENTS.contains(&name) {
                Kind::DialogStop
            } else {
                return None;
            };
            let bounded = |value: &Value| {
                value
                    .as_str()
                    .map(|text| text.chars().take(300).collect::<String>())
            };
            Some(DialogEvent {
                kind,
                event: name.to_string(),
                dialog: bounded(&event["dialog"]),
                title: bounded(&event["title"]),
                text: bounded(&event["text"]),
                at: event["at"].as_str().unwrap_or_default().to_string(),
                journal: journal.display().to_string(),
            })
        })
        .collect()
}

/// `pls-backup-restore-qualify.ps1` records on failure whether it left
/// PLS-CADD open for the operator; it never kills it after an unexpected
/// modal. Carried through so the caller knows before touching the desktop.
fn qualify_failure(folders: &[&str]) -> Option<Value> {
    folders.iter().find_map(|folder| {
        let path = Path::new(folder).join("evidence").join("failure.json");
        let bytes = std::fs::read(path).ok()?;
        let document = super::parse_document(&bytes)?;
        Some(json!({
            "process_left_for_operator": document["process_left_for_operator"],
            "active_process_id": document["active_process_id"],
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::bundle;

    /// Every phrase the classifier depends on is something an embedded
    /// driver actually throws. A reworded driver fails here.
    #[test]
    fn every_phrase_is_thrown_by_an_embedded_driver() {
        let drivers: String = bundle::BUNDLE
            .iter()
            .map(|script| String::from_utf8_lossy(script.bytes).into_owned())
            .collect();
        for (kind, phrases) in MESSAGE_RULES {
            for phrase in *phrases {
                assert!(
                    drivers.contains(phrase),
                    "{kind:?}: no embedded driver says `{phrase}`"
                );
            }
        }
        let watcher = bundle::text("pls-dialog-watch.ps1").unwrap();
        for (outcome, _) in OUTCOMES {
            assert!(
                watcher.contains(&format!("'{outcome}'")),
                "the watcher has no `{outcome}` outcome"
            );
        }
        for name in UNKNOWN_EVENTS.iter().chain(STOP_EVENTS) {
            assert!(
                watcher.contains(&format!("event = '{name}'")),
                "the watcher never journals `{name}`"
            );
        }
    }

    #[test]
    fn the_rule_table_keeps_causes_before_families() {
        let order: Vec<Kind> = MESSAGE_RULES.iter().map(|(kind, _)| *kind).collect();
        assert_eq!(
            &order[..8],
            [
                Kind::PlsCaddRunning,
                Kind::PlsCaddMismatch,
                Kind::BackupDigestMismatch,
                Kind::BackupInvalid,
                Kind::RestoredTreeMismatch,
                Kind::WordNotFound,
                Kind::SystemDrive,
                Kind::OutputExists,
            ]
        );
    }

    #[test]
    fn driver_messages_classify_by_their_cause() {
        for (message, expected) in [
            (
                "PLS-CADD is already running; close it first",
                Kind::PlsCaddRunning,
            ),
            (
                "Refusing to run while PLS-CADD is already running (PID(s): 4242)",
                Kind::PlsCaddRunning,
            ),
            (
                "PLS-CADD executable digest does not match profile: 00ff",
                Kind::PlsCaddMismatch,
            ),
            (
                "Candidate digest mismatch: 1234",
                Kind::BackupDigestMismatch,
            ),
            (
                "Candidate is neither a native PLSBACKUPFILE nor a ZIP local-file container",
                Kind::BackupInvalid,
            ),
            (
                r"Restored member length mismatch: cables\acsr 70-12mm2",
                Kind::RestoredTreeMismatch,
            ),
            (
                "Restored tree verification failed after close: Restored member digest mismatch: a.xyz",
                Kind::RestoredTreeMismatch,
            ),
            (
                "Microsoft Word is not registered (Word.Application): the report PDFs are made from the RTFs with Word",
                Kind::WordNotFound,
            ),
            (
                r"Refusing a run directory on C: (C:\runs\x)",
                Kind::SystemDrive,
            ),
            (
                r"Run directory already exists: G:\runs\x",
                Kind::OutputExists,
            ),
            (
                "Restored project requires repair and is not eligible for final backup: title='PLS-CADD Project Repair Wizard', body=''",
                Kind::DialogStop,
            ),
            (
                "Unexpected restore/open dialog: title='Warning', body='Something new'",
                Kind::UnknownDialog,
            ),
            (
                "save (after autosag and paging) did not return to ready: unknown",
                Kind::UnknownDialog,
            ),
            ("PLS-CADD not ready after open (r1): timeout", Kind::Timeout),
            (
                "dialog at exit (r2, stop): stop repair_wizard",
                Kind::DialogStop,
            ),
            (
                "report run ended with 'timeout'; journal: unknown_dialog x",
                Kind::Timeout,
            ),
            (
                "no Sheets View (frame 'PLS-CADD - a.xyz - 1 - [Profile View]', watch flow)",
                Kind::DialogStop,
            ),
            ("PLS-CADD did not exit within 60 s (r1)", Kind::Timeout),
            (
                "Restore/open did not complete within 900 seconds",
                Kind::Timeout,
            ),
            ("Section Table still open after OK", Kind::Other),
        ] {
            assert_eq!(classify(message), expected, "{message}");
        }
    }

    fn scratch(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("ds-cli-pls-failure-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("reports")).unwrap();
        root
    }

    /// Journal lines in the shapes the watcher writes, from the recorded
    /// 2026-09-23 native report run (PowerShell's `\u0027` escapes, a BOM on
    /// the first line of a new file).
    const WATCH_JOURNAL: &str = concat!(
        "\u{feff}{\"at\":\"2026-09-23T14:20:01.0000000Z\",\"event\":\"click\",\"dialog\":\"about\",\"title\":\"About PLS-CADD\",\"button\":1}\n",
        "{\"handle\":1182416,\"text\":\"Report usage on sections that include any of the selected structures and any of the selected circuit labels. | Selection summary:\",\"at\":\"2026-09-23T14:25:06.8442676Z\",\"event\":\"unknown_dialog\",\"controls\":[\"  child 5900348 id=-1 [] vis=True en=True \\u0027Report usage\\u0027\"],\"title\":\"Section Usage\"}\n",
        "{\"at\":\"2026-09-23T14:25:08.0000000Z\",\"event\":\"progress\",\"dialog\":\"progress\",\"title\":\"Computing\",\"status\":\"\"}\n",
        "not json at all\n",
    );

    #[test]
    fn the_latest_journaled_dialog_is_found_below_the_run_folder() {
        let root = scratch("journal");
        std::fs::write(root.join("watch-journal.jsonl"), WATCH_JOURNAL).unwrap();
        std::fs::write(
            root.join("reports").join("journal-40016.jsonl"),
            "{\"at\":\"2026-09-23T13:00:00.0000000Z\",\"event\":\"stop\",\"dialog\":\"repair_wizard\",\"title\":\"PLS-CADD Project Repair Wizard\",\"text\":\"\"}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("deliver.log"),
            "unknown_dialog in a log is not a journal\n",
        )
        .unwrap();

        let folder = [root.to_str().unwrap()];
        let found = latest_dialog_event(&folder, Kind::UnknownDialog).expect("an event");
        assert_eq!(found.title.as_deref(), Some("Section Usage"));
        assert!(found.text.unwrap().starts_with("Report usage on sections"));
        assert!(found.journal.ends_with("watch-journal.jsonl"));
        let stop = latest_dialog_event(&folder, Kind::DialogStop).expect("the stop event");
        assert_eq!(stop.dialog.as_deref(), Some("repair_wizard"));
        assert!(latest_dialog_event(&folder, Kind::Timeout).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Negative control. `pls-report-any.ps1` runs the watcher once per pass
    /// while it saves and ignores the outcome, so a successful step can
    /// journal a dialog event (the catalogued Save As is a `flow` dialog). A
    /// later timeout must stay a timeout, and must not borrow that event.
    #[test]
    fn a_journaled_dialog_never_decides_the_cause_of_a_later_failure() {
        let root = scratch("stale");
        std::fs::write(root.join("watch-journal.jsonl"), WATCH_JOURNAL).unwrap();
        std::fs::write(
            root.join("reports").join("journal-40014.jsonl"),
            "{\"at\":\"2026-09-23T15:00:00.0000000Z\",\"event\":\"flow_dialog\",\"dialog\":\"save_as\",\"title\":\"Save As\"}\n",
        )
        .unwrap();
        let refusal = outcome(
            finished(Some(failed_document(
                "report run ended with 'timeout'; journal: flow_dialog save_as",
            ))),
            &[UNKNOWN_DIALOG, DIALOG_STOP, PLS_CADD_TIMEOUT, DRIVER_FAILED],
            &[root.to_str().unwrap()],
        )
        .unwrap_err();
        assert_eq!(refusal.code(), "pls_cadd_timeout");
        assert!(refusal.detail_value().unwrap().get("dialog").is_none());

        let refusal = outcome(
            finished(Some(failed_document("Section Table still open after OK"))),
            &[UNKNOWN_DIALOG, DIALOG_STOP, PLS_CADD_TIMEOUT, DRIVER_FAILED],
            &[root.to_str().unwrap()],
        )
        .unwrap_err();
        assert_eq!(refusal.code(), "driver_failed");
        std::fs::remove_dir_all(root).unwrap();
    }

    fn failed_document(message: &str) -> Value {
        json!({
            "schema": "ds.pls.desktop_entry.v1",
            "verb": "deliver",
            "status": "failed",
            "message": message,
            "script": "pls-deliver-autosag.ps1",
            "line": 71,
            "pls_cadd_running": [4242],
        })
    }

    fn finished(document: Option<Value>) -> Finished {
        Finished {
            command_line: "-File ds-desktop-deliver.ps1".into(),
            exit_code: Some(1),
            document,
            stdout_tail: String::new(),
            stderr_tail: "boom".into(),
        }
    }

    #[test]
    fn an_unknown_dialog_becomes_a_typed_refusal_carrying_the_dialog() {
        let root = scratch("refusal");
        std::fs::write(root.join("watch-journal.jsonl"), WATCH_JOURNAL).unwrap();
        let folder = root.to_str().unwrap();
        let refusal = outcome(
            finished(Some(failed_document(
                "save (after autosag and paging) did not return to ready: unknown",
            ))),
            &[UNKNOWN_DIALOG, DRIVER_FAILED],
            &[folder],
        )
        .unwrap_err();
        assert_eq!(refusal.code(), "unknown_dialog");
        assert!(
            refusal
                .remedy_text()
                .unwrap()
                .contains("never click through it blind")
        );
        assert!(refusal.message().contains("still open"));
        let detail = refusal.detail_value().unwrap();
        assert_eq!(detail["dialog"]["title"], "Section Usage");
        assert_eq!(detail["pls_cadd_running"], json!([4242]));
        assert_eq!(detail["script"], "pls-deliver-autosag.ps1");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_cause_the_verb_does_not_document_is_reported_as_driver_failed() {
        let refusal = outcome(
            finished(Some(failed_document("Candidate digest mismatch: 12"))),
            &[DRIVER_FAILED],
            &[],
        )
        .unwrap_err();
        assert_eq!(refusal.code(), "driver_failed");
        assert_eq!(
            refusal.detail_value().unwrap()["message"],
            "Candidate digest mismatch: 12"
        );
    }

    #[test]
    fn qualify_s_own_failure_record_is_carried_through() {
        let root = scratch("qualify");
        std::fs::create_dir_all(root.join("evidence")).unwrap();
        // The recorded 2026-09-23 qualifier failure, verbatim in shape.
        std::fs::write(
            root.join("evidence").join("failure.json"),
            "{\r\n    \"schema\":  \"ds.pls.backup_restore_failure.v1\",\r\n    \"failed_at_utc\":  \"2026-09-23T00:04:39.8137918Z\",\r\n    \"message\":  \"Restored member length mismatch: cables\\\\acsr 70-12mm2\",\r\n    \"active_process_id\":  null,\r\n    \"process_left_for_operator\":  false,\r\n    \"destructive_recovery_attempted\":  false\r\n}\r\n",
        )
        .unwrap();
        let refusal = outcome(
            finished(Some(failed_document(
                r"Restored member length mismatch: cables\acsr 70-12mm2",
            ))),
            &[RESTORED_TREE_MISMATCH, DRIVER_FAILED],
            &[root.to_str().unwrap()],
        )
        .unwrap_err();
        assert_eq!(refusal.code(), "restored_tree_mismatch");
        assert_eq!(
            refusal.detail_value().unwrap()["process_left_for_operator"]["process_left_for_operator"],
            false
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_or_statusless_document_is_unreadable_not_guessed() {
        let missing = outcome(finished(None), &[DRIVER_FAILED], &[]).unwrap_err();
        assert_eq!(missing.code(), "driver_result_unreadable");
        assert_eq!(missing.detail_value().unwrap()["stderr"], "boom");
        let statusless = outcome(finished(Some(json!({}))), &[DRIVER_FAILED], &[]).unwrap_err();
        assert_eq!(statusless.code(), "driver_result_unreadable");
        let ok = outcome(
            finished(Some(
                json!({ "status": "ok", "result": { "receipt": "x" } }),
            )),
            &[],
            &[],
        )
        .unwrap();
        assert_eq!(ok["receipt"], "x");
    }
}
