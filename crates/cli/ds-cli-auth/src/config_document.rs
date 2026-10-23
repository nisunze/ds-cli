//! One complete configuration read through the CLI's existing closed native
//! client and the Kernel's retained ProjectConfig flow. Both Desktop and CLI
//! use Core's protected Store; held reads never restore or refresh credentials.
use std::{
    cell::RefCell,
    future::Future,
    path::Path,
    task::{Context, Poll, Waker},
};

use ds_cli_contract::{Failure, spec::Refusal as DeclaredRefusal};
use ds_command_kernel::{
    durable_store::{Call, Identity, Principal},
    gateway_exchange::{
        Admitted, Exchange, Refusal,
        design::{
            DesignCall, ProjectConfigurationDocument, ProjectConfigurationObservation,
            ProjectConfigurationRead, ProjectConfigurationReply,
        },
        held_reads::project_config,
        retained::{self, Edge},
    },
};
use ds_sync_store::Store;
use serde_json::Value;

pub const NOT_HELD: DeclaredRefusal = DeclaredRefusal {
    code: "project_config_not_held",
    when: "Core holds no configuration for this account, lane and project",
    remedy: "read this project configuration once with --document --read refresh; held remains available offline afterward",
};
pub const CONTEXT_CHANGED: DeclaredRefusal = DeclaredRefusal {
    code: "project_config_context_changed",
    when: "the protected account, lane or credential audience changed during the read",
    remedy: "restore the intended account and repeat the explicit project read",
};
pub const STORE_REFUSED: DeclaredRefusal = DeclaredRefusal {
    code: "project_config_store_refused",
    when: "the protected native Store cannot read or retain the configuration",
    remedy: "resolve the named protected Store refusal; retained work is left in place",
};
pub const STALE: DeclaredRefusal = DeclaredRefusal {
    code: "retained_reads_stale",
    when: "configuration was invalidated while its refresh was in flight",
    remedy: "repeat the explicit refresh after the configuration save completes",
};
pub const ANSWER_INVALID: DeclaredRefusal = DeclaredRefusal {
    code: "design_answer_invalid",
    when: "the complete configuration response does not match the shared Rust document contract",
    remedy: "keep the held configuration and retry the explicit refresh after the gateway response is corrected",
};

fn failure(declared: DeclaredRefusal, message: impl ToString) -> Failure {
    Failure::failed(declared.code, message.to_string()).remedy(declared.remedy)
}
fn changed() -> Failure {
    Failure::conflict(
        CONTEXT_CHANGED.code,
        "the protected configuration context changed",
    )
    .remedy(CONTEXT_CHANGED.remedy)
}
fn refusal(error: Failure) -> Refusal {
    let mut refusal = Refusal::new(error.code(), error.message());
    refusal.status = error
        .detail_value()
        .and_then(|detail| detail["http_status"].as_u64())
        .and_then(|status| u16::try_from(status).ok());
    refusal.retryable = error.retryable();
    refusal.transient = refusal.retryable;
    refusal
}
fn identity(lane: &str) -> Result<Identity, Failure> {
    let principal = super::headless_principal_read_only(lane)?;
    Ok(Identity {
        principal: Principal::Account(principal.account_uid().into()),
        lane: lane.into(),
        audience: principal.job_owner()?,
    })
}

/// The exact same identity fence and database location as `core.store`.
pub fn read(
    lane: &str,
    project: &str,
    mode: ProjectConfigurationRead,
) -> Result<ProjectConfigurationReply, Failure> {
    validate_project(project)?;
    let database = ds_compute_runtime::server_state_directory(lane, None)
        .map_err(|error| failure(STORE_REFUSED, error))?
        .join("store.sqlite");
    let expected = identity(lane)?;
    read_captured_at(
        &database,
        project,
        mode,
        &expected,
        || identity(lane),
        || {
            let fresh = super::settings_configuration_receipt(
                lane,
                project,
                ds_client_core::ProjectConfigurationChange::ReadSettings,
            )?;
            let provider = fresh.identity();
            let actual = Identity {
                principal: Principal::Account(provider.uid().into()),
                lane: provider.lane().into(),
                audience: super::job_owner_fence(
                    provider.uid(),
                    provider.lane(),
                    provider.credential_audience_sha256(),
                )?,
            };
            if actual != expected || fresh.project_id() != project {
                return Err(changed());
            }
            Ok(fresh.into_result().document)
        },
    )
}

