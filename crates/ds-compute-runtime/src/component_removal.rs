//! Native Core's job-only component lifecycle boundary.
use crate::{open, HostIdentity};
use ds_command_kernel::{module_install::ActivationGuard, module_release::ModuleComponent};
use std::path::Path;

/// Hold the same admission writer transaction as native engine capture while
/// observing pending pins and atomically removing one component selection.
/// The identity and database path belong to Core, never the request payload.
/// The callback only locks/writes the module store; it must not reenter the
/// database, acquire an engine, make network calls, or inspect Desktop.
pub fn with_component_removal_admission<T>(
    path: &Path,
    identity: &HostIdentity,
    component: ModuleComponent,
    remove: impl FnOnce(ActivationGuard) -> Result<T, String>,
) -> Result<T, String> {
    let mut store = open(path)?;
    store
        .with_core_component_removal(&identity.caller(None), component, |guard| {
            remove(guard).map_err(ds_sync_store::Error::Kernel)
        })
        .map_err(|error| match error {
            // Owner refusals retain their exact code and remedy for the typed
            // Core transport; database failures remain explicit host faults.
            ds_sync_store::Error::Kernel(message) => message,
            ds_sync_store::Error::Refused(refusal) => {
                format!("{}: {}", refusal.code, refusal.message)
            }
            other => other.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::execution_context::Principal;
    use std::cell::Cell;

    fn identity() -> HostIdentity {
        HostIdentity {
            owner: "core-owner".into(),
            principal: Principal {
                uid: "core-user".into(),
                lane: "stable".into(),
                deployment: "https://core.example".into(),
                install_id: "core-install".into(),
            },
        }
    }

    #[test]
    fn native_wrapper_returns_owner_errors_exactly_and_releases_admission() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("store.sqlite");
        let error = with_component_removal_admission::<()>(
            &path,
            &identity(),
            ModuleComponent::Network,
            |guard| {
                guard.admit()?;
                Err("module_selection_stale: retain the current generation".into())
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            "module_selection_stale: retain the current generation"
        );
        let result = with_component_removal_admission(
            &path,
            &identity(),
            ModuleComponent::Network,
            |guard| {
                guard.admit()?;
                Ok("removed")
            },
        )
        .unwrap();
        assert_eq!(result, "removed");
    }

    #[test]
    fn invalid_native_lane_never_calls_selection_removal() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("store.sqlite");
        let mut identity = identity();
        identity.principal.lane = "foreign".into();
        let called = Cell::new(false);
        let error =
            with_component_removal_admission(&path, &identity, ModuleComponent::Solar, |_| {
                called.set(true);
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error, "component_removal_context_invalid");
        assert!(!called.get());
    }
}
