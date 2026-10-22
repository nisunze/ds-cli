    use super::*;
    use std::io::Write;

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
    fn ordinary_fixture() -> FixtureDirectory {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Managed TEMP can grant foreign SIDs writes. Create an ordinary
        // inherited directory in the safe current-user known folder instead.
        let directory = FixtureDirectory(local_application_data().unwrap().join(format!(
            "ds-private-output-test-{}-{nonce}-{sequence}",
            std::process::id()
        )));
        fs::create_dir(directory.path()).unwrap();
        grant(directory.path(), "*S-1-1-0:(R)");
        crate::state_windows::validate_directory(directory.path()).unwrap();
        assert!(validate_directory(directory.path()).is_err());
        directory
    }
    fn grant(path: &Path, access: &str) {
        let result = std::process::Command::new("icacls.exe")
            .arg(path)
            .args(["/grant", access])
            .output()
            .unwrap();
        assert!(result.status.success(), "native ACL fixture failed");
    }
    fn acl(path: &Path) -> Vec<u8> {
        let result = std::process::Command::new("icacls.exe")
            .arg(path)
            .output()
            .unwrap();
        assert!(result.status.success(), "native ACL inspection failed");
        result.stdout
    }
    #[test]
    fn windows_private_output_is_owner_only_without_changing_ordinary_parent() {
        let directory = ordinary_fixture();
        let before = acl(directory.path());
        let path = directory.path().join("output.json");
        let mut file = create_private_output_file(&path).unwrap();
        crate::state_windows::validate_file(&file).unwrap();
        validate_private(&file).unwrap();
        file.write_all(b"private output").unwrap();
        drop(file);
        assert_eq!(fs::read(&path).unwrap(), b"private output");
        assert_eq!(acl(directory.path()), before);
        assert!(validate_directory(directory.path()).is_err());
    }
    #[test]
    fn windows_private_output_preserves_existing_file() {
        let directory = ordinary_fixture();
        let before = acl(directory.path());
        let path = directory.path().join("occupied.json");
        fs::write(&path, b"retained original bytes").unwrap();
        let file_acl = acl(&path);
        assert!(create_private_output_file(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"retained original bytes");
        assert_eq!(acl(&path), file_acl);
        assert_eq!(acl(directory.path()), before);
    }
    #[test]
    fn windows_private_output_refuses_foreign_writable_parent_before_creation() {
        let directory = ordinary_fixture();
        grant(directory.path(), "*S-1-1-0:(W)");
        let before = acl(directory.path());
        assert!(crate::state_windows::validate_directory(directory.path()).is_err());
        let path = directory.path().join("must-not-exist.json");
        assert!(create_private_output_file(&path).is_err());
        assert!(!path.exists());
        assert_eq!(acl(directory.path()), before);
    }
    #[test]
    fn windows_private_output_reader_matches_writer_without_repairing_parent() {
        use std::io::Read;
        let directory = ordinary_fixture();
        let before = acl(directory.path());
        let path = directory.path().join("read-output.json");
        let mut output = create_private_output_file(&path).unwrap();
        output.write_all(b"private receipt").unwrap();
        drop(output);
        let file_acl = acl(&path);
        let mut bytes = Vec::new();
        read_private_output_file(&path)
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, b"private receipt");
        assert!(
            read_file(&path).is_err(),
            "credential readers keep their strict private-parent contract"
        );
        assert_eq!(acl(&path), file_acl);
        assert_eq!(acl(directory.path()), before);
    }
    #[test]
    fn windows_private_output_reader_refuses_broad_file_acl_without_repair() {
        let directory = ordinary_fixture();
        let path = directory.path().join("broad-output.json");
        let mut output = create_private_output_file(&path).unwrap();
        output.write_all(b"retained receipt").unwrap();
        drop(output);
        grant(&path, "*S-1-1-0:(R)");
        let file_acl = acl(&path);
        let parent_acl = acl(directory.path());
        assert!(read_private_output_file(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"retained receipt");
        assert_eq!(acl(&path), file_acl);
        assert_eq!(acl(directory.path()), parent_acl);
    }
    #[test]
    fn windows_private_output_reader_refuses_hardlinks_without_repair() {
        let directory = ordinary_fixture();
        let path = directory.path().join("linked-output.json");
        let mut output = create_private_output_file(&path).unwrap();
        output.write_all(b"retained linked receipt").unwrap();
        drop(output);
        let alias = directory.path().join("hardlink-output.json");
        fs::hard_link(&path, &alias).unwrap();
        let file_acl = acl(&path);
        assert!(read_private_output_file(&path).is_err());
        assert!(read_private_output_file(&alias).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"retained linked receipt");
        assert_eq!(fs::read(&alias).unwrap(), b"retained linked receipt");
        assert_eq!(acl(&path), file_acl);
    }
    #[test]
    fn windows_private_output_reader_refuses_reparse_ancestor_without_repair() {
        let directory = ordinary_fixture();
        let target = directory.path().join("target");
        fs::create_dir(&target).unwrap();
        let path = target.join("output.json");
        let mut output = create_private_output_file(&path).unwrap();
        output.write_all(b"retained target receipt").unwrap();
        drop(output);
        let junction = directory.path().join("junction");
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()
            .unwrap();
        assert!(result.status.success(), "native junction fixture failed");
        let file_acl = acl(&path);
        let parent_acl = acl(&target);
        assert!(read_private_output_file(&junction.join("output.json")).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"retained target receipt");
        assert_eq!(acl(&path), file_acl);
        assert_eq!(acl(&target), parent_acl);
        fs::remove_dir(&junction).unwrap();
    }
    #[test]
    fn windows_private_output_reader_refuses_unsafe_parent_without_repair() {
        let directory = ordinary_fixture();
        let path = directory.path().join("private-output.json");
        let mut output = create_private_output_file(&path).unwrap();
        output.write_all(b"retained private receipt").unwrap();
        drop(output);
        grant(directory.path(), "*S-1-1-0:(W)");
        let parent_acl = acl(directory.path());
        let file_acl = acl(&path);
        assert!(read_private_output_file(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"retained private receipt");
        assert_eq!(acl(&path), file_acl);
        assert_eq!(acl(directory.path()), parent_acl);
    }
    #[test]
    fn windows_private_output_reader_excludes_concurrent_writers() {
        let directory = ordinary_fixture();
        let path = directory.path().join("busy-output.json");
        let mut output = create_private_output_file(&path).unwrap();
        output.write_all(b"complete receipt").unwrap();
        assert!(
            read_private_output_file(&path).is_err(),
            "an unfinished writer must exclude the receipt reader"
        );
        drop(output);
        let held = read_private_output_file(&path).unwrap();
        assert!(
            OpenOptions::new()
                .write(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .open(&path)
                .is_err(),
            "a retained reader must exclude a concurrent writer"
        );
        drop(held);
        drop(
            OpenOptions::new()
                .write(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .open(&path)
                .unwrap(),
        );
        assert_eq!(fs::read(&path).unwrap(), b"complete receipt");
    }
