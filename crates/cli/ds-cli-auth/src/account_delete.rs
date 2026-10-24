//! `ds account delete --yes` — delete the signed-in DS account.
//!
//! Everything a person can do in the app, the CLI does (owner, 24 Oct: Google
//! Play requires an account-deletion path wherever an account is created).
//! ds-brain deletes the account the lane's credential belongs to — profile,
//! personal notes and uploads, notifications, devices, project memberships and
//! the sign-in itself — and keeps records that belong to organisation projects
//! (ds-brain `docs/contracts/account-deletion.md`). The request names no
//! account: it carries only the fixed confirmation phrase, and `--yes` is the
//! person's explicit confirmation. Once the account is gone this lane's local
//! credentials can never work again, so they are forgotten here.
//!
//! Human-only: MCP never publishes this command.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::account_deletion::Receipt;
use ds_client_core::{ClientError, ErrorKind};
use serde_json::{Value, json};

use crate::profile::Lane;

const LANE: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::LANE.summary("Deployment lane; stable is the default.");

pub(crate) const INCOMPLETE: Refusal = Refusal {
    code: "account_delete_incomplete",
    when: "ds-brain stopped part-way; detail.step names where",
    remedy: "run the same command again; finished steps are not repeated",
};
pub(crate) const UNAVAILABLE: Refusal = Refusal {
    code: "account_delete_unavailable",
    when: "the deletion service or the gateway is unavailable; nothing was deleted",
    remedy: "retry later",
};
pub(crate) const NOT_ACCEPTED: Refusal = Refusal {
    code: "account_delete_not_accepted",
    when: "the gateway did not accept this lane's credential (the account may already be gone)",
    remedy: "run ds account connect, then retry if the account still exists",
};
pub(crate) const IDENTITY_INVALID: Refusal = Refusal {
    code: "account_delete_identity_invalid",
    when: "the signed-in identity cannot address its own account data",
    remedy: "sign in again with the account to delete",
};
pub(crate) const REQUEST_INVALID: Refusal = Refusal {
    code: "account_delete_request_invalid",
    when: "ds-brain refused the request shape this ds sends",
    remedy: "update ds; nothing was deleted",
};
pub(crate) const ROUTE_UNAVAILABLE: Refusal = Refusal {
    code: "account_delete_route_unavailable",
    when: "this lane's gateway does not serve account deletion yet",
    remedy: "delete the account in the DS app or at datasolutions.rw/delete-account",
};
pub(crate) const UNREADABLE: Refusal = Refusal {
    code: "account_delete_response_unreadable",
    when: "the deletion answer is not a receipt that says the account is deleted",
    remedy: "run the same command again; it answers the existing receipt",
};

const REFUSALS: &[Refusal] = &[
    crate::account::SIGNED_OUT_REFUSAL,
    crate::PROFILE_REFUSAL,
    crate::STATE_REFUSAL,
    INCOMPLETE,
    UNAVAILABLE,
    NOT_ACCEPTED,
    IDENTITY_INVALID,
    REQUEST_INVALID,
    ROUTE_UNAVAILABLE,
    UNREADABLE,
];

pub static DELETE_COMMAND: Command = Command {
    id: "account.delete",
    path: &["account", "delete"],
    contract: 1,
    chapter: Chapter::Project,
    summary: "Delete your DS account and its personal data; irreversible.",
    purpose: "Deletes the signed-in account through ds-brain: profile, personal notes and uploads, notifications, devices, project memberships and the sign-in. Records in organisation projects stay with those projects. Then forgets this lane's local credentials. Re-run to finish an interrupted deletion.",
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[LANE],
    output: "The audited receipt (id, time, deleted counts, retained record classes) and whether local credentials were forgotten.",
    examples: &[Example {
        command: "ds account delete --yes",
        note: "Deletes the account signed in on the stable lane.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/auth.md"),
    search: &[
        "close account",
        "erase data",
        "forget me",
        "gdpr",
        "google play",
    ],
    requires: Requires::Server,
    availability: crate::native_availability,
};

/// The declared refusal a route answer stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Incomplete,
    Unavailable,
    NotAccepted,
    IdentityInvalid,
    RequestInvalid,
    RouteUnavailable,
    Unreadable,
}

