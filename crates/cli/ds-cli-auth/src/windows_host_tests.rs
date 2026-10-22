use std::{fs, path::Path};
    use super::*;
    use ds_command_kernel::module_install::ActivationGuard;
    use ds_command_kernel::module_release::ui::UiComponent;
    use ds_update_runtime::modules::ui::{MAX_MEMBER_CHUNK, UiModuleStore};

    struct FixtureDirectory(std::path::PathBuf);
    impl FixtureDirectory {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn private_fixture() -> (FixtureDirectory, UiModuleStore) {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        register_ui_store_owner().unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // The managed sandbox grants foreign SIDs writes to TEMP; the native
        // owner correctly refuses it. Use its safe current-user known folder.
        let base = FixtureDirectory(local_application_data().unwrap().join(format!(
            "ds-ui-native-{}-{nonce}-{sequence}",
            std::process::id()
        )));
        prepare_directory(base.path()).unwrap();
        let store = UiModuleStore::new(base.path(), UiComponent::Interactions, "canary").unwrap();
        (base, store)
    }
    fn grant_read(path: &Path) {
        let result = std::process::Command::new("icacls.exe")
            .arg(path)
            .args(["/grant", "*S-1-1-0:(R)"])
            .output()
            .unwrap();
        assert!(result.status.success(), "native ACL fixture failed");
    }
    #[test]
    fn windows_ui_store_reuses_sid_owner_and_refuses_permissive_retained_files() {
        let (base, store) = private_fixture();
        let root = base.path().join("interactions/canary");
        validate_directory(&root).unwrap();
        assert!(store.selection().unwrap().is_none());
        let file = root.join("foreign-readable.json");
        fs::write(&file, b"opaque fixture").unwrap();
        drop(read_file(&file).unwrap());
        grant_read(&file);
        assert!(
            store.selection().is_err(),
            "known filenames must not bypass a permissive child DACL"
        );
        assert_eq!(fs::read(&file).unwrap(), b"opaque fixture");
    }
    #[test]
    fn windows_ui_store_refuses_permissive_directories_without_repair() {
        let (base, store) = private_fixture();
        let component = base.path().join("interactions");
        grant_read(&component);
        assert!(store.selection().is_err());
        assert!(
            validate_directory(&component).is_err(),
            "refusal must preserve the existing ACL"
        );
    }
    #[test]
    fn windows_ui_store_single_owner_lock_is_native() {
        let (base, store) = private_fixture();
        let path = base.path().join("interactions/canary/update.lock");
        let held = open_lock(&path).unwrap();
        held.try_lock().unwrap();
        assert!(
            store
                .capture(&|| false)
                .unwrap_err()
                .contains("module_update_busy")
        );
        drop(held);
        assert!(
            store
                .capture(&|| false)
                .unwrap_err()
                .contains("module_not_installed")
        );
    }
    #[test]
    #[ignore = "requires the packaged Canary profile and explicit public UI unit digest"]
    fn windows_published_ui_install_remove_rollback_and_reopen() {
        let digest = std::env::var("DS_UI_QUALIFICATION_UNIT_SHA256")
            .expect("name the exact public UI unit");
        let (base, store) = private_fixture();
        let authority =
            crate::module_updates::capture_release_authority(crate::Lane::Canary).unwrap();
        let grant = authority
            .acquire_ui(UiComponent::Interactions, &digest, &|| false)
            .unwrap();
        let staged = store.stage(grant, |_| {}, &|| false).unwrap();
        assert_eq!(staged.expected_generation, 0);
        let selection = store
            .activate(&staged.receipt_id, 0, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(selection.generation, 1);
        let captured = store.capture(&|| false).unwrap();
        for (name, _) in UiComponent::Interactions.files() {
            let (bytes, verified) = store
                .member(&digest, name, 0, MAX_MEMBER_CHUNK, &|| false)
                .unwrap();
            assert_eq!(bytes.len() as i64, verified.size_bytes);
        }
        store
            .remove(1, ActivationGuard::default(), &|| false)
            .unwrap();
        assert!(
            store
                .capture(&|| false)
                .unwrap_err()
                .contains("module_removed")
        );
        store
            .rollback(2, ActivationGuard::default(), &|| false)
            .unwrap();
        let reopened =
            UiModuleStore::new(base.path(), UiComponent::Interactions, "canary").unwrap();
        assert_eq!(
            reopened.capture(&|| false).unwrap().receipt,
            captured.receipt
        );
        assert_eq!(reopened.selection().unwrap().unwrap().generation, 3);
        let member = base
            .path()
            .join("interactions/canary/versions")
            .join(&digest)
            .join("ds_interactions_wasm_bg.wasm");
        let original = fs::read(&member).unwrap();
        fs::write(&member, b"tampered").unwrap();
        assert!(reopened.capture(&|| false).is_err());
        fs::write(&member, original).unwrap();
        reopened.capture(&|| false).unwrap();
    }
