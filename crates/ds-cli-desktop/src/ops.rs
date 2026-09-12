//! The declared wire contract every paired-application domain shares.
//!
//! `ds map` established the shape and `ds work` needed the same one, so it
//! lives here rather than being written twice: an operation is a value, its
//! argument keys are declared beside it, and [`invoke`] refuses to send a key
//! the operation does not declare. Combined with each domain's parity test —
//! which proves the declaration matches the application's own input schema —
//! a misspelled field is caught inside `ds`, at the boundary, instead of
//! arriving as a validation error from a webview.
//!
//! Nothing domain-specific belongs here. A bound, a marker or a projection
//! field that only one domain reads stays in that domain, where its parity
//! test can hold it to the application's source.

use std::cell::RefCell;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Authority, Availability, Refusal};
use ds_command_kernel::desktop_instance as kernel;
use ds_command_kernel::project_context::{
    self, Context as ProjectContext, Requirement, Route, Switch,
};
use serde_json::{Value, json};

use crate::bridge;
use crate::discover::Descriptor;

thread_local! {
    static HEADLESS_IDENTITY: RefCell<Option<HeadlessIdentity>> = const { RefCell::new(None) };
}

/// One non-secret protected-provider observation scoped by registry dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessIdentity {
    pub uid: String,
    pub lane: String,
    pub credential_audience_sha256: String,
    pub project: Option<String>,
    /// The command contract whose invocation this observation fences.
    /// DesktopUser proves user identity only; Project additionally narrows
    /// which live instance may serve the work — it never moves a live map.
    pub command_authority: Authority,
    /// The host this invocation named: `desktop`, `desktop:<instance_id>` or
    /// `server`, exactly as `--target` (or `DS_TARGET`) spelled it. Carried
    /// beside the identity because dispatch is the one place that has both the
    /// command's declared inputs and the seam that will use them.
    pub target: Option<String>,
}

pub struct HeadlessIdentityGuard(Option<HeadlessIdentity>);

pub fn scope_headless_identity(identity: Option<HeadlessIdentity>) -> HeadlessIdentityGuard {
    let previous = HEADLESS_IDENTITY.replace(identity);
    HeadlessIdentityGuard(previous)
}

impl Drop for HeadlessIdentityGuard {
    fn drop(&mut self) {
        HEADLESS_IDENTITY.replace(self.0.take());
    }
}

/// One bridge operation and the exact argument keys a domain may send it.
///
/// A key written as `settings.intervalM` declares a nested key inside the
/// `settings` object — which is where the dangerous hand copies live, because
/// the application's own settings are camelCase and CLI flags are not.
pub struct BridgeOp {
    pub operation: &'static str,
    pub arguments: &'static [&'static str],
}

/// Resolve the instance this operation is for: the descriptor a caller named,
/// or the one live instance the kernel selects for the host `--target` (or
/// `DS_TARGET`) named.
pub fn paired(explicit: Option<&str>) -> Result<Descriptor, Failure> {
    Ok(bridge::paired(explicit)?.descriptor)
}

