//! The embedded PLS-CADD desktop drivers, byte for byte, each digest pinned.
//!
//! The drivers are the owner of desktop PLS-CADD work: they were proven on
//! the Nyamagabe delivery in ds-work, and `ds` carries them rather than
//! re-implementing a single click. Three kinds of file live here:
//!
//! * [`Origin::Vendored`] — identical to ds-work [`UPSTREAM`]; its digest is
//!   the upstream file's digest.
//! * [`Origin::Modified`] — vendored from [`UPSTREAM`] and changed here, with
//!   the upstream digest kept beside the new one. `pls-deliver-autosag.ps1`
//!   is the only one: it gained `-NoSheets` (the owner no longer prints PLS
//!   plan & profile), and its receipt schema moved to v4 with it.
//! * [`Origin::Owned`] — the `ds-desktop-*.ps1` entries `ds` runs, one per
//!   verb, which compose the drivers and write the one result document `ds`
//!   reads.
//!
//! A digest is written here by hand, on purpose. Changing a script means
//! changing its pin in the same commit, and the bundle test refuses a file on
//! disk that this table does not name, or a name whose bytes moved. The
//! repository's `.gitattributes` exempts `crates/ds-cli-pls/desktop/` from
//! line-ending conversion, so every checkout embeds the same bytes.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use serde_json::json;
use sha2::{Digest, Sha256};

use super::BUNDLE_FAILED;

/// The ds-work commit the drivers were vendored from, after the deliver chain
/// was proven end to end on the v19 cap6 export.
pub const UPSTREAM: &str = "ds-work ea67e9b1ddd4eed4550798950806667dfb94c1ef";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Vendored,
    Modified { upstream_sha256: &'static str },
    Owned,
}

pub struct Script {
    /// Path below the bundle root, `/`-separated. The drivers find each other
    /// through `$PSScriptRoot`, so the layout is part of the contract.
    pub path: &'static str,
    pub bytes: &'static [u8],
    /// Lowercase hex sha256 of `bytes`.
    pub sha256: &'static str,
    pub origin: Origin,
}

macro_rules! script {
    ($path:literal, $sha256:literal, $origin:expr) => {
        Script {
            path: $path,
            bytes: include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/desktop/", $path)),
            sha256: $sha256,
            origin: $origin,
        }
    };
}

