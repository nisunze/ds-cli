//! Published compute artifacts as ds-brain serves them (`POST
//! /api/v1/compute_artifacts`): a project's current heads, one head, and one
//! output's short-lived download. Heads and outputs are admitted by
//! [`crate::compute_artifact_inventory`]; a download URL is HTTPS and pinned
//! to its object generation, and must answer the head and output it was asked
//! for. The unsigned `compute-artifact://v1/…` pointer a status answer
//! carries names one output ([`Locator::parse`]).

use ds_gateway_client::Operation;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};
use crate::compute_artifact_inventory::{self as inventory, Declaration, Head};

/// A page holds at most 100 heads, each with at most 100 declarations.
const PAGE_CAP: u64 = 16 * 1024 * 1024;
const HEAD_CAP: u64 = 1024 * 1024;
pub const LIMIT_MAX: u32 = 100;
const FILE_NAME_MAX: usize = 255;
pub const ENGINES: [&str; 5] = [
    "solar",
    "print",
    "vector_tiler",
    "network_reporter",
    crate::project_dataset_revisions::ENGINE,
];
pub const INVALID: &str = "compute_artifact_answer_invalid";
pub const INPUT_INVALID: &str = "compute_artifact_input_invalid";
/// The answer names another head or output than the one asked for, or bytes
/// that differ from the declaration a caller holds.
pub const MISMATCH: &str = "compute_artifact_mismatch";

fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn input(what: &str) -> Refusal {
    Refusal::new(INPUT_INVALID, format!("A compute artifact request names one valid {what}."))
}

fn invalid(what: &str) -> Refusal {
    Refusal::new(INVALID, format!("The compute artifact service answered an invalid {what}."))
}

/// One published head: a project's engine, operation and variant.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct HeadIdentity {
    pub project_id: String,
    pub engine: String,
    pub operation: String,
    pub variant: String,
}

impl HeadIdentity {
    fn checked(&self) -> Result<Map<String, Value>, Refusal> {
        project_reference(&self.project_id)?;
        if !ENGINES.contains(&self.engine.as_str()) {
            return Err(input("engine"));
        }
        if !inventory::valid_operation(&self.engine, &self.operation) {
            return Err(input("operation"));
        }
        if !id(&self.variant) {
            return Err(input("variant"));
        }
        let mut body = Map::new();
        body.insert("project_id".into(), self.project_id.clone().into());
        body.insert("engine".into(), self.engine.clone().into());
        body.insert("operation".into(), self.operation.clone().into());
        body.insert("variant".into(), self.variant.clone().into());
        Ok(body)
    }

    fn names(&self, head: &Head) -> bool {
        head.engine == self.engine && head.operation == self.operation && head.variant == self.variant
    }
}

/// One output of one published head.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct OutputRef {
    #[serde(flatten)]
    pub head: HeadIdentity,
    pub output_id: String,
}

/// The stable pointer `compute-artifact://v1/{project}/{engine}/{operation}/
/// {variant}/{output}/{file name}`, each segment percent-encoded.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Locator {
    #[serde(flatten)]
    pub output: OutputRef,
    pub file_name: String,
}

pub const LOCATOR_SCHEME: &str = "compute-artifact";

impl Locator {
    /// The pointer's output, or `None` for anything else.
    pub fn parse(value: &str) -> Option<Self> {
        let parsed = url::Url::parse(value.trim()).ok()?;
        if parsed.scheme() != LOCATOR_SCHEME
            || parsed.host_str() != Some("v1")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.port().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return None;
        }
        let fields = parsed
            .path_segments()?
            .filter(|segment| !segment.is_empty())
            .map(|segment| {
                percent_encoding::percent_decode_str(segment)
                    .decode_utf8()
                    .ok()
                    .map(|text| text.into_owned())
            })
            .collect::<Option<Vec<_>>>()?;
        let [project_id, engine, operation, variant, output_id, file_name] =
            <[String; 6]>::try_from(fields).ok()?;
        let locator = Self {
            output: OutputRef {
                head: HeadIdentity {
                    project_id,
                    engine,
                    operation,
                    variant,
                },
                output_id,
            },
            file_name,
        };
        let name = &locator.file_name;
        let valid = locator.output.head.checked().is_ok()
            && id(&locator.output.output_id)
            && !name.is_empty()
            && name.len() <= FILE_NAME_MAX
            && !name.contains(['/', '\\'])
            && !name.chars().any(char::is_control);
        valid.then_some(locator)
    }
}

