//! Semantic admission of the two fixed standalone module catalogs.
//!
//! Public metadata is not an installation or signature grant. This pure owner
//! performs no read, download, process launch, subscription or activation.

use std::{collections::BTreeMap, fmt};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use url::Url;

pub const MODULE_RELEASE_SCHEMA: &str = "ds.module-release/v1";
pub const MODULE_PROTOCOL: &str = "ds.module/v1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModuleComponent {
    Solar,
    Reporter,
}

impl ModuleComponent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Solar => "solar",
            Self::Reporter => "reporter",
        }
    }

    pub fn engine_owner(self) -> &'static str {
        match self {
            Self::Solar => "ds-solar",
            Self::Reporter => "ds-network-reporter",
        }
    }

    fn parse(component: &str) -> Result<Self, ModuleReleaseError> {
        match component {
            "solar" => Ok(Self::Solar),
            "reporter" => Ok(Self::Reporter),
            _ => fail(
                "module_release_component_invalid",
                "Component must be solar or reporter",
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleEngineIdentity {
    pub engine: String,
    pub release: String,
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleReleaseAsset {
    pub platform: String,
    pub arch: String,
    pub kind: String,
    pub protocol: String,
    pub url: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub source_sha: String,
    pub built_at: String,
    pub source_revisions: BTreeMap<String, String>,
    pub engines: Vec<ModuleEngineIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleRelease {
    pub schema: String,
    pub component: ModuleComponent,
    pub channel: String,
    pub product_id: String,
    pub version: String,
    pub protocol: String,
    pub released_at: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub assets: Vec<ModuleReleaseAsset>,
}

pub type ModuleReleaseManifest = ModuleRelease;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModuleReleaseError {
    pub code: &'static str,
    pub message: String,
}

impl fmt::Display for ModuleReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ModuleReleaseError {}

fn fail<T>(code: &'static str, message: &str) -> Result<T, ModuleReleaseError> {
    Err(ModuleReleaseError {
        code,
        message: message.into(),
    })
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn release_folder(value: &str) -> bool {
    value.len() <= 80
        && Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+(?:-r[0-9]+)?$")
            .unwrap()
            .is_match(value)
}

fn bounded_fields(value: &Value, allowed: &[&str], max_string: usize) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.iter().all(|(key, value)| {
        allowed.contains(&key.as_str())
            && match key.as_str() {
                "assets" | "source_revisions" | "engines" => true,
                "size_bytes" => value.is_i64(),
                _ => value.as_str().is_some_and(|text| text.len() <= max_string),
            }
    })
}

// Bound collections and strings before deserializing or cloning the held DTO.
fn bounded_shape(value: &Value) -> bool {
    if !bounded_fields(
        value,
        &[
            "schema",
            "component",
            "channel",
            "product_id",
            "version",
            "protocol",
            "released_at",
            "status",
            "notes",
            "assets",
        ],
        8000,
    ) {
        return false;
    }
    let Some(assets) = value.get("assets").and_then(Value::as_array) else {
        return false;
    };
    if assets.is_empty() || assets.len() > 2 {
        return false;
    }
    assets.iter().all(|asset| {
        if !bounded_fields(
            asset,
            &[
                "platform",
                "arch",
                "kind",
                "protocol",
                "url",
                "size_bytes",
                "sha256",
                "source_sha",
                "built_at",
                "source_revisions",
                "engines",
            ],
            2048,
        ) {
            return false;
        }
        let Some(revisions) = asset.get("source_revisions").and_then(Value::as_object) else {
            return false;
        };
        if revisions.is_empty()
            || revisions.len() > 16
            || revisions.iter().any(|(key, value)| {
                key.len() > 64 || value.as_str().is_none_or(|text| text.len() != 40)
            })
        {
            return false;
        }
        let Some(engines) = asset.get("engines").and_then(Value::as_array) else {
            return false;
        };
        engines.len() == 1
            && bounded_fields(&engines[0], &["engine", "release", "manifest_sha256"], 200)
    })
}

/// Admit one held catalog for exactly the requested component and lane.
/// The caller obtains metadata only on explicit consumer demand and retains it
/// offline. This function does not assert executable signature trust.
pub fn validate_module_release(
    component: &str,
    channel: &str,
    release: &Value,
) -> Result<ModuleRelease, ModuleReleaseError> {
    let component = ModuleComponent::parse(component)?;
    if !matches!(channel, "stable" | "canary") {
        return fail(
            "module_release_channel_invalid",
            "Channel must be stable or canary",
        );
    }
    if !bounded_shape(release) {
        return fail(
            "module_release_invalid",
            "Module release is outside its closed bounded shape",
        );
    }
    let release: ModuleRelease =
        serde_json::from_value(release.clone()).map_err(|_| ModuleReleaseError {
            code: "module_release_invalid",
            message: "Module release does not match its declared schema".into(),
        })?;
    if release.schema != MODULE_RELEASE_SCHEMA {
        return fail(
            "module_release_schema_invalid",
            "Module release schema is unsupported",
        );
    }
    let product_id = format!(
        "rw.datasolutions.module.{}{}",
        component.as_str(),
        if channel == "canary" { ".canary" } else { "" }
    );
    if release.component != component
        || release.channel != channel
        || release.product_id != product_id
    {
        return fail(
            "module_release_identity_mismatch",
            "Module release does not match the requested component and lane",
        );
    }
    if release.protocol != MODULE_PROTOCOL {
        return fail(
            "module_release_protocol_unsupported",
            "Module protocol is unsupported",
        );
    }
    if !release_folder(&release.version)
        || OffsetDateTime::parse(&release.released_at, &Rfc3339).is_err()
        || !matches!(
            release.status.as_str(),
            "active" | "blocked" | "stopped" | "rolled_back"
        )
    {
        return fail(
            "module_release_invalid",
            "Module release version, timestamp or governance status is invalid",
        );
    }
    let repository_pattern = Regex::new(r"^ds-[a-z][a-z0-9-]{0,60}$").unwrap();
    let engine_release_pattern = Regex::new(r"^[A-Za-z0-9][A-Za-z0-9.+_@-]{0,199}$").unwrap();
    let mut platforms = Vec::with_capacity(2);
    for asset in &release.assets {
        if !matches!(asset.platform.as_str(), "linux" | "windows")
            || asset.arch != "x86_64"
            || asset.kind != "module-executable"
            || asset.protocol != MODULE_PROTOCOL
            || platforms.contains(&asset.platform.as_str())
        {
            return fail(
                "module_release_target_unsupported",
                "Module asset target, kind or protocol is unsupported or repeated",
            );
        }
        platforms.push(asset.platform.as_str());
        if asset.size_bytes <= 0 || !hex(&asset.sha256, 64) {
            return fail(
                "module_release_asset_invalid",
                "Module asset size and SHA-256 must be exact",
            );
        }
        let engine = &asset.engines[0];
        if !hex(&asset.source_sha, 40)
            || asset
                .source_revisions
                .get(component.engine_owner())
                .map(String::as_str)
                != Some(asset.source_sha.as_str())
            || asset.source_revisions.iter().any(|(repository, revision)| {
                !repository_pattern.is_match(repository) || !hex(revision, 40)
            })
            || OffsetDateTime::parse(&asset.built_at, &Rfc3339).is_err()
            || engine.engine != component.engine_owner()
            || !engine_release_pattern.is_match(&engine.release)
            || !hex(&engine.manifest_sha256, 64)
        {
            return fail(
                "module_release_provenance_invalid",
                "Module asset lacks its exact producer closure and engine identity",
            );
        }
        let Ok(url) = Url::parse(&asset.url) else {
            return fail("module_release_url_invalid", "Module asset URL is invalid");
        };
        let segments: Vec<_> = url.path().split('/').collect();
        let filename = format!(
            "{}{}",
            component.as_str(),
            if asset.platform == "windows" {
                ".exe"
            } else {
                ""
            }
        );
        if url.scheme() != "https"
            || url.host_str() != Some("storage.googleapis.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || segments.len() != 7
            || segments[1].is_empty()
            || segments[2] != "modules"
            || segments[3] != component.as_str()
            || segments[4] != channel
            || !release_folder(segments[5])
            || segments[6] != filename
            || asset.url != url.as_str()
        {
            return fail(
                "module_release_url_invalid",
                "Module asset URL does not name its immutable component and lane executable",
            );
        }
    }
    Ok(release)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog() -> Value {
        json!({"schema":MODULE_RELEASE_SCHEMA,"component":"solar","channel":"canary","product_id":"rw.datasolutions.module.solar.canary","version":"0.1.0-r2","protocol":MODULE_PROTOCOL,"released_at":"2026-10-13T08:00:00Z","status":"active","notes":"Module release","assets":[{
            "platform":"linux","arch":"x86_64","kind":"module-executable","protocol":MODULE_PROTOCOL,"url":"https://storage.googleapis.com/dedicated-public-releases/modules/solar/canary/0.1.0-r1/solar","size_bytes":10,"sha256":"a".repeat(64),"source_sha":"b".repeat(40),"built_at":"2026-10-12T08:00:00Z","source_revisions":{"ds-solar":"b".repeat(40),"ds-command-kernel":"c".repeat(40)},"engines":[{"engine":"ds-solar","release":"ds-solar@0.1.0+bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","manifest_sha256":"d".repeat(64)}]
        }]})
    }

    #[test]
    fn accepts_carried_platform_sources_without_inventing_a_vector() {
        let value = catalog();
        let admitted = validate_module_release("solar", "canary", &value).unwrap();
        assert_eq!(admitted.assets[0].source_revisions.len(), 2);
        assert!(admitted.assets[0].url.contains("0.1.0-r1"));
        assert_eq!(serde_json::to_value(admitted).unwrap(), value);
    }

    #[test]
    fn component_lane_and_legacy_identity_are_fenced() {
        assert_eq!(
            validate_module_release("shell", "canary", &catalog())
                .unwrap_err()
                .code,
            "module_release_component_invalid"
        );
        assert_eq!(
            validate_module_release("solar", "beta", &catalog())
                .unwrap_err()
                .code,
            "module_release_channel_invalid"
        );
        assert_eq!(
            validate_module_release("reporter", "canary", &catalog())
                .unwrap_err()
                .code,
            "module_release_identity_mismatch"
        );
        let mut value = catalog();
        value.as_object_mut().unwrap().remove("schema");
        assert_eq!(
            validate_module_release("solar", "canary", &value)
                .unwrap_err()
                .code,
            "module_release_invalid"
        );
    }

    #[test]
    fn refuses_malformed_targets_provenance_urls_and_unknown_payloads() {
        for (path, replacement) in [
            ("/assets/0/platform", json!("macos")),
            ("/assets/0/arch", json!("aarch64")),
            ("/assets/0/protocol", json!("ds.module/v2")),
            ("/assets/0/sha256", json!("A".repeat(64))),
            ("/assets/0/source_revisions/ds-solar", json!("e".repeat(40))),
            ("/assets/0/engines/0/engine", json!("ds-network-reporter")),
            ("/assets/0/engines/0/manifest_sha256", json!("")),
            ("/assets/0/url", json!("https://example.com/solar")),
            ("/assets/0/url", json!("https://storage.googleapis.com/bucket/modules/solar/stable/0.1.0-r1/solar")),
            ("/assets/0/url", json!("https://storage.googleapis.com/bucket/modules/solar/canary/latest/solar")),
            ("/assets/0/url", json!("https://storage.googleapis.com/bucket/modules/solar/canary/0.1.0-r1/solar?command=run")),
            ("/status", json!("draft")),
            ("/notes", json!("x".repeat(8001))),
        ] {
            let mut value = catalog();
            *value.pointer_mut(path).unwrap() = replacement;
            assert!(validate_module_release("solar", "canary", &value).is_err(), "{path}");
        }
        let mut value = catalog();
        value["command"] = json!("run");
        assert!(validate_module_release("solar", "canary", &value).is_err());
        let mut value = catalog();
        let duplicate = value["assets"][0].clone();
        value["assets"].as_array_mut().unwrap().push(duplicate);
        assert!(validate_module_release("solar", "canary", &value).is_err());
    }

    #[test]
    fn withdrawal_status_is_preserved_and_never_grants_installation() {
        let mut value = catalog();
        value["status"] = json!("blocked");
        assert_eq!(
            validate_module_release("solar", "canary", &value)
                .unwrap()
                .status,
            "blocked"
        );
    }
}
