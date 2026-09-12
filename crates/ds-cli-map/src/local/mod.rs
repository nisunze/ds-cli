//! `ds map local` — the prepared local layers this machine keeps for itself.
//!
//! A prepared local layer is a catalogue row on the operator's own disk: a
//! name, an origin, a geometry type, a style, a column schema, a feature count
//! and a copied payload beside it. The browser keeps its catalogue in
//! IndexedDB; `ds` and the installed desktop keep theirs in a file under the
//! shared native layer root, one per lane and DS account.
//!
//! Every decision belongs to the shared kernel
//! (`ds-command-kernel::local_layers`) and every file belongs to
//! `ds-layer-store::prepared`. These four commands carry inputs in and a
//! receipt out; they mint nothing, default nothing and delete nothing the
//! receipt did not name. Refusals keep the kernel's own codes, so a caller
//! plans for the same words here, in the desktop and in the browser.

pub mod list;
pub mod register;
pub mod remove;
pub mod rename;

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Availability, Refusal};
use ds_layer_store::prepared::{Host, Scope, StoreError};
use serde_json::{Value, json};

pub use crate::layer::native::LANE_ARG;

pub const ACCOUNT_ARG: Arg = Arg::value(
    "account",
    "<uid>",
    "DS account this catalogue belongs to; `local` is the machine's own.",
)
.default("local");

pub const LAYER_ARG: Arg = Arg::value("layer", "<id>", "Id from `ds map local list`.");

pub const STORE_REFUSED: Refusal = Refusal {
    code: "local_layer_refused",
    when: "the catalogue file cannot be read or persisted, or is malformed",
    remedy: "repair the store under DS_LAYER_HOME (or the local data directory); it is never overwritten for you",
};
pub const INVALID_PAYLOAD: Refusal = Refusal {
    code: "invalid_payload",
    when: "--file is not a readable bounded GeoJSON FeatureCollection of the declared geometry",
    remedy: "pass one FeatureCollection under 64 MiB whose features all carry the --geometry type",
};
pub const MALFORMED_DESCRIPTOR: Refusal = Refusal {
    code: "malformed_descriptor",
    when: "the kernel refused a name, a bound or a row already in the catalogue",
    remedy: "use a trimmed name of 1 to 200 characters; repair a row the message names",
};
pub const DUPLICATE_LAYER: Refusal = Refusal {
    code: "duplicate_layer",
    when: "the catalogue holds one id twice",
    remedy: "remove the repeated row from the catalogue file; ids are never reassigned for you",
};
pub const UNKNOWN_LAYER: Refusal = Refusal {
    code: "unknown_layer",
    when: "--layer is not an id in this lane and account's catalogue",
    remedy: "copy the id from ds map local list --output json",
};
pub const SCOPE_MISMATCH: Refusal = Refusal {
    code: "scope_mismatch",
    when: "the catalogue found under this lane and account records a different one",
    remedy: "run the command under the lane and account the catalogue names, or move the file",
};
pub const UNSUPPORTED_SOURCE_KIND: Refusal = Refusal {
    code: "unsupported_source_kind",
    when: "a row in the catalogue is a Project Work root, which never persists",
    remedy: "delete that row; Project Work is an ephemeral projection of governed entities",
};
pub const CONFIRMATION_REQUIRED: Refusal = Refusal {
    code: "confirmation_required",
    when: "--yes was not supplied to a removal",
    remedy: "review ds map local list, then repeat the command with --yes",
};

/// What every op can refuse regardless of which one it is: a store that cannot
/// be read, and the three faults a catalogue file itself can carry.
pub const STORE_REFUSALS: &[Refusal] = &[
    STORE_REFUSED,
    MALFORMED_DESCRIPTOR,
    DUPLICATE_LAYER,
    SCOPE_MISMATCH,
    UNSUPPORTED_SOURCE_KIND,
];

/// Reading and writing a file on this machine needs no principal and no map.
pub fn availability() -> Availability {
    Availability::Available
}

/// Which catalogue this invocation is about. A lane and an account, because
/// two lanes and two signed-in people on one machine never share one.
pub fn scope(inputs: &Inputs) -> Result<Scope, Failure> {
    Ok(Scope {
        host: Host::Native,
        lane: Some(inputs.require("lane")?.trim().to_owned()),
        uid: Some(inputs.require("account")?.trim().to_owned()),
    })
}

