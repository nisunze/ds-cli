//! Installed release identity of the main application and DS Core.
//!
//! Their platform installers place bytes that were built before publication
//! assigned a release number, so an installed build learns its release only by
//! proof: the lane's admitted catalog member publishes exactly this build for
//! this platform. The host retains that proof beside its module receipts, and
//! the installed row keeps its version, build number and release date after the
//! catalog moves on or cannot be read. Pure: the host reads the catalog, reads
//! and persists the record, and supplies the clock.
use super::{
    inspect_proving, parse_release_number, release_identity, BuildIdentity, DesktopUpdate,
    InspectionInput, ReleaseIdentity,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub const INSTALLATION_SCHEMA: &str = "ds.release-installation/v1";
/// A record is a few hundred bytes; anything larger is not one.
pub const MAX_RECORD_BYTES: usize = 4 * 1024;
const MESSAGE_MAX_CHARS: usize = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Application {
    /// The installed main application (the Desktop shell).
    Shell,
    /// DS Core, the `ds` executable.
    Core,
}

impl Application {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "shell" => Ok(Self::Shell),
            "core" => Ok(Self::Core),
            _ => Err(
                "release_identity_invalid: name the main application (shell) or DS Core (core)"
                    .into(),
            ),
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Core => "core",
        }
    }
    /// The exact fields that bind a record to one build, and whether each is required.
    fn fields(self) -> &'static [(&'static str, Kind, bool)] {
        match self {
            Self::Shell => &[
                ("source_sha", Kind::Source, true),
                ("built_at", Kind::Time, true),
            ],
            Self::Core => &[
                ("source_sha", Kind::Commit, true),
                ("executable_sha256", Kind::Digest, true),
                ("command_kernel_source_sha", Kind::Commit, false),
                ("ds_network_source_sha", Kind::Commit, false),
                ("native_client_profile_catalog_sha256", Kind::Digest, false),
            ],
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    /// A Git commit.
    Commit,
    /// A SHA-256.
    Digest,
    /// A commit or the main application's composite SHA-256 build identity.
    Source,
    /// An RFC 3339 build time.
    Time,
}

impl Kind {
    fn admits(self, value: &str) -> bool {
        let hex = |lengths: &[usize]| {
            lengths.contains(&value.len())
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        };
        match self {
            Self::Commit => hex(&[40]),
            Self::Digest => hex(&[64]),
            Self::Source => hex(&[40, 64]),
            Self::Time => value.len() <= 40 && OffsetDateTime::parse(value, &Rfc3339).is_ok(),
        }
    }
}

/// Proof that one exact installed build is one platform publication.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstallationRecord {
    pub schema: String,
    pub component: Application,
    pub channel: String,
    pub platform: String,
    /// The installed build's exact identity, closed per component.
    pub build: BTreeMap<String, String>,
    /// The publication of these bytes, e.g. `0.2.0-r118`.
    pub release_version: String,
    pub released_at: String,
    pub recorded_at: String,
}

/// What the host observed of the lane's catalog for this read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogRead {
    /// The catalog answered: its member, or null when it publishes none.
    Read(Value),
    /// The read failed; the reason is shown and the installed row stays.
    Unavailable(String),
    /// The caller asked for retained facts only.
    Offline,
    /// Local development has no public release lane.
    Local,
}

