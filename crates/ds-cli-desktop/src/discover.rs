//! Finding the running application's bridge descriptor.
//!
//! Discovery is deterministic and refuses ambiguity. Three install profiles
//! can be present at once — Stable, Canary and a developer build — and each
//! writes its own descriptor under its own Tauri identifier. Picking one
//! silently would mean an agent's command landing in whichever application
//! happened to sort first, which is exactly the class of mistake that is
//! invisible until it has written to the wrong project.
//!
//! So: exactly one live descriptor is a pairing. More than one is a refusal
//! naming the choices. None is a refusal naming how to start the app. The
//! caller can always settle it with `--desktop-descriptor <path>`, or for a
//! whole session with `DS_DESKTOP_DESCRIPTOR` — which is what the desktop's own
//! `cl` command line sets, so a terminal it opened stays pinned to the window
//! that opened it.

use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

/// The install profiles, in the order they are reported. Each is one Tauri
/// bundle identifier, which is what determines the descriptor's directory.
pub const PROFILES: &[(&str, &str)] = &[
    ("stable", "rw.datasolutions.desktop"),
    ("canary", "rw.datasolutions.desktop.canary"),
    ("dev", "rw.datasolutions.desktop.dev"),
];

/// Published by DS GridDesign's narrow CLI bridge.  This replaces the retired
/// assistant bridge; it carries only the loopback endpoint and pairing secret
/// needed by typed `ds` commands.
pub const DESCRIPTOR_FILE: &str = "cli-bridge.json";

/// The environment variable naming a descriptor for every command in a
/// session. `--desktop-descriptor` still wins when both are present, and
/// automatic discovery runs only when neither is: the variable is a default
/// for the flag, never an override of it.
pub const DESCRIPTOR_ENV: &str = "DS_DESKTOP_DESCRIPTOR";

/// Bound the descriptor read. The real file is a few hundred bytes.
pub const MAX_DESCRIPTOR_BYTES: u64 = 16 * 1024;

/// Automatic discovery probes at most three loopback endpoints. A dead
/// descriptor must not make a live Stable session ambiguous, and each dead
/// probe must stay cheap.
const LIVE_PROBE_TIMEOUT: Duration = Duration::from_millis(150);

/// The application's published pairing descriptor.
///
/// `token` is a short-lived pairing secret. It is never printed, never
/// logged, and never placed in a result — `Descriptor` deliberately has no
/// `Debug` derive so it cannot be formatted into one by accident.
#[derive(Deserialize)]
pub struct Descriptor {
    pub version: u8,
    pub url: String,
    pub token: String,
    pub pid: u32,
}

/// One profile's descriptor, found on disk.
pub struct Found {
    pub profile: &'static str,
    pub path: PathBuf,
    pub descriptor: Descriptor,
}

pub enum Discovery {
    /// Exactly one live descriptor.
    Paired(Box<Found>),
    /// No descriptor for any profile.
    None,
    /// More than one. Never resolved by preference order.
    Ambiguous(Vec<(&'static str, PathBuf)>),
    /// A descriptor exists but cannot be used.
    Unusable { path: PathBuf, reason: String },
}

/// Where a Tauri app with `identifier` keeps its app data, matching
/// `AppHandle::path().app_data_dir()` on each platform.
pub fn app_data_dir(identifier: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA").map(|base| PathBuf::from(base).join(identifier))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library/Application Support")
                .join(identifier)
        })
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
            .map(|base| base.join(identifier))
    }
}

/// Read one descriptor path, or say why it cannot be used.
pub fn read(path: &Path) -> Result<Descriptor, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.kind().to_string())?;
    if metadata.len() > MAX_DESCRIPTOR_BYTES {
        return Err("descriptor is larger than its bound".into());
    }
    let bytes = std::fs::read(path).map_err(|error| error.kind().to_string())?;
    let descriptor: Descriptor =
        serde_json::from_slice(&bytes).map_err(|_| "descriptor is not valid JSON".to_string())?;
    if descriptor.version != 1 {
        return Err(format!(
            "descriptor speaks version {}, this build speaks 1",
            descriptor.version
        ));
    }
    if !descriptor.url.starts_with("http://127.0.0.1:") {
        // The bridge is loopback by construction. A descriptor pointing
        // anywhere else is not a bridge to trust with a pairing secret.
        return Err("descriptor does not point at loopback".into());
    }
    Ok(descriptor)
}