/// The host `--target` named for this dispatch, as text. `DS_TARGET` is its
/// default and never its override: a command that named a host has named it.
pub fn scoped_target() -> Option<String> {
    HEADLESS_IDENTITY
        .with(|headless| headless.borrow().as_ref().and_then(|h| h.target.clone()))
        .or_else(|| {
            std::env::var(TARGET_ENV)
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
}

/// What this operation needs of the instance that serves it: the caller's own
/// identity, and the project the work is about when it is about one.
///
/// `None` when nothing has told `ds` who is running it — no native profile is
/// configured on this machine — and the live instances' own identity is then
/// what selection adopts.
pub fn scoped_requirement() -> Option<kernel::Requirement> {
    HEADLESS_IDENTITY.with(|headless| {
        headless
            .borrow()
            .as_ref()
            .map(|identity| kernel::Requirement {
                lane: identity.lane.clone(),
                uid: identity.uid.clone(),
                audience_sha256: identity.credential_audience_sha256.clone(),
                project: identity.project.clone(),
                // A project narrows which instance may serve project work. Every
                // other paired operation runs wherever this account is signed in;
                // the project that instance happens to show is not a condition.
                project_independent: identity.command_authority != Authority::Project,
            })
    })
}

/// Send one declared operation.
///
/// The guard is the point: an argument key the operation does not declare
/// never leaves this process.
pub fn invoke(
    descriptor: &Descriptor,
    op: &BridgeOp,
    arguments: Value,
    timeout: Duration,
) -> Result<Value, Failure> {
    if let Some(undeclared) = undeclared_key(op, &arguments) {
        return Err(Failure::internal(
            "undeclared_bridge_argument",
            format!(
                "`{}` does not declare the argument `{undeclared}`",
                op.operation
            ),
        )
        .remedy("this is a defect in ds; report it with the command you ran"));
    }
    // Only this shared seam is map-attached. Pure ds-brain/device operations
    // never reach it and therefore never acquire a Desktop dependency.
    HEADLESS_IDENTITY.with(|headless| {
        invoke_routed(
            op,
            arguments,
            timeout,
            headless.borrow().as_ref(),
            Some(&descriptor.instance_id),
            || bridge::session(descriptor),
            |operation, arguments, fence, timeout| {
                bridge::invoke(descriptor, operation, arguments, fence, timeout)
            },
        )
    })
}

fn invocation_route<'a>(
    operation: &str,
    headless: Option<&'a HeadlessIdentity>,
    fence: &'a bridge::IdentityFence,
    instance: Option<&'a str>,
) -> Result<Route<'a>, Failure> {
    if operation == "auth.link.approve" {
        return Ok(Route::CurrentDesktop);
    }
    let requirement = if headless.is_some_and(|h| h.command_authority == Authority::Project) {
        Requirement::MapProject
    } else {
        Requirement::DesktopUser
    };
    project_context::route(
        requirement,
        switch_for(operation),
        headless.map(|h| ProjectContext {
            uid: &h.uid,
            lane: &h.lane,
            audience: &h.credential_audience_sha256,
            project: h.project.as_deref(),
            // A caller's saved context names an identity and a default, never
            // a runtime.
            instance: None,
        }),
        fence_context(fence, instance),
    )
    .map_err(refused_route)
}

/// Whether this operation may move the instance's project at all.
///
/// One operation may: the explicit, instance-qualified switch the operator
/// asked for. Every other paired operation passes `Never`, which is the
/// 2026-09-12 ruling in one line — a saved CLI selection never flips a live
/// map, and a project difference is refused with the switch as its remedy.
fn switch_for(operation: &str) -> Switch {
    if operation == crate::project::SWITCH_OP.operation {
        Switch::Explicit
    } else {
        Switch::Never
    }
}

pub(crate) fn fence_context<'a>(
    fence: &'a bridge::IdentityFence,
    instance: Option<&'a str>,
) -> ProjectContext<'a> {
    ProjectContext {
        uid: &fence.uid,
        lane: &fence.lane,
        audience: &fence.credential_audience_sha256,
        project: fence.project.as_deref(),
        instance,
    }
}

/// Relay the kernel's own name for why an operation may not run inside the
/// instance that was chosen for it.
fn refused_route(refusal: project_context::Refusal<'_>) -> Failure {
    match refusal {
        project_context::Refusal::IdentityMismatch => context_mismatch(),
        project_context::Refusal::ProjectNotOpen { project } => project_not_open(project),
    }
}

/// The project the caller selected is not the project this instance has open,
/// and nothing here may move a live map to it.
pub fn project_not_open(project: &str) -> Failure {
    Failure::conflict(
        PROJECT_NOT_OPEN.code,
        "no targeted DS GridDesign instance has that project open",
    )
    .remedy(PROJECT_NOT_OPEN.remedy)
    .detail(json!({ "project": project }))
}

fn context_mismatch() -> Failure {
    Failure::conflict(
        "auth_context_mismatch",
        "the paired runtime identity or project changed during command routing",
    )
    .remedy("verify the paired account and lane, then retry the intended command")
}

/// Host effects for the kernel's routing decision. Never recurse through
/// dispatch, never change the CLI's durable project selection, and — since
/// 2026-09-12 — never change a live window's project either.
///
/// What used to be here was an automatic `project.switch`: any map command
/// whose CLI project differed from the window's moved the window first. Under
/// the owner's ruling that a saved CLI selection must never flip a live map,
/// the kernel now refuses that difference by name, and the operator switches
/// explicitly with `ds desktop project switch --target desktop:<id>`. The one
/// operation that is allowed to move a project is that switch, and its own
/// operation *is* the switch — so there is nothing left for a host to perform
/// between the decision and the send.
fn invoke_routed(
    op: &BridgeOp,
    arguments: Value,
    timeout: Duration,
    headless: Option<&HeadlessIdentity>,
    instance: Option<&str>,
    mut observe: impl FnMut() -> Result<Value, Failure>,
    mut send: impl FnMut(
        &'static str,
        Value,
        &bridge::IdentityFence,
        Duration,
    ) -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    let fence = bridge::IdentityFence::from_session(&observe()?)?;
    match invocation_route(op.operation, headless, &fence, instance)? {
        Route::CurrentDesktop => {}
        // Only the explicit switch reaches this, and sending it is the next
        // line: the decision and the effect are the same operation.
        Route::SwitchDesktop { .. } => {}
    }
    send(op.operation, arguments, &fence, timeout)
}