#[derive(Clone, Debug)]
pub struct ReleaseIdentityInput {
    pub catalog: CatalogRead,
    /// The build's own identity: Core's runtime identity with its executable
    /// digest, or the main application's build identity. Null for a main
    /// application the host cannot see; its last proven build then stands in.
    pub installed: Value,
    pub platform: String,
    /// The host's retained record for this component and lane, unvalidated.
    pub record: Option<Value>,
    /// The host clock, RFC 3339.
    pub now: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    /// `catalog`: the lane's member publishes exactly this build. `recorded`:
    /// an earlier proof of this build (or, for a main application the host
    /// cannot see, its last proven build). `unrecorded`: only its own version.
    pub source: &'static str,
    pub recorded_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CatalogState {
    /// `read`, `unpublished`, `unavailable`, `offline` or `local`.
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReleaseIdentityAnswer {
    pub component: Application,
    pub channel: String,
    pub installed: ReleaseIdentity,
    pub published: Option<ReleaseIdentity>,
    pub release: Value,
    pub update: Option<DesktopUpdate>,
    pub installation: Installation,
    pub catalog: CatalogState,
    /// A new or corrected proof the host persists. Never shown.
    #[serde(skip)]
    pub record: Option<InstallationRecord>,
}

/// The exact build identity of an installed claim, or none when the build
/// cannot be bound (unknown fields, a debug or dirty Core).
pub fn build_key(application: Application, installed: &Value) -> Option<BTreeMap<String, String>> {
    let mut key = BTreeMap::new();
    match application {
        Application::Shell => {
            let identity: BuildIdentity = serde_json::from_value(installed.clone()).ok()?;
            key.insert("source_sha".into(), identity.source_sha?);
            key.insert("built_at".into(), identity.built_at?);
        }
        Application::Core => {
            if installed.get("dirty") == Some(&Value::Bool(true))
                || installed
                    .get("profile")
                    .and_then(Value::as_str)
                    .is_some_and(|profile| profile != "release")
            {
                return None;
            }
            for (field, _, _) in application.fields() {
                if let Some(value) = installed.get(*field).and_then(Value::as_str) {
                    key.insert((*field).to_owned(), value.to_owned());
                }
            }
        }
    }
    valid_build(application, &key).then_some(key)
}

fn valid_build(application: Application, build: &BTreeMap<String, String>) -> bool {
    let fields = application.fields();
    build.iter().all(|(name, value)| {
        fields
            .iter()
            .any(|(field, kind, _)| field == name && kind.admits(value))
    }) && fields
        .iter()
        .all(|(field, _, required)| !required || build.contains_key(*field))
}

fn rfc3339(value: &str) -> bool {
    value.len() <= 40 && OffsetDateTime::parse(value, &Rfc3339).is_ok()
}

/// A retained record of exactly this component, lane and platform. A foreign,
/// corrupt or excessive record is ignored and never trusted.
fn admit_record(
    raw: &Value,
    application: Application,
    channel: &str,
    platform: &str,
) -> Option<InstallationRecord> {
    if raw.to_string().len() > MAX_RECORD_BYTES {
        return None;
    }
    let record: InstallationRecord = serde_json::from_value(raw.clone()).ok()?;
    (record.schema == INSTALLATION_SCHEMA
        && record.component == application
        && record.channel == channel
        && record.platform == platform
        && parse_release_number(&record.release_version).is_some()
        && rfc3339(&record.released_at)
        && rfc3339(&record.recorded_at)
        && valid_build(application, &record.build))
    .then_some(record)
}

/// The main application's last proven build, as its own build identity.
fn recorded_claim(record: &InstallationRecord) -> Value {
    json!({
        "version": parse_release_number(&record.release_version).map(|(base, _)| base),
        "sourceSha": record.build.get("source_sha"),
        "builtAt": record.build.get("built_at"),
    })
}

/// Decide the installed and published rows of the main application or Core.
pub fn project(
    application: Application,
    channel: &str,
    input: ReleaseIdentityInput,
) -> Result<ReleaseIdentityAnswer, String> {
    let invalid = |message: &str| format!("release_identity_invalid: {message}");
    if !rfc3339(&input.now) {
        return Err(invalid("the host clock is not RFC 3339"));
    }
    if (channel == "local") != (input.catalog == CatalogRead::Local) {
        return Err(invalid(
            "local development reads no public lane, and a public lane reads its catalog or names why not",
        ));
    }
    let retained = input
        .record
        .as_ref()
        .and_then(|raw| admit_record(raw, application, channel, &input.platform));
    // Core always names itself. A main application the host cannot see is
    // represented by its last proven build, and says so.
    let from_record = application == Application::Shell && input.installed.is_null();
    let claim = match (&retained, from_record) {
        (Some(record), true) => recorded_claim(record),
        (None, true) => Value::Null,
        (_, false) => input.installed,
    };
    let key = build_key(application, &claim);
    let (release, catalog) = match input.catalog {
        CatalogRead::Read(release) if release.is_null() => {
            (Value::Null, state("unpublished", None))
        }
        CatalogRead::Read(release) => (release, state("read", None)),
        CatalogRead::Unavailable(message) => (
            Value::Null,
            state(
                "unavailable",
                Some(message.chars().take(MESSAGE_MAX_CHARS).collect()),
            ),
        ),
        CatalogRead::Offline => (Value::Null, state("offline", None)),
        CatalogRead::Local => (Value::Null, state("local", None)),
    };
    let (inspection, proven) = inspect_proving(
        application.as_str(),
        channel,
        InspectionInput {
            release,
            installed: claim,
            platform: input.platform.clone(),
        },
    )?;
    let mut answer = ReleaseIdentityAnswer {
        component: application,
        channel: channel.to_owned(),
        installed: inspection.installed,
        published: inspection.published,
        release: inspection.release,
        update: inspection.update,
        installation: Installation {
            source: if from_record && retained.is_some() {
                "recorded"
            } else {
                "unrecorded"
            },
            recorded_at: retained
                .as_ref()
                .filter(|_| from_record)
                .map(|record| record.recorded_at.clone()),
        },
        catalog,
        record: None,
    };
    // A proof without a publication date cannot be a complete record.
    let key = key.filter(|_| proven.as_ref().is_none_or(|p| rfc3339(&p.released_at)));
    match (proven, key) {
        (Some(proven), Some(build)) => {
            let kept = retained.as_ref().filter(|record| {
                record.build == build
                    && record.release_version == proven.release_version
                    && record.released_at == proven.released_at
            });
            answer.installed =
                release_identity(Some(&proven.release_version), Some(&proven.released_at));
            answer.installation = Installation {
                source: if from_record { "recorded" } else { "catalog" },
                recorded_at: Some(
                    kept.map_or_else(|| input.now.clone(), |r| r.recorded_at.clone()),
                ),
            };
            if kept.is_none() {
                answer.record = Some(InstallationRecord {
                    schema: INSTALLATION_SCHEMA.into(),
                    component: application,
                    channel: channel.to_owned(),
                    platform: input.platform,
                    build,
                    release_version: proven.release_version,
                    released_at: proven.released_at,
                    recorded_at: input.now,
                });
            }
        }
        // Proven, but this build cannot be bound to a record (no executable
        // digest): show the proof, retain nothing.
        (Some(proven), None) => {
            answer.installed =
                release_identity(Some(&proven.release_version), Some(&proven.released_at));
            answer.installation = Installation {
                source: "catalog",
                recorded_at: None,
            };
        }
        (None, Some(build)) => {
            let version = answer.installed.version.clone();
            if let Some(record) = retained.filter(|record| {
                record.build == build
                    && parse_release_number(&record.release_version)
                        .is_some_and(|(base, _)| version.as_deref() == Some(base))
            }) {
                answer.installed =
                    release_identity(Some(&record.release_version), Some(&record.released_at));
                answer.installation = Installation {
                    source: "recorded",
                    recorded_at: Some(record.recorded_at),
                };
            }
        }
        (None, None) => {}
    }
    Ok(answer)
}

fn state(state: &'static str, message: Option<String>) -> CatalogState {
    CatalogState { state, message }
}

#[cfg(test)]
#[path = "installation_tests.rs"]
mod tests;