fn failure(kind: Kind, message: Option<&str>) -> Failure {
    let (failure, remedy) = match kind {
        Kind::Incomplete => (
            Failure::unavailable(INCOMPLETE.code, message.unwrap_or(INCOMPLETE.when)),
            INCOMPLETE.remedy,
        ),
        Kind::Unavailable => (
            Failure::unavailable(UNAVAILABLE.code, message.unwrap_or(UNAVAILABLE.when)),
            UNAVAILABLE.remedy,
        ),
        Kind::NotAccepted => (
            Failure::unauthorized(NOT_ACCEPTED.code, message.unwrap_or(NOT_ACCEPTED.when)),
            NOT_ACCEPTED.remedy,
        ),
        Kind::IdentityInvalid => (
            Failure::unauthorized(
                IDENTITY_INVALID.code,
                message.unwrap_or(IDENTITY_INVALID.when),
            ),
            IDENTITY_INVALID.remedy,
        ),
        Kind::RequestInvalid => (
            Failure::invalid(
                REQUEST_INVALID.code,
                message.unwrap_or(REQUEST_INVALID.when),
            ),
            REQUEST_INVALID.remedy,
        ),
        Kind::RouteUnavailable => (
            Failure::unavailable(
                ROUTE_UNAVAILABLE.code,
                message.unwrap_or(ROUTE_UNAVAILABLE.when),
            ),
            ROUTE_UNAVAILABLE.remedy,
        ),
        Kind::Unreadable => (
            Failure::unavailable(UNREADABLE.code, message.unwrap_or(UNREADABLE.when)),
            UNREADABLE.remedy,
        ),
    };
    failure.remedy(remedy)
}

/// One client error as the refusal this command declares for it.
pub(crate) fn map(error: ClientError) -> Failure {
    if let Some(service) = error.service_refusal() {
        let kind = match (service.status(), service.code()) {
            (_, Some("account_delete_incomplete")) => Kind::Incomplete,
            (_, Some("account_delete_unavailable")) => Kind::Unavailable,
            (_, Some("account_delete_identity_invalid")) => Kind::IdentityInvalid,
            (
                _,
                Some("account_delete_request_invalid" | "account_delete_confirmation_required"),
            ) => Kind::RequestInvalid,
            (401 | 403, _) => Kind::NotAccepted,
            (404, None) => Kind::RouteUnavailable,
            (400, _) => Kind::RequestInvalid,
            _ => Kind::Unavailable,
        };
        let mut out = failure(kind, service.message());
        if let Some(step) = service.detail_text("step") {
            out = out.detail(json!({"step": step, "http_status": service.status()}));
        }
        return out;
    }
    match error.kind() {
        ErrorKind::UnreadableResponse => failure(Kind::Unreadable, None),
        ErrorKind::RouteUnavailable => failure(Kind::RouteUnavailable, None),
        _ => crate::map_client(error),
    }
}

pub fn run_delete(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = Lane::parse(inputs.require("lane")?)?;
    let (receipt, provider) = delete_account(lane)?;
    let forgotten = forget_deleted_account(lane);
    Ok(answer(lane, &receipt, provider, forgotten))
}

/// Delete the account of the lane's credential: its device credential when
/// it holds one, otherwise the restored native user.
fn delete_account(lane: Lane) -> Result<(Receipt, &'static str), Failure> {
    if let Some(mut device) = crate::restored_device_session(lane)? {
        return Ok((device.delete_account().map_err(map)?, "ds_device"));
    }
    let profile = crate::profile::load(lane)?;
    let store = crate::NativeRefreshStore::open()?;
    let mut client = crate::Client::new(profile, crate::NativeTransport, store);
    crate::require_restore_before_context(&mut client)?;
    let receipt = client.delete_account(crate::now()).map_err(map)?;
    Ok((receipt, "firebase"))
}

/// Forget every local credential and the project context of a lane whose
/// account is gone. Answers whether all of it was forgotten.
fn forget_deleted_account(lane: Lane) -> bool {
    let device = crate::device::forget_credential(lane).is_ok();
    let native = (|| -> Result<(), Failure> {
        let profile = crate::profile::load(lane)?;
        let store = crate::NativeRefreshStore::open()?;
        let mut client = crate::Client::new(profile, crate::NativeTransport, store);
        let context = crate::ProjectContextLease::acquire(client.profile())?;
        client.sign_out().map_err(crate::map_client)?;
        context.clear().map_err(|_| crate::cleanup_required())
    })()
    .is_ok();
    device && native
}

pub(crate) fn answer(lane: Lane, receipt: &Receipt, provider: &str, forgotten: bool) -> Value {
    let mut out = json!({
        "lane": lane.token(),
        "deleted": true,
        "credential_provider": provider,
        "receipt": receipt,
        "local_credentials_forgotten": forgotten,
    });
    if !forgotten {
        out["next"] = json!(format!("ds auth logout --lane {}", lane.token()));
    }
    out
}

pub fn render_delete(data: &Value) -> String {
    let receipt = &data["receipt"];
    let deleted = &receipt["deleted"];
    let mut text = format!(
        "account deleted ({})  receipt {}{}\n  removed  {} profile records, {} memberships, {} files, {} devices\n  kept     records in organisation projects, audit and licence records\n",
        data["lane"].as_str().unwrap_or(""),
        receipt["receipt_id"].as_str().unwrap_or(""),
        if receipt["already_deleted"] == true {
            " (already deleted)"
        } else {
            ""
        },
        deleted["profile_documents"],
        deleted["project_memberships"],
        deleted["storage_objects"],
        deleted["device_credentials"],
    );
    if let Some(next) = data["next"].as_str() {
        text.push_str(&format!("  local credentials remain; run: {next}\n"));
    }
    text
}

#[cfg(test)]
#[path = "account_delete_tests.rs"]
mod tests;
