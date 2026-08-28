//! `ds map ui` — put one named panel of the running application on screen.
//!
//! This exists for one reason: a screenshot is only evidence if the thing it
//! is meant to show is visible. Navigating to the right extent is already
//! `ds map zoom`, and changing a property is already `ds map design set`; what
//! neither can do is open the attribute table over the layer being discussed,
//! or bring up the Style Center a tutorial step is about to describe.
//!
//! ## What this is not
//!
//! It is not a driver. There is no selector, no click, no key, no script and
//! no coordinate here, and there is no room for one: the operation declares
//! exactly two argument keys, `target` and `ref`, and `target` is a closed set
//! of three panels the application already publishes. A caller cannot ask for
//! "the third button in the legend", because the CLI has no vocabulary for
//! saying it and the bridge would refuse the key if it did.
//!
//! `--ref` is held to the same idea locally. It must read as a name the
//! application publishes — a style ref, a local layer id, a feature id — and a
//! CSS selector, a URL or a filesystem path is refused here rather than sent
//! and misunderstood. That check is what keeps this from becoming the generic
//! UI-automation door `CLAUDE.md` forbids.

pub mod open {
    use ds_cli_contract::outcome::Failure;
    use ds_cli_contract::spec::{
        Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
    };
    use ds_cli_contract::{Context, Inputs};
    use serde_json::{Value, json};

    use crate::DESCRIPTOR_ARG;

    /// The panels the application publishes to the CLI. A closed set, because
    /// the alternative to a closed set is a selector. Held to the adapter's own
    /// `UI_TARGETS` by `tests/bridge_parity.rs`.
    pub const TARGETS: &[&str] = &["attribute-table", "style-center", "selection-properties"];

    /// The application's own bound on a reference, hand-copied from the
    /// `exactText` default the adapter validates `ref` with, and checked
    /// against it by the parity test. Enforced here as well so an accidental
    /// paste is refused before it is sent, not after.
    pub const MAX_REF_CHARS: usize = 512;

    pub const REF_NOT_SEMANTIC: Refusal = Refusal {
        code: "ui_ref_not_semantic",
        when: "--ref is empty, over its length bound, carries a URL scheme, does not start with a letter or digit, or uses characters outside letters, digits and ._:/-",
        remedy: "pass a name the application published: a ref from `ds style list`, a layer from `ds map view`, or a feature id from `ds map design select`",
    };

