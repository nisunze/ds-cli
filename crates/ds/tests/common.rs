//! Shared test helpers.

use std::path::PathBuf;

/// The real `.dsgrid` fixture, in the authoritative repository that owns the
/// format.
///
/// It is referenced rather than copied on purpose. A vendored copy would keep
/// passing after the format moved on, which is worse than no test: it would
/// report parity that no longer exists. `ds-cli` already cannot build without
/// `ds-network` on disk — it links its crates by path — so depending on it
/// here adds no new requirement.
pub fn fixture() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
    let path = path.canonicalize().unwrap_or(path);
    assert!(
        path.is_file(),
        "the ds-network fixture is missing at {}. ds-cli links ds-network by \
         path, so the sibling repository is expected to be present.",
        path.display()
    );
    path.display().to_string()
}
