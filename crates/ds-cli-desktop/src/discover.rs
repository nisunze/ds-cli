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
//! caller can always settle it with `--desktop-descriptor <path>`.

use std::path::{Path, PathBuf};

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

/// Bound the descriptor read. The real file is a few hundred bytes.
pub const MAX_DESCRIPTOR_BYTES: u64 = 16 * 1024;

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

/// Discover the descriptor. An explicit path is used verbatim and is never
/// second-guessed; automatic discovery scans the known profiles.
pub fn discover(explicit: Option<&str>) -> Discovery {
    if let Some(path) = explicit {
        let path = PathBuf::from(path);
        return match read(&path) {
            Ok(descriptor) => Discovery::Paired(Box::new(Found {
                profile: "explicit",
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

    match candidates.len() {
        0 => Discovery::None,
        1 => {
            let (profile, path) = candidates.into_iter().next().expect("length checked");
            match read(&path) {
                Ok(descriptor) => Discovery::Paired(Box::new(Found {
                    profile,
                    path,
                    descriptor,
                })),
                Err(reason) => Discovery::Unusable { path, reason },
            }
        }
        _ => Discovery::Ambiguous(candidates),
    }
}
