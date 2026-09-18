//! Closed governed geographic data-distribution operations.
//!
//! Two of ds-brain's `/api/v1/data-distribution` actions are declared for the
//! native client: the project's reference catalogue (`list_datasets`), which a
//! headless seeder plans national-bundle installs from, and one bounded derived
//! print-context acquisition (`query_print_context`), which ds-brain relays to
//! the vector tiler for Google Open Buildings footprints or DEM contours. The
//! billed BigQuery page query (`query_dataset`) and the publication actions are
//! deliberately outside the vocabulary: a catalogue row without a published
//! bundle is a refusal upstream, never a silent fallback to a per-page query.
//!
//! ds-brain rechecks project membership (`requireProjectAccess`) on every call.
//! Project identity is injected from the selected-client context and cannot be
//! supplied inside the request.
//!
//! The bundle a catalogue row names is fetched separately, through
//! [`download_bundle`]: an unauthenticated `GET` of one public object on
//! `storage.googleapis.com`, admitted only when the streamed bytes carry the
//! row's exact size and SHA-256. No DS credential ever travels with it.
use std::io::Write;

use crate::{
    BundleDownloadCall, ClientError, ErrorKind, ServiceRefusal, Transport, TransportError,
    TransportResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const DATA_DISTRIBUTION_RESPONSE_LIMIT: usize = 32 * 1024 * 1024;
/// The gateway's own deadline for the route (`ds-apis-tf/api_data_distribution.tf`).
pub const DATA_DISTRIBUTION_TIMEOUT_SECONDS: u64 = 180;
/// ds-brain caps the request body at 2 MiB; a print-context area is a fraction
/// of that, and anything larger is a coverage plan gone wrong, not a request.
pub const DATA_DISTRIBUTION_MAX_AREA_BYTES: usize = 1024 * 1024;
/// The most catalogue rows one bounded read admits.
pub const DATA_DISTRIBUTION_MAX_CATALOG_ROWS: usize = 1000;
/// The one host a published reference bundle may live on. ds-brain maps every
/// `gs://bucket/path` bundle to `https://storage.googleapis.com/bucket/path`
/// (`reference_resources.go`, `expectedTileWorkerURL`), unsigned.
pub const BUNDLE_DOWNLOAD_HOST: &str = "storage.googleapis.com";
pub const BUNDLE_DOWNLOAD_MAX_URL_BYTES: usize = 2048;
/// A bundle larger than this is not a national catalogue layer; it is a defect.
pub const BUNDLE_DOWNLOAD_MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// The closed derived print-context kinds ds-brain relays to the vector tiler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintContextKind {
    GoogleOpenBuildings,
    ElevationContours,
}

impl PrintContextKind {
    pub const fn token(self) -> &'static str {
        match self {
            Self::GoogleOpenBuildings => "google_open_buildings",
            Self::ElevationContours => "elevation_contours",
        }
    }
}

/// Authored contour intervals, forwarded intact. The bounds are ds-brain's own
/// (`TileOrchestrator::QueryPrintContext`) and the kernel's
/// (`printing::validate` for `ElevationContours`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContourParameters {
    pub minor_interval_m: u16,
    pub index_interval_m: u16,
    pub sample_spacing_m: f64,
}

impl ContourParameters {
    pub fn validate(&self) -> Result<(), ClientError> {
        let bounded = self.minor_interval_m != 0
            && self.index_interval_m >= self.minor_interval_m
            && self.index_interval_m.is_multiple_of(self.minor_interval_m)
            && self.index_interval_m <= 1_000
            && self.sample_spacing_m.is_finite()
            && (5.0..=250.0).contains(&self.sample_spacing_m);
        if bounded {
            Ok(())
        } else {
            Err(ClientError::new(
                ErrorKind::InvalidInput,
                "Invalid contour parameters: the index interval must be a multiple of the minor \
                 interval up to 1000 m, and sampling spacing 5-250 m",
            ))
        }
    }
}

/// The closed request vocabulary. Order of the variants is the order of
/// [`crate::DATA_DISTRIBUTION_ACTIONS`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DataDistributionRequest {
    /// The catalogue rows the selected project may read.
    ListDatasets {},
    /// One bounded derived acquisition over a GeoJSON Polygon or MultiPolygon.
    /// Contour parameters are required for contours and refused otherwise.
    QueryPrintContext {
        context_kind: PrintContextKind,
        area: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        contour_parameters: Option<ContourParameters>,
    },
}

