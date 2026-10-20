use super::native::{directory, paths, trusted_installer_owner};
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::OWNER_SECURITY_INFORMATION;

#[test]
fn refuses_remote_device_alias_and_stream_paths() {
    for value in [
        r"relative\snapshot",
        r"\\server\share\snapshot",
        r"\\?\C:\snapshot",
        r"C:\snapshot:stream",
        r"C:\private\..\snapshot",
        r"C:\private\.\snapshot",
        r"C:\private\snapshot.",
        r"C:\private\CON",
        r"C:\private\COM1.txt",
    ] {
        assert!(paths(Path::new(value)).is_err(), "{value}");
    }
    assert!(paths(Path::new(r"C:\private\snapshot.json")).is_ok());
}

#[test]
fn trusted_installer_is_the_owner_of_system32() {
    let path: Vec<u16> = "C:\\Windows\\System32\0".encode_utf16().collect();
    let mut owner = null_mut();
    let mut descriptor = null_mut();
    let code = unsafe {
        GetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(code, 0, "read the System32 owner");
    let trusted = trusted_installer_owner(owner);
    unsafe { LocalFree(descriptor) };
    assert_eq!(trusted, Ok(true));
}

#[test]
fn a_private_directory_under_the_user_profile_passes_every_ancestor() {
    // Windows 11 makes TrustedInstaller the owner of C:\ itself, so this walk
    // crosses a TrustedInstaller-owned ancestor before the user's own folders.
    let base = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"));
    let root = base.join(format!("ds-snapshot-privacy-test-{}", std::process::id()));
    let result = directory(&root.join("nested"));
    let _ = std::fs::remove_dir_all(&root);
    result.expect("a private directory under the user profile");
}