pub static BUNDLE: &[Script] = &[
    script!(
        "ds-desktop-autosag.ps1",
        "e2b1923f9055a09e83bebfc88aeb049242ff0dfa973befb557843a69610aacd2",
        Origin::Owned
    ),
    script!(
        "ds-desktop-check.ps1",
        "200f1a6a577c89d34203fba5d3e07eba6aa66341d0a27bc5e5c286bd07d26896",
        Origin::Owned
    ),
    script!(
        "ds-desktop-deliver.ps1",
        "57e7b7ecd5efb53215d20b4c6fde34e0c67e1fe511c9de032da8042476652b7d",
        Origin::Owned
    ),
    script!(
        "ds-desktop-lib.ps1",
        "1d88a8bf038acfe7e32aa92a23a083c3053b56cdf05f8c74a71ac18f8d555c82",
        Origin::Owned
    ),
    script!(
        "ds-desktop-qualify.ps1",
        "9caa1a5491598322bee3a8d560a88c6be2c68df5ec2dcd80f48c5fe345d6d8a8",
        Origin::Owned
    ),
    script!(
        "ds-desktop-reports.ps1",
        "34c5bc9e9b73665360898c93fbefcffca8e0fa3bc31cb57dd9dbc375c286feb4",
        Origin::Owned
    ),
    script!(
        "ds-desktop-restore.ps1",
        "8b87402aab2648ac7225b08ea342efaf5f7399c04089f1bfe6d4089cba24e8e2",
        Origin::Owned
    ),
    script!(
        "ds-desktop-sheets-pdf.ps1",
        "1a11fe770edcdd147e9b11745d22a4f4e530d069128e0d528b3d73c2fec78433",
        Origin::Owned
    ),
    script!(
        "interim/pls-backup-open-interim.ps1",
        "1bdb770e826761293856e9409050a1ee2ac321352c0e7567d5b6bd80f6d855ae",
        Origin::Vendored
    ),
    script!(
        "interim/pls-close-interim.ps1",
        "1f520968b57002364153e1b16aaa6971f6d0445cbfccb9a2fad1337e2e065a1a",
        Origin::Vendored
    ),
    script!(
        "interim/pls-interim-loader.ps1",
        "2f6a39f0fb190ceae42d03a3083c30bb990af3e87b9d32d4d2f1b6da4c05f286",
        Origin::Vendored
    ),
    script!(
        "interim/pls-restore-open-interim.ps1",
        "17f6472684fe5c9f675d146466445e71fc6a98017fd03f6cf3613f41796e707d",
        Origin::Vendored
    ),
    script!(
        "pls-backup-restore-lib.psm1",
        "e6ad0371536cadbd6ba1be24bdeb675ad48c0469feb77bd6c97107df7dd73638",
        Origin::Vendored
    ),
    script!(
        "pls-backup-restore-profile.psd1",
        "bc6ce626b1f20ec4e36e65caa4ddcfca58bff7582188215b771a1a7ab04edd8a",
        Origin::Vendored
    ),
    script!(
        "pls-backup-restore-qualify.ps1",
        "916218a06150faf20e8f72c6d08fa98a9d1bcb0683a0ed443071c5cf899ed285",
        Origin::Vendored
    ),
    script!(
        "pls-click-at.ps1",
        "1d91d36cb9d62aa0a6a3d8d19d58fd1fbfe4aa06a3b956697bdaa90f9e887d1b",
        Origin::Vendored
    ),
    script!(
        "pls-combo.ps1",
        "c6d5341267cdb879a2a621352f2a49d5c0d62f1ef65ac19d8c6f081bf3782dac",
        Origin::Vendored
    ),
    script!(
        "pls-command.ps1",
        "303e6518e97862967c9314b9906b2aecb90994aac69c328dcacc3fefcb58119b",
        Origin::Vendored
    ),
    script!(
        "pls-context-menu.ps1",
        "bb2e5f5b8713162d0890f089c8a4efced5c771efeab5ea8d99d9a8d6767364ed",
        Origin::Vendored
    ),
    script!(
        "pls-control.ps1",
        "073bae78395205491b1b11f05ed2d249166c55ebf7fe7371025767b7bdb5e776",
        Origin::Vendored
    ),
    script!(
        "pls-deliver-autosag.ps1",
        "36f5847c8e59ff9420c23866e5265047e69d76747aed14fdeb6fbef2dccb8cee",
        Origin::Modified {
            upstream_sha256: "e11ba7bd16359772267ad243025cb7cb6b747ee32fe947592614b5721189a186",
        }
    ),
    script!(
        "pls-dialog-capture.ps1",
        "dd34d469ebaa3c0d2af762ca440b863cf6eb28eec38fcee4a3b334130c9f6ac7",
        Origin::Vendored
    ),
    script!(
        "pls-dialog-catalog.psd1",
        "7ff62ae6543dee0e73ca1b792592084d6996763e1cbc612a7b98eaa1d233a76c",
        Origin::Vendored
    ),
    script!(
        "pls-dialog-text.ps1",
        "4a5f369f666aef39ecf76497c6d1dc5acc4511d7e3fa378fb7f6c8dec7004fa6",
        Origin::Vendored
    ),
    script!(
        "pls-dialog-watch.ps1",
        "dbd417cf08457a2ad12f56f886903ae468f5183f9b0ab1f57c7f1b741e0d7621",
        Origin::Vendored
    ),
    script!(
        "pls-launch-project.ps1",
        "39d19072cd97ed272cfd85441d16043a16a8838beedf779cc3de794b82574784",
        Origin::Vendored
    ),
    script!(
        "pls-printwindow.ps1",
        "18c41954fa427b82ae1aff199c8a6c7422686cc78b7499610e36270ed2688c34",
        Origin::Vendored
    ),
    script!(
        "pls-report-any.ps1",
        "c8b388fecddcfb29d93de7658f94afca5050dad6409a1d72efe57efa5c6329f5",
        Origin::Vendored
    ),
    script!(
        "pls-rtf-to-pdf.ps1",
        "e02e33f25f09b1f2e37ee09c530d032b5b7af1695375a402b5f0a494be3d380e",
        Origin::Vendored
    ),
    script!(
        "pls-save-sheets-pdf.ps1",
        "6e6268307cd5ef9e5abf243a4171d0bb343c58a0fef2f7efef79a91900d63b44",
        Origin::Vendored
    ),
    script!(
        "pls-section-table-autosag.ps1",
        "37c5d4b1e06b72a76132aae9fc38a8fe1ffae2ce13482ec2466b97519732b780",
        Origin::Vendored
    ),
    script!(
        "pls-sheet-paging.ps1",
        "4ddd574a5855e0a8caeb2cdb61277b508a4ae1cda2dc15f84b857ad1a3a0241b",
        Origin::Vendored
    ),
    script!(
        "pls-window-classification.psm1",
        "874c1b67a055c46cc83e73db288fcb002913a3ea93edb728e456d584bb960bcb",
        Origin::Vendored
    ),
    script!(
        "pls-windows.ps1",
        "d855b7890fa05968af87f39112febbef4595a520c78cd131fdd0fdb76179cc84",
        Origin::Vendored
    ),
];