impl DataDistributionRequest {
    pub const fn action(&self) -> &'static str {
        match self {
            Self::ListDatasets {} => "list_datasets",
            Self::QueryPrintContext { .. } => "query_print_context",
        }
    }

    pub fn validate(&self) -> Result<(), ClientError> {
        let invalid = |message: &'static str| ClientError::new(ErrorKind::InvalidInput, message);
        match self {
            Self::ListDatasets {} => Ok(()),
            Self::QueryPrintContext {
                context_kind,
                area,
                contour_parameters,
            } => {
                ds_command_kernel::reference_cache::validate_area(area).map_err(|_| {
                    invalid(
                        "Invalid print context area: expected a GeoJSON Polygon or MultiPolygon",
                    )
                })?;
                if serde_json::to_vec(area)
                    .map_or(true, |bytes| bytes.len() > DATA_DISTRIBUTION_MAX_AREA_BYTES)
                {
                    return Err(invalid("Print context area exceeds its request bound"));
                }
                match (context_kind, contour_parameters) {
                    (PrintContextKind::GoogleOpenBuildings, None) => Ok(()),
                    (PrintContextKind::GoogleOpenBuildings, Some(_)) => Err(invalid(
                        "Contour parameters apply only to elevation contours",
                    )),
                    (PrintContextKind::ElevationContours, None) => Err(invalid(
                        "Elevation contours require explicit contour parameters",
                    )),
                    (PrintContextKind::ElevationContours, Some(parameters)) => {
                        parameters.validate()
                    }
                }
            }
        }
    }

    /// The exact request the desktop sends (`reference-data/resources.ts`,
    /// `project-data/buildings.ts`, `reports/transformer-print-context.ts`),
    /// with the project injected by the host boundary. ds-brain decodes with
    /// `DisallowUnknownFields`, so the key set is closed here too.
    pub(crate) fn body(&self, project: &str) -> Vec<u8> {
        #[derive(Serialize)]
        struct Body<'a> {
            action: &'static str,
            project_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            context_kind: Option<PrintContextKind>,
            #[serde(skip_serializing_if = "Option::is_none")]
            area: Option<&'a Value>,
            #[serde(skip_serializing_if = "Option::is_none")]
            contour_parameters: Option<&'a ContourParameters>,
        }
        let body = match self {
            Self::ListDatasets {} => Body {
                action: self.action(),
                project_id: project,
                context_kind: None,
                area: None,
                contour_parameters: None,
            },
            Self::QueryPrintContext {
                context_kind,
                area,
                contour_parameters,
            } => Body {
                action: self.action(),
                project_id: project,
                context_kind: Some(*context_kind),
                area: Some(area),
                contour_parameters: contour_parameters.as_ref(),
            },
        };
        serde_json::to_vec(&body).expect("closed validated data distribution request")
    }
}

/// Decode one answer minimally: the standard envelope's success flag and the
/// `data` container, shaped by the action. Row contents are the seeding
/// library's to validate (catalogue rules, provider receipts); this boundary
/// only refuses what cannot be a ds-brain answer at all.
pub(crate) fn decode(
    response: TransportResponse,
    request: &DataDistributionRequest,
) -> Result<Value, ClientError> {
    if response.body.len() > DATA_DISTRIBUTION_RESPONSE_LIMIT {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "Data distribution response exceeds limit",
        ));
    }
    // A refused response is classified from its STATUS first, as printing
    // does: a refusal in a shape this client cannot parse is still a refusal
    // with a known class, not an unreadable success.
    let parsed: Option<Value> = serde_json::from_slice(&response.body).ok();
    let value = match (response.status, parsed) {
        (200, Some(value)) if value["success"] == true => value,
        (200, Some(_)) => return Err(refusal(200, &Value::Null)),
        (200, None) => {
            return Err(ClientError::new(
                ErrorKind::UnreadableResponse,
                "Invalid data distribution response",
            ))
        }
        (status, parsed) => return Err(refusal(status, &parsed.unwrap_or(Value::Null))),
    };
    let data = &value["data"];
    let bad = || {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "Invalid data distribution receipt",
        )
    };
    match request {
        DataDistributionRequest::ListDatasets {} => {
            // Go marshals an empty catalogue slice as `null`.
            if data.is_null() {
                return Ok(json!([]));
            }
            let rows = data.as_array().ok_or_else(bad)?;
            if rows.len() > DATA_DISTRIBUTION_MAX_CATALOG_ROWS
                || rows
                    .iter()
                    .any(|row| !row.is_object() || !row["id"].is_string())
            {
                return Err(bad());
            }
        }
        DataDistributionRequest::QueryPrintContext { .. } => {
            if !data.is_object() {
                return Err(bad());
            }
        }
    }
    Ok(data.clone())
}