fn head(value: &Value) -> Result<Head, Refusal> {
    let bytes = serde_json::to_vec(value).map_err(|_| invalid("head"))?;
    inventory::decode_head(&bytes).map_err(|_| invalid("head"))
}

/// One page of a project's current heads.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct HeadsPage {
    pub heads: Vec<Head>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub limit: u32,
}

/// A project's current heads, a page at a time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListHeads {
    pub project_id: String,
    pub limit: Option<u32>,
    pub cursor: Option<String>,
}

impl Exchange for ListHeads {
    type Output = HeadsPage;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body =
            json!({"action": "list", "project_id": project_reference(&self.project_id)?});
        if let Some(limit) = self.limit {
            if !(1..=LIMIT_MAX).contains(&limit) {
                return Err(input("page size"));
            }
            body["limit"] = limit.into();
        }
        if let Some(cursor) = self
            .cursor
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            body["cursor"] = cursor.into();
        }
        Ok(
            Request::generated(Operation::ComputeArtifactsAction, Some(body))?
                .response_cap(PAGE_CAP)
                .into(),
        )
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let heads = answer
            .get("heads")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("heads page"))?
            .iter()
            .map(head)
            .collect::<Result<Vec<_>, _>>()?;
        let has_more = answer
            .get("has_more")
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid("heads page"))?;
        let next_cursor = answer
            .get("next_cursor")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|cursor| !cursor.is_empty())
            .map(str::to_owned);
        if has_more && next_cursor.is_none() {
            return Err(invalid("heads page cursor"));
        }
        let limit = answer
            .get("limit")
            .and_then(Value::as_u64)
            .filter(|limit| (1..=u64::from(LIMIT_MAX)).contains(limit))
            .ok_or_else(|| invalid("heads page size"))? as u32;
        Ok(Admitted::Done(HeadsPage {
            heads,
            next_cursor,
            has_more,
            limit,
        }))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project_id)
    }
}

/// One project's current head. A head never published answers 404.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadHead(pub HeadIdentity);

impl Exchange for ReadHead {
    type Output = Head;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = self.0.checked()?;
        body.insert("action".into(), "read".into());
        Ok(
            Request::generated(Operation::ComputeArtifactsAction, Some(Value::Object(body)))?
                .response_cap(HEAD_CAP)
                .into(),
        )
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let head = head(data(&body))?;
        if !self.0.names(&head) {
            return Err(Refusal::new(MISMATCH, "The service answered another head than the one asked for."));
        }
        Ok(Admitted::Done(head))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.0.project_id)
    }
}

/// Whether a refusal says the head was never published.
pub fn unpublished(refusal: &Refusal) -> bool {
    refusal.status == Some(404)
}

/// One output's short-lived download, resolved from its server-recorded
/// object generation.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Download {
    pub head: Head,
    pub output: Declaration,
    /// HTTPS, generation-pinned; used once, never retained.
    pub download_url: String,
    pub expires_at: String,
    pub file_name: String,
}

/// Resolve one output's download.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveDownload(pub OutputRef);

fn generation_pinned(value: Option<&Value>) -> Result<String, Refusal> {
    let raw = value.and_then(Value::as_str).ok_or_else(|| invalid("download URL"))?;
    let parsed = url::Url::parse(raw).map_err(|_| invalid("download URL"))?;
    let pinned = parsed
        .query_pairs()
        .find(|(name, _)| name == "generation")
        .is_some_and(|(_, value)| {
            !value.is_empty()
                && !value.starts_with('0')
                && value.bytes().all(|b| b.is_ascii_digit())
        });
    if parsed.scheme() != "https" || !pinned {
        return Err(invalid("download URL"));
    }
    Ok(parsed.into())
}

