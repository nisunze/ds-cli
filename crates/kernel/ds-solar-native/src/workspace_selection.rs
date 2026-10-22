//! One declared native Core-owned selection. Existing opaque workspace receipts
//! remain the filesystem authority; no browser cache or default-on-refusal path.
use crate::{NativeHost, native, workspace};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Approved,
    Default,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub workspace_handle: String,
    pub dir: String,
    pub origin: Origin,
    pub generation: u64,
}
/// Injected by Core under its captured native owner/lane/project. This is a
/// closed selection effect, never arbitrary keys, SQL or filesystem approval.
pub trait Provider: Send + Sync {
    fn read(&self, project: &str) -> Result<Option<Selection>, String>;
    fn compare_set(
        &self,
        project: &str,
        expected_generation: Option<u64>,
        workspace: &workspace::NativeSolarWorkspace,
        origin: Origin,
    ) -> Result<Selection, String>;
}
fn provider(host: &NativeHost) -> Result<&dyn Provider, String> {
    host.workspace_selection.as_deref().ok_or_else(|| {
        "solar_workspace_selection_unavailable: this Core has no protected selection owner".into()
    })
}
fn validate(host: &NativeHost, project: &str, selected: &Selection) -> Result<(), String> {
    if selected.generation == 0
        || selected.generation > i64::MAX as u64
        || selected.dir.len() > 4096
        || !workspace::valid_workspace_handle(&selected.workspace_handle)
    {
        return Err("solar_workspace_selection_invalid: malformed retained selection".into());
    }
    // This intentionally reads zero cities: no engine, preparation, acquisition,
    // directory creation or calculation is admitted by workspace validation.
    native::solar_native_readiness(
        host.clone(),
        native::ReadinessCommand {
            workspace_handle: selected.workspace_handle.clone(),
            root: format!("eds_project/{project}/eds_solar"),
            city_ids: Vec::new(),
        },
    )?;
    let resolved = workspace::resolve_workspace_handle(host, &selected.workspace_handle)?;
    if resolved.to_string_lossy() != selected.dir {
        return Err("solar_workspace_selection_invalid: approved directory differs from retained selection; choose a workspace explicitly".into());
    }
    Ok(())
}
fn prior(host: &NativeHost, project: &str, expected: Option<u64>) -> Result<(), String> {
    let current = provider(host)?.read(project)?;
    if current.as_ref().map(|s| s.generation) != expected {
        return Err(
            "solar_workspace_selection_changed: read the current selection before choosing again"
                .into(),
        );
    }
    Ok(())
}
fn admit_result(
    selected: &Selection,
    workspace: &workspace::NativeSolarWorkspace,
    expected: Option<u64>,
    origin: Origin,
) -> Result<(), String> {
    if selected.workspace_handle != workspace.workspace_handle
        || selected.dir != workspace.dir
        || selected.origin != origin
        || selected.generation
            != expected
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("Solar selection generation exhausted")?
    {
        return Err("solar_workspace_selection_invalid: returned selection differs from the exact approved intent".into());
    }
    Ok(())
}
pub fn read(host: &NativeHost, project: &str) -> Result<Option<Selection>, String> {
    let current = provider(host)?.read(project)?;
    if let Some(selected) = &current {
        validate(host, project, selected)?;
    }
    if provider(host)?.read(project)? != current {
        return Err(
            "solar_workspace_selection_changed: selection changed during its receipt validation"
                .into(),
        );
    }
    Ok(current)
}
pub fn select(
    host: &NativeHost,
    project: &str,
    handle: String,
    expected: Option<u64>,
) -> Result<Selection, String> {
    if !workspace::valid_workspace_handle(&handle) {
        return Err(
            "solar_workspace_selection_invalid: an approved opaque handle is required".into(),
        );
    }
    prior(host, project, expected)?;
    let resolved = workspace::resolve_workspace_handle(host, &handle)?;
    let choice = workspace::NativeSolarWorkspace {
        workspace_handle: handle,
        dir: resolved.to_string_lossy().into_owned(),
    };
    validate(
        host,
        project,
        &Selection {
            workspace_handle: choice.workspace_handle.clone(),
            dir: choice.dir.clone(),
            origin: Origin::Approved,
            generation: expected
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("Solar selection generation exhausted")?,
        },
    )?;
    let selected = provider(host)?.compare_set(project, expected, &choice, Origin::Approved)?;
    admit_result(&selected, &choice, expected, Origin::Approved)?;
    validate(host, project, &selected)?;
    if provider(host)?.read(project)?.as_ref() != Some(&selected) {
        return Err("solar_workspace_selection_changed: selection changed before adoption".into());
    }
    Ok(selected)
}
pub fn select_default(
    host: &NativeHost,
    project: &str,
    expected: Option<u64>,
) -> Result<Selection, String> {
    prior(host, project, expected)?; // Stale/occupied selection refuses BEFORE default creation.
    let choice = workspace::solar_native_prepare_default_workspace(host.clone())?;
    let selected = provider(host)?.compare_set(project, expected, &choice, Origin::Default)?;
    admit_result(&selected, &choice, expected, Origin::Default)?;
    validate(host, project, &selected)?;
    if provider(host)?.read(project)?.as_ref() != Some(&selected) {
        return Err("solar_workspace_selection_changed: selection changed before adoption".into());
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Memory(Mutex<Option<Selection>>);
    impl Provider for Memory {
        fn read(&self, _project: &str) -> Result<Option<Selection>, String> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn compare_set(
            &self,
            _project: &str,
            expected: Option<u64>,
            workspace: &workspace::NativeSolarWorkspace,
            origin: Origin,
        ) -> Result<Selection, String> {
            let mut held = self.0.lock().unwrap();
            if held.as_ref().map(|s| s.generation) != expected {
                return Err("selection changed".into());
            }
            let next = Selection {
                workspace_handle: workspace.workspace_handle.clone(),
                dir: workspace.dir.clone(),
                origin,
                generation: expected.unwrap_or(0) + 1,
            };
            *held = Some(next.clone());
            Ok(next)
        }
    }
    fn host() -> (tempfile::TempDir, NativeHost, Arc<Memory>) {
        let temp = tempfile::tempdir().unwrap();
        let state = Arc::new(Memory::default());
        let host = NativeHost::headless(temp.path().join("native"))
            .unwrap()
            .with_workspace_selection_provider(state.clone());
        (temp, host, state)
    }
    #[test]
    fn absence_is_readonly_and_never_initializes_a_default() {
        let (temp, host, _) = host();
        assert_eq!(read(&host, "project-a").unwrap(), None);
        assert!(!temp.path().join("native/solar-workspaces").exists());
    }
    #[test]
    fn selected_custom_receipt_is_reused_and_zero_city_validation_creates_no_prepared_tree() {
        let (temp, host, _) = host();
        let custom = temp.path().join("custom");
        std::fs::create_dir(&custom).unwrap();
        let approval =
            workspace::solar_native_choose_workspace(host.clone(), custom.clone()).unwrap();
        let selected = select(&host, "project-a", approval.workspace_handle, None).unwrap();
        assert_eq!(selected.origin, Origin::Approved);
        assert_eq!(read(&host, "project-a").unwrap(), Some(selected));
        assert!(!custom.join("solar-rust").exists());
        assert!(
            !temp
                .path()
                .join("native/solar-workspaces/workspace")
                .exists()
        );
    }
    #[test]
    fn unavailable_selected_receipt_refuses_and_preserves_row_without_default() {
        let (temp, host, state) = host();
        let custom = temp.path().join("custom");
        std::fs::create_dir(&custom).unwrap();
        let approval = workspace::solar_native_choose_workspace(host.clone(), custom).unwrap();
        let selected = select(&host, "project-a", approval.workspace_handle, None).unwrap();
        std::fs::remove_file(
            temp.path()
                .join("native/solar-workspaces")
                .join(format!("{}.json", selected.workspace_handle)),
        )
        .unwrap();
        assert!(read(&host, "project-a").is_err());
        assert_eq!(state.read("project-a").unwrap(), Some(selected));
        assert!(
            !temp
                .path()
                .join("native/solar-workspaces/workspace")
                .exists()
        );
    }
    #[test]
    fn stale_default_refuses_before_creation_and_explicit_successor_uses_same_owner() {
        let (temp, host, _) = host();
        let custom = temp.path().join("custom");
        std::fs::create_dir(&custom).unwrap();
        let approval = workspace::solar_native_choose_workspace(host.clone(), custom).unwrap();
        let selected = select(&host, "project-a", approval.workspace_handle, None).unwrap();
        assert!(select_default(&host, "project-a", None).is_err());
        assert!(
            !temp
                .path()
                .join("native/solar-workspaces/workspace")
                .exists()
        );
        let default = select_default(&host, "project-a", Some(selected.generation)).unwrap();
        assert_eq!(default.origin, Origin::Default);
        assert_eq!(default.generation, 2);
        assert_eq!(read(&host, "project-a").unwrap(), Some(default));
    }
    #[test]
    fn missing_provider_or_unapproved_handle_never_creates_workspace_authority() {
        let temp = tempfile::tempdir().unwrap();
        let host = NativeHost::headless(temp.path().join("native")).unwrap();
        assert!(select_default(&host, "project-a", None).is_err());
        assert!(!temp.path().join("native/solar-workspaces").exists());
        let (_, host, _) = self::host();
        assert!(select(&host, "project-a", "../custom".into(), None).is_err());
        assert!(!host.data_dir().unwrap().join("solar-workspaces").exists());
    }
    #[test]
    fn another_host_cannot_adopt_a_stored_handle() {
        let (temp, host, state) = host();
        let custom = temp.path().join("custom");
        std::fs::create_dir(&custom).unwrap();
        let approval = workspace::solar_native_choose_workspace(host.clone(), custom).unwrap();
        select(&host, "project-a", approval.workspace_handle, None).unwrap();
        let other = NativeHost::headless(temp.path().join("other"))
            .unwrap()
            .with_workspace_selection_provider(state);
        assert!(read(&other, "project-a").is_err());
        assert!(!temp.path().join("other/solar-workspaces").exists());
    }
}
