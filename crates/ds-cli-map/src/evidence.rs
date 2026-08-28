//! `ds map evidence` — one deterministic frame of the running application.
//!
//! ## What this domain does and does not stage
//!
//! It writes still frames. **Video recording is a third-party tool's job and
//! is not in this CLI at all** — there is no record, no start/stop, no
//! duration and no encoder here, and adding one would be adding a media
//! engine to a repository that owns no engines. What `ds` contributes to a
//! tutorial is the part a screen recorder cannot do well: putting the
//! application into an exactly described state, one command at a time, so the
//! frames a human or a recorder captures are reproducible rather than
//! improvised.
//!
//! A frame is staged with the commands that already exist —
//! [`crate::zoom`] for where the map is looking, [`crate::ui`] for which panel
//! is open, `ds map design select`/`set` for what a property edit looks like —
//! and then captured here.
//!
//! ## Why the receipt is fixed
//!
//! A PNG on disk is not evidence on its own; it is a picture that could have
//! come from anywhere. What makes it evidence is the fixed set of facts
//! written beside it: the exact path, its size and SHA-256, the pixel
//! dimensions, whether the frame is the map or the whole window, where the map
//! was looking, and which panel was open. Those seven are declared once, in
//! [`crate::EVIDENCE_RECEIPT_FIELDS`], and this command returns exactly them.
//!
//! ## Who writes the file
//!
//! The application. The frame exists only inside its webview, so `ds` names an
//! absolute path, the application renders and writes it, and what comes back
//! is the receipt. `ds` validates the path first — absolute, `.png`, in a
//! directory that already exists — so the common mistakes are named locally
//! instead of arriving as an opaque application refusal, and it refuses to
//! overwrite an existing frame unless the caller both asks with `--replace`
//! and confirms with `--yes`.

pub mod capture {
    use std::path::Path;

    use ds_cli_contract::outcome::Failure;
    use ds_cli_contract::spec::{
        Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
    };
    use ds_cli_contract::{Context, Inputs};
    use serde_json::{Value, json};

    use crate::DESCRIPTOR_ARG;

    /// What the frame covers. `map` is the canvas alone, which is what a
    /// cartography or design step wants; `app` is the whole application
    /// window, which is what a step about a panel wants. Held to the adapter's
    /// own `CAPTURE_SCOPES` by `tests/bridge_parity.rs`.
    pub const SCOPES: &[&str] = &["map", "app"];

    /// The cheapest useful default: most evidence is about what is on the map.
    const DEFAULT_SCOPE: &str = "map";

    pub const OUT_INVALID: Refusal = Refusal {
        code: "evidence_out_invalid",
        when: "--out is not an absolute path ending in .png, or its directory does not exist",
        remedy: "pass an absolute .png path inside a directory that already exists",
    };
    pub const ALREADY_EXISTS: Refusal = Refusal {
        code: "evidence_exists",
        when: "--out already names a file and --replace was not given",
        remedy: "choose another --out, or re-run with --replace --yes to overwrite it",
    };
    pub const REPLACE_UNCONFIRMED: Refusal = Refusal {
        code: "confirmation_required",
        when: "--replace was given without --yes",
        remedy: "check which frame you are about to overwrite, then repeat with --yes",
    };

    pub static COMMAND: Command = Command {
        id: "map.evidence.capture",
        path: &["map", "evidence", "capture"],
        contract: 1,
        summary: "Write one PNG frame of the paired map or application window.",
        purpose: "\
Asks the running application to render what is on screen and write it to one \
absolute .png path, then returns a fixed receipt: the file, its size and \
SHA-256, its pixel dimensions, the scope captured, where the map was looking \
and which panel was open. It records no video — that is a third-party tool's \
job — and it stages nothing itself: compose the frame first with `ds map \
zoom`, `ds map ui open` and the design commands, then capture it.",
        chapter: Chapter::Survey,
        effect: Effect::LocalFileWrite,
        authority: Authority::DesktopPairing,
        execution: Execution::Sync,
        args: &[
            Arg::value("out", "<file>", "Absolute path of the .png to write.").required(),
            Arg::value(
                "scope",
                "<scope>",
                "Capture the map canvas or the whole window.",
            )
            .default(DEFAULT_SCOPE)
            .choices(SCOPES),
            Arg::switch(
                "replace",
                "Overwrite --out if it already exists; needs --yes as well.",
            ),
            DESCRIPTOR_ARG,
        ],
        output: "path, bytes, sha256, dimensions, scope, view and ui — the whole receipt, and nothing else.",
        examples: &[
            Example {
                command: "ds map evidence capture --out /evidence/step-3-attribute-table.png --scope app",
                note: "Capture the window after `ds map ui open`.",
                runnable: false,
            },
            Example {
                command: "ds map evidence capture --out /evidence/step-4-map.png --replace --yes --output json",
                note: "Re-shoot one frame of a sequence after changing the view.",
                runnable: false,
            },
        ],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            Refusal {
                code: "desktop_refused",
                when: "the application has no map open, or could not write the frame",
                remedy: "open a project map and check --out is writable; `ds map view` reports whether one is open",
            },
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            OUT_INVALID,
            ALREADY_EXISTS,
            REPLACE_UNCONFIRMED,
        ],
        reference: Some("docs/reference/map.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
        let out = inputs.require("out")?;
        let scope = inputs.value("scope").unwrap_or(DEFAULT_SCOPE);
        let replace = inputs.switch("replace");
        let path = destination(out, replace, context.confirmed)?;

        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        let captured = crate::invoke(
            &descriptor,
            &crate::EVIDENCE_CAPTURE,
            json!({ "scope": scope, "path": path, "replace": replace }),
            crate::EVIDENCE_TIMEOUT,
        )?;
        Ok(receipt(&captured))
    }

