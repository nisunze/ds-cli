//! Which host executes a layer operation — and nothing else.
//!
//! The standing ruling (2026-09-11) is that CLI/MCP → Desktop or Server is no
//! difference at all: one command id, the same arguments, the same answer,
//! whichever host runs it. So `ds map layer list|show|hide|reorder` are the
//! layer drawer's four operations, and `--target` is the one routing decision
//! on top of them. There is deliberately no `ds server layers …`: a second id
//! that differed only by who answered is exactly what that ruling ended.
//!
//! What each target means:
//!
//! * `desktop` (the default) — this machine's own native client, exactly as
//!   before. With no `--project`, its subject is the saved selection.
//! * `desktop:<instance>` — one named Desktop window. Accepted as a shape and
//!   refused by name (`target_instance_unsupported`) so a caller learns the
//!   spelling now and gets a real answer when slice 2 wires it, rather than
//!   discovering the argument does not parse.
//! * `server` — the running `ds server serve` on this machine, over its
//!   protected loopback connection. The project is sent explicitly on every
//!   request, so the Server verifies one named project and reads no selection
//!   of its own.

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};

/// The host this invocation runs against. Declared without `choices` on
/// purpose: `desktop:<instance>` must reach the handler to be refused by its
/// own name instead of by the parser's generic `invalid_choice`.
pub const TARGET_ARG: Arg = Arg::value(
    "target",
    "<desktop|desktop:instance|server>",
    "Which host executes this operation; the desktop is the default.",
)
.default("desktop");

/// The protected state directory of the `--target server` host, and the
/// project every request to it names. Both are `ds-cli-server`'s own
/// declarations, so the two halves of one operation cannot drift apart.
pub const STATE_DIR_ARG: Arg = ds_cli_server::STATE_DIR_ARG;
pub const PROJECT_ARG: Arg = ds_cli_server::PROJECT_ARG;

pub const TARGET_INSTANCE_UNSUPPORTED: Refusal = Refusal {
    code: "target_instance_unsupported",
    when: "--target names one Desktop instance (desktop:<instance>)",
    remedy: "run it with --target desktop; addressing a single window is the next slice",
};
pub const UNKNOWN_TARGET: Refusal = Refusal {
    code: "unknown_target",
    when: "--target is not desktop, desktop:<instance> or server",
    remedy: "pass --target desktop or --target server",
};
/// None of these four operations needs a rendered map — the catalogue, the
/// remembered visibility and the governed order are all decided without one —
/// so none of them declares `needs_paired_map`. That refusal belongs to the
/// map-bound commands the day they take a host; on this host it is declared by
/// `ds server serve`, which is what answers it.
///
/// The rest of what taking `--target` adds to a layer command's declared
/// refusals. Every one of these is the Server's own answer re-raised literally
/// by `ds_cli_server::typed_refusal`, so a caller plans for the same codes and
/// the same remedies whichever host executed the operation.
pub const SERVER_REFUSED: Refusal = Refusal {
    code: "server_refused",
    when: "--target server was passed and the host is unreachable, or it refused the request",
    remedy: "read the stated reason; verify ds server serve is running under this lane",
};
pub const SERVER_OWNER_CHANGED: Refusal = Refusal {
    code: "server_owner_changed",
    when: "the Server's account differs from this caller's, or its credential was revoked",
    remedy: "sign in under the intended Server account and explicitly restart ds server serve",
};
pub const PROJECT_REQUIRED: Refusal = Refusal {
    code: "project_required",
    when: "--target server was passed with no --project and no saved selection to send",
    remedy: "pass --project <exact-id> or run ds auth project use --project <exact-id>",
};
pub const CONTEXT_CORRUPT: Refusal = Refusal {
    code: "context_corrupt",
    when: "a --project id is empty, padded, over 500 characters or holds a control character",
    remedy: "copy one exact ds_project value from ds auth project list",
};
pub const MULTI_PRINCIPAL: Refusal = Refusal {
    code: "multi_principal_unsupported",
    when: "the Server is bound to a different native account than this caller's",
    remedy: "run one Server per native account, each with its own --state-dir and --listen",
};

/// The host, resolved once, before anything is read or sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Desktop,
    Server,
}

/// The desktop host's document source — the target every one of these four
/// commands has always had, and still the default.
///
/// `--project` names the project for THIS call only: it reads that project's
/// document and never rewrites the saved selection or a window's project.
/// Without it the subject is the saved selection, exactly as before.
pub fn desktop_documents(inputs: &Inputs) -> Result<ds_layer_ops::Native, Failure> {
    let lane = inputs.require("lane")?;
    Ok(match inputs.value("project") {
        Some(project) => ds_layer_ops::Native::for_project(lane, project),
        None => ds_layer_ops::Native::new(lane),
    })
}

pub fn resolve(inputs: &Inputs) -> Result<Target, Failure> {
    match inputs.value("target").unwrap_or("desktop") {
        "desktop" => Ok(Target::Desktop),
        "server" => Ok(Target::Server),
        instance if instance.starts_with("desktop:") => Err(Failure::invalid(
            TARGET_INSTANCE_UNSUPPORTED.code,
            format!(
                "this build addresses the desktop as one host, not the instance `{}`",
                instance.trim_start_matches("desktop:")
            ),
        )
        .remedy(TARGET_INSTANCE_UNSUPPORTED.remedy)),
        other => Err(
            Failure::invalid(UNKNOWN_TARGET.code, format!("`{other}` is not a host"))
                .remedy(UNKNOWN_TARGET.remedy),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parsed the way `ds` parses it, against the real declaration, so the
    /// default this reads is the declared one and never a second copy of it.
    fn inputs(target: Option<&str>) -> Inputs {
        let tokens: Vec<String> = match target {
            Some(target) => vec!["--target".to_owned(), target.to_owned()],
            None => vec![],
        };
        ds_cli_contract::parse(&crate::layer::list::COMMAND, &tokens).expect("declared arguments")
    }

    #[test]
    fn the_default_host_is_the_desktop_and_an_instance_is_refused_by_name() {
        assert_eq!(resolve(&inputs(None)).unwrap(), Target::Desktop);
        assert_eq!(resolve(&inputs(Some("desktop"))).unwrap(), Target::Desktop);
        assert_eq!(resolve(&inputs(Some("server"))).unwrap(), Target::Server);

        let instance = resolve(&inputs(Some("desktop:kigali"))).unwrap_err();
        assert_eq!(instance.code(), "target_instance_unsupported");
        assert!(
            instance.message().contains("kigali"),
            "the refusal names the instance asked for: {}",
            instance.message()
        );
        assert_eq!(
            resolve(&inputs(Some("cloud"))).unwrap_err().code(),
            "unknown_target"
        );
    }
}