impl Exchange for ResolveDownload {
    type Output = Download;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = self.0.head.checked()?;
        if !id(&self.0.output_id) {
            return Err(input("output"));
        }
        body.insert("action".into(), "download".into());
        body.insert("output_id".into(), self.0.output_id.clone().into());
        Ok(
            Request::generated(Operation::ComputeArtifactsAction, Some(Value::Object(body)))?
                .response_cap(HEAD_CAP)
                .into(),
        )
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let answer = data(&body);
        let head = head(answer.get("head").unwrap_or(&Value::Null))?;
        let file_name = answer
            .get("file_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| invalid("file name"))?
            .to_owned();
        let projection = json!({
            "engine": head.engine, "operation": head.operation, "outputs": head.outputs,
            "download": {"output": answer.get("output"), "file_name": file_name},
        });
        let output = inventory::evaluate(projection.to_string().as_bytes())
            .ok()
            .and_then(|inventory| serde_json::from_str::<Value>(&inventory).ok())
            .and_then(|inventory| {
                Declaration::deserialize(inventory.get("download_output")?).ok()
            })
            .ok_or_else(|| invalid("download output"))?;
        if !self.0.head.names(&head) || output.output_id != self.0.output_id {
            return Err(Refusal::new(MISMATCH, "The service answered another output than the one asked for."));
        }
        let expires_at = answer
            .get("expires_at")
            .and_then(Value::as_str)
            .filter(|stamp| {
                time::OffsetDateTime::parse(stamp, &time::format_description::well_known::Rfc3339)
                    .is_ok()
            })
            .ok_or_else(|| invalid("expiry"))?
            .to_owned();
        Ok(Admitted::Done(Download {
            download_url: generation_pinned(answer.get("download_url"))?,
            head,
            output,
            expires_at,
            file_name,
        }))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.0.head.project_id)
    }
}

/// What a caller already holds about an output's bytes; a download that
/// differs is refused before any byte is read.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct Expected {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub content_type: Option<String>,
}

/// Whether a resolved download may be read under `max_bytes` for a caller
/// that holds `expected`.
pub fn admit_read(download: &Download, expected: &Expected, max_bytes: u64) -> Result<(), Refusal> {
    let output = &download.output;
    let differs = expected.file_name.as_ref().is_some_and(|name| *name != download.file_name)
        || expected.sha256.as_ref().is_some_and(|sha| *sha != output.sha256)
        || expected.size_bytes.is_some_and(|size| size != output.size_bytes)
        || expected
            .content_type
            .as_ref()
            .is_some_and(|kind| *kind != output.content_type);
    if differs {
        return Err(Refusal::new(
            MISMATCH,
            "The published artifact changed. Refresh to read its current version.",
        ));
    }
    if output.size_bytes > max_bytes {
        return Err(Refusal::new(
            super::RESPONSE_TOO_LARGE,
            "The artifact exceeds the requested byte bound.",
        ));
    }
    Ok(())
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{
        admit_read, unpublished, Download, Expected, HeadIdentity, ListHeads, Locator, OutputRef,
        ReadHead, ResolveDownload,
    };
    use crate::gateway_exchange::browser::{attempt, rejection, run, typed, GatewayHost};
    use crate::gateway_exchange::storage::fetch_verified;
    use js_sys::{Reflect, Uint8Array};
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(typescript_custom_section)]
    const TYPES: &'static str = r#"
