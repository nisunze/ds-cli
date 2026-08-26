//! `ds desktop status` — is there a paired session, is it signed in, which
//! project is selected, and whether a transformer is open for design editing.
//!
//! This is the command an agent runs first when anything fails with an
//! authority error, so it has to answer without ever being the thing that
//! fails. It reports the same three facts in every state, including the
//! states where the answer is "no": not paired, paired but signed out,
//! signed in with no project. Each of those is a *success* — the question
//! was answered — with a remedy attached to whatever is missing.
//!
//! It never prints the pairing token, the URL's secret, a JWT, or the user's
//! credentials. Identity is reported as the application already displays it.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::discover::{self, Discovery, PROFILES};

const SESSION_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_SESSION_BYTES: u64 = 256 * 1024;

pub static COMMAND: Command = Command {
    id: "desktop.status",
    path: &["desktop", "status"],
    contract: 1,
    summary: "Report pairing, project and active design context.",
    purpose: "\
Answers whether a DS GridDesign session is running on this machine, whether it \
is signed in, which project is selected, and whether a transformer is open in \
the design editor. Run this first when a command refuses with an authority \
error. Not being paired is an answer, not a failure: the command succeeds and \
says what is missing.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value(
        "desktop-descriptor",
        "<path>",
        "Use this bridge descriptor instead of discovering one; DS_DESKTOP_DESCRIPTOR sets the same default.",
    )],
    output: "\
`paired`, `signed_in`, `project` and `design_context` always present. When \
paired, the install profile and the application's process id. `design_context` \
is null unless a project transformer is open for editing. Never a token, JWT \
or credential.",
    examples: &[
        Example {
            command: "ds desktop status",
            note: "",
            runnable: true,
        },
        Example {
            command: "ds desktop status --output json",
            note: "Branch on .data.paired and .data.signed_in.",
            runnable: true,
        },
    ],
    refusals: &[
        Refusal {
            code: "desktop_ambiguous",
            when: "Stable, Canary or a dev build are running together",
            remedy: "pass --desktop-descriptor <path> to name which one",
        },
        Refusal {
            code: "desktop_unreachable",
            when: "the descriptor names a port nothing answers on",
            remedy: "DS GridDesign may have exited; restart it and retry",
        },
        Refusal {
            code: "pairing_rejected",
            when: "the application refused the descriptor's pairing secret",
            remedy: "the descriptor is stale; restart DS GridDesign",
        },
        Refusal {
            code: "desktop_refused",
            when: "the paired session answered, but refused the status request",
            remedy: "restart DS GridDesign and retry",
        },
        Refusal {
            code: "desktop_unreadable",
            when: "the session response could not be read within its bound",
            remedy: "restart DS GridDesign and retry",
        },
        Refusal {
            code: "desktop_contract_mismatch",
            when: "the session's reply does not match this build's contract",
            remedy: "update DS GridDesign and `ds` to matching releases",
        },
        Refusal {
            code: "descriptor_unusable",
            when: "a descriptor exists but is unreadable, stale or not loopback",
            remedy: "restart DS GridDesign to republish its descriptor",
        },
    ],
    reference: Some("docs/reference/desktop.status.md"),
    availability: available,
};

/// Always available, and deliberately so.
///
/// It is tempting to make this report `unavailable` when no desktop is
/// running — `doctor` would then say something useful about the environment.
/// It would also be circular: this is the command whose entire job is to
/// report whether a desktop is running, so gating it on a desktop running
/// means the one call that could explain the situation is the one call that
/// refuses to. "Not paired" is an answer, and answering is this command
/// working correctly.
///
/// Commands that genuinely need the session — anything reading or writing
/// project data — declare `Authority::DesktopUser` and report unavailable
/// through their own availability check. Those are what make `doctor`
/// informative, without making the diagnostic itself undiagnosable.
fn available() -> Availability {
    Availability::Available
}

/// The bounded session view the bridge publishes. Deliberately a subset: the
/// bridge also returns map context, capabilities and folder grants, and this
/// command reports none of them — a status check that dumped the whole
/// session would be the largest response in the product.
#[derive(Deserialize)]
struct SessionView {
    #[serde(default)]
    signed_in: bool,
    #[serde(default)]
    uid: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    design_context: Option<DesignContextView>,
}

#[derive(Deserialize)]
struct DesignContextView {
    mode: DesignContextMode,
    transformer: String,
}

#[derive(Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum DesignContextMode {
    Edit,
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let explicit = inputs.value("desktop-descriptor");

    let found = match discover::discover(explicit) {
        Discovery::Paired(found) => found,
        Discovery::None => {
            return Ok(json!({
                "paired": false,
                "signed_in": false,
                "project": Value::Null,
                "design_context": Value::Null,
                "reason": "no_session",
                "remedy": "start DS GridDesign, then run `ds desktop status`",
                "searched": PROFILES.iter().map(|(profile, _)| *profile).collect::<Vec<_>>(),
            }));
        }
        Discovery::Ambiguous(candidates) => {
            return Err(Failure::unavailable(
                "desktop_ambiguous",
                "more than one DS GridDesign profile is running",
            )
            .remedy("pass --desktop-descriptor <path> to name which one")
            .detail(json!({
                "candidates": candidates
                    .iter()
                    .map(|(profile, path)| json!({
                        "profile": profile,
                        "descriptor": path.display().to_string(),
                    }))
                    .collect::<Vec<_>>(),
            })));
        }
        Discovery::Unusable { path, reason } => {
            return Err(Failure::unavailable(
                "descriptor_unusable",
                "the bridge descriptor cannot be used",
            )
            .remedy("restart DS GridDesign to republish its descriptor")
            .detail(json!({ "descriptor": path.display().to_string(), "reason": reason })));
        }
    };

    let session = fetch_session(&found.descriptor)?;

    Ok(paired_data(
        found.profile,
        found.descriptor.pid,
        found.path.display().to_string(),
        session,
    ))
}