/// Forget the held configuration after a native configuration change. The
/// next held-first read acquires the saved document once; a refresh fetched
/// before the change is refused (`retained_reads_stale`) instead of kept.
pub fn forget(lane: &str, project: &str) -> Result<(), Failure> {
    validate_project(project)?;
    let database = ds_compute_runtime::server_state_directory(lane, None)
        .map_err(|error| failure(STORE_REFUSED, error))?
        .join("store.sqlite");
    if !database
        .try_exists()
        .map_err(|error| failure(STORE_REFUSED, error))?
    {
        return Ok(());
    }
    forget_at(&database, project, &identity(lane)?)
}

fn forget_at(database: &Path, project: &str, identity: &Identity) -> Result<(), Failure> {
    let cleared = |error: String| {
        failure(
            STORE_REFUSED,
            format!(
                "the configuration change was saved, but this device's held copy could not be cleared: {error}"
            ),
        )
    };
    let edge = NativeEdge {
        store: RefCell::new(Store::open(database).map_err(|error| cleared(error.to_string()))?),
        identity: identity.clone(),
        project,
        observe: || Ok(identity.clone()),
        fetch: || {
            Err(failure(
                STORE_REFUSED,
                "forgetting held configuration fetches nothing",
            ))
        },
        original_failure: RefCell::new(None),
    };
    ready(project_config::forget(&edge, project)).map_err(|error| cleared(error.message))
}

struct NativeEdge<'a, O, F> {
    store: RefCell<Store>,
    identity: Identity,
    project: &'a str,
    observe: O,
    fetch: F,
    original_failure: RefCell<Option<Failure>>,
}
impl<O, F> NativeEdge<'_, O, F>
where
    O: Fn() -> Result<Identity, Failure>,
    F: Fn() -> Result<Value, Failure>,
{
    fn current(&self) -> Result<(), Refusal> {
        if (self.observe)().map_err(refusal)? != self.identity {
            return Err(refusal(changed()));
        }
        Ok(())
    }
}
impl<O, F> Edge for NativeEdge<'_, O, F>
where
    O: Fn() -> Result<Identity, Failure>,
    F: Fn() -> Result<Value, Failure>,
{
    async fn fetch<C: Exchange>(&self, call: C) -> Result<C::Output, Refusal> {
        // This private port serves exactly one declared call. It never exposes
        // an arbitrary native HTTP request or a second configuration route.
        let expected = DesignCall::ProjectConfig {
            project: self.project.into(),
        }
        .plan()?;
        if call.plan()? != expected {
            return Err(Refusal::new(
                "config_input_invalid",
                "the native port serves only this project's configuration",
            ));
        }
        self.current()?;
        let document = (self.fetch)().map_err(|error| {
            let answer = refusal(error.clone());
            *self.original_failure.borrow_mut() = Some(error);
            answer
        })?;
        self.current()?;
        match call.admit(200, document)? {
            Admitted::Done(answer) => Ok(answer),
            Admitted::Next(_) => Err(Refusal::new(
                ANSWER_INVALID.code,
                "configuration is one complete document",
            )),
        }
    }
    async fn store(&self, call: Call) -> Result<Value, Refusal> {
        self.current()?;
        let answer = self
            .store
            .borrow_mut()
            .durable_store(&self.identity, call, None)
            .map_err(|error| match error {
                ds_sync_store::Error::Kernel(text) => retained::store_refusal(text),
                other => Refusal::new(STORE_REFUSED.code, other.to_string()),
            })?;
        self.current()?;
        serde_json::to_value(answer)
            .map_err(|error| Refusal::new(STORE_REFUSED.code, error.to_string()))
    }
    fn now_ms(&self) -> u64 {
        super::now().saturating_mul(1000)
    }
    fn online(&self) -> bool {
        true
    }
}