/** One published output's declaration, admitted by the Kernel. */
export interface ComputeArtifactOutput {
    output_id: string;
    filename?: string;
    format: string;
    content_type: string;
    sha256: string;
    size_bytes: number;
    paper_size?: string;
    presentation?: Record<string, unknown>;
}
/** One project's current published head. */
export interface ComputeArtifactHead {
    work_id: string;
    engine: string;
    engine_version?: string;
    engine_build_manifest_sha256?: string;
    operation: string;
    variant: string;
    input_base_fingerprint: string;
    head_revision: number;
    updated_at: string;
    outputs: ComputeArtifactOutput[];
    output_origins?: Record<string, { work_id: string; input_base_fingerprint: string; engine_version?: string; engine_build_manifest_sha256?: string }>;
    printing_preview?: Record<string, unknown>;
}
export interface ComputeArtifactHeadsPage { heads: ComputeArtifactHead[]; next_cursor?: string; has_more: boolean; limit: number }
/** One output's short-lived download; use the URL now and forget it. */
export interface ComputeArtifactDownload { head: ComputeArtifactHead; output: ComputeArtifactOutput; download_url: string; expires_at: string; file_name: string }
/** One output's verified bytes with the head and declaration they match. */
export interface ComputeArtifactBytes { head: ComputeArtifactHead; output: ComputeArtifactOutput; file_name: string; bytes: Uint8Array }
"#;

    fn input<T: serde::de::DeserializeOwned>(value: JsValue) -> Result<T, JsValue> {
        tsify::serde_wasm_bindgen::from_value(value)
            .map_err(|error| js_sys::Error::new(&error.to_string()).into())
    }

    /// A project's current published heads, one page at a time.
    #[wasm_bindgen(js_name = computeArtifactHeads, unchecked_return_type = "ComputeArtifactHeadsPage")]
    pub async fn compute_artifact_heads(
        host: GatewayHost,
        project_id: String,
        limit: Option<u32>,
        cursor: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let call = ListHeads {
            project_id,
            limit,
            cursor,
        };
        typed(&run(&host, call).await?)
    }

    /// One current head, or `undefined` when it was never published.
    #[wasm_bindgen(js_name = computeArtifactPublishedHead, unchecked_return_type = "ComputeArtifactHead | undefined")]
    pub async fn compute_artifact_head(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "HeadIdentity")] identity: JsValue,
    ) -> Result<JsValue, JsValue> {
        match attempt(&host, ReadHead(input::<HeadIdentity>(identity)?)).await? {
            Ok(head) => typed(&head),
            Err(refusal) if unpublished(&refusal) => Ok(JsValue::UNDEFINED),
            Err(refusal) => Err(rejection(&refusal)),
        }
    }

    /// One output's short-lived download.
    #[wasm_bindgen(js_name = computeArtifactDownload, unchecked_return_type = "ComputeArtifactDownload")]
    pub async fn compute_artifact_download(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "OutputRef")] output: JsValue,
    ) -> Result<JsValue, JsValue> {
        typed(&run(&host, ResolveDownload(input::<OutputRef>(output)?)).await?)
    }

    /// One output's bytes, resolved, read and verified against its published
    /// size and digest — and against what the caller already holds.
    #[wasm_bindgen(js_name = computeArtifactBytes, unchecked_return_type = "ComputeArtifactBytes")]
    pub async fn compute_artifact_bytes(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "OutputRef")] output: JsValue,
        max_bytes: f64,
        #[wasm_bindgen(unchecked_param_type = "Expected | undefined")] expected: JsValue,
    ) -> Result<JsValue, JsValue> {
        let expected: Expected = if expected.is_undefined() || expected.is_null() {
            Expected::default()
        } else {
            input(expected)?
        };
        let download: Download = run(&host, ResolveDownload(input::<OutputRef>(output)?)).await?;
        let cap = if max_bytes.is_finite() && max_bytes >= 0.0 {
            max_bytes as u64
        } else {
            u64::MAX
        };
        admit_read(&download, &expected, cap).map_err(|refusal| rejection(&refusal))?;
        let bytes = fetch_verified(
            &download.download_url,
            download.output.size_bytes,
            &download.output.sha256,
        )
        .await
        .map_err(|refusal| rejection(&refusal))?;
        let answer = typed(&serde_json::json!({
            "head": download.head, "output": download.output, "file_name": download.file_name,
        }))?;
        Reflect::set(&answer, &JsValue::from_str("bytes"), &Uint8Array::from(&bytes[..]))?;
        Ok(answer)
    }

    /// The output a `compute-artifact://v1/…` pointer names, or `undefined`
    /// when the reference is not one.
    #[wasm_bindgen(js_name = computeArtifactLocator, unchecked_return_type = "Locator | undefined")]
    pub fn compute_artifact_locator(reference: &str) -> Result<JsValue, JsValue> {
        match Locator::parse(reference) {
            Some(locator) => typed(&locator),
            None => Ok(JsValue::UNDEFINED),
        }
    }
}

#[cfg(test)]
#[path = "compute_artifacts_tests.rs"]
mod tests;