/// One entry script `ds` may run. A closed set: the run boundary takes one of
/// these, never a file name from a caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Check,
    Restore,
    Qualify,
    Deliver,
    Autosag,
    Reports,
    SheetsPdf,
}

impl Entry {
    pub const ALL: &[Self] = &[
        Self::Check,
        Self::Restore,
        Self::Qualify,
        Self::Deliver,
        Self::Autosag,
        Self::Reports,
        Self::SheetsPdf,
    ];

    pub const fn file(self) -> &'static str {
        match self {
            Self::Check => "ds-desktop-check.ps1",
            Self::Restore => "ds-desktop-restore.ps1",
            Self::Qualify => "ds-desktop-qualify.ps1",
            Self::Deliver => "ds-desktop-deliver.ps1",
            Self::Autosag => "ds-desktop-autosag.ps1",
            Self::Reports => "ds-desktop-reports.ps1",
            Self::SheetsPdf => "ds-desktop-sheets-pdf.ps1",
        }
    }
}

pub fn script(path: &str) -> Option<&'static Script> {
    BUNDLE.iter().find(|script| script.path == path)
}

/// The UTF-8 text of an embedded file, for the readers that need it (the
/// dialog catalogue). Every file in the bundle is UTF-8.
pub fn text(path: &str) -> Option<&'static str> {
    script(path).and_then(|script| std::str::from_utf8(script.bytes).ok())
}

/// The bundle's identity: one digest over every path and pin, in table order.
/// A receipt carries it, so a result can be traced to the exact drivers.
pub fn digest() -> String {
    let mut hasher = Sha256::new();
    for script in BUNDLE {
        hasher.update(script.path.as_bytes());
        hasher.update(b"\0");
        hasher.update(script.sha256.as_bytes());
        hasher.update(b"\n");
    }
    format!("sha256:{:x}", hasher.finalize())
}

/// The bundle, written into a fresh private folder for one run and removed
/// with it.
pub struct Extracted {
    root: PathBuf,
}

impl Extracted {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .fold(self.root.clone(), |path, part| path.join(part))
    }
}

