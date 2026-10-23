    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn test_path(store: &str, suffix: &str) -> PathBuf {
        let root = state_root().expect("Windows protected-state root");
        let store = root.join(store);
        ensure_directory(&store).expect("Windows protected-state store");
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        store.join(format!(
            "windows-test-{}-{sequence}-{suffix}.json",
            std::process::id()
        ))
    }

    #[test]
    fn native_acl_write_admission_checks_foreign_sids_and_parent_child_delete() {
        use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
        use windows_sys::Win32::Security::GetSecurityDescriptorDacl;
        for (sddl, admitted) in [
            ("D:(A;;FA;;;BU)", false),
            ("D:(A;;DC;;;BU)", false),
            ("D:(A;;FR;;;BU)", true),
            ("D:(A;OICIIO;FA;;;BU)", true),
            ("D:(A;;FA;;;SY)(A;;FA;;;BA)", true),
        ] {
            let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
            let mut descriptor = null_mut();
            assert_ne!(
                unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        wide.as_ptr(),
                        1,
                        &mut descriptor,
                        null_mut(),
                    )
                },
                0
            );
            let mut present = 0;
            let mut defaulted = 0;
            let mut acl = null_mut();
            assert_ne!(
                unsafe {
                    GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted)
                },
                0
            );
            assert_ne!(present, 0);
            assert!(!acl.is_null());
            let result = with_current_user_sid(|sid| only_trusted_writers(acl, sid))
                .unwrap()
                .unwrap();
            unsafe { LocalFree(descriptor) };
            assert_eq!(result, admitted, "{sddl}");
        }
    }

    #[test]
    fn dpapi_envelope_is_not_plaintext_and_is_key_bound() {
        let first = test_path("credentials", "first");
        let second = test_path("credentials", "second");
        let secret = b"refresh-token-must-never-be-plaintext";
        let protected = protect(&first, secret).expect("protect");
        assert!(!protected.windows(secret.len()).any(|part| part == secret));
        let restored = Zeroizing::new(unprotect(&first, &protected).expect("unprotect"));
        assert_eq!(restored.as_slice(), secret);
        assert_eq!(
            unprotect(&second, &protected),
            Err(StoreError::UnsafeOrUnreadable)
        );
    }

    #[test]
    fn dpapi_error_is_captured_before_cleanup_and_success_ignores_stale_error() {
        use windows_sys::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_DATA, SetLastError};

        unsafe { SetLastError(ERROR_ACCESS_DENIED) };
        let failure = dpapi_failure(0, true, StoreError::UnsafeOrUnreadable);
        // Model a cleanup call replacing the thread's last-error value.
        unsafe { SetLastError(ERROR_INVALID_DATA) };
        assert_eq!(failure, Some(StoreError::Unavailable));
        assert_eq!(
            dpapi_failure(0, true, StoreError::UnsafeOrUnreadable),
            Some(StoreError::UnsafeOrUnreadable)
        );
        unsafe { SetLastError(ERROR_ACCESS_DENIED) };
        assert_eq!(dpapi_failure(1, false, StoreError::UnsafeOrUnreadable), None);
        assert_eq!(
            dpapi_failure(1, true, StoreError::UnsafeOrUnreadable),
            Some(StoreError::UnsafeOrUnreadable)
        );
        unsafe { SetLastError(ERROR_INVALID_DATA) };
        assert_eq!(
            dpapi_failure(0, true, StoreError::Unavailable),
            Some(StoreError::Unavailable)
        );
    }

    #[test]
    fn malformed_envelope_headers_remain_unsafe() {
        let path = test_path("credentials", "malformed");
        let protected = protect(&path, b"protected-state-test").expect("protect");
        for offset in [0, 8, 9, 10, 12, 44] {
            let mut malformed = protected.to_vec();
            malformed[offset] ^= 1;
            assert_eq!(
                unprotect(&path, &malformed),
                Err(StoreError::UnsafeOrUnreadable)
            );
        }
        assert_eq!(
            unprotect(&path, &protected[..ENVELOPE_HEADER - 1]),
            Err(StoreError::UnsafeOrUnreadable)
        );
    }

    #[test]
    fn encrypted_atomic_file_round_trips_without_plaintext_on_disk() {
        let destination = test_path("devices", "roundtrip");
        let stage = destination.parent().unwrap().join(format!(
            ".{}.stage",
            destination.file_name().unwrap().to_string_lossy()
        ));
        let secret = b"device-private-state-must-never-be-plaintext";
        let protected = protect(&destination, secret).expect("protect");
        let mut file = create_new(&stage).expect("create stage");
        file.write_all(&protected).expect("write stage");
        file.sync_all().expect("sync stage");
        drop(file);
        replace(&stage, &destination).expect("replace");

        let mut disk = Vec::new();
        open_read(&destination)
            .expect("open encrypted state")
            .read_to_end(&mut disk)
            .expect("read encrypted state");
        assert!(!disk.windows(secret.len()).any(|part| part == secret));
        let restored = Zeroizing::new(unprotect(&destination, &disk).expect("unprotect"));
        assert_eq!(restored.as_slice(), secret);
        disk.zeroize();
        fs::remove_file(&destination).expect("cleanup");
    }

    #[test]
    fn lock_is_exclusive_and_hardlinks_are_refused() {
        let lock_path = test_path("contexts", "lock");
        let first = open_lock(&lock_path).expect("first lock handle");
        let second = open_lock(&lock_path).expect("second lock handle");
        lock_exclusive(&first, Duration::from_millis(50), Duration::from_millis(5))
            .expect("first lock");
        assert_eq!(
            lock_exclusive(&second, Duration::from_millis(50), Duration::from_millis(5)),
            Err(StoreError::Conflict)
        );
        unlock(&first).expect("unlock");
        drop(first);
        drop(second);
        fs::remove_file(&lock_path).expect("lock cleanup");

        let original = test_path("devices", "original");
        let linked = test_path("devices", "linked");
        let mut file = create_new(&original).expect("original");
        file.write_all(b"ciphertext fixture").expect("write");
        drop(file);
        fs::hard_link(&original, &linked).expect("hard link");
        assert!(matches!(
            open_read(&original),
            Err(StoreError::UnsafeOrUnreadable)
        ));
        fs::remove_file(&linked).expect("linked cleanup");
        fs::remove_file(&original).expect("original cleanup");
    }