/// Classify one refused data-distribution response.
///
/// ds-brain answers every service failure on this route as `409` with its
/// standard envelope (`data_distribution.go`), so the sentence is the only
/// actionable half of a refusal and crosses as a bounded [`ServiceRefusal`],
/// exactly as printing carries a layout validator's words.
fn refusal(status: u16, value: &Value) -> ClientError {
    let envelope = ServiceRefusal::new(status, envelope_code(value), envelope_message(value));
    let (kind, message) = match status {
        400 | 422 => (
            ErrorKind::InvalidInput,
            "Data distribution request refused: the request is not acceptable to this deployment",
        ),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "Data distribution request rejected for this identity or selected project",
        ),
        404 => (
            ErrorKind::ResourceNotFound,
            "The data distribution route or resource does not exist",
        ),
        409 => (
            ErrorKind::InvalidInput,
            "Data distribution refused by ds-brain: the catalogue, area, or provider request was not accepted",
        ),
        429 | 500..=599 => (
            ErrorKind::Transient,
            "Data distribution is temporarily unavailable",
        ),
        _ => (
            ErrorKind::UnreadableResponse,
            "Data distribution request refused by ds-brain",
        ),
    };
    if envelope.code().is_none() && envelope.message().is_none() {
        return ClientError::new(kind, message);
    }
    ClientError::with_service_refusal(kind, message, envelope)
}

fn envelope_code(value: &Value) -> Option<&str> {
    value["error"]["code"].as_str()
}

fn envelope_message(value: &Value) -> Option<&str> {
    value["error"]["message"]
        .as_str()
        .or_else(|| value["detail"].as_str())
        .or_else(|| value["error"].as_str())
        .or_else(|| value["message"].as_str())
}

/// What one admitted bundle fetch proved about the bytes it wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleDownloadReceipt {
    pub bytes: u64,
    pub sha256: String,
}

/// Stream one catalogue bundle through the host transport into `sink`, and
/// admit the bytes only when their count and SHA-256 equal the catalogue row's.
///
/// The call was pinned to [`BUNDLE_DOWNLOAD_HOST`] when it was built; the
/// transport attaches no DS credential; and this function, not the host, is
/// what says whether the bytes are the bundle. A short, long, or altered body
/// is refused after the fact — the sink already holds it, so a host that keeps
/// the destination must discard it on an error.
pub fn download_bundle<T: Transport>(
    transport: &mut T,
    call: BundleDownloadCall<'_>,
    sink: &mut dyn Write,
) -> Result<BundleDownloadReceipt, ClientError> {
    let expected_bytes = call.expected_bytes();
    let expected_sha256 = call.expected_sha256().to_owned();
    let mut digesting = DigestingSink {
        inner: sink,
        hasher: Sha256::new(),
        written: 0,
        limit: expected_bytes,
        overflowed: false,
    };
    let outcome = transport.download_bundle(call, &mut digesting);
    let mismatch = || {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "the reference bundle bytes do not match the catalogue's size and SHA-256",
        )
    };
    // An object longer than the catalogue says is a mismatch whether the
    // adapter reports the refused write as its own failure or not.
    if digesting.overflowed {
        return Err(mismatch());
    }
    outcome.map_err(|error| match error {
        TransportError::TimedOut => ClientError::new(
            ErrorKind::Transient,
            "the reference bundle host did not answer within its deadline",
        ),
        TransportError::Unreachable => ClientError::new(
            ErrorKind::Transient,
            "the reference bundle host is unreachable or refused the object",
        ),
    })?;
    let sha256 = format!("{:x}", digesting.hasher.finalize());
    if digesting.written != expected_bytes || sha256 != expected_sha256 {
        return Err(mismatch());
    }
    Ok(BundleDownloadReceipt {
        bytes: digesting.written,
        sha256,
    })
}

/// Counts and hashes what the transport writes, and stops passing bytes to
/// the destination once the catalogue's size is exceeded so an oversized body
/// cannot fill a disk before it is refused.
struct DigestingSink<'a> {
    inner: &'a mut dyn Write,
    hasher: Sha256,
    written: u64,
    limit: u64,
    overflowed: bool,
}