    /// Check the destination before anything is rendered, and settle the
    /// overwrite question here.
    ///
    /// The gate is on `--replace` itself rather than on the file existing.
    /// Keying it to existence would mean the same invocation needs `--yes` or
    /// does not depending on what is on disk at that instant, which is both a
    /// race and a surprise; `--replace` is the caller stating an intention, and
    /// stating it is what has to be confirmed.
    fn destination(raw: &str, replace: bool, confirmed: bool) -> Result<String, Failure> {
        let refuse = |message: &str| {
            Failure::invalid("evidence_out_invalid", message.to_string())
                .remedy(OUT_INVALID.remedy)
                .detail(json!({ "given": raw }))
        };
        let path = Path::new(raw);
        if !path.is_absolute() {
            return Err(refuse(
                "--out must be absolute; the application writes the file, not this process's directory",
            ));
        }
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        {
            return Err(refuse(
                "--out must end in .png; this command writes one PNG",
            ));
        }
        match path.parent() {
            Some(parent) if parent.is_dir() => {}
            _ => {
                return Err(refuse(
                    "--out is inside a directory that does not exist; this command creates files, not trees",
                ));
            }
        }

        if replace && !confirmed {
            return Err(
                Failure::invalid("confirmation_required", "--replace overwrites a frame")
                    .remedy(REPLACE_UNCONFIRMED.remedy)
                    .next("ds map evidence capture --help"),
            );
        }
        if path.exists() && !replace {
            return Err(
                Failure::conflict("evidence_exists", format!("`{raw}` already exists"))
                    .remedy(ALREADY_EXISTS.remedy),
            );
        }
        Ok(raw.to_string())
    }

    /// The whole receipt, and nothing else.
    ///
    /// Built by walking the declared pairs rather than writing the keys inline,
    /// so "these seven fields" is one table the tests below, the parity suite
    /// and the docs all read, instead of a claim made in four places. The one
    /// field that is not a straight copy is `dimensions`: the application
    /// publishes the frame size as two top-level numbers, and they are read as
    /// a pair, so they are reported as a pair.
    fn receipt(captured: &Value) -> Value {
        let mut data = json!({});
        for (reported, returned) in crate::EVIDENCE_RECEIPT_FIELDS {
            data[*reported] = captured[*returned].clone();
        }
        data["dimensions"] = json!({
            "width": captured[crate::EVIDENCE_WIDTH],
            "height": captured[crate::EVIDENCE_HEIGHT],
        });
        data
    }

