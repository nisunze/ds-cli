//! DS Core's own published release, projected beside the shell's. Read-only:
//! Core installation stays with its installer; nothing here downloads or runs.
use super::{asset_publication, release_identity, Inspection, InspectionInput, Proven};
use crate::module_release::{same_family, MODULE_RELEASE_SCHEMA};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

const CORE_PROTOCOL: &str = "ds.core-release/v1";

/// Brain's published Core member: additive members are accepted (owner 22 Oct).
#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogRequest {
    channel: String,
    platform: String,
    read: Option<CatalogRead>,
}

#[derive(Deserialize)]
struct CatalogRead {
    release: Value,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogPresentation {
    state: &'static str,
    version: String,
    released_at: String,
    assets: Vec<CatalogAsset>,
}

/// The presentation window keeps unknown dates and provenance explicit. The
/// older catalog asset DTO omits empty optional members on its wire.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogAsset {
    platform: String,
    arch: String,
    kind: String,
    url: String,
    size_bytes: u64,
    sha256: String,
    source_sha: String,
    built_at: String,
    released_at: String,
    source_revisions: BTreeMap<String, String>,
}

impl CatalogPresentation {
    fn empty(state: &'static str) -> Self {
        Self {
            state,
            version: String::new(),
            released_at: String::new(),
            assets: vec![],
        }
    }
}

/// Shared read-only presentation of Core's independent catalog. Hosts supply
/// the anonymous member; they neither admit it nor infer Core from Desktop.
#[cfg_attr(feature = "wasm", wasm_bindgen::prelude::wasm_bindgen(js_name = coreReleasePresentation))]
pub fn core_release_presentation(input: &[u8]) -> Result<String, String> {
    if input.len() > 128 * 1024 {
        return Err("The Core catalog request exceeds its bound".into());
    }
    let request: CatalogRequest = serde_json::from_slice(input)
        .map_err(|_| "The Core catalog request is malformed".to_owned())?;
    if !matches!(request.channel.as_str(), "stable" | "canary")
        || !matches!(request.platform.as_str(), "windows" | "linux" | "macos")
    {
        return Err("Name the exact Core release lane and platform".into());
    }
    serde_json::to_string(&present_catalog(request))
        .map_err(|_| "The Core catalog presentation is unreadable".to_owned())
}

fn present_catalog(request: CatalogRequest) -> CatalogPresentation {
    let empty = CatalogPresentation::empty;
    let Some(kind) = executable_kind(&request.platform) else {
        return empty("unsupported");
    };
    let Some(read) = request.read else {
        return empty("failed");
    };
    if read.release.is_null() {
        return empty("unpublished");
    }
    let Ok(release) = admit(&request.channel, read.release) else {
        return empty("malformed");
    };
    if release.status != "active" {
        return empty("withdrawn");
    }
    let mut assets = release.assets.iter().filter(|asset| {
        asset.platform == request.platform && asset.arch == "x86_64" && asset.kind == kind
    });
    let Some(asset) = assets.next() else {
        return empty("unpublished");
    };
    if assets.next().is_some()
        || !catalog_package(asset, &request.channel)
        || super::OffsetDateTime::parse(&release.released_at, &super::Rfc3339).is_err()
        || (!asset.released_at.is_empty()
            && super::OffsetDateTime::parse(&asset.released_at, &super::Rfc3339).is_err())
    {
        return empty("malformed");
    }
    let (version, released_at) = asset_publication(
        &release.version,
        &release.released_at,
        &asset.url,
        &asset.released_at,
    );
    CatalogPresentation {
        state: "available",
        version,
        released_at: released_at.clone(),
        assets: vec![CatalogAsset {
            platform: asset.platform.clone(),
            arch: asset.arch.clone(),
            kind: asset.kind.clone(),
            url: asset.url.clone(),
            size_bytes: asset.size_bytes,
            sha256: asset.sha256.clone(),
            source_sha: asset.source_sha.clone(),
            built_at: asset.built_at.clone(),
            released_at,
            source_revisions: asset.source_revisions.clone(),
        }],
    }
}

