//! Closed update inspection and download admission. Hosts schedule and execute
//! the existing verified transfer; inspection never installs or activates.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

mod core_release;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DownloadRequest {
    pub version: String,
    pub channel: String,
    pub product_id: String,
    pub platform: String,
    pub kind: String,
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
}

pub fn validate_request(
    request: &DownloadRequest,
    lane: &str,
    platform: &str,
) -> Result<(), String> {
    let expected = match lane {
        "canary" => "rw.datasolutions.desktop.canary",
        "stable" => "rw.datasolutions.desktop",
        _ => return Err("Local desktop development cannot acquire public updates".into()),
    };
    if request.channel != lane || request.product_id != expected {
        return Err("The update does not belong to this installed release lane".into());
    }
    if request.platform != platform || !kind_order(platform).contains(&request.kind.as_str()) {
        return Err("The update installer does not match this desktop platform".into());
    }
    if request.version.is_empty()
        || request.version.len() > 64
        || !request
            .version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    {
        return Err("The update version is invalid".into());
    }
    let url = url::Url::parse(&request.url).map_err(|_| "The update URL is invalid")?;
    if request.url.len() > 2048
        || url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("The update URL must be an HTTPS download without embedded credentials".into());
    }
    if request.size_bytes == 0 || request.size_bytes > 1_200_000_000 {
        return Err("The update requires an exact bounded installer size".into());
    }
    if request.sha256.len() != 64
        || !request
            .sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("The update requires an exact lowercase SHA-256".into());
    }
    Ok(())
}