    pub static COMMAND: Command = Command {
        id: "map.ui.open",
        path: &["map", "ui", "open"],
        contract: 1,
        summary: "Open one named panel of the paired application over a ref.",
        purpose: "\
Asks the running application to show one of three panels it publishes — the \
attribute table, the Style Center, or selection properties — over the thing \
--ref names. This is how a frame is staged before `ds map evidence capture`, \
and it is the whole of what it does: there is no selector, click, keystroke or \
script here, only a closed target and a reference the application already \
published. Navigate with `ds map zoom`; edit with `ds map design set`.",
        chapter: Chapter::Survey,
        effect: Effect::LocalUi,
        authority: Authority::DesktopPairing,
        execution: Execution::Sync,
        args: &[
            Arg::value("target", "<panel>", "Which published panel to open.")
                .required()
                .choices(TARGETS),
            Arg::value(
                "ref",
                "<ref>",
                "What the panel opens over: a style ref or layer id, or a feature id for selection-properties.",
            )
            .required(),
            DESCRIPTOR_ARG,
        ],
        output: "The target, the ref given, the ref the application resolved it to, and whether the panel opened.",
        examples: &[
            Example {
                command: "ds map ui open --target attribute-table --ref master/customers",
                note: "Show the rows behind a layer before capturing it.",
                runnable: false,
            },
            Example {
                command: "ds map ui open --target style-center --ref master/lv_lines --output json",
                note: "Stage the panel a styling tutorial step describes.",
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
                when: "the application has no map open, or nothing matches --ref",
                remedy: "open a project map; `ds map view` and `ds style list` report what a ref may name",
            },
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            REF_NOT_SEMANTIC,
        ],
        reference: Some("docs/reference/map.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
        let target = inputs.require("target")?;
        let reference = semantic_ref(inputs.require("ref")?)?;

        let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
        let opened = crate::invoke(
            &descriptor,
            &crate::UI_OPEN,
            json!({ "target": target, "ref": reference }),
            crate::UI_TIMEOUT,
        )?;
        Ok(reply(&opened))
    }

    /// What the application answered, projected through the declared pairs so
    /// a renamed field fails the parity test rather than quietly reporting
    /// `null`.
    fn reply(opened: &Value) -> Value {
        let mut data = json!({});
        for (reported, published) in crate::UI_OPEN_REPLY_FIELDS {
            data[*reported] = opened[*published].clone();
        }
        data
    }

    /// Hold `--ref` to the shape of a published name.
    ///
    /// The point is not sanitisation — the application validates its own
    /// references and would refuse an unknown one anyway. The point is that
    /// this command must never become a way to address the interface by
    /// structure. A value that is a selector, a URL or a path is refused here,
    /// with a remedy naming the three commands that hand out real references,
    /// rather than travelling to the application as if it might mean something.
    pub fn semantic_ref(raw: &str) -> Result<&str, Failure> {
        let refuse = |message: &str| {
            Failure::invalid("ui_ref_not_semantic", message.to_string())
                .remedy(REF_NOT_SEMANTIC.remedy)
                .detail(json!({ "given": raw.chars().take(80).collect::<String>() }))
        };
        if raw.is_empty() {
            return Err(refuse("--ref is empty"));
        }
        if raw.chars().count() > MAX_REF_CHARS {
            return Err(refuse(
                "--ref is longer than any reference the application publishes",
            ));
        }
        if raw.contains("://") {
            return Err(refuse(
                "--ref carries a URL scheme; this command opens a panel, not a page",
            ));
        }
        if !raw.starts_with(|character: char| character.is_ascii_alphanumeric()) {
            return Err(refuse(
                "--ref must start with a letter or digit; a selector, flag or path does not",
            ));
        }
        if !raw
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._:/-".contains(character))
        {
            return Err(refuse(
                "--ref may hold only letters, digits and ._:/- ; a selector or expression may not",
            ));
        }
        Ok(raw)
    }

    pub fn render(data: &Value) -> String {
        let given = data["ref"].as_str().unwrap_or("?");
        let resolved = data["resolved_ref"].as_str().unwrap_or(given);
        let mut out = format!(
            "{} {} over {resolved}\n",
            data["target"].as_str().unwrap_or("panel"),
            if data["opened"].as_bool().unwrap_or(false) {
                "open"
            } else {
                "not open"
            },
        );
        if resolved != given {
            out.push_str(&format!("  {given} resolved to {resolved}\n"));
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_reference_is_a_published_name_and_never_a_way_to_address_the_interface() {
            // The references the application actually hands out. Every one of
            // these must survive, or the command is unusable for the panels it
            // exists to open.
            for good in [
                "master/customers",
                "sketch-1f3a",
                "sketch:sketch-1f3a",
                "lv:lv_lines",
                "live/edcl_customers_survey",
                "4821",
                "TX-1042.lv_lines/17",
            ] {
                assert_eq!(
                    semantic_ref(good).expect("a published reference"),
                    good,
                    "`{good}` is a reference this domain hands out and must be accepted"
                );
            }

            // The shapes that would make this a UI driver rather than a way to
            // name a thing. Each is refused before the bridge is opened.
            for bad in [
                "",
                "#attribute-table",
                ".legend-row",
                "div > span",
                "[data-layer='customers']",
                "javascript:alert(1)",
                "http://127.0.0.1/panel",
                "/home/operator/frame.png",
                "customers'); drop table",
                "click(3, 400)",
            ] {
                assert_eq!(
                    semantic_ref(bad).expect_err("must refuse").code(),
                    "ui_ref_not_semantic",
                    "`{bad}` was accepted as a semantic reference"
                );
            }

            let long = "a".repeat(MAX_REF_CHARS + 1);
            assert_eq!(
                semantic_ref(&long).expect_err("over bound").code(),
                "ui_ref_not_semantic"
            );
        }

        #[test]
        fn the_reply_reports_what_the_application_resolved_the_ref_to() {
            // `resolvedRef` is the reason this command answers at all rather
            // than just succeeding: `master/customers` may be a layer id that
            // resolves to a style key, and a sketch layer resolves to a
            // `sketch-` tab id. A caller that captured the given ref instead
            // would describe the wrong thing in its own tutorial step.
            let opened = json!({
                "target": "attribute-table",
                "ref": "customers",
                "resolvedRef": "sketch-1f3a",
                "opened": true,
                "ui": { "attributeTable": { "open": true } }
            });
            let reply = reply(&opened);
            assert_eq!(
                reply,
                json!({
                    "target": "attribute-table",
                    "ref": "customers",
                    "resolved_ref": "sketch-1f3a",
                    "opened": true
                }),
                "the reply is the declared projection, not whatever the application returned"
            );
            assert!(
                render(&reply).contains("customers resolved to sketch-1f3a"),
                "a resolution a caller has to reuse must be visible without --output json"
            );
        }

        #[test]
        fn the_declared_targets_are_the_only_ones_the_operation_can_carry() {
            // `choices` is enforced by the parser, so this pins the other half:
            // the closed set is three panels, and the operation declares only
            // the two keys that carry them. A fourth target or a third key is a
            // deliberate change, not a drift.
            assert_eq!(TARGETS.len(), 3);
            assert_eq!(crate::UI_OPEN.arguments, &["target", "ref"]);
            assert_eq!(COMMAND.arg("target").expect("declared").choices, TARGETS);
        }
    }
}