fn catalog_package(asset: &CoreAsset, channel: &str) -> bool {
    let Ok(url) = url::Url::parse(&asset.url) else {
        return false;
    };
    if asset.url.len() > 2048
        || url.scheme() != "https"
        || url.host_str() != Some("storage.googleapis.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || asset.url != url.as_str()
        || !matches!(asset.source_sha.len(), 40 | 64)
        || !asset.source_sha.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return false;
    }
    // Core publications have their own immutable namespace. A Desktop asset
    // in a historical shared pointer cannot become a Core publication here.
    let Some(segments) = url.path_segments() else {
        return false;
    };
    let segments: Vec<_> = segments.collect();
    if segments.len() != 5 || segments[0].is_empty() {
        return false;
    }
    let part = &segments[1..];
    part[0] == "core"
        && part[1] == channel
        && super::parse_release_number(part[2]).is_some()
        && !part[3].is_empty()
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
    if !same_family(&release.schema, MODULE_RELEASE_SCHEMA)
        || release.component != "core"
        || release.channel != channel
        || release.product_id != product
        || !same_family(&release.protocol, CORE_PROTOCOL)
    {
        return Err("The Core release belongs to another lane or product".into());
    }
    if !matches!(
        release.status.as_str(),
        "active" | "blocked" | "stopped" | "rolled_back"
    ) || release.assets.is_empty()
        || release.assets.len() > 12
        || release.assets.iter().any(|asset| {
            !same_family(&asset.protocol, CORE_PROTOCOL)
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

/// The runtime's declared source closure, beside the producing revisions an
/// asset names. Every revision both declare must agree.
const CLOSURE: [(&str, &str); 2] = [
    ("command_kernel_source_sha", "ds-command-kernel"),
    ("ds_network_source_sha", "ds-network"),
];

/// Whether this platform asset is provably the installed Core build: the same
/// producing revision and closure, from a clean release build. A debug or
/// dirty executable compiled from a published commit is not that release.
fn installed_build(installed: &Value, asset: &CoreAsset) -> bool {
    let text = |key: &str| installed.get(key).and_then(Value::as_str);
    text("source_sha").is_some_and(|sha| !sha.is_empty() && sha == asset.source_sha)
        && CLOSURE.iter().all(|(runtime, producer)| {
            match (text(runtime), asset.source_revisions.get(*producer)) {
                (Some(installed), Some(published)) => installed == published,
                _ => true,
            }
        })
        && installed.get("dirty") != Some(&Value::Bool(true))
        && text("profile").is_none_or(|profile| profile == "release")
}

/// Installed Core's runtime identity names its exact producing revisions. The
/// release built from them is the installed release; a different or missing
/// revision keeps the installed publication fields unrecorded.
pub(super) fn inspect(
    channel: &str,
    input: InspectionInput,
) -> Result<(Inspection, Option<Proven>), String> {
    let version = input
        .installed
        .get("version")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty());
    let mut installed = release_identity(version, None);
    if input.release.is_null() {
        let inspection = Inspection {
            component: "core".into(),
            installed,
            published: None,
            release: Value::Null,
            update: None,
        };
        return Ok((inspection, None));
    }
    let release = admit(channel, input.release)?;
    let asset = executable_kind(&input.platform).and_then(|kind| {
        release
            .assets
            .iter()
            .find(|a| a.platform == input.platform && a.arch == "x86_64" && a.kind == kind)
    });
    // A carried platform executable keeps the folder and date that first
    // published it; the member's own fields describe only its new bytes.
    let (published_version, published_date) = asset.map_or_else(
        || (release.version.clone(), release.released_at.clone()),
        |a| {
            asset_publication(
                &release.version,
                &release.released_at,
                &a.url,
                &a.released_at,
            )
        },
    );
    let published = release_identity(Some(&published_version), Some(&published_date));
    let mut proven = None;
    if asset.is_some_and(|a| installed_build(&input.installed, a)) {
        installed = published.clone();
        proven = Some(Proven {
            release_version: published_version,
            released_at: published_date,
        });
    }
    let inspection = Inspection {
        component: "core".into(),
        installed,
        published: Some(published),
        release: serde_json::to_value(&release).map_err(|e| e.to_string())?,
        update: None,
    };
    Ok((inspection, proven))
}

#[cfg(test)]
#[path = "core_release_catalog_tests.rs"]
mod catalog_tests;
