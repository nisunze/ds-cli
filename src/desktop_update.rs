//! Closed update inspection and download admission. Hosts schedule and execute
//! the existing verified transfer; inspection never installs or activates.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

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

/// Three existing release owners, with no new installation registry. Module
/// admission remains read-only; only the shell has an update transaction.
pub fn inspect(
    component: &str,
    channel: &str,
    input: InspectionInput,
) -> Result<Inspection, String> {
    if !matches!(component, "shell" | "solar" | "reporter")
        || !matches!(channel, "stable" | "canary" | "local")
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
    if component != "shell" {
        let version = ["release_version", "package_version", "version"]
            .iter()
            .find_map(|key| {
                input
                    .installed
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
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
    let published = release_identity(Some(&release.version), Some(&release.released_at));
    let asset = kind_order(&input.platform)
        .iter()
        .find_map(|kind| {
            release
                .assets
                .iter()
                .find(|a| a.platform == input.platform && a.arch == "x86_64" && a.kind == *kind)
        })
        .cloned();
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
mod tests {
    use super::*;
    use serde_json::json;
    fn input() -> InspectionInput {
        serde_json::from_value(json!({"platform":"linux","installed":{"version":"1.0.0","sourceSha":"old","builtAt":"2026-10-01T00:00:00Z"},"release":{"channel":"canary","productId":"rw.datasolutions.desktop.canary","version":"1.0.0-r4","releasedAt":"2026-10-03T00:00:00Z","assets":[{"platform":"linux","arch":"x86_64","kind":"deb","url":"https://example.test/app.deb","sizeBytes":10,"sha256":"a".repeat(64),"sourceSha":"new","builtAt":"2026-10-02T00:00:00Z"}]}})).unwrap()
    }
    #[test]
    fn release_number_is_real_publication_identity_not_build_time() {
        assert_eq!(
            release_identity(Some("1.0.0-r4"), Some("2026-10-03T00:00:00Z")),
            ReleaseIdentity {
                version: Some("1.0.0".into()),
                build_number: Some(4),
                release_date: Some("2026-10-03T00:00:00Z".into())
            }
        );
        assert_eq!(release_identity(Some("1.0.0"), None).build_number, None);
        assert_eq!(
            release_identity(Some("1.0.0-rx"), Some("invalid")).release_date,
            None
        );
    }
    #[test]
    fn inspection_admits_main_update_and_truthful_missing_installed_fields() {
        let result = inspect("shell", "canary", input()).unwrap();
        assert!(result.update.is_some());
        assert_eq!(result.installed.build_number, None);
        assert_eq!(result.installed.release_date, None);
        assert_eq!(result.published.unwrap().build_number, Some(4));
    }
    #[test]
    fn carried_asset_ahead_identity_withdrawal_and_crossed_lane_never_update() {
        for (field, value) in [("status", json!("blocked")), ("version", json!("0.9.0-r8"))] {
            let mut held = input();
            held.release[field] = value;
            assert!(inspect("shell", "canary", held).unwrap().update.is_none());
        }
        let mut held = input();
        held.release["assets"][0]["builtAt"] = json!("2026-09-01T00:00:00Z");
        assert!(inspect("shell", "canary", held).unwrap().update.is_none());
        let mut held = input();
        held.release["productId"] = json!("rw.datasolutions.desktop");
        assert!(inspect("shell", "canary", held).is_err());
        let mut held = input();
        held.release["assets"][0]["sha256"] = json!("wrong");
        assert!(inspect("shell", "canary", held).is_err());
        let mut held = input();
        held.release["assets"][0]["sizeBytes"] = json!(0);
        assert!(inspect("shell", "canary", held).is_err());
        let mut held = input();
        held.release["assets"][0]["arch"] = json!("arm64");
        assert!(inspect("shell", "canary", held).unwrap().update.is_none());
    }
    #[test]
    fn development_acquisition_and_unbounded_metadata_refuse() {
        let request = DownloadRequest {
            version: "1.0.0-r2".into(),
            channel: "canary".into(),
            product_id: "rw.datasolutions.desktop.canary".into(),
            platform: "linux".into(),
            kind: "deb".into(),
            url: "https://example.test/app".into(),
            size_bytes: 1,
            sha256: "a".repeat(64),
        };
        assert!(validate_request(&request, "local", "linux").is_err());
        assert!(validate_request(&request, "stable", "linux").is_err());
        let mut held = input();
        held.installed = json!({"version":"x".repeat(33*1024)});
        assert!(inspect("shell", "canary", held).is_err());
    }

    #[test]
    fn same_version_identity_comparison_preserves_rebuilds_offsets_and_missing_facts() {
        let mut same_source = input();
        same_source.release["assets"][0]["sourceSha"] = json!("old");
        assert!(inspect("shell", "canary", same_source)
            .unwrap()
            .update
            .is_some());
        let mut offset = input();
        offset.installed["builtAt"] = json!("2026-10-02T01:00:00+02:00");
        assert!(inspect("shell", "canary", offset).unwrap().update.is_some());
        for field in ["sourceSha", "builtAt"] {
            let mut missing = input();
            missing.installed[field] = Value::Null;
            assert!(inspect("shell", "canary", missing)
                .unwrap()
                .update
                .is_none());
            let mut missing = input();
            missing.release["assets"][0][field] = json!("");
            assert!(inspect("shell", "canary", missing)
                .unwrap()
                .update
                .is_none());
        }
        let mut malformed = input();
        malformed.release["assets"][0]["builtAt"] = json!("later-ish");
        assert!(inspect("shell", "canary", malformed)
            .unwrap()
            .update
            .is_none());
        let mut newer = input();
        newer.release["version"] = json!("2.0.0-r1");
        newer.installed["sourceSha"] = Value::Null;
        newer.installed["builtAt"] = Value::Null;
        assert!(inspect("shell", "canary", newer).unwrap().update.is_some());
        let mut identical = input();
        identical.release["assets"][0]["sourceSha"] = json!("old");
        identical.release["assets"][0]["builtAt"] = json!("2026-10-01T00:00:00Z");
        assert!(inspect("shell", "canary", identical)
            .unwrap()
            .update
            .is_none());
    }

    #[test]
    fn carry_is_platform_specific_and_legacy_source_fallback_remains_readable() {
        let mut carry = input();
        let mut windows = carry.release["assets"][0].clone();
        windows["platform"] = json!("windows");
        windows["kind"] = json!("nsis");
        windows["sourceSha"] = json!("old");
        windows["builtAt"] = json!("2026-10-01T00:00:00Z");
        carry.release["assets"]
            .as_array_mut()
            .unwrap()
            .push(windows);
        let mut win = carry.clone();
        win.platform = "windows".into();
        assert!(inspect("shell", "canary", win).unwrap().update.is_none());
        let mut newer_pointer = carry.clone();
        newer_pointer.release["version"] = json!("2.0.0-r5");
        newer_pointer.platform = "windows".into();
        assert!(inspect("shell", "canary", newer_pointer)
            .unwrap()
            .update
            .is_none());
        assert!(inspect("shell", "canary", carry).unwrap().update.is_some());
        let mut legacy = input();
        legacy.release["sourceSha"] = json!("new");
        legacy.release["builtAt"] = json!("2026-10-02T00:00:00Z");
        legacy.release["assets"][0]["sourceSha"] = json!("");
        legacy.release["assets"][0]["builtAt"] = json!("");
        assert!(inspect("shell", "canary", legacy).unwrap().update.is_some());
        let mut unsupported = input();
        unsupported.platform = "windows".into();
        assert!(inspect("shell", "canary", unsupported)
            .unwrap()
            .update
            .is_none());
    }
    #[test]
    fn absent_optional_receipt_fields_do_not_hide_known_versions() {
        for missing in [Value::Null, json!("")] {
            let input = InspectionInput {
                release: Value::Null,
                installed: json!({"release_version":missing,"package_version":"1.2.3"}),
                platform: "linux".into(),
            };
            assert_eq!(
                inspect("solar", "local", input)
                    .unwrap()
                    .installed
                    .version
                    .as_deref(),
                Some("1.2.3")
            );
        }
        let mut held = input();
        held.installed["releaseVersion"] = json!("");
        assert_eq!(
            inspect("shell", "canary", held)
                .unwrap()
                .installed
                .version
                .as_deref(),
            Some("1.0.0")
        );
    }
}