/// Re-raise the kernel's refusal under its own name.
///
/// The vocabulary is closed and shared with the browser and the desktop, so a
/// caller that has planned for `unknown_layer` once has planned for it
/// everywhere. Nothing is renamed into a local word here.
pub fn refuse(error: StoreError) -> Failure {
    match error {
        StoreError::Refused { code, message } => match code.as_str() {
            "malformed_descriptor" => Failure::invalid("malformed_descriptor", message)
                .remedy(MALFORMED_DESCRIPTOR.remedy),
            "duplicate_layer" => {
                Failure::invalid("duplicate_layer", message).remedy(DUPLICATE_LAYER.remedy)
            }
            "unknown_layer" => {
                Failure::invalid("unknown_layer", message).remedy(UNKNOWN_LAYER.remedy)
            }
            "scope_mismatch" => {
                Failure::invalid("scope_mismatch", message).remedy(SCOPE_MISMATCH.remedy)
            }
            "project_context_changed" => Failure::invalid("project_context_changed", message)
                .remedy("run the command again under the project the layer belongs to"),
            "unsupported_source_kind" => Failure::invalid("unsupported_source_kind", message)
                .remedy(UNSUPPORTED_SOURCE_KIND.remedy),
            _ => Failure::invalid("local_layer_refused", message).remedy(STORE_REFUSED.remedy),
        },
        StoreError::Payload(message) => {
            Failure::invalid("invalid_payload", message).remedy(INVALID_PAYLOAD.remedy)
        }
        StoreError::Store(message) => {
            Failure::invalid("local_layer_refused", message).remedy(STORE_REFUSED.remedy)
        }
    }
}

/// One catalogue row, in this CLI's own spelling.
pub fn row(row: &Value) -> Value {
    let mut out = json!({
        "layer": row["id"].clone(),
        "name": row["name"].clone(),
        "source_name": row["sourceName"].clone(),
        "source_layer_name": row["sourceLayerName"].clone(),
        "source_kind": row["sourceKind"].clone(),
        "geometry_type": row["geometryType"].clone(),
        "feature_count": row["featureCount"].clone(),
        "visible": row["visible"].clone(),
        "created_at": row["createdAt"].clone(),
    });
    if let Some(origin) = row.get("origin").filter(|origin| !origin.is_null()) {
        out["origin"] = json!({
            "kind": origin["kind"].clone(),
            "label": origin["label"].clone(),
        });
        if let Some(project) = origin.get("dsProject") {
            out["origin"]["project"] = project.clone();
        }
    }
    out
}

/// The scope and where it was persisted, said the same way by all four.
pub fn stamped(scope: &Scope, answer: &Value, mut data: Value) -> Value {
    data["lane"] = json!(scope.lane);
    data["account"] = json!(scope.uid);
    data["persisted"] = answer["persisted"].clone();
    data["revision"] = answer["revision"].clone();
    data["changed"] = answer["changed"].clone();
    data
}

