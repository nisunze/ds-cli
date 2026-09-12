//! `ds desktop list` — which DS GridDesign instances are live on this machine,
//! and which of them can serve this caller's work.
//!
//! This is the command every instance-targeted refusal points at. When two
//! windows are open on two projects, `desktop_ambiguous` says "name one with
//! `--target desktop:<instance_id>`", and this is where the ids come from.
//!
//! What it prints is exactly what the kernel says a client may show:
//! `{instance_id, profile, lane, project, build, started_at_ms, windows}`.
//! Never a pairing token, never a bridge url, never an account uid or email —
//! those fields do not exist on the projection, so this command cannot decide
//! to include them. Two facts are added around that projection, and both are
//! about *this client's own reading* rather than about the session: how the
//! instance was identified (minted by the instance, or derived by the kernel
//! from an older descriptor), and whether it is in a state that can serve work
//! at all.
//!
//! It answers on every machine, including one with nothing running: an empty
//! list is an answer, and the states where a descriptor exists but cannot be
//! used are reported rather than silently dropped — an operator whose app is
//! running and whose `ds` says "not paired" needs to be told which file is
//! wrong.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Availability, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::desktop_instance as kernel;
use serde_json::{Value, json};

use crate::discover::{self, Enumeration, Handshake, Unusable};
use crate::ops;

pub static COMMAND: Command = Command {
    id: "desktop.list",
    path: &["desktop", "list"],
    contract: 1,
    summary: "List the live DS GridDesign instances this machine is running.",
    purpose: "\
Answers which DS GridDesign instances are alive here and which can serve your \
work, so a command refused with `desktop_ambiguous`, `desktop_target_not_live` \
or `desktop_project_not_open` can be re-run with `--target desktop:<id>`. Each \
instance is proved alive by an authenticated handshake, so a stale descriptor \
is never a choice. Nothing running is an answer, not a failure.",
    chapter: Chapter::Project,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "\
`live` and `instances`: each id, how it was identified, profile, lane, open \
project, build, start time, window count and whether it can serve work. \
`compatible` names the ids this account may use. `unusable` names a descriptor \
file that cannot be used, and why. Never a token, an address or an account.",
    examples: &[
        Example {
            command: "ds desktop list",
            note: "",
            runnable: true,
        },
        Example {
            command: "ds desktop list --output json",
            note: "Take one .data.instances[].instance_id for --target desktop:<id>.",
            runnable: true,
        },
    ],
    refusals: &[],
    reference: Some("docs/reference/desktop.md"),
    availability: available,
};

/// Always available, for the same reason `ds desktop status` is: this is the
/// command that reports whether — and which — instances are running, so gating
/// it on one running would make the only call that could explain the situation
/// the one call that refuses to.
fn available() -> Availability {
    Availability::Available
}

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    Ok(data(
        &discover::enumerate(),
        ops::scoped_requirement().as_ref(),
    ))
}

pub fn data(enumeration: &Enumeration, requirement: Option<&discover::Requirement>) -> Value {
    let candidates = enumeration.candidates();
    // Which instances this caller may use is the kernel's answer, not a filter
    // written here: the same compatibility rule that routes an operation is
    // the one that marks a row usable.
    let compatible = kernel::list(&candidates, requirement)
        .ok()
        .and_then(|(_, compatible)| compatible);
    json!({
        "live": enumeration.live.len(),
        "instances": instances(enumeration),
        "compatible": compatible,
        "unusable": unusable(enumeration),
        "more": { "omitted": enumeration.omitted },
    })
}

/// The rows, ordered by instance id. Never by the order the files were read:
/// two machines that found the same instances print the same rows, and nothing
/// a caller reads here can be an accident of enumeration order.
pub fn instances(enumeration: &Enumeration) -> Vec<Value> {
    let mut rows: Vec<Value> = enumeration
        .live
        .iter()
        .map(|live| {
            let mut row = json!({
                "instance_id": live.instance_id(),
                "identity": live.found.descriptor.identity.wire(),
                "profile": live.found.descriptor.profile,
                "state": match &live.handshake {
                    Handshake::Session(_) => "ready",
                    Handshake::Unusable(Unusable::SignedOut) => "signed_out",
                    Handshake::Unusable(Unusable::Contract) => "contract_mismatch",
                },
            });
            if let Handshake::Session(candidate) = &live.handshake {
                let summary = candidate.summary();
                let object = row.as_object_mut().expect("an object");
                object.insert("lane".to_owned(), json!(summary.lane));
                object.insert("project".to_owned(), json!(summary.project));
                object.insert("build".to_owned(), json!(summary.build));
                object.insert("started_at_ms".to_owned(), json!(summary.started_at_ms));
                object.insert("windows".to_owned(), json!(summary.windows));
            }
            row
        })
        .collect();
    rows.sort_by(|left, right| {
        left["instance_id"]
            .as_str()
            .cmp(&right["instance_id"].as_str())
    });
    rows
}

/// Descriptor files that exist and cannot be used, with the kernel's own
/// reason. The path is the machine's own, and the caller is its operator.
pub fn unusable(enumeration: &Enumeration) -> Vec<Value> {
    enumeration
        .unusable
        .iter()
        .map(|(path, reason)| json!({ "descriptor": path.display().to_string(), "reason": reason }))
        .collect()
}