impl Write for DigestingSink<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let remaining = self.limit.saturating_sub(self.written);
        if (buffer.len() as u64) > remaining {
            self.overflowed = true;
            return Err(std::io::Error::other(
                "reference bundle exceeds its catalogue size",
            ));
        }
        let accepted = self.inner.write(buffer)?;
        self.hasher.update(&buffer[..accepted]);
        self.written += accepted as u64;
        Ok(accepted)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ClientProfile, ClientProfileInput, DeploymentLane, ProjectListCall, RefreshCall,
        SignInCall, SurveyControlCall, TransformerContextCall, CLIENT_PROFILE_SCHEMA,
    };

    fn area() -> Value {
        json!({"type":"Polygon","coordinates":[[[30.0,-2.0],[30.01,-2.0],[30.01,-1.99],[30.0,-1.99],[30.0,-2.0]]]})
    }

    fn contours() -> ContourParameters {
        ContourParameters {
            minor_interval_m: 10,
            index_interval_m: 50,
            sample_spacing_m: 10.0,
        }
    }

    fn refused(status: u16, body: &str) -> ClientError {
        decode(
            TransportResponse::new(status, body.as_bytes().to_vec()),
            &DataDistributionRequest::ListDatasets {},
        )
        .expect_err("a refused response never decodes as a receipt")
    }

    fn brain_envelope(code: &str, message: &str) -> String {
        format!(
            r#"{{"success":false,"service":"data-solutions-backend","version":"1.0.0","error":{{"code":"{code}","message":"{message}","timestamp":1}}}}"#
        )
    }

    /// The exact bytes the page sends, transcribed from the deleted-in-spirit
    /// TypeScript callers and checked against ds-brain's `DisallowUnknownFields`
    /// decoder: `action` and `project_id` always; `context_kind`, `area` and
    /// `contour_parameters` only for an acquisition.
    #[test]
    fn requests_are_the_grammar_the_page_sends() {
        let list = DataDistributionRequest::ListDatasets {};
        assert_eq!(
            list.body("proj-1"),
            br#"{"action":"list_datasets","project_id":"proj-1"}"#
        );
        let buildings = DataDistributionRequest::QueryPrintContext {
            context_kind: PrintContextKind::GoogleOpenBuildings,
            area: json!({"type":"Polygon","coordinates":[[[30.0,-2.0],[30.01,-2.0],[30.01,-1.99],[30.0,-2.0]]]}),
            contour_parameters: None,
        };
        assert_eq!(
            std::str::from_utf8(&buildings.body("proj-1")).unwrap(),
            r#"{"action":"query_print_context","project_id":"proj-1","context_kind":"google_open_buildings","area":{"type":"Polygon","coordinates":[[[30.0,-2.0],[30.01,-2.0],[30.01,-1.99],[30.0,-2.0]]]}}"#
        );
        let contours = DataDistributionRequest::QueryPrintContext {
            context_kind: PrintContextKind::ElevationContours,
            area: json!({"type":"Polygon","coordinates":[[[30.0,-2.0],[30.01,-2.0],[30.01,-1.99],[30.0,-2.0]]]}),
            contour_parameters: Some(contours()),
        };
        assert_eq!(
            std::str::from_utf8(&contours.body("proj-1")).unwrap(),
            r#"{"action":"query_print_context","project_id":"proj-1","context_kind":"elevation_contours","area":{"type":"Polygon","coordinates":[[[30.0,-2.0],[30.01,-2.0],[30.01,-1.99],[30.0,-2.0]]]},"contour_parameters":{"minor_interval_m":10,"index_interval_m":50,"sample_spacing_m":10.0}}"#
        );
        // The vocabulary the profile pins is the vocabulary the requests spell.
        assert_eq!(
            [list.action(), buildings.action()],
            crate::DATA_DISTRIBUTION_ACTIONS
        );
        // A request only deserializes under the same closed grammar.
        let parsed: DataDistributionRequest = serde_json::from_str(
            r#"{"action":"query_print_context","context_kind":"elevation_contours","area":{"type":"Polygon","coordinates":[]},"contour_parameters":{"minor_interval_m":10,"index_interval_m":50,"sample_spacing_m":10}}"#,
        )
        .unwrap();
        assert_eq!(parsed.action(), "query_print_context");
        assert!(serde_json::from_str::<DataDistributionRequest>(
            r#"{"action":"query_dataset","resource_id":"x"}"#
        )
        .is_err());
        assert!(serde_json::from_str::<DataDistributionRequest>(
            r#"{"action":"query_print_context","context_kind":"parcels","area":{}}"#
        )
        .is_err());
    }

    #[test]
    fn requests_fail_closed_before_a_credential_is_spent() {
        assert!(DataDistributionRequest::ListDatasets {}.validate().is_ok());
        let ok = DataDistributionRequest::QueryPrintContext {
            context_kind: PrintContextKind::ElevationContours,
            area: area(),
            contour_parameters: Some(contours()),
        };
        assert!(ok.validate().is_ok());
        let buildings = DataDistributionRequest::QueryPrintContext {
            context_kind: PrintContextKind::GoogleOpenBuildings,
            area: area(),
            contour_parameters: None,
        };
        assert!(buildings.validate().is_ok());

        let invalid: Vec<(&str, DataDistributionRequest)> = vec![
            (
                "a point is not an area",
                DataDistributionRequest::QueryPrintContext {
                    context_kind: PrintContextKind::GoogleOpenBuildings,
                    area: json!({"type":"Point","coordinates":[30.0,-2.0]}),
                    contour_parameters: None,
                },
            ),
            (
                "contours without parameters",
                DataDistributionRequest::QueryPrintContext {
                    context_kind: PrintContextKind::ElevationContours,
                    area: area(),
                    contour_parameters: None,
                },
            ),
            (
                "buildings with contour parameters",
                DataDistributionRequest::QueryPrintContext {
                    context_kind: PrintContextKind::GoogleOpenBuildings,
                    area: area(),
                    contour_parameters: Some(contours()),
                },
            ),
            (
                "index interval not a multiple of minor",
                DataDistributionRequest::QueryPrintContext {
                    context_kind: PrintContextKind::ElevationContours,
                    area: area(),
                    contour_parameters: Some(ContourParameters {
                        minor_interval_m: 10,
                        index_interval_m: 55,
                        sample_spacing_m: 10.0,
                    }),
                },
            ),
            (
                "sampling outside 5-250 m",
                DataDistributionRequest::QueryPrintContext {
                    context_kind: PrintContextKind::ElevationContours,
                    area: area(),
                    contour_parameters: Some(ContourParameters {
                        minor_interval_m: 10,
                        index_interval_m: 50,
                        sample_spacing_m: 251.0,
                    }),
                },
            ),
            (
                "index interval above 1000 m",
                DataDistributionRequest::QueryPrintContext {
                    context_kind: PrintContextKind::ElevationContours,
                    area: area(),
                    contour_parameters: Some(ContourParameters {
                        minor_interval_m: 500,
                        index_interval_m: 1500,
                        sample_spacing_m: 10.0,
                    }),
                },
            ),
        ];
        for (why, request) in invalid {
            let error = request.validate().expect_err(why);
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{why}");
        }
    }

    #[test]
    fn a_catalogue_is_an_array_of_rows_and_a_context_is_an_object() {
        let list = DataDistributionRequest::ListDatasets {};
        let rows = decode(
            TransportResponse::new(
                200,
                br#"{"success":true,"data":[{"id":"aa","layer":"roads"}],"message":"Geographic data distribution"}"#.to_vec(),
            ),
            &list,
        )
        .unwrap();
        assert_eq!(rows[0]["layer"], "roads");
        // Go marshals an empty slice as null; that is an empty catalogue.
        assert_eq!(
            decode(
                TransportResponse::new(200, br#"{"success":true,"data":null}"#.to_vec()),
                &list
            )
            .unwrap(),
            json!([])
        );
        for body in [
            r#"{"success":true,"data":{"layers":{}}}"#,
            r#"{"success":true,"data":[{"layer":"roads"}]}"#,
            r#"{"success":true,"data":["aa"]}"#,
        ] {
            assert_eq!(
                decode(TransportResponse::new(200, body.as_bytes().to_vec()), &list)
                    .unwrap_err()
                    .kind(),
                ErrorKind::UnreadableResponse,
                "{body}"
            );
        }
        let context = DataDistributionRequest::QueryPrintContext {
            context_kind: PrintContextKind::GoogleOpenBuildings,
            area: area(),
            contour_parameters: None,
        };
        let data = decode(
            TransportResponse::new(
                200,
                br#"{"success":true,"data":{"status":"success","layers":{"buildings_context":{"type":"FeatureCollection","features":[]}}}}"#.to_vec(),
            ),
            &context,
        )
        .unwrap();
        assert_eq!(data["status"], "success");
        assert_eq!(
            decode(
                TransportResponse::new(200, br#"{"success":true,"data":[]}"#.to_vec()),
                &context
            )
            .unwrap_err()
            .kind(),
            ErrorKind::UnreadableResponse
        );
    }

    #[test]
    fn every_governed_status_classifies_and_the_sentence_crosses() {
        // ds-brain folds every service failure on this route into 409 with its
        // standard envelope; the words are what a caller can act on.
        let conflict = refused(
            409,
            &brain_envelope("VALIDATION_FAILED", "unknown print context kind"),
        );
        assert_eq!(conflict.kind(), ErrorKind::InvalidInput);
        let carried = conflict.service_refusal().expect("the sentence crossed");
        assert_eq!(carried.status(), 409);
        assert_eq!(
            carried.code(),
            None,
            "an upper-case code is not an identifier"
        );
        assert_eq!(carried.message(), Some("unknown print context kind"));

        assert_eq!(
            refused(400, r#"{"success":false,"error":{"code":"bad_request","message":"project_id is required"}}"#).kind(),
            ErrorKind::InvalidInput
        );
        for status in [401, 403] {
            assert_eq!(
                refused(status, &brain_envelope("forbidden", "no access")).kind(),
                ErrorKind::AuthenticationRejected,
                "{status}"
            );
        }
        assert_eq!(refused(404, "{}").kind(), ErrorKind::ResourceNotFound);
        for status in [429, 500, 502, 503] {
            let transient = refused(status, &brain_envelope("unavailable", "later"));
            assert_eq!(transient.kind(), ErrorKind::Transient, "{status}");
            assert_eq!(
                transient.service_refusal().map(ServiceRefusal::status),
                Some(status)
            );
        }
        assert_eq!(
            refused(418, "<html>teapot</html>").kind(),
            ErrorKind::UnreadableResponse
        );
        assert_eq!(
            refused(200, "not json").kind(),
            ErrorKind::UnreadableResponse
        );
        assert_eq!(
            refused(200, r#"{"success":false}"#).kind(),
            ErrorKind::UnreadableResponse
        );
    }

    /// The host pin: only an unsigned object on the one bundle host, never a
    /// credentialed, ported, fragmented or differently-hosted URL.
    #[test]
    fn a_bundle_download_is_pinned_to_the_storage_host() {
        let sha = "a".repeat(64);
        assert!(BundleDownloadCall::new(
            "https://storage.googleapis.com/edcl/reference/roads.geojsonl.gz",
            &sha,
            1
        )
        .is_ok());
        for url in [
            "http://storage.googleapis.com/edcl/reference/roads.geojsonl.gz",
            "https://storage.googleapis.com.attacker.example/edcl/roads.geojsonl.gz",
            "https://attacker.example/storage.googleapis.com/roads.geojsonl.gz",
            "https://storage.googleapis.com:8443/edcl/roads.geojsonl.gz",
            "https://user:secret@storage.googleapis.com/edcl/roads.geojsonl.gz",
            "https://storage.googleapis.com/edcl/roads.geojsonl.gz?X-Goog-Signature=abc",
            "https://storage.googleapis.com/edcl/roads.geojsonl.gz#frag",
            "https://storage.googleapis.com/",
            "https://storage.googleapis.com/edcl/../roads.geojsonl.gz",
            "not a url",
            "",
        ] {
            let error = BundleDownloadCall::new(url, &sha, 1)
                .err()
                .unwrap_or_else(|| panic!("{url} must be refused"));
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{url}");
        }
        let good = "https://storage.googleapis.com/edcl/reference/roads.geojsonl.gz";
        assert!(BundleDownloadCall::new(good, "A".repeat(64).as_str(), 1).is_err());
        assert!(BundleDownloadCall::new(good, "a".repeat(63).as_str(), 1).is_err());
        assert!(BundleDownloadCall::new(good, &sha, 0).is_err());
        assert!(BundleDownloadCall::new(good, &sha, BUNDLE_DOWNLOAD_MAX_BYTES + 1).is_err());
        let long = format!("https://storage.googleapis.com/{}", "a".repeat(2100));
        assert!(BundleDownloadCall::new(&long, &sha, 1).is_err());
    }

    /// A scripted transport that writes whatever bytes it is told to write.
    struct BytesTransport {
        body: Vec<u8>,
        calls: usize,
    }

    impl Transport for BytesTransport {
        fn sign_in(&mut self, _: SignInCall<'_>) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn refresh(&mut self, _: RefreshCall<'_>) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn list_projects(
            &mut self,
            _: ProjectListCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn transformer_context(
            &mut self,
            _: TransformerContextCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn project_forms(
            &mut self,
            _: crate::ProjectFormsCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn project_form_editor(
            &mut self,
            _: crate::ProjectFormEditorCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn solar_snapshot(
            &mut self,
            _: crate::SolarSnapshotCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn survey_query(
            &mut self,
            _: crate::SurveyQueryCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn survey_entries_select(
            &mut self,
            _: crate::SurveyEntriesSelectCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn survey_entries_changes(
            &mut self,
            _: crate::SurveyEntriesChangesCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn survey_entry_create(
            &mut self,
            _: crate::SurveyEntryCreateCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn project_data(
            &mut self,
            _: crate::ProjectDataCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn upload_bytes(
            &mut self,
            _: crate::UploadBytesCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn survey_control(
            &mut self,
            _: SurveyControlCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn styles(
            &mut self,
            _: crate::StylesCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn layers(
            &mut self,
            _: crate::LayersCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn tiles(&mut self, _: crate::TileCall<'_>) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn project_report(
            &mut self,
            _: crate::ProjectReportCall<'_>,
        ) -> Result<TransportResponse, TransportError> {
            Err(TransportError::Unreachable)
        }
        fn download_bundle(
            &mut self,
            call: BundleDownloadCall<'_>,
            sink: &mut dyn Write,
        ) -> Result<(), TransportError> {
            self.calls += 1;
            assert_eq!(call.method(), "GET");
            // Stream in slices, as a socket would; an oversized body is cut off
            // by the sink, which is the refusal.
            for chunk in self.body.chunks(7) {
                if sink.write_all(chunk).is_err() {
                    return Ok(());
                }
            }
            sink.flush().map_err(|_| TransportError::Unreachable)
        }
    }

    fn profile_for_tests() -> ClientProfile {
        // Only used to prove the default trait methods refuse; the bundle path
        // never touches a profile.
        ClientProfile::validate(ClientProfileInput {
            schema_version: CLIENT_PROFILE_SCHEMA.to_owned(),
            lane: DeploymentLane::Stable,
            source_revision: "test".to_owned(),
            descriptor_sha256: "a".repeat(64),
            firebase_project_id: "firebase-project".to_owned(),
            firebase_api_key: "k".to_owned(),
            gateway_api_key: "g".to_owned(),
            gateway_origin: "https://eds-gateway-3c0q477h.ue.gateway.dev".to_owned(),
            auth_link_begin_method: "POST".to_owned(),
            auth_link_begin_path: "/api/v1/auth/device/begin".to_owned(),
            auth_link_status_method: "POST".to_owned(),
            auth_link_status_path: "/api/v1/auth/device/status".to_owned(),
            auth_link_complete_method: "POST".to_owned(),
            auth_link_complete_path: "/api/v1/auth/device/complete".to_owned(),
            auth_device_refresh_method: "POST".to_owned(),
            auth_device_refresh_path: "/api/v1/auth/device/refresh".to_owned(),
            auth_device_list_method: "GET".to_owned(),
            auth_device_list_path: "/api/v1/auth/devices".to_owned(),
            auth_device_read_method: "GET".to_owned(),
            auth_device_read_path_template: "/api/v1/auth/devices/{device_id}".to_owned(),
            auth_device_revoke_method: "DELETE".to_owned(),
            auth_device_revoke_path_template: "/api/v1/auth/devices/{device_id}".to_owned(),
            project_list_method: "GET".to_owned(),
            project_list_path: "/api/v1/user/projects".to_owned(),
            transformer_context_method: "POST".to_owned(),
            transformer_context_path: "/report".to_owned(),
            transformer_context_action: "get_transformers_data".to_owned(),
            transformer_context_fields: "full".to_owned(),
            project_forms_method: "POST".to_owned(),
            project_forms_path: "/api/v1/project-forms".to_owned(),
            project_forms_action: "activate".to_owned(),
            project_form_editor_action: "settings_editor".to_owned(),
            solar_snapshot_method: "POST".to_owned(),
            solar_snapshot_path: "/api/v1/solar".to_owned(),
            solar_snapshot_action: "desktop_snapshot".to_owned(),
            survey_query_method: "POST".to_owned(),
            survey_query_path: "/api/v1/survey/query".to_owned(),
            survey_entries_select_method: "POST".to_owned(),
            survey_entries_select_path: "/api/v1/survey/entries/select".to_owned(),
            survey_entries_changes_method: "POST".to_owned(),
            survey_entries_changes_path: "/api/v1/survey/entries/changes".to_owned(),
            survey_entry_create_method: "POST".to_owned(),
            survey_entry_create_path: "/api/v1/entries/mutate".to_owned(),
            survey_entry_create_operation: "create".to_owned(),
            project_data_method: "POST".to_owned(),
            project_data_path: "/api/v1/project_data".to_owned(),
            project_data_actions: crate::PROJECT_DATA_ACTIONS.map(str::to_owned).to_vec(),
            survey_control_routes: crate::SURVEY_CONTROL_ROUTES.map(str::to_owned).to_vec(),
            styles_method: "POST".to_owned(),
            styles_path: "/api/v1/styles".to_owned(),
            styles_action: "update_style".to_owned(),
            printing_method: "POST".to_owned(),
            printing_path: "/api/v1/printing".to_owned(),
            printing_actions: crate::PRINTING_ACTIONS.map(str::to_owned).to_vec(),
            layers_method: "POST".to_owned(),
            layers_path: "/api/v1/layers".to_owned(),
            layers_actions: crate::LAYERS_ACTIONS.map(str::to_owned).to_vec(),
            tiles_method: "POST".to_owned(),
            tiles_path: "/api/v1/tiles".to_owned(),
            tiles_actions: crate::TILES_ACTIONS.map(str::to_owned).to_vec(),
            project_report_method: "POST".to_owned(),
            project_report_path: "/report".to_owned(),
            project_report_actions: crate::PROJECT_REPORT_ACTIONS.map(str::to_owned).to_vec(),
            design_attachments_method: "POST".into(),
            design_attachments_path: "/api/v1/design/attachments".into(),
            design_attachments_actions: crate::DESIGN_ATTACHMENTS_ACTIONS
                .map(str::to_owned)
                .to_vec(),
            design_versions_method: "POST".into(),
            design_versions_path: "/api/v1/design/versions".into(),
            design_versions_actions: crate::DESIGN_VERSIONS_ACTIONS.map(str::to_owned).to_vec(),
            design_selections_method: "POST".to_owned(),
            design_selections_path: "/api/v1/design/selections".to_owned(),
            design_selections_actions: crate::DESIGN_SELECTIONS_ACTIONS.map(str::to_owned).to_vec(),
            data_distribution_method: "POST".to_owned(),
            data_distribution_path: "/api/v1/data-distribution".to_owned(),
            data_distribution_actions: crate::DATA_DISTRIBUTION_ACTIONS.map(str::to_owned).to_vec(),
            sre_overview_method: crate::SRE_OVERVIEW_METHOD.to_owned(),
            sre_overview_path: crate::SRE_OVERVIEW_PATH.to_owned(),
            sre_events_method: crate::SRE_EVENTS_METHOD.to_owned(),
            sre_events_path: crate::SRE_EVENTS_PATH.to_owned(),
            sre_events_actions: crate::SRE_EVENTS_ACTIONS.map(str::to_owned).to_vec(),
            admin_bounds_method: crate::ADMIN_BOUNDS_METHOD.to_owned(),
            admin_bounds_path: crate::ADMIN_BOUNDS_PATH.to_owned(),
            admin_bounds_actions: crate::ADMIN_BOUNDS_ACTIONS.map(str::to_owned).to_vec(),
        })
        .unwrap()
    }

    #[test]
    fn a_bundle_is_admitted_only_with_the_catalogue_size_and_digest() {
        let body = b"a national reference bundle, gzipped in spirit".to_vec();
        let sha = format!("{:x}", Sha256::digest(&body));
        let url = "https://storage.googleapis.com/edcl/reference/roads.geojsonl.gz";

        let mut transport = BytesTransport {
            body: body.clone(),
            calls: 0,
        };
        let mut sink = Vec::new();
        let receipt = download_bundle(
            &mut transport,
            BundleDownloadCall::new(url, &sha, body.len() as u64).unwrap(),
            &mut sink,
        )
        .unwrap();
        assert_eq!(sink, body);
        assert_eq!(receipt.bytes, body.len() as u64);
        assert_eq!(receipt.sha256, sha);
        assert_eq!(transport.calls, 1);

        // Wrong digest: the bytes reached the sink, the bundle is refused.
        let mut sink = Vec::new();
        let wrong = download_bundle(
            &mut transport,
            BundleDownloadCall::new(url, &"b".repeat(64), body.len() as u64).unwrap(),
            &mut sink,
        )
        .unwrap_err();
        assert_eq!(wrong.kind(), ErrorKind::UnreadableResponse);

        // Short body.
        let mut sink = Vec::new();
        let short = download_bundle(
            &mut transport,
            BundleDownloadCall::new(url, &sha, body.len() as u64 + 1).unwrap(),
            &mut sink,
        )
        .unwrap_err();
        assert_eq!(short.kind(), ErrorKind::UnreadableResponse);

        // Long body: cut off at the catalogue size, never written past it.
        let mut sink = Vec::new();
        let long = download_bundle(
            &mut transport,
            BundleDownloadCall::new(url, &sha, 10).unwrap(),
            &mut sink,
        )
        .unwrap_err();
        assert_eq!(long.kind(), ErrorKind::UnreadableResponse);
        assert!(sink.len() <= 10, "{}", sink.len());

        // The default transport refuses both new calls rather than reaching
        // anything, and does so before any byte is written.
        struct Unreachable;
        impl Transport for Unreachable {
            fn sign_in(&mut self, _: SignInCall<'_>) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn refresh(&mut self, _: RefreshCall<'_>) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn list_projects(
                &mut self,
                _: ProjectListCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn transformer_context(
                &mut self,
                _: TransformerContextCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn project_forms(
                &mut self,
                _: crate::ProjectFormsCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn project_form_editor(
                &mut self,
                _: crate::ProjectFormEditorCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn solar_snapshot(
                &mut self,
                _: crate::SolarSnapshotCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn survey_query(
                &mut self,
                _: crate::SurveyQueryCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn survey_entries_select(
                &mut self,
                _: crate::SurveyEntriesSelectCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn survey_entries_changes(
                &mut self,
                _: crate::SurveyEntriesChangesCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn survey_entry_create(
                &mut self,
                _: crate::SurveyEntryCreateCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn project_data(
                &mut self,
                _: crate::ProjectDataCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn upload_bytes(
                &mut self,
                _: crate::UploadBytesCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn survey_control(
                &mut self,
                _: SurveyControlCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn styles(
                &mut self,
                _: crate::StylesCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn layers(
                &mut self,
                _: crate::LayersCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn tiles(
                &mut self,
                _: crate::TileCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
            fn project_report(
                &mut self,
                _: crate::ProjectReportCall<'_>,
            ) -> Result<TransportResponse, TransportError> {
                Err(TransportError::Unreachable)
            }
        }
        let mut sink = Vec::new();
        let unreachable = download_bundle(
            &mut Unreachable,
            BundleDownloadCall::new(url, &sha, body.len() as u64).unwrap(),
            &mut sink,
        )
        .unwrap_err();
        assert_eq!(unreachable.kind(), ErrorKind::Transient);
        assert!(sink.is_empty());
        let profile = profile_for_tests();
        let request = DataDistributionRequest::ListDatasets {};
        let call = crate::DataDistributionCall {
            profile: &profile,
            ds_project: "proj-1",
            operation: &request,
            authority: crate::transport::CallAuthority {
                bearer_token: "bearer",
                canonical_uid: "uid",
                canonical_email: "user@example.com",
                device_id: None,
            },
        };
        assert_eq!(call.path(), "/api/v1/data-distribution");
        assert_eq!(call.method(), "POST");
        assert_eq!(call.action(), "list_datasets");
        assert_eq!(call.timeout_seconds(), 180);
        assert_eq!(call.response_limit(), 32 * 1024 * 1024);
        assert!(matches!(
            Unreachable.data_distribution(call),
            Err(TransportError::Unreachable)
        ));
    }
}