fn descriptor_is_live(descriptor: &Descriptor) -> bool {
    let Some(authority) = descriptor.url.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let Some(port) = authority
        .split('/')
        .next()
        .and_then(|raw| raw.parse::<u16>().ok())
    else {
        return false;
    };
    TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        LIVE_PROBE_TIMEOUT,
    )
    .is_ok()
}

fn select_candidates<F>(candidates: Vec<(&'static str, PathBuf)>, is_live: F) -> Discovery
where
    F: Fn(&Descriptor) -> bool,
{
    let mut live = Vec::new();
    let mut first_unusable = None;
    for (profile, path) in candidates {
        match read(&path) {
            Ok(descriptor) if is_live(&descriptor) => live.push(Found {
                profile,
                path,
                descriptor,
            }),
            Ok(_) => {}
            Err(reason) => {
                if first_unusable.is_none() {
                    first_unusable = Some((path, reason));
                }
            }
        }
    }

    match live.len() {
        0 => first_unusable.map_or(Discovery::None, |(path, reason)| Discovery::Unusable {
            path,
            reason,
        }),
        1 => Discovery::Paired(Box::new(live.pop().expect("length checked"))),
        _ => Discovery::Ambiguous(
            live.into_iter()
                .map(|found| (found.profile, found.path))
                .collect(),
        ),
    }
}

/// Discover the descriptor. An explicit path is used verbatim and is never
/// second-guessed; automatic discovery scans the known profiles.
pub fn discover(explicit: Option<&str>) -> Discovery {
    let environment = std::env::var(DESCRIPTOR_ENV)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let named = match explicit {
        Some(path) => Some((PathBuf::from(path), "explicit")),
        None => environment.map(|value| (PathBuf::from(value), "environment")),
    };
    if let Some((path, profile)) = named {
        return match read(&path) {
            Ok(descriptor) => Discovery::Paired(Box::new(Found {
                profile,
                path,
                descriptor,
            })),
            Err(reason) => Discovery::Unusable { path, reason },
        };
    }

    let candidates: Vec<(&'static str, PathBuf)> = PROFILES
        .iter()
        .filter_map(|(profile, identifier)| {
            app_data_dir(identifier).map(|dir| (*profile, dir.join(DESCRIPTOR_FILE)))
        })
        .filter(|(_, path)| path.is_file())
        .collect();

    select_candidates(candidates, descriptor_is_live)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn scratch() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ds-descriptor-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).expect("scratch");
        path
    }

    fn descriptor(root: &Path, name: &str, pid: u32) -> PathBuf {
        let path = root.join(name);
        fs::write(
            &path,
            format!(
                r#"{{"version":1,"url":"http://127.0.0.1:{}","token":"test","pid":{pid}}}"#,
                20_000 + pid,
            ),
        )
        .expect("descriptor");
        path
    }

    #[test]
    fn one_live_profile_ignores_dead_descriptor_leftovers() {
        let root = scratch();
        let stable = descriptor(&root, "stable.json", 1);
        let canary = descriptor(&root, "canary.json", 2);
        let dev = descriptor(&root, "dev.json", 3);
        let selected = select_candidates(
            vec![("stable", stable), ("canary", canary), ("dev", dev)],
            |candidate| candidate.pid == 1,
        );
        match selected {
            Discovery::Paired(found) => assert_eq!(found.profile, "stable"),
            _ => panic!("the sole live Stable descriptor was not selected"),
        }
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn two_live_profiles_remain_ambiguous() {
        let root = scratch();
        let stable = descriptor(&root, "stable.json", 1);
        let canary = descriptor(&root, "canary.json", 2);
        let selected =
            select_candidates(vec![("stable", stable), ("canary", canary)], |candidate| {
                candidate.pid <= 2
            });
        match selected {
            Discovery::Ambiguous(candidates) => {
                assert_eq!(
                    candidates
                        .iter()
                        .map(|(profile, _)| *profile)
                        .collect::<Vec<_>>(),
                    ["stable", "canary"]
                );
            }
            _ => panic!("two live profiles must still refuse ambiguity"),
        }
        fs::remove_dir_all(root).expect("cleanup");
    }
}