impl Drop for Extracted {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Write every file into a new folder below `parent` and read each back
/// against its pin before anything runs from it.
///
/// The folder is created, never reused: a name that already exists is skipped
/// rather than written into, so no other process's files are ever executed.
pub fn extract(parent: &Path) -> Result<Extracted, Failure> {
    let root = fresh_folder(parent)?;
    let extracted = Extracted { root };
    for script in BUNDLE {
        let path = extracted.path(script.path);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(|error| bundle_failure(&path, error))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| bundle_failure(&path, error))?;
        file.write_all(script.bytes)
            .and_then(|()| file.sync_all())
            .map_err(|error| bundle_failure(&path, error))?;
    }
    for script in BUNDLE {
        let path = extracted.path(script.path);
        let bytes = std::fs::read(&path).map_err(|error| bundle_failure(&path, error))?;
        let actual = format!("{:x}", Sha256::digest(&bytes));
        if actual != script.sha256 {
            return Err(Failure::failed(
                BUNDLE_FAILED.code,
                format!("{} does not match its pinned digest", script.path),
            )
            .remedy(BUNDLE_FAILED.remedy)
            .detail(json!({ "expected": script.sha256, "actual": actual })));
        }
    }
    Ok(extracted)
}

fn fresh_folder(parent: &Path) -> Result<PathBuf, Failure> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    for attempt in 0..16u32 {
        let root = parent.join(format!(
            "ds-pls-desktop-{}-{:x}-{attempt}",
            std::process::id(),
            nanos
        ));
        match create_private(&root) {
            Ok(()) => return Ok(root),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(bundle_failure(&root, error)),
        }
    }
    Err(Failure::failed(
        BUNDLE_FAILED.code,
        "no fresh temporary folder name was free",
    )
    .remedy(BUNDLE_FAILED.remedy))
}

/// A folder only this user can read: explicit on Unix; on Windows the
/// per-user `%TEMP%` it is created in already carries that ACL.
fn create_private(root: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new().mode(0o700).create(root)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(root)
    }
}