/// The first key in `arguments` the operation does not declare, if any.
pub fn undeclared_key(op: &BridgeOp, arguments: &Value) -> Option<String> {
    let object = arguments.as_object()?;
    for (key, value) in object {
        let nested_prefix = format!("{key}.");
        let has_nested = op
            .arguments
            .iter()
            .any(|declared| declared.starts_with(&nested_prefix));
        if has_nested {
            let mut objects = Vec::new();
            match value {
                Value::Object(inner) => objects.push(inner),
                Value::Array(items) => {
                    for item in items {
                        let Some(inner) = item.as_object() else {
                            return Some(key.clone());
                        };
                        objects.push(inner);
                    }
                }
                _ => return Some(key.clone()),
            }
            for inner in objects {
                for inner_key in inner.keys() {
                    let qualified = format!("{key}.{inner_key}");
                    if !op.arguments.contains(&qualified.as_str()) {
                        return Some(qualified);
                    }
                }
            }
        } else if !op.arguments.contains(&key.as_str()) {
            return Some(key.clone());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Availability and the shared refusals
// ---------------------------------------------------------------------------

/// Always available, and the reasoning settles it for every paired domain.
///
/// It is tempting to gate these commands on a descriptor existing, so
/// `ds doctor` says something about the application. Two things make that
/// wrong:
///
/// * **It would make `--desktop-descriptor` unreachable.** Dispatch checks
///   availability *before* it has parsed a single flag, so a gate that
///   refused because discovery found nothing would refuse the one invocation
///   that was about to name where to look.
/// * **It would make the input contract untestable.** Every handler
///   validates its own flags before it touches the bridge, so a malformed
///   argument is a typed refusal whether or not an application is running —
///   which is the only reachable behaviour on a CI machine.
///
/// `ds desktop status` answers whether there is a session, and every paired
/// command documents `desktop_not_paired` with the same code and remedy the
/// gate would have used.
pub fn paired_availability() -> Availability {
    Availability::Available
}

pub const NOT_PAIRED: Refusal = Refusal {
    code: "desktop_not_paired",
    when: "no DS GridDesign session is running on this machine",
    remedy: "start DS GridDesign, then run `ds desktop status`",
};
pub const AMBIGUOUS: Refusal = Refusal {
    code: "desktop_ambiguous",
    when: "more than one live DS GridDesign instance could serve this operation",
    remedy: "name one with --target desktop:<instance_id>, from `ds desktop list`",
};
pub const TARGET_NOT_LIVE: Refusal = Refusal {
    code: "desktop_target_not_live",
    when: "--target named an instance that is not among the live ones",
    remedy: "list the live instances with `ds desktop list` and name one of them",
};
pub const TARGET_MISMATCH: Refusal = Refusal {
    code: "desktop_target_mismatch",
    when: "--target named a live instance on another lane, account or credential audience",
    remedy: "name an instance that can serve this operation, from `ds desktop list`",
};
/// The saved CLI project is not open in any instance that could serve the
/// work. It is a refusal and not a switch: a CLI selection never moves a live
/// map, so the remedy is the explicit, instance-qualified switch.
pub const PROJECT_NOT_OPEN: Refusal = Refusal {
    code: "desktop_project_not_open",
    when: "no live instance eligible for this operation has the selected project open",
    remedy: "open it in the app, or switch one explicitly with `ds desktop project switch --target desktop:<instance_id> --project <project>`",
};
pub const CONTRACT_MISMATCH: Refusal = Refusal {
    code: "desktop_contract_mismatch",
    when: "a live instance published a session that is not this build's contract",
    remedy: "update DS GridDesign and `ds` to matching releases",
};
/// Raised by the application: the window's project or account changed while
/// this operation was in flight, so its result belongs to a view that no
/// longer exists.
pub const CONTEXT_GENERATION_STALE: Refusal = Refusal {
    code: "context_generation_stale",
    when: "the targeted window switched project or account while the operation was running",
    remedy: "retry the operation against the view as it is now",
};
pub const UNKNOWN_TARGET: Refusal = Refusal {
    code: "unknown_target",
    when: "--target is not desktop, desktop:<instance_id> or server",
    remedy: "pass --target desktop, --target desktop:<instance_id> or --target server",
};
pub const HOST_UNSUPPORTED: Refusal = Refusal {
    code: "target_host_unsupported",
    when: "--target server named a host that does not perform this operation",
    remedy: "run it with --target desktop; `ds server --help` lists what the Server performs",
};
pub const UNREACHABLE: Refusal = Refusal {
    code: "desktop_unreachable",
    when: "the descriptor is stale, or the application did not answer in time",
    remedy: "DS GridDesign may have exited; restart it and retry",
};
pub const PAIRING_REJECTED: Refusal = Refusal {
    code: "pairing_rejected",
    when: "the application refused the descriptor's pairing secret",
    remedy: "the descriptor is stale; restart DS GridDesign",
};
pub const REFUSED: Refusal = Refusal {
    code: "desktop_refused",
    when: "the application answered and refused the operation",
    remedy: "read detail.detail for the application's own message",
};
pub const UNSUPPORTED: Refusal = Refusal {
    code: "desktop_operation_unsupported",
    when: "this DS GridDesign build does not offer the operation",
    remedy: "update DS GridDesign; `ds desktop status` reports the profile",
};
pub const UNREADABLE: Refusal = Refusal {
    code: "desktop_unreadable",
    when: "the application's reply could not be read within its bound",
    remedy: "restart DS GridDesign and retry",
};
pub const SIGNED_OUT: Refusal = Refusal {
    code: "desktop_signed_out",
    when: "the application is running but signed out, or has no project selected",
    remedy: "sign in and select a project in DS GridDesign",
};
/// Offline mode reaches every bridge operation that needs the network, so it is
/// declared once here rather than repeated in each command that can meet it.
///
/// It is `unavailable`, not `failed`: the work is refused by a switch the
/// operator set, and it will succeed unchanged once that switch is off.
pub const OFFLINE: Refusal = Refusal {
    code: "offline_mode_enabled",
    when: "offline mode is on and the operation needs the network",
    remedy: "run `ds desktop offline set --enabled false`, or run a command that works from prepared local data",
};
/// The API itself did not answer, which reaches every bridge operation that
/// needs the network for the same reason [`OFFLINE`] does — so it is declared
/// once here too.
///
/// It is `unavailable`, not `failed`: nothing about the request is wrong and
/// the identical call succeeds once the service answers. The re-read is not
/// caution for its own sake — a request that never answered may still have
/// been applied, so repeating a write blind is how a duplicate is made.
pub const BACKEND_UNREACHABLE: Refusal = Refusal {
    code: "backend_unreachable",
    when: "the Data Solutions API did not answer",
    remedy: "restore the connection and retry; re-read first, because an unanswered write may already have been applied",
};

/// What the application says when a project operation is asked for without a
/// project session. Matched case-insensitively against its own message.
///
/// This is a hand copy of the application's prose, which is the least stable
/// thing to key on anywhere in the CLI — so each calling domain's parity test
/// requires a marker to still appear in the application's source, and the
/// fallback when none match is the untranslated refusal rather than a wrong
/// one.
pub const SIGNED_OUT_MARKERS: &[&str] = &[
    "no active project",
    "open a project",
    "sign in",
    "signed out",
];

/// Turn the application's own refusal into the signed-out refusal when that
/// is what it actually is.
///
/// Project operations require a signed-in session with a project selected,
/// and the application reports that as an ordinary operation refusal. Letting
/// it through as `desktop_refused` would send a caller to read `detail` for a
/// condition that has a name, a remedy and a `ds` command that diagnoses it.
pub fn classify_signed_out(failure: Failure) -> Failure {
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|detail| detail["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !SIGNED_OUT_MARKERS
        .iter()
        .any(|marker| detail.contains(marker))
    {
        return failure;
    }
    Failure::unauthorized(
        "desktop_signed_out",
        "the paired session is signed out, or has no project selected",
    )
    .remedy(SIGNED_OUT.remedy)
    .next("ds desktop status")
}

// ---------------------------------------------------------------------------
// Flag shapes shared by every paired domain
// ---------------------------------------------------------------------------

/// The host this invocation runs against, declared once for both of them.
///
/// One flag, both hosts, and no second spelling of the same question: the
/// standing ruling (2026-09-11) is that CLI/MCP → Desktop or Server is no
/// difference at all — one command id, the same arguments, the same answer,
/// whichever host runs it — so the host is an argument to the operation rather
/// than a different operation.
///
/// * `desktop` — this machine's own native client. The default, and the whole
///   default when exactly one live instance can serve the work.
/// * `desktop:<instance_id>` — one named live instance, from `ds desktop
///   list`. Honoured or refused by name; it never routes anywhere else.
/// * `server` — the running `ds server serve` on this machine, for the
///   operations the Server performs.
///
/// Declared without `choices` on purpose: `desktop:<instance_id>` must reach
/// the handler to be answered by the kernel's own vocabulary instead of the
/// parser's generic `invalid_choice`. And declared without a default, so an
/// absent flag is absent — which is what lets `DS_TARGET` be a session default
/// that an explicit flag still wins over.
pub const TARGET_ARG: Arg = Arg::value(
    "target",
    "<desktop|desktop:instance|server>",
    "Which host executes this operation; the desktop is the default, and DS_TARGET sets it for a session.",
);

/// The session default for [`TARGET_ARG`]. A default for the flag, never an
/// override of it.
pub const TARGET_ENV: &str = "DS_TARGET";

/// The host a caller named, resolved once, before anything is read or sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Host {
    /// The desktop, and one named instance of it when the caller named one.
    Desktop(Option<kernel::Target>),
    Server,
}

/// Read `--target` (or `DS_TARGET`) as a host.
///
/// The instance id itself is *not* validated here. A caller who mistypes one
/// is answered by the kernel — `desktop_target_mismatch`, reason `malformed` —
/// so one owner names every way a target can be wrong, and this layer decides
/// only which host the text is about.
pub fn host(named: Option<&str>) -> Result<Host, Failure> {
    let named = match named.map(str::trim).filter(|value| !value.is_empty()) {
        Some(named) => named.to_owned(),
        None => return Ok(Host::Desktop(None)),
    };
    match named.as_str() {
        "desktop" => Ok(Host::Desktop(None)),
        "server" => Ok(Host::Server),
        instance if instance.starts_with("desktop:") => Ok(Host::Desktop(Some(kernel::Target {
            instance_id: instance.trim_start_matches("desktop:").to_owned(),
            window: None,
        }))),
        other => Err(Failure::invalid(
            UNKNOWN_TARGET.code,
            format!("`{}` is not a host", crate::bridge::bounded(other)),
        )
        .remedy(UNKNOWN_TARGET.remedy)),
    }
}

/// The instance a paired operation is for, or the named refusal for a host
/// that does not perform it.
pub fn desktop_target(named: Option<&str>) -> Result<Option<kernel::Target>, Failure> {
    match host(named)? {
        Host::Desktop(target) => Ok(target),
        Host::Server => Err(Failure::invalid(
            HOST_UNSUPPORTED.code,
            "the Server does not perform this operation; the paired desktop does",
        )
        .remedy(HOST_UNSUPPORTED.remedy)),
    }
}

/// The `--desktop-descriptor` flag, declared identically by every paired
/// command so a caller who learned it once has learned it everywhere.
///
/// It stays beside [`TARGET_ARG`] rather than being replaced by it: they name
/// different things. A target names one live *instance* the kernel selects
/// among; a descriptor names one *file*, is used verbatim, and is what the
/// desktop's own `cl` terminal pins so a shell it opened keeps talking to the
/// window that opened it.
pub const DESCRIPTOR_ARG: Arg = Arg {
    name: "desktop-descriptor",
    kind: ArgKind::Value,
    value: "<path>",
    required: false,
    default: None,
    choices: &[],
    summary: "Use this bridge descriptor instead of discovering one; DS_DESKTOP_DESCRIPTOR sets the same default.",
};

pub const INVALID_NUMBER: Refusal = Refusal {
    code: "invalid_number",
    when: "a numeric flag is not a number, or falls outside the bound in its summary",
    remedy: "the refusal carries the accepted range",
};

/// A whole-number flag, held to the bound stated in its own summary.
pub fn integer(raw: &str, flag: &str, min: i64, max: i64) -> Result<i64, Failure> {
    let parsed = raw.parse::<i64>().map_err(|_| {
        Failure::invalid(
            "invalid_number",
            format!("`--{flag}` must be a whole number"),
        )
        .remedy(format!("pass {min}..{max}"))
    })?;
    if parsed < min || parsed > max {
        return Err(
            Failure::invalid("invalid_number", format!("`--{flag}` is outside its bound"))
                .remedy(format!("pass {min}..{max}"))
                .detail(json!({ "given": parsed, "min": min, "max": max })),
        );
    }
    Ok(parsed)
}

/// Render a count with its noun, so a human line reads as English.
pub fn plural(count: u64, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minted instance id, in the kernel's grammar: 32 lowercase hex.
    const INSTANCE: &str = "11111111111111111111111111111111";

    const ZOOM_TO: BridgeOp = BridgeOp {
        operation: "map.zoom_to",
        arguments: &["bbox", "padding"],
    };
    const POINTS_ALONG: BridgeOp = BridgeOp {
        operation: "gis.points_along",
        arguments: &["layerId", "settings.intervalM", "settings.includeEnds"],
    };
    const STAGE_BATCH: BridgeOp = BridgeOp {
        operation: "design.upload.stage_batch",
        arguments: &["items.transformer", "items.path", "parallel"],
    };

    #[test]
    fn an_argument_key_the_operation_does_not_declare_never_leaves_this_process() {
        // The guard behind `invoke`. It is what makes a declared wire
        // contract load-bearing rather than documentation: a handler cannot
        // send a key its domain's parity test has not proved.
        assert_eq!(
            undeclared_key(&ZOOM_TO, &json!({ "bbox": [1, 2, 3, 4], "padding": 8 })),
            None
        );
        assert_eq!(
            undeclared_key(&ZOOM_TO, &json!({ "bbox": [1, 2, 3, 4], "zoom": 8 })),
            Some("zoom".to_string())
        );

        // Nested keys are checked one level in, because that is where the
        // application's camelCase settings live.
        assert_eq!(
            undeclared_key(
                &POINTS_ALONG,
                &json!({ "layerId": "sketch:x", "settings": { "intervalM": 25 } })
            ),
            None
        );
        assert_eq!(
            undeclared_key(
                &POINTS_ALONG,
                &json!({ "layerId": "sketch:x", "settings": { "interval_m": 25 } })
            ),
            Some("settings.interval_m".to_string()),
            "a settings key that drifts case must be caught here"
        );
        // A declared object that arrives as a scalar is undeclared too: the
        // application would reject the whole payload rather than one field.
        assert_eq!(
            undeclared_key(&POINTS_ALONG, &json!({ "settings": 25 })),
            Some("settings".to_string())
        );

        // Repeated typed objects use the same dotted declaration. Every item
        // is validated; accepting the array must not create an open payload.
        assert_eq!(
            undeclared_key(
                &STAGE_BATCH,
                &json!({
                    "items": [
                        { "transformer": "A", "path": "a.zip" },
                        { "transformer": "B", "path": "b.zip" }
                    ],
                    "parallel": 2
                })
            ),
            None
        );
        assert_eq!(
            undeclared_key(
                &STAGE_BATCH,
                &json!({ "items": [{ "transformer": "A", "path": "a.zip", "overwrite": true }] })
            ),
            Some("items.overwrite".to_string())
        );
        assert_eq!(
            undeclared_key(&STAGE_BATCH, &json!({ "items": ["a.zip"] })),
            Some("items".to_string())
        );
    }

    #[test]
    fn a_numeric_flag_is_held_to_the_bound_its_summary_states() {
        assert_eq!(integer("4", "parallel", 1, 32).expect("in range"), 4);
        for bad in ["0", "33", "four", "", "1.5"] {
            assert_eq!(
                integer(bad, "parallel", 1, 32)
                    .expect_err("must refuse")
                    .code(),
                "invalid_number",
                "`{bad}` was accepted as a bounded whole number"
            );
        }
    }

    #[test]
    fn an_application_refusal_is_only_reclassified_when_it_really_is_signed_out() {
        let signed_out = Failure::failed("desktop_refused", "refused")
            .detail(json!({ "detail": "No active project. Open a project first." }));
        assert_eq!(
            classify_signed_out(signed_out).code(),
            "desktop_signed_out",
            "the application's own signed-out prose must become the named refusal"
        );

        let other = Failure::failed("desktop_refused", "refused")
            .detail(json!({ "detail": "that transformer has no staged edits" }));
        assert_eq!(
            classify_signed_out(other).code(),
            "desktop_refused",
            "an ordinary refusal must not be renamed"
        );

        let unreachable = Failure::unavailable("desktop_unreachable", "gone");
        assert_eq!(
            classify_signed_out(unreachable).code(),
            "desktop_unreachable"
        );
    }

    #[test]
    fn checked_identity_fence_is_transport_metadata_not_domain_input() {
        let fence = bridge::IdentityFence::from_session(&json!({
            "uid": "uid-1",
            "lane": "stable",
            "credential_audience_sha256": "a".repeat(64),
            "project": "project-1",
            "session_revision": 7,
        }))
        .expect("valid fence");
        assert_eq!(
            undeclared_key(&ZOOM_TO, &json!({ "identity_fence": fence })),
            Some("identity_fence".to_owned()),
            "a domain handler must never be able to inject the fence"
        );
    }

    #[test]
    fn scoped_headless_observation_is_restored_after_dispatch() {
        let identity = HeadlessIdentity {
            uid: "uid-1".to_owned(),
            lane: "stable".to_owned(),
            credential_audience_sha256: "a".repeat(64),
            project: Some("project-1".to_owned()),
            command_authority: Authority::Project,
            target: None,
        };
        {
            let _guard = scope_headless_identity(Some(identity.clone()));
            HEADLESS_IDENTITY
                .with(|current| assert_eq!(current.borrow().as_ref(), Some(&identity)));
        }
        HEADLESS_IDENTITY.with(|current| assert!(current.borrow().is_none()));
    }

    #[test]
    fn map_arbitration_is_exact_and_project_can_be_inherited() {
        let headless = HeadlessIdentity {
            uid: "uid-1".to_owned(),
            lane: "stable".to_owned(),
            credential_audience_sha256: "a".repeat(64),
            project: None,
            command_authority: Authority::Project,
            target: None,
        };
        let mut fence = bridge::IdentityFence::from_session(&json!({
            "uid": "uid-1", "lane": "stable",
            "credential_audience_sha256": "a".repeat(64),
            "project": "map-project", "session_revision": 4,
        }))
        .unwrap();
        invocation_route("map.zoom_to", Some(&headless), &fence, Some(INSTANCE))
            .expect("map project supplies authority when headless project is absent");
        fence.uid = "uid-2".to_owned();
        assert_eq!(
            invocation_route("map.zoom_to", Some(&headless), &fence, Some(INSTANCE))
                .unwrap_err()
                .code(),
            "auth_context_mismatch"
        );
        fence.uid = "uid-1".to_owned();
        fence.lane = "canary".to_owned();
        assert_eq!(
            invocation_route("map.zoom_to", Some(&headless), &fence, Some(INSTANCE))
                .unwrap_err()
                .code(),
            "auth_context_mismatch"
        );
    }

    #[test]
    fn desktop_user_reference_reads_ignore_only_the_selected_project() {
        let fence = bridge::IdentityFence::from_session(&json!({
            "uid": "uid-1", "lane": "stable",
            "credential_audience_sha256": "a".repeat(64),
            "project": "desktop-project", "session_revision": 4,
        }))
        .unwrap();
        let user_reference = HeadlessIdentity {
            uid: "uid-1".to_owned(),
            lane: "stable".to_owned(),
            credential_audience_sha256: "a".repeat(64),
            project: Some("unrelated-cli-project".to_owned()),
            command_authority: Authority::DesktopUser,
            target: None,
        };
        for operation in ["data.admin_bounds.list", "data.admin_bounds.read"] {
            invocation_route(operation, Some(&user_reference), &fence, Some(INSTANCE))
                .expect("national-reference DesktopUser reads ignore project selection");
        }

        let project_bound = HeadlessIdentity {
            command_authority: Authority::Project,
            ..user_reference.clone()
        };
        let refused = invocation_route(
            "data.admin_bounds.attach",
            Some(&project_bound),
            &fence,
            Some(INSTANCE),
        )
        .expect_err("project work never moves the window it found");
        assert_eq!(refused.code(), "desktop_project_not_open");
        assert!(
            refused
                .remedy_text()
                .is_some_and(|remedy| remedy.contains("ds desktop project switch --target")),
            "the remedy is the explicit switch: {:?}",
            refused.remedy_text()
        );

        let wrong_user = HeadlessIdentity {
            uid: "uid-2".to_owned(),
            ..user_reference
        };
        assert_eq!(
            invocation_route(
                "data.admin_bounds.read",
                Some(&wrong_user),
                &fence,
                Some(INSTANCE)
            )
            .unwrap_err()
            .code(),
            "auth_context_mismatch",
            "DesktopUser never means a different user may borrow the session"
        );
    }

    /// The retired automatic switch, stated as the rule that replaced it.
    ///
    /// Before 2026-09-12 this seam sent `project.switch` to the window
    /// whenever the CLI's saved project differed from it, and only then ran
    /// the operation. A saved CLI selection must never flip a live map, so the
    /// difference is now refused by name, nothing at all is sent, and the
    /// operator switches the window explicitly if that is what they meant.
    #[test]
    fn a_saved_selection_never_switches_a_live_map() {
        let headless = HeadlessIdentity {
            uid: "uid-1".into(),
            lane: "stable".into(),
            credential_audience_sha256: "a".repeat(64),
            project: Some("cli-project".into()),
            command_authority: Authority::Project,
            target: None,
        };
        let showing = json!({ "uid": "uid-1", "lane": "stable",
            "credential_audience_sha256": "a".repeat(64),
            "project": "ui-project", "session_revision": 4 });
        let op = BridgeOp {
            operation: "survey.working_area.download",
            arguments: &["entireProject"],
        };
        let mut sent: Vec<&'static str> = Vec::new();
        let refused = invoke_routed(
            &op,
            json!({"entireProject": true}),
            Duration::from_secs(60),
            Some(&headless),
            Some(INSTANCE),
            || Ok(showing.clone()),
            |name, _, _, _| {
                sent.push(name);
                Ok(json!({"ok": true}))
            },
        )
        .expect_err("the window is on another project");
        assert_eq!(refused.code(), "desktop_project_not_open");
        assert_eq!(
            refused.detail_value().expect("a detail")["project"],
            "cli-project"
        );
        assert!(
            sent.is_empty(),
            "nothing may be sent to a window this operation is not for: {sent:?}"
        );
    }

    /// The one operation that may move a project is the switch itself, and it
    /// is sent as the operation the caller asked for — not as a step before
    /// some other operation.
    #[test]
    fn the_explicit_switch_is_sent_as_itself_and_nothing_precedes_it() {
        let headless = HeadlessIdentity {
            uid: "uid-1".into(),
            lane: "stable".into(),
            credential_audience_sha256: "a".repeat(64),
            project: Some("cli-project".into()),
            command_authority: Authority::DesktopUser,
            target: Some(format!("desktop:{INSTANCE}")),
        };
        let showing = json!({ "uid": "uid-1", "lane": "stable",
            "credential_audience_sha256": "a".repeat(64),
            "project": "ui-project", "session_revision": 4 });
        let mut sent = Vec::new();
        invoke_routed(
            &crate::project::SWITCH_OP,
            json!({ "project": "cli-project" }),
            Duration::from_secs(30),
            Some(&headless),
            Some(INSTANCE),
            || Ok(showing.clone()),
            |name, arguments, fence, _| {
                sent.push((name, arguments, fence.clone()));
                Ok(json!({"changed": true}))
            },
        )
        .expect("an explicit switch is the operation, and it runs");
        assert_eq!(sent.len(), 1, "one operation, and it is the switch");
        assert_eq!(sent[0].0, "project.switch");
        assert_eq!(sent[0].1, json!({ "project": "cli-project" }));
        assert_eq!(sent[0].2.project.as_deref(), Some("ui-project"));
    }

    #[test]
    fn the_offline_remedy_names_the_command_that_turns_the_switch_off() {
        // A remedy is only actionable if the command it names exists, so the
        // expected text is built from the registered command rather than
        // retyped: a rename of the path or the flag fails here instead of
        // reaching a caller as advice that cannot be run.
        let path = crate::connectivity::SET.path.join(" ");
        let flag = crate::connectivity::SET
            .args
            .iter()
            .find(|arg| arg.name == "enabled")
            .expect("the offline switch is set through --enabled");
        assert!(
            OFFLINE
                .remedy
                .contains(&format!("`ds {path} --{} false`", flag.name)),
            "OFFLINE.remedy must name the real command, not describe it: {}",
            OFFLINE.remedy
        );
        assert!(
            flag.choices.contains(&"false"),
            "the remedy tells a caller to pass false; the command must accept it"
        );
    }

    #[test]
    fn malformed_or_unsigned_map_session_has_no_invocation_fence() {
        for bad in [
            json!({}),
            json!({
                "uid": "uid-1", "lane": "stable",
                "credential_audience_sha256": "a".repeat(64),
                "project": null, "session_revision": 0
            }),
            json!({
                "uid": "uid-1", "lane": "stable",
                "credential_audience_sha256": "A".repeat(64),
                "project": null, "session_revision": 1
            }),
        ] {
            assert_eq!(
                bridge::IdentityFence::from_session(&bad)
                    .expect_err("malformed fence")
                    .code(),
                "auth_context_mismatch"
            );
        }
    }
}
