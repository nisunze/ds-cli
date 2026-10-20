//! DS Core's own published release, projected beside the shell's. Read-only:
//! Core installation stays with its installer; nothing here downloads or runs.
use super::{release_identity, Inspection, InspectionInput, ReleaseIdentity};
use crate::module_release::MODULE_RELEASE_SCHEMA;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const CORE_PROTOCOL: &str = "ds.core-release/v1";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CoreAsset {
    platform: String,
    arch: String,
    kind: String,
    url: String,
    size_bytes: u64,
    sha256: String,
    source_sha: String,
    built_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    released_at: String,
    #[serde(default)]
    source_revisions: BTreeMap<String, String>,
    protocol: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CoreRelease {
    schema: String,
    component: String,
    channel: String,
    product_id: String,
    version: String,
    protocol: String,
    released_at: String,
    status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    notes: String,
    assets: Vec<CoreAsset>,
}

/// The executable this platform installs; receipts are not Core itself.
fn executable_kind(platform: &str) -> Option<&'static str> {
    match platform {
        "linux" => Some("core-deb"),
        "windows" => Some("core-zip"),
        _ => None,
    }
}

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn admit(channel: &str, release: Value) -> Result<CoreRelease, String> {
    let release: CoreRelease =
        serde_json::from_value(release).map_err(|_| "The held Core release is malformed")?;
    let product = if channel == "canary" {
        "rw.datasolutions.core.canary"
    } else {
        "rw.datasolutions.core"
    };
    if release.schema != MODULE_RELEASE_SCHEMA
        || release.component != "core"
        || release.channel != channel
        || release.product_id != product
        || release.protocol != CORE_PROTOCOL
    {
        return Err("The Core release belongs to another lane or product".into());
    }
    if !matches!(
        release.status.as_str(),
        "active" | "blocked" | "stopped" | "rolled_back"
    ) || release.assets.is_empty()
        || release.assets.len() > 12
        || release.assets.iter().any(|asset| {
            asset.protocol != CORE_PROTOCOL
                || !matches!(asset.platform.as_str(), "windows" | "linux" | "macos")
                || !digest(&asset.sha256)
                || asset.size_bytes == 0
                || asset.source_revisions.len() > 16
        })
    {
        return Err("The held Core release is invalid".into());
    }
    Ok(release)
}

/// Installed Core's runtime identity names its exact source revision. The
/// release built from that revision is the installed release; a different or
/// missing revision keeps the installed publication fields unrecorded.
pub(super) fn inspect(channel: &str, input: InspectionInput) -> Result<Inspection, String> {
    let text = |key: &str| {
        input
            .installed
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };
    let mut installed = release_identity(text("version"), None);
    let source = text("source_sha").map(str::to_owned);
    if input.release.is_null() {
        return Ok(Inspection {
            component: "core".into(),
            installed,
            published: None,
            release: Value::Null,
            update: None,
        });
    }
    let release = admit(channel, input.release)?;
    let asset = executable_kind(&input.platform).and_then(|kind| {
        release
            .assets
            .iter()
            .find(|a| a.platform == input.platform && a.arch == "x86_64" && a.kind == kind)
    });
    let date = asset
        .map(|a| a.released_at.as_str())
        .filter(|date| !date.is_empty())
        .unwrap_or(&release.released_at);
    let published: ReleaseIdentity = release_identity(Some(&release.version), Some(date));
    if asset.is_some_and(|a| source.as_deref() == Some(a.source_sha.as_str())) {
        installed = published.clone();
    }
    Ok(Inspection {
        component: "core".into(),
        installed,
        published: Some(published),
        release: serde_json::to_value(&release).map_err(|e| e.to_string())?,
        update: None,
    })
}
