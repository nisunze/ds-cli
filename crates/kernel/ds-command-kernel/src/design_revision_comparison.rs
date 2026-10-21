//! Exact immutable revision comparison. Hosts supply admitted reads, never geometry policy.
use crate::design_versions::{self, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub version_id: String,
    pub revision_id: String,
    pub content_digest: String,
    pub snapshot_digest: String,
    pub snapshot_sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub pin: Pin,
    pub response: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cache {
    pub schema: String,
    pub scope: Scope,
    pub reads: Vec<Read>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Geometry,
    VerifiedBytes,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub left: Pin,
    pub right: Pin,
    pub mode: Mode,
    #[serde(default)]
    pub retain_reads: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained: Option<Cache>,
}
pub trait Source {
    type Error;
    fn read(&mut self, project: &str, object: &str, pin: &Pin) -> Result<Value, Self::Error>;
}
#[derive(Debug)]
pub enum Error<E> {
    Source(E),
    Invalid(String),
}
impl<E> From<String> for Error<E> {
    fn from(s: String) -> Self {
        Self::Invalid(s)
    }
}
fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
impl Pin {
    fn validate(&self) -> Result<(), String> {
        if !crate::design_version_catalog::version_id(&self.version_id)
            || !crate::design_version_catalog::revision_id(&self.revision_id)
            || [
                &self.content_digest,
                &self.snapshot_digest,
                &self.snapshot_sha256,
            ]
            .iter()
            .any(|s| !sha(s))
        {
            return Err("comparison requires exact immutable version/revision and content/frame/UTF-8 digests".into());
        }
        Ok(())
    }
}
impl Request {
    /// Complete retained consumption requires no online identity or data acquisition.
    pub fn fully_retained(&self) -> bool {
        self.retained.as_ref().is_some_and(|cache| {
            [&self.left, &self.right]
                .iter()
                .all(|pin| cache.reads.iter().any(|read| &read.pin == *pin))
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        self.left.validate()?;
        self.right.validate()?;
        if self.left.version_id == self.right.version_id
            && self.left.revision_id == self.right.revision_id
            && self.left != self.right
        {
            return Err(
                "one exact immutable revision cannot be requested with conflicting source digests"
                    .into(),
            );
        }
        if let Some(cache) = &self.retained {
            if cache.schema != "ds.lv-revision-read-cache/v1"
                || cache.reads.len() > 2
                || serde_json::to_vec(cache).map_err(|e| e.to_string())?.len() > 64 * 1024 * 1024
            {
                return Err(
                    "retained revision cache exceeds the closed two-read/64MiB contract".into(),
                );
            }
            for (i, read) in cache.reads.iter().enumerate() {
                read.pin.validate()?;
                if read.pin != self.left && read.pin != self.right
                    || cache.reads[..i].iter().any(|r| r.pin == read.pin)
                {
                    return Err(
                        "retained reads must uniquely name only the requested exact revisions"
                            .into(),
                    );
                }
            }
        }
        Ok(())
    }
}
fn accept(scope: &Scope, pin: &Pin, response: &Value) -> Result<Value, String> {
    let c = crate::design_version_catalog::Command::Revision {
        object_id: scope.object_id.clone(),
        version_id: pin.version_id.clone(),
        revision_id: pin.revision_id.clone(),
    };
    c.validate(&scope.project)?;
    c.verify_response(&scope.project, response)?;
    if response["metadata"]["content_digest"] != pin.content_digest
        || response["metadata"]["snapshot_digest"] != pin.snapshot_digest
        || response["snapshot_sha256"] != pin.snapshot_sha256
    {
        return Err("immutable read differs from the exact requested source digests".into());
    }
    if !matches!(
        response["metadata"]["snapshot_scope"].as_str(),
        Some("runtime_design" | "exported_design")
    ) {
        return Err("immutable snapshot layout is unsupported".into());
    }
    crate::design_version_catalog::decode_pinned_snapshot(response)
}
fn pure(value: Value) -> Result<Value, String> {
    serde_json::from_str(&design_versions::evaluate(
        &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
    )?)
    .map_err(|e| e.to_string())
}
pub fn run<S: Source>(
    scope: &Scope,
    source: &mut S,
    request: &Request,
) -> Result<Value, Error<S::Error>> {
    request.validate()?;
    if scope.object_kind != "lv_transformer"
        || scope.owner.is_empty()
        || scope.audience.is_empty()
        || !matches!(scope.lane.as_str(), "stable" | "canary")
    {
        return Err(Error::Invalid(
            "comparison needs the authenticated LV owner/audience/lane scope".into(),
        ));
    }
    if request.retained.as_ref().is_some_and(|c| &c.scope != scope) {
        return Err(Error::Invalid(
            "retained immutable reads belong to another actor, audience, lane or project/object"
                .into(),
        ));
    }
    crate::design_version_catalog::Command::Catalog {
        object_id: scope.object_id.clone(),
    }
    .validate(&scope.project)?;
    let mut reads = Vec::<Read>::new();
    let mut decoded = Vec::<Value>::new();
    let mut acquired = Vec::new();
    for pin in [&request.left, &request.right] {
        if reads.iter().any(|r| &r.pin == pin) {
            continue;
        }
        let retained = request
            .retained
            .as_ref()
            .and_then(|c| c.reads.iter().find(|r| &r.pin == pin));
        let response = if let Some(retained) = retained {
            retained.response.clone()
        } else {
            source
                .read(&scope.project, &scope.object_id, pin)
                .map_err(Error::Source)?
        };
        acquired.push(if retained.is_some() {
            "retained_file"
        } else {
            "admitted_immutable_read"
        });
        let snapshot = accept(scope, pin, &response)?;
        if matches!(request.mode, Mode::Geometry)
            && response["metadata"]["snapshot_scope"] != "runtime_design"
        {
            return Err(Error::Invalid("exported-design archive IDs do not prove runtime feature correspondence; use verified_bytes for exact byte/digest comparison".into()));
        }
        if matches!(request.mode, Mode::Geometry)
            && !snapshot["layers"]["tr"]["features"]
                .as_array()
                .is_some_and(|r| r.len() == 1)
        {
            return Err(Error::Invalid("runtime comparison requires one retained transformer feature; metadata-only layouts cannot establish geometry equality".into()));
        }
        reads.push(Read {
            pin: pin.clone(),
            response,
        });
        decoded.push(snapshot);
    }
    let indices = |pin: &Pin| {
        reads
            .iter()
            .position(|r| &r.pin == pin)
            .expect("validated pin consumed")
    };
    let left = indices(&request.left);
    let right = indices(&request.right);
    let mut result = json!({"schema":"ds.lv-revision-comparison/v1","scope":scope,"left":request.left,"right":request.right,"mode":request.mode,"mutated":false,"acquisition":acquired,"capture_scope_verified":true,"working_currentness_checked":false,"engineering_approval_claimed":false,"retained_file_provenance":"declared capture scope and exact pins verified; retained JSON does not establish fresh cloud authorization or currentness"});
    match request.mode {
        Mode::VerifiedBytes => {
            result["exact_utf8_bytes_equal"] = json!(
                reads[left].response["snapshot_json"] == reads[right].response["snapshot_json"]
            );
            result["stored_frame_digest_equal"] =
                json!(request.left.snapshot_digest == request.right.snapshot_digest);
            result["engineering_content_digest_equal"] =
                json!(request.left.content_digest == request.right.content_digest);
            result["geometry_equality_claimed"] = json!(false);
            result["runtime_geometry_correspondence_established"] = json!(false);
        }
        Mode::Geometry => {
            let prepare = |i: usize| {
                pure(
                    json!({"action":"prepare","scope":scope,"operation":"comparison","parent":null,"local_sequence":1,"created_at":"immutable-comparison-observation","reason":"read-only exact revision comparison","snapshot":decoded[i],"project_config":null,"evidence":{"stable_object_id":scope.object_id,"source_digest":reads[i].pin.content_digest}}),
                )
            };
            let l = prepare(left)?;
            let r = if left == right {
                l.clone()
            } else {
                prepare(right)?
            };
            let mut comparison =
                pure(json!({"action":"compare","scope":scope,"left":l,"right":r,"tolerance_m":0}))?;
            if comparison["comparison"]["totals"]["ambiguousIdentityFeatures"]
                .as_u64()
                .is_some_and(|n| n > 0)
                || comparison["comparison"]["totals"]["unmatchableFeatures"]
                    .as_u64()
                    .is_some_and(|n| n > 0)
            {
                return Err(Error::Invalid("immutable feature identities cannot prove geometric correspondence; use verified_bytes".into()));
            }
            crate::design_versions_headless::bound_comparison(&mut comparison["comparison"]);
            crate::design_versions_headless::bound_comparison(
                &mut comparison["metadata_comparison"],
            );
            result["comparison"] = comparison["comparison"].take();
            result["metadata_comparison"] = comparison["metadata_comparison"].take();
            result["consistency"] = comparison["consistency"].take();
            result["details_included"] = json!(false);
        }
    }
    if request.retain_reads {
        result["retained"] = json!(Cache {
            schema: "ds.lv-revision-read-cache/v1".into(),
            scope: scope.clone(),
            reads
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    struct Reader {
        values: std::collections::VecDeque<Value>,
        calls: usize,
    }
    impl Source for Reader {
        type Error = String;
        fn read(&mut self, _: &str, _: &str, _: &Pin) -> Result<Value, String> {
            self.calls += 1;
            self.values
                .pop_front()
                .ok_or("unexpected acquisition".into())
        }
    }
    fn scope() -> Scope {
        Scope {
            owner: "actor".into(),
            audience: "a".repeat(64),
            lane: "stable".into(),
            project: "project".into(),
            object_kind: "lv_transformer".into(),
            object_id: "TR-1".into(),
        }
    }
    fn input(id: &str, x: f64, layout: &str) -> Read {
        let raw=json!({"schema":"ds.transformer-version-snapshot/v1","layers":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"TR-1","geometry":{"type":"Point","coordinates":[x,-2.0]},"properties":{"id":"TR-1"}}]}}}).to_string();
        let pin = Pin {
            version_id: id.into(),
            revision_id: id.into(),
            content_digest: "c".repeat(64),
            snapshot_digest: "d".repeat(64),
            snapshot_sha256: format!("{:x}", Sha256::digest(raw.as_bytes())),
        };
        let response = json!({"metadata":{"project_id":"project","transformer_id":"TR-1","version_id":id,"revision_id":id,"revision_number":1,"snapshot_scope":layout,"playback_available":true,"content_digest":pin.content_digest,"snapshot_digest":pin.snapshot_digest},"snapshot_json":raw,"snapshot_sha256":pin.snapshot_sha256});
        Read { pin, response }
    }
    #[test]
    fn two_explicit_consumed_reads_compare_real_geometry_and_retained_reuse_has_no_reads() {
        let l = input("v0", 30.0, "runtime_design");
        let r = input("v1", 30.001, "runtime_design");
        let req = Request {
            left: l.pin.clone(),
            right: r.pin.clone(),
            mode: Mode::Geometry,
            retain_reads: true,
            retained: None,
        };
        let mut source = Reader {
            values: vec![l.response, r.response].into(),
            calls: 0,
        };
        let result = run(&scope(), &mut source, &req).unwrap();
        assert_eq!(source.calls, 2);
        assert_eq!(result["comparison"]["totals"]["geometryOnlyChanged"], 1);
        let mut cached = req;
        cached.retained = Some(serde_json::from_value(result["retained"].clone()).unwrap());
        let mut source = Reader {
            values: Default::default(),
            calls: 0,
        };
        let offline = run(&scope(), &mut source, &cached).unwrap();
        assert_eq!(source.calls, 0);
        assert_eq!(offline["comparison"], result["comparison"]);
        assert_eq!(offline["working_currentness_checked"], false);
        assert_eq!(
            offline["acquisition"],
            json!(["retained_file", "retained_file"])
        );
        cached.retained.as_mut().unwrap().scope.owner = "another".into();
        assert!(run(&scope(), &mut source, &cached).is_err());
        assert_eq!(source.calls, 0);
    }
    #[test]
    fn same_exact_revision_reads_once_and_unknown_identities_never_claim_empty_equality() {
        let l = input("v0", 30.0, "runtime_design");
        let req = Request {
            left: l.pin.clone(),
            right: l.pin.clone(),
            mode: Mode::Geometry,
            retain_reads: false,
            retained: None,
        };
        let mut source = Reader {
            values: vec![l.response].into(),
            calls: 0,
        };
        let result = run(&scope(), &mut source, &req).unwrap();
        assert_eq!(source.calls, 1);
        assert_eq!(result["comparison"]["totals"]["unchanged"], 1);
        let mut l = input("v0", 30.0, "runtime_design");
        let mut snapshot: Value =
            serde_json::from_str(l.response["snapshot_json"].as_str().unwrap()).unwrap();
        snapshot["layers"]["tr"]["features"][0]
            .as_object_mut()
            .unwrap()
            .remove("id");
        snapshot["layers"]["tr"]["features"][0]["properties"] = json!({});
        let raw = snapshot.to_string();
        l.pin.snapshot_sha256 = format!("{:x}", Sha256::digest(raw.as_bytes()));
        l.response["snapshot_json"] = json!(raw);
        l.response["snapshot_sha256"] = json!(l.pin.snapshot_sha256);
        let mut req = req;
        req.left = l.pin.clone();
        req.right = l.pin;
        let mut source = Reader {
            values: vec![l.response].into(),
            calls: 0,
        };
        assert!(run(&scope(), &mut source, &req).is_err());
        assert_eq!(source.calls, 1);
    }
    #[test]
    fn metadata_only_unplayable_and_exported_geometry_refuse_but_archive_bytes_are_explicit() {
        let l = input("v0", 30.0, "exported_design");
        let mut req = Request {
            left: l.pin.clone(),
            right: l.pin.clone(),
            mode: Mode::Geometry,
            retain_reads: false,
            retained: None,
        };
        let mut source = Reader {
            values: vec![l.response.clone()].into(),
            calls: 0,
        };
        assert!(run(&scope(), &mut source, &req).is_err());
        assert_eq!(source.calls, 1);
        req.mode = Mode::VerifiedBytes;
        let mut source = Reader {
            values: vec![l.response.clone()].into(),
            calls: 0,
        };
        let result = run(&scope(), &mut source, &req).unwrap();
        assert_eq!(result["exact_utf8_bytes_equal"], true);
        assert_eq!(result["runtime_geometry_correspondence_established"], false);
        for key in ["playback_available", "snapshot_json"] {
            let mut bad = l.response.clone();
            if key == "playback_available" {
                bad["metadata"][key] = json!(false)
            } else {
                bad.as_object_mut().unwrap().remove(key);
            }
            let mut source = Reader {
                values: vec![bad].into(),
                calls: 0,
            };
            assert!(run(&scope(), &mut source, &req).is_err());
            assert_eq!(source.calls, 1);
        }
    }
}

/// Local retained-file comparison deliberately performs no identity/profile restore.
/// It verifies declared capture scope and bytes, never fresh cloud authorization.
pub fn run_retained(
    project: &str,
    object: &str,
    lane: &str,
    request: &Request,
) -> Result<Value, String> {
    struct NoNetwork;
    impl Source for NoNetwork {
        type Error = String;
        fn read(&mut self, _: &str, _: &str, _: &Pin) -> Result<Value, String> {
            Err("offline comparison requires both exact retained inputs".into())
        }
    }
    request.validate()?;
    let cache = request
        .retained
        .as_ref()
        .ok_or("offline comparison requires an explicit retained cache")?;
    if cache.scope.project != project
        || cache.scope.object_id != object
        || cache.scope.lane != lane
        || [&request.left, &request.right]
            .iter()
            .any(|pin| !cache.reads.iter().any(|r| &r.pin == *pin))
    {
        return Err(
            "offline declared capture scope differs or an exact retained input is missing".into(),
        );
    }
    let mut result = run(&cache.scope, &mut NoNetwork, request).map_err(|e| match e {
        Error::Source(e) | Error::Invalid(e) => e,
    })?;
    result["capture_scope_verified"] = json!(false);
    result["declared_capture_scope_verified"] = json!(true);
    result["fresh_cloud_authorization_checked"] = json!(false);
    result["provenance_mode"] = json!("declared_retained_file");
    Ok(result)
}