fn paired_data(profile: &str, pid: u32, descriptor: String, session: SessionView) -> Value {
    json!({
        "paired": true,
        "profile": profile,
        "pid": pid,
        "descriptor": descriptor,
        "signed_in": session.signed_in,
        "uid": session.uid,
        "email": session.email,
        "project": session.project,
        "design_context": session.design_context.map(|context| json!({
            "mode": context.mode,
            "transformer": context.transformer,
        })),
    })
}

fn fetch_session(descriptor: &discover::Descriptor) -> Result<SessionView, Failure> {
    let response = ureq::get(&format!("{}/v1/session", descriptor.url))
        .header("authorization", &format!("Bearer {}", descriptor.token))
        .config()
        .timeout_global(Some(SESSION_TIMEOUT))
        .build()
        .call();

    let mut response = match response {
        Ok(response) => response,
        Err(error) => {
            // The descriptor named a port nothing is listening on, or the app
            // stopped between discovery and this call. Both mean the same
            // thing to a caller: the file is stale.
            return Err(Failure::unavailable(
                "desktop_unreachable",
                "the paired session did not answer",
            )
            .remedy("DS GridDesign may have exited; restart it and retry")
            .detail(json!({ "detail": bounded(&error.to_string()) })));
        }
    };

    if response.status() == 401 {
        return Err(Failure::unauthorized(
            "pairing_rejected",
            "the application refused this pairing secret",
        )
        .remedy("the descriptor is stale; restart DS GridDesign"));
    }
    if response.status() != 200 {
        return Err(Failure::unavailable(
            "desktop_refused",
            "the paired session refused the status request",
        )
        .remedy("restart DS GridDesign and retry")
        .detail(json!({ "http_status": response.status().as_u16() })));
    }

    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_SESSION_BYTES)
        .read_to_string()
        .map_err(|_| {
            Failure::unavailable(
                "desktop_unreadable",
                "the session response could not be read",
            )
            .remedy("restart DS GridDesign and retry")
        })?;

    serde_json::from_str(&body).map_err(|_| {
        Failure::unavailable(
            "desktop_contract_mismatch",
            "the paired session's reply does not match this build's contract",
        )
        .remedy("update DS GridDesign and `ds` to matching releases")
    })
}

/// Keep an upstream error string short and free of anything path- or
/// credential-shaped before it becomes part of a result.
fn bounded(detail: &str) -> String {
    let first = detail.lines().next().unwrap_or_default();
    let trimmed: String = first.chars().take(160).collect();
    trimmed
}

/// Human presentation of the same value the envelope carries.
pub fn render(data: &Value) -> String {
    if !data["paired"].as_bool().unwrap_or(false) {
        return format!(
            "not paired — {}\n  → {}",
            data["reason"].as_str().unwrap_or("no session"),
            data["remedy"].as_str().unwrap_or(""),
        );
    }
    let mut out = format!(
        "paired  {} (pid {})\n",
        data["profile"].as_str().unwrap_or("?"),
        data["pid"],
    );
    if data["signed_in"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "signed in  {}\n",
            data["email"].as_str().unwrap_or("?")
        ));
        match data["project"].as_str() {
            Some(project) => out.push_str(&format!("project  {project}\n")),
            None => out.push_str("project  none selected\n  → select a project in DS GridDesign\n"),
        }
        if let Some(context) = data["design_context"].as_object() {
            out.push_str(&format!(
                "design  {} {}\n",
                context["mode"].as_str().unwrap_or("?"),
                context["transformer"].as_str().unwrap_or("?"),
            ));
        }
    } else {
        out.push_str("signed out\n  → sign in to DS GridDesign\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_status_projects_the_active_design_edit_context() {
        let session: SessionView = serde_json::from_value(json!({
            "signed_in": true,
            "uid": "uid-1",
            "email": "operator@example.test",
            "project": "arjgpydw_survey_test",
            "design_context": {
                "mode": "edit",
                "transformer": "agasharu"
            }
        }))
        .expect("session view parses");
        let data = paired_data("dev", 42, "descriptor.json".into(), session);

        assert_eq!(data["design_context"]["mode"], "edit");
        assert_eq!(data["design_context"]["transformer"], "agasharu");
        assert!(render(&data).contains("design  edit agasharu"));
    }

    #[test]
    fn older_desktop_sessions_have_no_design_context() {
        let session: SessionView = serde_json::from_value(json!({
            "signed_in": true,
            "project": "arjgpydw_survey_test"
        }))
        .expect("backward-compatible session view parses");
        let data = paired_data("canary", 42, "descriptor.json".into(), session);

        assert!(data["design_context"].is_null());
        assert!(!render(&data).contains("design  "));
    }
}
