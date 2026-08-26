//! The durable, per-user registration a *new* shell reads.
//!
//! Windows keeps the user's PATH in `HKCU\Environment\Path`; every process a
//! user starts afterwards — PowerShell, cmd, Windows Terminal, Git Bash, which
//! builds its POSIX PATH from the Windows one — inherits it. Unix shells read
//! no registry, but every mainstream distribution puts `~/.local/bin` on PATH
//! for a login shell when the directory exists, and a package install into
//! `/usr/bin` needs no registration at all.
//!
//! Only this executable's own directory is ever written, and only through the
//! platform's ordinary API for the user's own profile: no shell is spawned,
//! no rc file is edited, no machine-wide setting is touched.

use std::path::Path;

use ds_cli_contract::outcome::Failure;
use serde_json::{Value, json};

use crate::reach::{self, Reach};

/// The registration as it stands, in the platform's own terms.
pub struct Registration {
    /// `windows_user_path` or `unix_local_bin`.
    pub kind: &'static str,
    /// The entry that is, or would be, registered.
    pub entry: String,
    /// Whether that entry is present right now.
    pub present: bool,
    /// Whether a freshly opened shell will resolve `ds` to this executable.
    /// Differs from `present` when the entry exists but the platform does not
    /// yet honour it, or when a system install makes it unnecessary.
    pub new_shells_see: bool,
    /// One sentence a person needs, when there is one.
    pub note: Option<String>,
}

impl Registration {
    pub fn json(&self) -> Value {
        json!({
            "kind": self.kind,
            "entry": self.entry,
            "present": self.present,
            "new_shells_see": self.new_shells_see,
            "note": self.note,
        })
    }
}

/// Read the registration without changing it.
pub fn inspect(reach: &Reach) -> Result<Registration, Failure> {
    imp::inspect(reach)
}

/// Register this executable's directory. Returns whether anything changed.
pub fn register(reach: &Reach) -> Result<bool, Failure> {
    imp::register(reach)
}

/// Remove this executable's directory. Returns whether anything changed.
pub fn unregister(reach: &Reach) -> Result<bool, Failure> {
    imp::unregister(reach)
}

fn unreadable(detail: String) -> Failure {
    Failure::failed(
        "registration_unreadable",
        format!("the user's PATH registration cannot be read: {detail}"),
    )
    .remedy(crate::REGISTRATION_UNREADABLE.remedy)
}

fn unwritable(detail: String) -> Failure {
    Failure::failed(
        "registration_unwritable",
        format!("the user's PATH registration cannot be written: {detail}"),
    )
    .remedy(crate::REGISTRATION_UNWRITABLE.remedy)
}

fn directory_entry(reach: &Reach) -> String {
    reach::display(&reach.directory)
}

#[allow(dead_code)]
fn unused(_: &Path) {}