    pub fn render(data: &Value) -> String {
        let dimensions = &data["dimensions"];
        format!(
            "{} frame written\n  {}\n  {} × {} px · {} bytes\n  sha256 {}\n",
            data["scope"].as_str().unwrap_or("map"),
            data["path"].as_str().unwrap_or("?"),
            dimensions["width"],
            dimensions["height"],
            data["bytes"],
            data["sha256"].as_str().unwrap_or("?"),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::collections::BTreeSet;

        fn temp_dir() -> std::path::PathBuf {
            let dir = std::env::temp_dir().join("ds-map-evidence-tests");
            std::fs::create_dir_all(&dir).expect("temp directory is writable");
            dir
        }

        #[test]
        fn a_destination_is_checked_before_the_application_renders_anything() {
            let dir = temp_dir();
            let fresh = dir.join("fresh.png");
            let _ = std::fs::remove_file(&fresh);
            let fresh = fresh.display().to_string();

            assert_eq!(
                destination(&fresh, false, false).expect("a fresh absolute png"),
                fresh
            );

            // Relative, wrong extension, and a directory that is not there.
            // Each would otherwise reach the application and come back as an
            // opaque `desktop_refused` after a full render.
            for bad in [
                "step-1.png",
                "./evidence/step-1.png",
                dir.join("step-1.jpg").display().to_string().as_str(),
                dir.join("step-1").display().to_string().as_str(),
                dir.join("no-such-dir/step-1.png")
                    .display()
                    .to_string()
                    .as_str(),
            ] {
                assert_eq!(
                    destination(bad, false, false)
                        .expect_err("must refuse")
                        .code(),
                    "evidence_out_invalid",
                    "`{bad}` was accepted as a capture destination"
                );
            }

            // Uppercase is still a PNG. Refusing it would be a rule about
            // spelling rather than about file type.
            let shouty = dir.join("step-1.PNG").display().to_string();
            let _ = std::fs::remove_file(&shouty);
            assert!(destination(&shouty, false, false).is_ok());
        }

        #[test]
        fn overwriting_an_existing_frame_takes_both_replace_and_yes() {
            let dir = temp_dir();
            let existing = dir.join("existing.png");
            std::fs::write(&existing, b"not really a png").expect("temp file is writable");
            let existing = existing.display().to_string();

            // Present, unasked: refused, and named as a conflict rather than a
            // bad path — the remedy is different.
            assert_eq!(
                destination(&existing, false, false)
                    .expect_err("must refuse")
                    .code(),
                "evidence_exists"
            );
            // Asked, unconfirmed: the gate. `local_file_write` is not in the
            // dispatch confirmation set, so this is the only thing standing
            // between a model and an overwritten frame.
            assert_eq!(
                destination(&existing, true, false)
                    .expect_err("must refuse")
                    .code(),
                "confirmation_required"
            );
            // Asked and confirmed.
            assert!(destination(&existing, true, true).is_ok());

            // The gate is on the intention, not on what happens to be on disk,
            // so it holds for a path that does not exist yet too.
            let fresh = dir.join("fresh-replace.png");
            let _ = std::fs::remove_file(&fresh);
            assert_eq!(
                destination(&fresh.display().to_string(), true, false)
                    .expect_err("must refuse")
                    .code(),
                "confirmation_required"
            );
        }

        #[test]
        fn the_receipt_is_exactly_seven_fields_taken_from_the_application() {
            // A receipt that quietly grew a field would change what "evidence"
            // means for every consumer downstream, and one that silently lost
            // the digest would leave a picture with nothing tying it to a run.
            //
            // The reply below is the application's own shape: the native
            // capture receipt spread flat — `width` and `height` as two
            // top-level numbers — plus the scope, view and UI state the
            // frontend adds.
            let captured = json!({
                "path": "/evidence/step-3.png",
                "bytes": 184_320,
                "sha256": "3b1f",
                "width": 1600,
                "height": 900,
                "scope": "app",
                "view": { "center": [30.06, -1.94], "zoom": 14.5, "bbox": [29.9, -2.1, 30.2, -1.85] },
                "ui": { "styleCenter": { "open": true, "styleRef": "master/customers" } },
                // Anything else the application chooses to return stays out.
                "elapsedMs": 812,
                "temporaryPath": "/tmp/whatever.png"
            });
            let receipt = receipt(&captured);
            let keys: BTreeSet<&str> = receipt
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                crate::EVIDENCE_RECEIPT_KEYS
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>(),
                "the evidence receipt is a fixed projection, not whatever the application returned"
            );
            assert_eq!(receipt["sha256"], "3b1f");
            assert_eq!(receipt["ui"]["styleCenter"]["open"], true);
            assert_eq!(receipt["view"]["zoom"], 14.5);

            // The one shape change: two published numbers read as one pair.
            assert_eq!(
                receipt["dimensions"],
                json!({ "width": 1600, "height": 900 })
            );
            assert!(
                render(&receipt).contains("1600 × 900 px"),
                "the frame size a person reads comes from the same pair"
            );
        }

        #[test]
        fn nothing_here_offers_a_recording() {
            // Video is a third-party tool's job. This asserts the absence
            // structurally rather than in prose: the operation carries three
            // keys, none of which could start, stop or bound a recording, and
            // the command declares no flag that could either.
            assert_eq!(
                crate::EVIDENCE_CAPTURE.arguments,
                &["scope", "path", "replace"]
            );
            for forbidden in ["record", "video", "duration", "fps", "seconds", "stop"] {
                assert!(
                    !crate::EVIDENCE_CAPTURE
                        .arguments
                        .iter()
                        .any(|argument| argument.contains(forbidden)),
                    "`{forbidden}` reads as a recording argument"
                );
                assert!(
                    COMMAND.arg(forbidden).is_none(),
                    "`--{forbidden}` would make this a recorder"
                );
            }
        }
    }
}