fn bundle_failure(path: &Path, error: std::io::Error) -> Failure {
    Failure::failed(
        BUNDLE_FAILED.code,
        format!("could not write {}", path.display()),
    )
    .remedy(BUNDLE_FAILED.remedy)
    .detail(json!({ "detail": error.kind().to_string() }))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn on_disk() -> BTreeSet<String> {
        fn walk(root: &Path, folder: &Path, into: &mut BTreeSet<String>) {
            for entry in std::fs::read_dir(folder).expect("the bundle folder reads") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    walk(root, &path, into);
                } else {
                    let relative = path.strip_prefix(root).expect("below the root");
                    into.insert(relative.to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("desktop");
        let mut files = BTreeSet::new();
        walk(&root, &root, &mut files);
        files
    }

    #[test]
    fn every_file_on_disk_is_embedded_and_nothing_else() {
        let declared: BTreeSet<String> = BUNDLE.iter().map(|s| s.path.to_string()).collect();
        assert_eq!(declared.len(), BUNDLE.len(), "a path is declared twice");
        assert_eq!(
            on_disk(),
            declared,
            "crates/ds-cli-pls/desktop and the BUNDLE table must name the same files"
        );
    }

    #[test]
    fn every_embedded_file_matches_its_pinned_digest() {
        for script in BUNDLE {
            assert_eq!(
                format!("{:x}", Sha256::digest(script.bytes)),
                script.sha256,
                "{} moved without its pin; if the change is deliberate, pin the new digest",
                script.path
            );
            if let Origin::Modified { upstream_sha256 } = script.origin {
                assert_ne!(
                    upstream_sha256, script.sha256,
                    "{} is marked modified but equals its upstream digest",
                    script.path
                );
            }
        }
    }

    #[test]
    fn only_the_deliver_chain_diverges_from_upstream() {
        let modified: Vec<&str> = BUNDLE
            .iter()
            .filter(|s| matches!(s.origin, Origin::Modified { .. }))
            .map(|s| s.path)
            .collect();
        assert_eq!(modified, ["pls-deliver-autosag.ps1"]);
        let deliver = text("pls-deliver-autosag.ps1").unwrap();
        assert!(deliver.contains("[switch] $NoSheets"));
        assert!(deliver.contains("'ds.pls.deliver_autosag.v4'"));
    }

    #[test]
    fn owned_files_are_exactly_the_entries_and_their_plumbing() {
        let owned: BTreeSet<&str> = BUNDLE
            .iter()
            .filter(|s| s.origin == Origin::Owned)
            .map(|s| s.path)
            .collect();
        let mut expected: BTreeSet<&str> = Entry::ALL.iter().map(|e| e.file()).collect();
        expected.insert("ds-desktop-lib.ps1");
        assert_eq!(owned, expected);
        for entry in Entry::ALL {
            let body = text(entry.file()).expect("an entry is UTF-8");
            assert!(
                body.contains("[Parameter(Mandatory = $true)][string] $ResultPath"),
                "{} must take the result path ds reads",
                entry.file()
            );
            assert!(
                body.contains(". (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')")
                    && body.contains("Invoke-DsEntry $ResultPath"),
                "{} must write its outcome through Invoke-DsEntry",
                entry.file()
            );
        }
    }

    /// Every `& (Join-Path $here '<file>')` and `"$here\<file>"` an embedded
    /// script runs must be in the bundle. A driver that calls a helper the
    /// bundle left behind fails only on the desktop, hours into a run. Only
    /// lines that locate a file beside the script count: the catalogue's notes
    /// mention helpers the bundle deliberately does not carry.
    #[test]
    fn every_script_a_driver_calls_is_embedded() {
        const LOCATORS: &[&str] = &["$here", "$PSScriptRoot", "$pls"];
        let names: BTreeSet<String> = BUNDLE
            .iter()
            .map(|s| s.path.rsplit('/').next().unwrap().to_string())
            .collect();
        let mut missing = Vec::new();
        for script in BUNDLE {
            let body = std::str::from_utf8(script.bytes).expect("UTF-8");
            let locating = body.lines().filter(|line| {
                !line.trim_start().starts_with('#')
                    && LOCATORS.iter().any(|locator| line.contains(locator))
            });
            for line in locating {
                for (at, _) in line.match_indices("pls-") {
                    let rest = &line[at..];
                    let end = rest
                        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'))
                        .unwrap_or(rest.len());
                    let name = rest[..end].trim_end_matches('.');
                    let callable = [".ps1", ".psm1", ".psd1"]
                        .iter()
                        .any(|suffix| name.ends_with(suffix));
                    if callable && !names.contains(name) {
                        missing.push(format!("{} -> {name}", script.path));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "called but not embedded: {missing:?}");
    }

    #[test]
    fn extraction_writes_the_layout_verifies_it_and_cleans_up() {
        let parent = std::env::temp_dir();
        let extracted = extract(&parent).expect("the bundle extracts");
        let root = extracted.root().to_path_buf();
        for script in BUNDLE {
            let bytes = std::fs::read(extracted.path(script.path)).expect("written");
            assert_eq!(bytes, script.bytes, "{}", script.path);
        }
        assert!(extracted.path("interim/pls-interim-loader.ps1").is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&root).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "the run folder is private");
        }
        let second = extract(&parent).expect("a second run gets its own folder");
        assert_ne!(second.root(), root.as_path());
        drop(extracted);
        assert!(!root.exists(), "the run folder is removed with the run");
    }

    #[test]
    fn the_bundle_digest_is_stable_and_names_every_pin() {
        assert_eq!(digest(), digest());
        assert!(digest().starts_with("sha256:"));
        assert_eq!(digest().len(), "sha256:".len() + 64);
    }
}
