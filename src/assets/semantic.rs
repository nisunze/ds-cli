//! Pinned metadata for an externally produced document derivative. No extraction or IO.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "ds.asset-semantic/v1";
pub const MARKDOWN_TYPE: &str = "text/markdown; charset=utf-8";
pub const MAX_SOURCE_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    CatalogueAsset,
    PmAttachment,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePin {
    pub kind: SourceKind,
    pub id: String,
    pub version: u64,
    pub generation: u64,
    pub size_bytes: u64,
    pub content_type: String,
    /// Measured by native source acquisition; not a claim of server attestation.
    pub measured_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extractor {
    pub name: String,
    pub version: String,
    pub options_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub source: SourcePin,
    pub extractor: Extractor,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepare {
    #[serde(flatten)]
    pub selection: Selection,
    pub output: Output,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finalize {
    #[serde(flatten)]
    pub selection: Selection,
    pub derivative_id: String,
    pub manifest_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Lookup(Selection),
    Read(Selection),
    Prepare(Prepare),
    Finalize(Finalize),
}
fn require(ok: bool, message: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn identifier(s: &str, max: usize) -> bool {
    s.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && s.len() <= max
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:@-".contains(&b))
}
fn source_id(kind: &SourceKind, s: &str) -> bool {
    if !s.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        || s.len() > 200
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return false;
    }
    match kind {
        SourceKind::PmAttachment => true,
        SourceKind::CatalogueAsset => s.strip_prefix("a_").is_some_and(|tail| {
            tail.len() == 12
                && tail
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
        }),
    }
}
fn package(s: &str) -> bool {
    s.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._+@-".contains(&b))
}
pub fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
impl SourcePin {
    pub fn validate(&self) -> Result<(), String> {
        require(source_id(&self.kind,&self.id) && self.version>0 && self.version<=i64::MAX as u64 && self.generation>0 && self.generation<=i64::MAX as u64 && (1..=MAX_SOURCE_BYTES).contains(&self.size_bytes) && self.content_type=="application/pdf" && sha(&self.measured_sha256),"Semantic source requires one exact PDF identity/version/generation and native measured SHA")
    }
}
impl Extractor {
    pub fn validate(&self) -> Result<(), String> {
        require(package(&self.name) && package(&self.version) && sha(&self.options_sha256),"Pin the actual extractor package/version and options SHA; unknown labels cannot be guessed")
    }
}
impl Selection {
    pub fn validate(&self, project: &str) -> Result<(), String> {
        require(
            identifier(project, 256),
            "Semantic selection needs the exact project",
        )?;
        self.source.validate()?;
        self.extractor.validate()
    }
    pub fn key_sha256(&self, project: &str) -> Result<String, String> {
        self.validate(project)?;
        let bytes = serde_json::to_vec(&json!([
            SCHEMA,
            project,
            self.source.kind,
            self.source.id,
            self.source.version,
            self.source.generation,
            self.source.size_bytes,
            self.source.content_type,
            self.source.measured_sha256,
            self.extractor.name,
            self.extractor.version,
            self.extractor.options_sha256
        ]))
        .map_err(|e| e.to_string())?;
        Ok(digest(&bytes))
    }
    pub fn derivative_id(&self, project: &str) -> Result<String, String> {
        Ok(format!("sem_{}", self.key_sha256(project)?))
    }
}
impl Output {
    pub fn validate(&self) -> Result<(), String> {
        require(
            sha(&self.sha256) && (1..=MAX_OUTPUT_BYTES).contains(&self.bytes),
            "Semantic output needs bounded nonempty Markdown bytes and their exact SHA",
        )
    }
}
impl Prepare {
    pub fn validate(&self, project: &str) -> Result<(), String> {
        self.selection.validate(project)?;
        self.output.validate()
    }
    pub fn manifest_sha256(&self, project: &str) -> Result<String, String> {
        self.validate(project)?;
        Ok(digest(
            &serde_json::to_vec(&json!([
                self.selection.key_sha256(project)?,
                self.output.sha256,
                self.output.bytes
            ]))
            .map_err(|e| e.to_string())?,
        ))
    }
}
impl Command {
    pub fn action(&self) -> &'static str {
        match self {
            Self::Lookup(_) => "semantic_lookup",
            Self::Read(_) => "semantic_read",
            Self::Prepare(_) => "semantic_prepare",
            Self::Finalize(_) => "finalize_asset_semantic",
        }
    }
    pub fn payload(&self, project: &str) -> Result<Value, String> {
        let semantic = match self {
            Self::Lookup(s) | Self::Read(s) => {
                s.validate(project)?;
                serde_json::to_value(s)
            }
            Self::Prepare(s) => {
                s.validate(project)?;
                serde_json::to_value(s)
            }
            Self::Finalize(s) => {
                s.selection.validate(project)?;
                require(
                    s.derivative_id == s.selection.derivative_id(project)?
                        && sha(&s.manifest_sha256),
                    "Semantic finalize needs exact source/derivative/manifest pins",
                )?;
                serde_json::to_value(s)
            }
        }
        .map_err(|e| e.to_string())?;
        Ok(if matches!(self, Self::Finalize(_)) {
            json!({"action":self.action(),"eds_project_id":project,"semantic":semantic})
        } else {
            json!({"action":self.action(),"ds_project":project,"semantic":semantic})
        })
    }
}

fn verify_receipt(project: &str, selection: &Selection, receipt: &Value) -> Result<Output, String> {
    require(
        receipt["schema"] == SCHEMA
            && receipt["project_id"] == project
            && receipt["derivative_id"] == selection.derivative_id(project)?,
        "Semantic receipt belongs to another project/source key",
    )?;
    let source: SourcePin =
        serde_json::from_value(receipt["source"].clone()).map_err(|e| e.to_string())?;
    let extractor: Extractor =
        serde_json::from_value(receipt["extractor"].clone()).map_err(|e| e.to_string())?;
    require(
        source == selection.source && extractor == selection.extractor,
        "Semantic receipt source/extractor pins differ",
    )?;
    let output = Output {
        sha256: receipt["output"]["sha256"]
            .as_str()
            .unwrap_or_default()
            .into(),
        bytes: receipt["output"]["bytes"].as_u64().unwrap_or_default(),
    };
    output.validate()?;
    require(
        receipt["output"]["content_type"] == MARKDOWN_TYPE
            && receipt["manifest_sha256"]
                == Prepare {
                    selection: selection.clone(),
                    output: output.clone(),
                }
                .manifest_sha256(project)?,
        "Semantic output differs from its exact manifest",
    )?;
    let basis = receipt["source_sha256_basis"].as_str().unwrap_or_default();
    require(
        matches!(basis, "authoritative" | "native_measured")
            && (basis != "authoritative" || receipt["advertised_sha256"] == source.measured_sha256),
        "Native measurement cannot be relabelled as server source attestation",
    )?;
    if let Some(advertised) = receipt.get("advertised_sha256").filter(|v| !v.is_null()) {
        require(
            advertised.as_str() == Some(source.measured_sha256.as_str()),
            "Advertised source SHA differs from measured bytes",
        )?;
    }
    if receipt["state"] == "ready" {
        require(
            receipt["output_generation"]
                .as_u64()
                .is_some_and(|n| n > 0 && n <= i64::MAX as u64),
            "Ready semantic receipt lacks its verified output generation",
        )?;
    }
    Ok(output)
}
impl Command {
    pub fn verify_response(&self, project: &str, value: &Value) -> Result<(), String> {
        match self {
            Self::Lookup(selection) => {
                require(
                    value["schema"] == SCHEMA
                        && value["project_id"] == project
                        && value["found"].is_boolean(),
                    "Semantic lookup lacks truthful exact-project presence",
                )?;
                if value["found"] == true {
                    verify_receipt(project, selection, &value["derivative"])?;
                    require(
                        matches!(
                            value["derivative"]["state"].as_str(),
                            Some("ready" | "uploading")
                        ),
                        "Semantic lookup needs the actual derivative lifecycle",
                    )?;
                } else {
                    require(
                        value.get("derivative").is_none_or(Value::is_null),
                        "Missing semantic lookup cannot carry a usable derivative",
                    )?;
                }
            }
            Self::Read(selection) => {
                verify_receipt(project, selection, value)?;
                require(
                    value["state"] == "ready"
                        && value["download_url"]
                            .as_str()
                            .is_some_and(|s| !s.is_empty())
                        && value["expires_at"].as_str().is_some_and(|s| !s.is_empty()),
                    "Semantic read requires the ready derivative and its fresh bounded grant",
                )?;
            }
            Self::Prepare(prepare) => {
                let output = verify_receipt(project, &prepare.selection, value)?;
                require(
                    output == prepare.output
                        && matches!(value["state"].as_str(), Some("ready" | "uploading")),
                    "Semantic prepare did not admit the selected output",
                )?;
                if value["state"] == "uploading" {
                    require(
                        value["upload_url"].as_str().is_some_and(|s| !s.is_empty())
                            && value["expires_at"].as_str().is_some_and(|s| !s.is_empty()),
                        "Semantic prepare lacks its bounded upload grant",
                    )?;
                }
            }
            Self::Finalize(finalize) => {
                verify_receipt(project, &finalize.selection, value)?;
                require(
                    value["manifest_sha256"] == finalize.manifest_sha256
                        && value["state"] == "ready"
                        && value["replayed"].is_boolean(),
                    "Semantic finalize is not the exact committed/replayed manifest",
                )?;
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn selection() -> Selection {
        Selection {
            source: SourcePin {
                kind: SourceKind::PmAttachment,
                id: "pma_sample".into(),
                version: 3,
                generation: 29,
                size_bytes: 7,
                content_type: "application/pdf".into(),
                measured_sha256: "a".repeat(64),
            },
            extractor: Extractor {
                name: "markitdown".into(),
                version: "0.1.4".into(),
                options_sha256: digest(b"{}"),
            },
        }
    }
    #[test]
    fn exact_key_changes_with_every_source_and_extractor_pin() {
        let s = selection();
        let key = s.key_sha256("project").unwrap();
        assert_eq!(
            key,
            "f6368d8e8f784fadb196bc202a23827b1a94fa8803536b88e9b43a868a9f7d5d"
        );
        for changed in ["project-other", "Project"] {
            assert_ne!(key, s.key_sha256(changed).unwrap());
        }
        let mut other = s.clone();
        other.source.generation += 1;
        assert_ne!(key, other.key_sha256("project").unwrap());
        other = s.clone();
        other.extractor.version = "0.1.5".into();
        assert_ne!(key, other.key_sha256("project").unwrap());
        let p = Prepare {
            selection: s.clone(),
            output: Output {
                sha256: "b".repeat(64),
                bytes: 12,
            },
        };
        assert_eq!(
            p.manifest_sha256("project").unwrap(),
            "e43258377cd99c6016fad77766b7f2c7816d58bac7280d846e6eb613d27709de"
        );
        let mut changed = p.clone();
        changed.output.sha256 = "c".repeat(64);
        assert_ne!(
            p.manifest_sha256("project").unwrap(),
            changed.manifest_sha256("project").unwrap()
        );
        assert_eq!(
            p.selection.derivative_id("project").unwrap(),
            changed.selection.derivative_id("project").unwrap()
        );
    }
    #[test]
    fn closed_metadata_cannot_take_urls_bytes_authority_or_attestation() {
        let mut raw = serde_json::to_value(selection()).unwrap();
        raw["url"] = json!("https://example.test/source");
        assert!(serde_json::from_value::<Selection>(raw).is_err());
        for label in ["", "markitdown ", "wrong<script>", "known\nlabel"] {
            let mut s = selection();
            s.extractor.name = label.into();
            assert!(s.validate("project").is_err());
        }
        let mut s = selection();
        s.source.version = 0;
        assert!(s.validate("project").is_err());
        s = selection();
        s.source.content_type = "application/octet-stream".into();
        assert!(s.validate("project").is_err());
        let raw = json!({"sha256":"b".repeat(64),"bytes":12,"markdown":"claimed content"});
        assert!(serde_json::from_value::<Output>(raw).is_err());
    }
    #[test]
    fn metadata_reads_and_bulk_commit_have_distinct_fixed_envelopes() {
        let s = selection();
        let read = Command::Read(s.clone()).payload("project").unwrap();
        assert_eq!(read["action"], "semantic_read");
        assert_eq!(read["ds_project"], "project");
        assert!(read.get("eds_project_id").is_none());
        let prepare = Prepare {
            selection: s,
            output: Output {
                sha256: "b".repeat(64),
                bytes: 12,
            },
        };
        let encoded = serde_json::to_value(&prepare).unwrap();
        let decoded: Prepare = serde_json::from_value(encoded.clone()).unwrap();
        assert_eq!(decoded, prepare);
        let mut extra = encoded;
        extra["url"] = json!("https://example.test/unadmitted");
        assert!(serde_json::from_value::<Prepare>(extra).is_err());
        let commit = Command::Finalize(Finalize {
            selection: prepare.selection.clone(),
            derivative_id: prepare.selection.derivative_id("project").unwrap(),
            manifest_sha256: prepare.manifest_sha256("project").unwrap(),
        })
        .payload("project")
        .unwrap();
        assert_eq!(commit["action"], "finalize_asset_semantic");
        assert!(commit.get("ds_project").is_none());
        assert_eq!(commit["eds_project_id"], "project");
        assert_eq!(commit["semantic"]["source"]["id"], "pma_sample");
    }
    #[test]
    fn missing_ready_and_source_attestation_receipts_are_not_interchangeable() {
        let s = selection();
        let output = Output {
            sha256: "b".repeat(64),
            bytes: 12,
        };
        let p = Prepare {
            selection: s.clone(),
            output: output.clone(),
        };
        let ready = json!({"schema":SCHEMA,"project_id":"project","derivative_id":s.derivative_id("project").unwrap(),"manifest_sha256":p.manifest_sha256("project").unwrap(),"source":s.source,"extractor":s.extractor,"output":{"sha256":output.sha256,"bytes":output.bytes,"content_type":MARKDOWN_TYPE},"state":"ready","output_generation":7,"source_sha256_basis":"native_measured"});
        let command = Command::Lookup(s.clone());
        command
            .verify_response(
                "project",
                &json!({"schema":SCHEMA,"project_id":"project","found":true,"derivative":ready}),
            )
            .unwrap();
        command
            .verify_response(
                "project",
                &json!({"schema":SCHEMA,"project_id":"project","found":false}),
            )
            .unwrap();
        let mut partial = ready.clone();
        partial["state"] = json!("uploading");
        partial.as_object_mut().unwrap().remove("output_generation");
        command
            .verify_response(
                "project",
                &json!({"schema":SCHEMA,"project_id":"project","found":true,"derivative":partial}),
            )
            .unwrap();
        assert!(Command::Read(s.clone())
            .verify_response("project", &partial)
            .is_err());
        for (field, val) in [
            ("state", json!("unexpected")),
            ("project_id", json!("foreign")),
            ("source_sha256_basis", json!("authoritative")),
            ("manifest_sha256", json!("c".repeat(64))),
        ] {
            let mut changed = ready.clone();
            changed[field] = val;
            assert!(command.verify_response("project",&json!({"schema":SCHEMA,"project_id":"project","found":true,"derivative":changed})).is_err());
        }
    }
}