/// All native port effects above complete synchronously. Polling this private
/// flow once requires no runtime, thread, sleep or freshness timer.
fn ready<T>(future: impl Future<Output = Result<T, Refusal>>) -> Result<T, Refusal> {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(answer) => answer,
        Poll::Pending => Err(Refusal::new(
            STORE_REFUSED.code,
            "the synchronous configuration port unexpectedly suspended",
        )),
    }
}
#[cfg(test)]
fn read_at(
    database: &Path,
    project: &str,
    mode: ProjectConfigurationRead,
    observe: impl Fn() -> Result<Identity, Failure>,
    fetch: impl Fn() -> Result<Value, Failure>,
) -> Result<ProjectConfigurationReply, Failure> {
    validate_project(project)?;
    let expected = observe()?;
    read_captured_at(database, project, mode, &expected, observe, fetch)
}
fn validate_project(project: &str) -> Result<(), Failure> {
    DesignCall::ProjectConfig {
        project: project.into(),
    }
    .plan()
    .map_err(|error| Failure::invalid("config_input_invalid", error.message))?;
    Ok(())
}
fn read_captured_at(
    database: &Path,
    project: &str,
    mode: ProjectConfigurationRead,
    expected: &Identity,
    observe: impl Fn() -> Result<Identity, Failure>,
    fetch: impl Fn() -> Result<Value, Failure>,
) -> Result<ProjectConfigurationReply, Failure> {
    expected
        .validate()
        .map_err(|error| failure(STORE_REFUSED, error))?;
    if mode == ProjectConfigurationRead::Held
        && !database
            .try_exists()
            .map_err(|error| failure(STORE_REFUSED, error))?
    {
        return Err(failure(
            NOT_HELD,
            "configuration is not held for this account and project",
        ));
    }
    let edge = NativeEdge {
        store: RefCell::new(Store::open(database).map_err(|error| failure(STORE_REFUSED, error))?),
        identity: expected.clone(),
        project,
        observe,
        fetch,
        original_failure: RefCell::new(None),
    };
    let (shown, source) = match mode {
        ProjectConfigurationRead::Held => ready(project_config::held(&edge, project))
            .map_err(|error| failure(STORE_REFUSED, error))?
            .map(|held| (held, ProjectConfigurationRead::Held))
            .ok_or_else(|| {
                failure(
                    NOT_HELD,
                    "configuration is not held for this account and project",
                )
            })?,
        ProjectConfigurationRead::Refresh | ProjectConfigurationRead::Acquire => ready(async {
            if mode == ProjectConfigurationRead::Acquire {
                if let Some(held) = project_config::held(&edge, project).await? {
                    return Ok((held, ProjectConfigurationRead::Held));
                }
            }
            Ok((
                project_config::refresh(&edge, project).await?,
                ProjectConfigurationRead::Refresh,
            ))
        })
        .map_err(|error| {
            if let Some(original) = edge
                .original_failure
                .borrow_mut()
                .take()
                .filter(|original| original.code() == error.code)
            {
                return original;
            }
            if error.code == CONTEXT_CHANGED.code {
                return changed();
            }
            let declared = if error.code == STALE.code {
                STALE
            } else if error.code == ANSWER_INVALID.code {
                ANSWER_INVALID
            } else {
                STORE_REFUSED
            };
            failure(declared, error)
        })?,
    };
    edge.current().map_err(|_| changed())?;
    let document: ProjectConfigurationDocument =
        serde_json::from_value(shown.value).map_err(|error| failure(ANSWER_INVALID, error))?;
    Ok(ProjectConfigurationReply {
        project: project.into(),
        document,
        observation: ProjectConfigurationObservation {
            source,
            saved_at_ms: shown.saved_at_ms,
        },
    })
}

#[cfg(test)]
#[path = "config_document_tests.rs"]
mod tests;