pub fn render_row(row: &Value) -> String {
    format!(
        "{:<26} {:<11} {:>7} {:<8} {}\n",
        row["layer"].as_str().unwrap_or("?"),
        row["geometry_type"].as_str().unwrap_or("?"),
        row["feature_count"],
        if row["visible"].as_bool().unwrap_or(false) {
            "visible"
        } else {
            "hidden"
        },
        row["name"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::spec::{Authority, Chapter, Command, Effect};
    use ds_cli_contract::{Context, Format, Output};
    use std::collections::BTreeSet;

    const COMMANDS: &[&Command] = &[
        &list::COMMAND,
        &register::COMMAND,
        &rename::COMMAND,
        &remove::COMMAND,
    ];

    fn parse(command: &'static Command, tokens: &[&str]) -> Inputs {
        let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
        ds_cli_contract::parse(command, &tokens).expect("declared inputs")
    }

    #[test]
    fn a_kernel_refusal_keeps_the_kernel_s_own_code() {
        // The vocabulary is closed and shared with the browser and the
        // desktop. Renaming one here would mean an agent that planned for
        // `unknown_layer` has to learn a second word for the same fact.
        for code in [
            "malformed_descriptor",
            "duplicate_layer",
            "unknown_layer",
            "scope_mismatch",
            "project_context_changed",
            "unsupported_source_kind",
        ] {
            let failure = refuse(StoreError::Refused {
                code: code.into(),
                message: "why".into(),
            });
            assert_eq!(failure.code(), code);
            assert_eq!(failure.message(), "why");
        }
        // A code this build has never heard of is still a refusal, not a
        // panic and not a success.
        assert_eq!(
            refuse(StoreError::Refused {
                code: "from_a_newer_kernel".into(),
                message: "why".into(),
            })
            .code(),
            "local_layer_refused"
        );
        assert_eq!(
            refuse(StoreError::Payload("not geojson".into())).code(),
            "invalid_payload"
        );
        assert_eq!(
            refuse(StoreError::Store("unreadable".into())).code(),
            "local_layer_refused"
        );
    }

    #[test]
    fn every_refusal_a_command_can_raise_is_one_it_declares() {
        // The per-command lists are what a caller reads before it runs
        // anything; the shared set is what any catalogue file can carry.
        for command in COMMANDS {
            let declared: BTreeSet<&str> = command
                .refusals
                .iter()
                .map(|refusal| refusal.code)
                .collect();
            for shared in STORE_REFUSALS {
                assert!(
                    declared.contains(shared.code),
                    "`{}` does not declare `{}`",
                    command.id,
                    shared.code
                );
            }
        }
        let declared = |command: &Command| -> BTreeSet<&str> {
            command.refusals.iter().map(|r| r.code).collect()
        };
        assert!(declared(&register::COMMAND).contains("invalid_payload"));
        assert!(declared(&remove::COMMAND).contains("confirmation_required"));
        assert!(declared(&remove::COMMAND).contains("unknown_layer"));
        assert!(declared(&rename::COMMAND).contains("unknown_layer"));
        // A listing cannot name a layer it was not given, so it does not
        // advertise a refusal a caller could never see.
        assert!(!declared(&list::COMMAND).contains("unknown_layer"));
    }

    #[test]
    fn the_family_is_machine_local_and_needs_no_principal() {
        for command in COMMANDS {
            assert_eq!(command.authority, Authority::None, "{}", command.id);
            assert_eq!(command.chapter, Chapter::Survey, "{}", command.id);
            assert!(
                matches!((command.availability)(), Availability::Available),
                "`{}` is gated on something a local file is not",
                command.id
            );
            assert!(
                command.purpose.contains("achine-local"),
                "`{}` does not say it is machine-local",
                command.id
            );
        }
        assert_eq!(list::COMMAND.effect, Effect::ReadOnly);
        for command in [&register::COMMAND, &rename::COMMAND, &remove::COMMAND] {
            assert_eq!(command.effect, Effect::LocalFileWrite, "{}", command.id);
        }
        // The one thing a caller must not assume: a Server on another host
        // cannot read the path `--file` names.
        assert!(
            register::COMMAND
                .purpose
                .contains("a remote Server never reads a client path")
        );
    }

    #[test]
    fn a_removal_is_refused_until_it_is_confirmed() {
        // `local_file_write` is not a gated effect class, so the gate is this
        // command's own and has to be proved here rather than assumed from the
        // declaration.
        let inputs = parse(&remove::COMMAND, &["--layer", "sketch-1"]);
        let unconfirmed = Context {
            confirmed: false,
            output: Output::resolve(Format::Json, false, true),
        };
        let failure = remove::run(&inputs, &unconfirmed).expect_err("must refuse");
        assert_eq!(failure.code(), "confirmation_required");
        assert!(!remove::COMMAND.effect.needs_confirmation());
        assert!(
            remove::COMMAND
                .examples
                .iter()
                .any(|example| example.command.contains("--yes"))
        );
    }

    #[test]
    fn the_scope_is_a_lane_and_an_account_with_defaults_that_need_no_sign_in() {
        let default = scope(&parse(&list::COMMAND, &[])).expect("declared defaults");
        assert_eq!(default.host, Host::Native);
        assert_eq!(default.lane.as_deref(), Some("stable"));
        assert_eq!(default.uid.as_deref(), Some("local"));
        let named = scope(&parse(
            &list::COMMAND,
            &["--lane", "canary", "--account", "uid-7"],
        ))
        .expect("named scope");
        assert_eq!(named.lane.as_deref(), Some("canary"));
        assert_eq!(named.uid.as_deref(), Some("uid-7"));
    }

    #[test]
    fn a_listed_row_carries_the_origin_and_never_a_capability() {
        // The kernel already bounds what a row says; this proves the CLI does
        // not widen it on the way out.
        let projected = row(&json!({
            "id": "sketch-1",
            "name": "Access road",
            "sourceName": "roads.zip",
            "sourceLayerName": "Access road",
            "sourceKind": "import",
            "geometryType": "LineString",
            "featureCount": 12,
            "visible": true,
            "createdAt": 1_758_000_000_000i64,
            "origin": {"kind": "gis_folder", "label": "gis/roads.geojson", "dsProject": "p-kigali"},
        }));
        let keys: BTreeSet<&str> = projected
            .as_object()
            .expect("a row is an object")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "layer",
                "name",
                "source_name",
                "source_layer_name",
                "source_kind",
                "geometry_type",
                "feature_count",
                "visible",
                "created_at",
                "origin"
            ])
        );
        assert_eq!(projected["origin"]["project"], "p-kigali");
        let text = serde_json::to_string(&projected).unwrap();
        for secret in ["handleKey", "handle_key", "color", "schema", "styleDoc"] {
            assert!(!text.contains(secret), "a listing leaked {secret}");
        }
    }

    #[test]
    fn the_geometry_choices_are_the_ones_the_store_can_read() {
        let geometry = register::COMMAND
            .arg("geometry")
            .expect("a declared geometry");
        assert!(geometry.required);
        for word in geometry.choices {
            assert!(
                ds_layer_store::prepared::geometry_type(word).is_some(),
                "`{word}` is offered but the store cannot read it"
            );
        }
        assert_eq!(geometry.choices.len(), 3);
    }
}