pub fn render(data: &Value) -> String {
    let live = data["live"].as_u64().unwrap_or(0);
    if live == 0 {
        let mut out = "no DS GridDesign instance is running\n  → start DS GridDesign, then run `ds desktop list`\n".to_owned();
        out.push_str(&unusable_lines(data));
        return out;
    }
    let compatible: Vec<&str> = data["compatible"]
        .as_array()
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut out = format!("{}\n", ops::plural(live, "live instance"));
    for instance in data["instances"].as_array().into_iter().flatten() {
        let id = instance["instance_id"].as_str().unwrap_or("");
        out.push_str(&format!(
            "  {id}  {:<7} {:<8} {:<24} {}{}\n",
            instance["profile"].as_str().unwrap_or("—"),
            instance["lane"].as_str().unwrap_or("—"),
            instance["project"].as_str().unwrap_or("no project"),
            instance["state"].as_str().unwrap_or("—"),
            if compatible.contains(&id) {
                "  ← yours"
            } else {
                ""
            },
        ));
    }
    out.push_str(&unusable_lines(data));
    if let Some(omitted) = data["more"]["omitted"].as_u64().filter(|value| *value > 0) {
        out.push_str(&format!("  {omitted} more descriptor files not read\n"));
    }
    out
}

fn unusable_lines(data: &Value) -> String {
    data["unusable"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entry| {
            format!(
                "  unusable  {}  ({})\n",
                entry["descriptor"].as_str().unwrap_or(""),
                entry["reason"].as_str().unwrap_or(""),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = "11111111111111111111111111111111";
    const TWO: &str = "22222222222222222222222222222222";
    const AUDIENCE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn candidate(instance: &str, uid: &str, project: Option<&str>) -> kernel::Candidate {
        kernel::Candidate {
            instance_id: instance.to_owned(),
            profile: Some("canary".to_owned()),
            lane: "canary".to_owned(),
            uid: uid.to_owned(),
            audience_sha256: AUDIENCE.to_owned(),
            project: project.map(str::to_owned),
            build: Some("2026.9.12+1".to_owned()),
            started_at_ms: Some(1_757_000_000_000),
            session_revision: 4,
            windows: vec![kernel::Window {
                label: "main".to_owned(),
                project: project.map(str::to_owned),
                generation: 1,
            }],
        }
    }

    fn live(instance: &str, handshake: Handshake) -> discover::Live {
        discover::Live {
            found: discover::Found {
                profile: "canary",
                path: std::path::PathBuf::from("/tmp/cli-bridge.d/x.json"),
                descriptor: discover::Descriptor {
                    url: "http://127.0.0.1:41234".to_owned(),
                    token: "0123456789abcdef0123456789abcdef".to_owned(),
                    pid: 4711,
                    instance_id: instance.to_owned(),
                    identity: kernel::Identity::Minted,
                    profile: Some("canary".to_owned()),
                    path: std::path::PathBuf::from("/tmp/cli-bridge.d/x.json"),
                    window: None,
                },
            },
            handshake,
            session: json!({}),
        }
    }

    #[test]
    fn a_listing_names_no_token_no_address_and_no_account() {
        let enumeration = Enumeration {
            live: vec![
                live(
                    TWO,
                    Handshake::Session(Box::new(candidate(TWO, "uid-b", Some("project-b")))),
                ),
                live(
                    ONE,
                    Handshake::Session(Box::new(candidate(ONE, "uid-a", Some("project-a")))),
                ),
            ],
            unusable: Vec::new(),
            omitted: 0,
        };
        let requirement = discover::Requirement {
            lane: "canary".to_owned(),
            uid: "uid-a".to_owned(),
            audience_sha256: AUDIENCE.to_owned(),
            project: None,
            project_independent: true,
        };
        let data = data(&enumeration, Some(&requirement));
        let rendered = serde_json::to_string(&data).expect("encodes");
        for secret in [
            "0123456789abcdef",
            "127.0.0.1",
            "uid-a",
            "uid-b",
            AUDIENCE,
            "/tmp/cli-bridge.d",
        ] {
            assert!(!rendered.contains(secret), "{rendered} named {secret}");
        }
        // Ordered by identity, and only this caller's own instance is theirs.
        assert_eq!(data["instances"][0]["instance_id"], json!(ONE));
        assert_eq!(data["instances"][1]["instance_id"], json!(TWO));
        assert_eq!(data["compatible"], json!([ONE]));
        assert_eq!(data["live"], json!(2));
        assert!(render(&data).contains("← yours"));
    }

    #[test]
    fn an_instance_that_cannot_serve_work_is_listed_with_its_state_and_no_session() {
        let enumeration = Enumeration {
            live: vec![
                live(ONE, Handshake::Unusable(Unusable::SignedOut)),
                live(TWO, Handshake::Unusable(Unusable::Contract)),
            ],
            unusable: vec![(
                std::path::PathBuf::from("/tmp/cli-bridge.json"),
                "descriptor is not valid JSON".to_owned(),
            )],
            omitted: 0,
        };
        let data = data(&enumeration, None);
        assert_eq!(data["instances"][0]["state"], json!("signed_out"));
        assert_eq!(data["instances"][1]["state"], json!("contract_mismatch"));
        assert!(data["instances"][0].get("project").is_none());
        assert_eq!(data["compatible"], Value::Null);
        assert_eq!(
            data["unusable"][0]["reason"],
            json!("descriptor is not valid JSON")
        );
        assert!(render(&data).contains("unusable  /tmp/cli-bridge.json"));
    }

    #[test]
    fn nothing_running_is_an_answer() {
        let data = data(&Enumeration::default(), None);
        assert_eq!(data["live"], json!(0));
        assert_eq!(data["instances"], json!([]));
        assert!(render(&data).contains("no DS GridDesign instance is running"));
    }
}