// ---------------------------------------------------------------------------
// Windows: HKCU\Environment\Path
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod imp {
    use super::{Registration, directory_entry, unreadable, unwritable};
    use crate::reach::{self, Reach};
    use ds_cli_contract::outcome::Failure;

    const NOTE_PRESENT: &str = "new PowerShell, cmd and Git Bash windows resolve `ds` here; \
                                windows already open keep the PATH they started with";

    pub fn inspect(reach: &Reach) -> Result<Registration, Failure> {
        let list = win::read_user_path()
            .map_err(unreadable)?
            .unwrap_or_default();
        let entry = directory_entry(reach);
        let present = reach::list_contains(&list, &entry);
        Ok(Registration {
            kind: "windows_user_path",
            entry,
            present,
            new_shells_see: present,
            note: present.then(|| NOTE_PRESENT.to_string()),
        })
    }

    pub fn register(reach: &Reach) -> Result<bool, Failure> {
        let list = win::read_user_path()
            .map_err(unreadable)?
            .unwrap_or_default();
        let entry = directory_entry(reach);
        if reach::list_contains(&list, &entry) {
            return Ok(false);
        }
        win::write_user_path(&reach::list_with(&list, &entry)).map_err(unwritable)?;
        Ok(true)
    }

    pub fn unregister(reach: &Reach) -> Result<bool, Failure> {
        let Some(list) = win::read_user_path().map_err(unreadable)? else {
            return Ok(false);
        };
        let entry = directory_entry(reach);
        if !reach::list_contains(&list, &entry) {
            return Ok(false);
        }
        win::write_user_path(&reach::list_without(&list, &entry)).map_err(unwritable)?;
        Ok(true)
    }

    /// The raw Win32 registry calls, kept in one place. The value is written
    /// back as `REG_EXPAND_SZ`, which is what Windows itself creates, so an
    /// operator's `%USERPROFILE%`-style entries keep expanding.
    mod win {
        use std::ptr;

        use windows_sys::Win32::Foundation::{
            ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS,
        };
        use windows_sys::Win32::System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ, RegCloseKey,
            RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
        };

        fn wide(text: &str) -> Vec<u16> {
            text.encode_utf16().chain(std::iter::once(0)).collect()
        }

        fn open(access: u32) -> Result<HKEY, String> {
            let mut key: HKEY = ptr::null_mut();
            let subkey = wide("Environment");
            // SAFETY: every pointer is to a live local; the subkey is
            // NUL-terminated; the out-pointer receives a handle we close.
            let status =
                unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut key) };
            if status != ERROR_SUCCESS {
                return Err(format!(
                    "HKCU\\Environment could not be opened (error {status})"
                ));
            }
            Ok(key)
        }

        pub fn read_user_path() -> Result<Option<String>, String> {
            let key = open(KEY_READ)?;
            let name = wide("Path");
            let mut kind: u32 = 0;
            let mut size: u32 = 0;
            // SAFETY: a size query; no data buffer is passed.
            let status = unsafe {
                RegQueryValueExW(
                    key,
                    name.as_ptr(),
                    ptr::null(),
                    &mut kind,
                    ptr::null_mut(),
                    &mut size,
                )
            };
            if status == ERROR_FILE_NOT_FOUND {
                // SAFETY: closing the handle this function opened.
                unsafe { RegCloseKey(key) };
                return Ok(None);
            }
            if status != ERROR_SUCCESS && status != ERROR_MORE_DATA {
                // SAFETY: as above.
                unsafe { RegCloseKey(key) };
                return Err(format!("Path could not be read (error {status})"));
            }
            let mut buffer = vec![0u8; size as usize + 2];
            let mut filled = buffer.len() as u32;
            // SAFETY: the buffer is at least `filled` bytes long and lives for
            // the call; `filled` is updated to the bytes actually written.
            let status = unsafe {
                RegQueryValueExW(
                    key,
                    name.as_ptr(),
                    ptr::null(),
                    &mut kind,
                    buffer.as_mut_ptr(),
                    &mut filled,
                )
            };
            // SAFETY: closing the handle this function opened.
            unsafe { RegCloseKey(key) };
            if status != ERROR_SUCCESS {
                return Err(format!("Path could not be read (error {status})"));
            }
            if kind != REG_SZ && kind != REG_EXPAND_SZ {
                return Err("Path is not a string value".to_string());
            }
            let units: Vec<u16> = buffer[..(filled as usize).min(buffer.len())]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            let text = String::from_utf16_lossy(&units);
            Ok(Some(text.trim_end_matches('\0').to_string()))
        }

        pub fn write_user_path(value: &str) -> Result<(), String> {
            let key = open(KEY_SET_VALUE)?;
            let name = wide("Path");
            let data = wide(value);
            let bytes = data.len() * 2;
            // SAFETY: `data` is a NUL-terminated UTF-16 buffer of exactly
            // `bytes` bytes, alive for the call.
            let status = unsafe {
                RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    REG_EXPAND_SZ,
                    data.as_ptr().cast::<u8>(),
                    bytes as u32,
                )
            };
            // SAFETY: closing the handle this function opened.
            unsafe { RegCloseKey(key) };
            if status != ERROR_SUCCESS {
                return Err(format!("Path could not be written (error {status})"));
            }
            broadcast_environment_change();
            Ok(())
        }

        /// Tell already-running shells and Explorer that the environment
        /// changed, so a terminal opened from the Start menu after this call
        /// sees the new PATH without a sign-out. Best effort by design: a hung
        /// window must not hold up the registration, hence the timeout.
        fn broadcast_environment_change() {
            let section = wide("Environment");
            let mut result: usize = 0;
            // SAFETY: HWND_BROADCAST with a NUL-terminated string LPARAM is the
            // documented WM_SETTINGCHANGE shape; the timeout bounds the call.
            unsafe {
                SendMessageTimeoutW(
                    HWND_BROADCAST,
                    WM_SETTINGCHANGE,
                    0,
                    section.as_ptr() as isize,
                    SMTO_ABORTIFHUNG,
                    5_000,
                    &mut result,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unix: ~/.local/bin/ds, or a system directory that needs nothing
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
mod imp {
    use std::path::{Path, PathBuf};

    use super::{Registration, unreadable, unwritable};
    use crate::reach::{self, Reach};
    use ds_cli_contract::outcome::Failure;

    /// Directories a package manager installs into; every shell searches
    /// them already, so registering a link there too would only add a second
    /// spelling of the same executable.
    const SYSTEM_DIRECTORIES: &[&str] = &["/usr/bin", "/usr/local/bin", "/bin"];

    fn is_system_install(reach: &Reach) -> bool {
        SYSTEM_DIRECTORIES
            .iter()
            .any(|dir| reach.directory == Path::new(dir))
    }

    fn local_bin() -> Result<PathBuf, Failure> {
        std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(|home| PathBuf::from(home).join(".local").join("bin"))
            .ok_or_else(|| unreadable("HOME is not set".to_string()))
    }

    fn local_bin_on_path(bin: &Path) -> bool {
        std::env::var_os("PATH")
            .is_some_and(|path| std::env::split_paths(&path).any(|entry| entry == bin))
    }

    pub fn inspect(reach: &Reach) -> Result<Registration, Failure> {
        let bin = local_bin()?;
        let link = bin.join("ds");
        let entry = reach::display(&link);
        if is_system_install(reach) {
            return Ok(Registration {
                kind: "unix_local_bin",
                entry,
                present: false,
                new_shells_see: true,
                note: Some(format!(
                    "installed in {}, which every shell already searches; no link is needed",
                    reach::display(&reach.directory)
                )),
            });
        }
        let (present, points_here) = match std::fs::read_link(&link) {
            Ok(target) => (true, reach::same_file(&target, &reach.executable)),
            Err(_) => (link.exists(), false),
        };
        let on_path = local_bin_on_path(&bin);
        let note = if present && !points_here {
            Some("~/.local/bin/ds exists but is not this executable".to_string())
        } else if points_here && !on_path {
            Some(
                "~/.local/bin is not on this shell's PATH; a new login shell adds it on most \
                 distributions, or add it to your shell's rc file"
                    .to_string(),
            )
        } else if points_here {
            Some("new shells resolve `ds` through ~/.local/bin/ds".to_string())
        } else {
            None
        };
        Ok(Registration {
            kind: "unix_local_bin",
            entry,
            present,
            new_shells_see: points_here && on_path,
            note,
        })
    }

    pub fn register(reach: &Reach) -> Result<bool, Failure> {
        if is_system_install(reach) {
            return Ok(false);
        }
        let bin = local_bin()?;
        let link = bin.join("ds");
        match std::fs::read_link(&link) {
            Ok(target) if reach::same_file(&target, &reach.executable) => return Ok(false),
            Ok(_) => {
                std::fs::remove_file(&link)
                    .map_err(|error| unwritable(error.kind().to_string()))?;
            }
            Err(_) if link.exists() => {
                return Err(Failure::conflict(
                    "link_foreign",
                    "~/.local/bin/ds exists and is not a link to this executable",
                )
                .remedy(crate::LINK_FOREIGN.remedy));
            }
            Err(_) => {}
        }
        std::fs::create_dir_all(&bin).map_err(|error| unwritable(error.kind().to_string()))?;
        std::os::unix::fs::symlink(&reach.executable, &link)
            .map_err(|error| unwritable(error.kind().to_string()))?;
        Ok(true)
    }

    pub fn unregister(reach: &Reach) -> Result<bool, Failure> {
        let link = local_bin()?.join("ds");
        match std::fs::read_link(&link) {
            Ok(target) if reach::same_file(&target, &reach.executable) => {
                std::fs::remove_file(&link)
                    .map_err(|error| unwritable(error.kind().to_string()))?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