fn kind_order(platform: &str) -> &'static [&'static str] {
    match platform {
        "windows" => &["nsis", "msi"],
        "linux" => &["deb", "appimage", "rpm"],
        "macos" => &["dmg", "app"],
        _ => &[],
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BuildIdentity {
    pub version: Option<String>,
    pub source_sha: Option<String>,
    pub built_at: Option<String>,
    #[serde(default)]
    pub release_version: Option<String>,
    #[serde(default)]
    pub released_at: Option<String>,
}

/// Release date is a publication receipt field. Build time is never substituted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseIdentity {
    pub version: Option<String>,
    pub build_number: Option<u64>,
    pub release_date: Option<String>,
}

pub fn release_identity(version: Option<&str>, released_at: Option<&str>) -> ReleaseIdentity {
    let version = version.filter(|v| !v.is_empty() && v.len() <= 128);
    let parsed = version.and_then(parse_release_number);
    ReleaseIdentity {
        version: version.map(|v| parsed.map_or(v, |(base, _)| base).to_owned()),
        build_number: parsed.map(|(_, number)| number),
        release_date: released_at
            .filter(|v| OffsetDateTime::parse(v, &Rfc3339).is_ok())
            .map(str::to_owned),
    }
}

fn parse_release_number(value: &str) -> Option<(&str, u64)> {
    let (version, release) = value.rsplit_once("-r")?;
    if semver(version).is_none()
        || release.is_empty()
        || !release.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let number = release.parse().ok()?;
    Some((version, number))
}

fn semver(value: &str) -> Option<[u64; 3]> {
    let values = value
        .split('.')
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    values.try_into().ok()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseAsset {
    pub platform: String,
    pub arch: String,
    pub kind: String,
    pub url: String,
    #[serde(alias = "size_bytes")]
    pub size_bytes: Option<u64>,
    pub sha256: String,
    #[serde(default, alias = "source_sha")]
    pub source_sha: String,
    #[serde(default, alias = "built_at")]
    pub built_at: String,
    /// Publication date of these bytes; a carried asset keeps its own.
    #[serde(
        default,
        alias = "released_at",
        skip_serializing_if = "String::is_empty"
    )]
    pub released_at: String,
    #[serde(
        default,
        alias = "source_revisions",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub source_revisions: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopRelease {
    pub channel: String,
    #[serde(alias = "product_id")]
    pub product_id: String,
    pub version: String,
    #[serde(default, alias = "released_at")]
    pub released_at: String,
    #[serde(default, alias = "source_sha")]
    pub source_sha: String,
    #[serde(default, alias = "built_at")]
    pub built_at: String,
    #[serde(default = "active")]
    pub status: String,
    pub assets: Vec<ReleaseAsset>,
}
fn active() -> String {
    "active".into()
}

#[derive(Clone, Debug, Serialize)]
pub struct DesktopUpdate {
    pub release: DesktopRelease,
    pub version: String,
    pub url: String,
    pub asset: ReleaseAsset,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InspectionInput {
    pub release: Value,
    #[serde(default)]
    pub installed: Value,
    pub platform: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Inspection {
    pub component: String,
    pub installed: ReleaseIdentity,
    pub published: Option<ReleaseIdentity>,
    pub release: Value,
    pub update: Option<DesktopUpdate>,
}

/// Existing release owners, with no new installation registry. Module
/// admission remains read-only; only the shell has an update transaction.
pub fn inspect(
    component: &str,
    channel: &str,
    input: InspectionInput,
) -> Result<Inspection, String> {
    if !matches!(
        component,
        "shell" | "core" | "solar" | "reporter" | "network"
    ) || !matches!(channel, "stable" | "canary" | "local")
        || !matches!(input.platform.as_str(), "windows" | "linux" | "macos")
    {
        return Err("Name the exact installed component, release lane and platform".into());
    }
    if channel == "local" && !input.release.is_null() {
        return Err("Local development cannot acquire public updates".into());
    }
    if input.release.to_string().len() > 128 * 1024 || input.installed.to_string().len() > 32 * 1024
    {
        return Err("The held update identity exceeds its bound".into());
    }
    if component == "core" {
        return core_release::inspect(channel, input);
    }
    if component != "shell" {
        let version = ["release_version", "package_version", "version"]
            .iter()
            .find_map(|key| {
                input
                    .installed
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
            })
            .or_else(|| {
                if component == "network" {
                    input
                        .installed
                        .pointer("/engineIdentity/coreVersion")
                        .and_then(Value::as_str)
                        .filter(|v| !v.is_empty())
                } else {
                    None
                }
            });
        let installed = release_identity(
            version,
            input.installed.get("released_at").and_then(Value::as_str),
        );
        let release = if input.release.is_null() {
            None
        } else {
            Some(
                crate::module_release::validate_module_release(component, channel, &input.release)
                    .map_err(|e| e.to_string())?,
            )
        };
        return Ok(Inspection {
            component: component.into(),
            installed,
            published: release
                .as_ref()
                .map(|r| release_identity(Some(&r.version), Some(&r.released_at))),
            release: serde_json::to_value(release).map_err(|e| e.to_string())?,
            update: None,
        });
    }
    let identity: BuildIdentity = if input.installed.is_null() {
        BuildIdentity::default()
    } else {
        serde_json::from_value(input.installed)
            .map_err(|_| "The installed shell identity is malformed")?
    };
    let installed = release_identity(
        identity
            .release_version
            .as_deref()
            .filter(|v| !v.is_empty())
            .or(identity.version.as_deref()),
        identity.released_at.as_deref(),
    );
    if input.release.is_null() {
        return Ok(Inspection {
            component: component.into(),
            installed,
            published: None,
            release: Value::Null,
            update: None,
        });
    }
    let release: DesktopRelease = serde_json::from_value(input.release)
        .map_err(|_| "The held desktop release is malformed")?;
    if release.assets.is_empty()
        || release.assets.len() > 12
        || !matches!(
            release.status.as_str(),
            "active" | "blocked" | "stopped" | "rolled_back"
        )
    {
        return Err("The held desktop release is invalid".into());
    }
    let expected = if channel == "canary" {
        "rw.datasolutions.desktop.canary"
    } else {
        "rw.datasolutions.desktop"
    };
    if release.channel != channel || release.product_id != expected {
        return Err("The desktop release belongs to another lane".into());
    }
    let asset = kind_order(&input.platform)
        .iter()
        .find_map(|kind| {
            release
                .assets
                .iter()
                .find(|a| a.platform == input.platform && a.arch == "x86_64" && a.kind == *kind)
        })
        .cloned();
    // This platform's bytes carry their own publication date.
    let published_date = asset
        .as_ref()
        .map(|a| a.released_at.as_str())
        .filter(|date| !date.is_empty())
        .unwrap_or(&release.released_at);
    let published = release_identity(Some(&release.version), Some(published_date));
    let mut installed = installed;
    let mut update = None;
    if let Some(asset) = asset {
        let request = DownloadRequest {
            version: release.version.clone(),
            channel: release.channel.clone(),
            product_id: release.product_id.clone(),
            platform: asset.platform.clone(),
            kind: asset.kind.clone(),
            url: asset.url.clone(),
            size_bytes: asset.size_bytes.unwrap_or(0),
            sha256: asset.sha256.clone(),
        };
        validate_request(&request, channel, &input.platform)?;
        let latest = published.version.as_deref().and_then(semver);
        let current = identity.version.as_deref().and_then(semver);
        let asset_sha = if asset.source_sha.is_empty() {
            &release.source_sha
        } else {
            &asset.source_sha
        };
        let asset_time = if asset.built_at.is_empty() {
            &release.built_at
        } else {
            &asset.built_at
        };
        let same_build = identity
            .source_sha
            .as_deref()
            .is_some_and(|sha| !sha.is_empty() && sha == asset_sha)
            && identity
                .built_at
                .as_deref()
                .and_then(|v| OffsetDateTime::parse(v, &Rfc3339).ok())
                .zip(OffsetDateTime::parse(asset_time, &Rfc3339).ok())
                .is_some_and(|(installed, published)| installed == published);
        // The installed bytes are this published asset, so its publication
        // identity is proven rather than invented from build time.
        if same_build && installed.build_number.is_none() && installed.release_date.is_none() {
            installed = published.clone();
        }
        let newer = !same_build
            && match (latest, current) {
                (Some(latest), Some(current)) if latest > current => true,
                (Some(latest), Some(current)) if latest == current => {
                    let sha = if asset.source_sha.is_empty() {
                        &release.source_sha
                    } else {
                        &asset.source_sha
                    };
                    let built = if asset.built_at.is_empty() {
                        &release.built_at
                    } else {
                        &asset.built_at
                    };
                    match (
                        identity.source_sha.as_deref().filter(|v| !v.is_empty()),
                        identity.built_at.as_deref(),
                        OffsetDateTime::parse(built, &Rfc3339).ok(),
                    ) {
                        (Some(installed_sha), Some(installed_time), Some(published_time)) => {
                            OffsetDateTime::parse(installed_time, &Rfc3339).is_ok_and(
                                |installed_time| {
                                    !sha.is_empty()
                                        && (!installed_sha.is_empty())
                                        && published_time > installed_time
                                },
                            )
                        }
                        _ => false,
                    }
                }
                _ => false,
            };
        if newer && release.status == "active" {
            update = Some(DesktopUpdate {
                version: published.version.clone().unwrap(),
                url: asset.url.clone(),
                asset,
                release: release.clone(),
            });
        }
    }
    Ok(Inspection {
        component: component.into(),
        installed,
        published: Some(published),
        release: serde_json::to_value(release).map_err(|e| e.to_string())?,
        update,
    })
}

#[cfg(test)]
#[path = "desktop_update_tests.rs"]
mod tests;
