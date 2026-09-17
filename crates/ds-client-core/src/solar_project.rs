//! Closed Solar synchronization. Credentials never enter a local workspace.
use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, SolarProjectCall, UploadBytesCall,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub struct Output {
    pub id: String,
    pub format: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}
pub enum Command {
    ListCities,
    ReadMedia {
        city: String,
        reference: String,
    },
    Portfolio(crate::solar_portfolio::Command),
    ImportMap {
        city: String,
        role: String,
        file_name: String,
        bytes: Vec<u8>,
    },
    Reference {
        request: ds_solar_contracts::reference_request::ReferenceRequest,
    },
    Seed {
        source: Option<String>,
        cities: Vec<String>,
        overwrite: bool,
        digest: Option<String>,
    },
    CommitCity {
        city: String,
        expected_base: String,
        snapshot_json: String,
        fingerprint: String,
    },
    Publish {
        city: String,
        client_run_id: String,
        engine_version: String,
        build_manifest: String,
        fingerprint: String,
        calculation: Output,
        reports: Vec<Output>,
    },
}
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "invalid bounded Solar project handoff",
    )
}
fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "Solar publication response does not match its request",
    )
}
fn conflict() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "Solar publication authority has moved; local results remain available",
    )
}
impl Command {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !id(project) {
            return Err(invalid());
        }
        match self {
            Self::ListCities => {}
            Self::ReadMedia { city, reference } => {
                crate::solar_media::validate_read(project, city, reference)?
            }
            Self::Portfolio(command) => command.validate(project)?,
            Self::ImportMap {
                city,
                role,
                file_name,
                bytes,
            } => {
                crate::solar_media::validate(city, role, file_name, bytes)?;
            }
            Self::Reference { request } => {
                if request.city.trim().is_empty()
                    || request.city.len() > 256
                    || request.country.trim().is_empty()
                    || request.country.len() > 128
                    || !request.latitude.is_finite()
                    || !request.longitude.is_finite()
                    || !(-90.0..=90.0).contains(&request.latitude)
                    || !(-180.0..=180.0).contains(&request.longitude)
                    || serde_json::to_vec(request).map_err(|_| invalid())?.len() > 8192
                {
                    return Err(invalid());
                }
            }
            Self::Seed {
                source,
                cities,
                digest,
                ..
            } => {
                if cities.len() > 64
                    || cities.iter().any(|city| !id(city))
                    || cities
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != cities.len()
                    || source.as_ref().is_some_and(|s| {
                        s.is_empty()
                            || s.len() > 512
                            || s.trim() != s
                            || s.chars().any(char::is_control)
                    })
                    || digest.as_ref().is_some_and(|s| !sha(s))
                {
                    return Err(invalid());
                }
            }
            Self::CommitCity {
                city,
                expected_base,
                snapshot_json,
                fingerprint,
            } => {
                if !id(city)
                    || (!expected_base.is_empty() && !sha(expected_base))
                    || !sha(fingerprint)
                    || snapshot_json.is_empty()
                    || snapshot_json.len() > 8 * 1024 * 1024
                {
                    return Err(invalid());
                }
                let value: Value = serde_json::from_str(snapshot_json).map_err(|_| invalid())?;
                if !value.is_object()
                    || format!("{:x}", Sha256::digest(snapshot_json.as_bytes())) != *fingerprint
                {
                    return Err(invalid());
                }
            }
            Self::Publish {
                city,
                client_run_id,
                engine_version,
                build_manifest,
                fingerprint,
                calculation,
                reports,
            } => {
                if !sha(build_manifest) {
                    return Err(ClientError::new(ErrorKind::InvalidInput,
                        "Draft publication requires a release engine build manifest; keep inputs synced separately and rerun with an admitted release engine"));
                }
                if !id(city)
                    || !id(client_run_id)
                    || !sha(build_manifest)
                    || !sha(fingerprint)
                    || engine_version.is_empty()
                    || engine_version.len() > 128
                    || calculation.id != "report-input"
                    || calculation.format != "json"
                    || calculation.content_type != "application/json"
                    || reports.len() > 99
                {
                    return Err(invalid());
                }
                if std::iter::once(calculation)
                    .chain(reports)
                    .map(|o| o.bytes.len())
                    .sum::<usize>()
                    > 128 * 1024 * 1024
                {
                    return Err(invalid());
                }
                let mut unique = std::collections::BTreeSet::new();
                for output in std::iter::once(calculation).chain(reports) {
                    if !id(&output.id)
                        || !unique.insert(&output.id)
                        || output.bytes.is_empty()
                        || output.bytes.len() > 64 * 1024 * 1024
                    {
                        return Err(invalid());
                    }
                    if output.id == "report-input" {
                        continue;
                    }
                    let draft = draft_id(&output.id)
                        && output.format == "md"
                        && output.content_type == "text/markdown";
                    let chart = output.id.starts_with("plot-")
                        && output.format == "png"
                        && output.content_type == "image/png";
                    if !draft && !chart {
                        return Err(invalid());
                    }
                }
            }
        }
        Ok(())
    }
}
pub(crate) fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    artifacts: bool,
    payload: Value,
) -> Result<Value, ClientError> {
    let catalog = payload["action"] == "list_portfolios";
    let action = payload["action"].as_str().unwrap_or("request").to_owned();
    let response = transport
        .solar_project(SolarProjectCall {
            profile,
            authority: authority.call(),
            artifacts,
            payload,
        })
        .map_err(|e| {
            let e: ClientError = e.into();
            ClientError::new(
                e.kind(),
                match action.as_str() {
                    "open" => "Solar publication opening transport is temporarily unavailable",
                    "finalize" => {
                        "Solar publication finalization transport is temporarily unavailable"
                    }
                    _ => "Solar project producer transport is temporarily unavailable",
                },
            )
        })?;
    if response.status == 409 {
        return Err(conflict());
    }
    if response.status != 200 {
        let error = classify_tile_error(response.status);
        return Err(ClientError::new(
            error.kind(),
            match (action.as_str(), response.status) {
                ("open", 500) => "Solar publication opening returned HTTP 500",
                ("finalize", 500) => "Solar publication finalization returned HTTP 500",
                ("open", _) => "Solar publication opening was refused by its producer",
                ("finalize", _) => "Solar publication finalization was refused by its producer",
                _ => "Solar project producer refused the request",
            },
        ));
    }
    if response.body.len() > 8 * 1024 * 1024 {
        return Err(unreadable());
    }
    let value: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if value["success"] != true
        || !(value["data"].is_object()
            || (catalog && (value["data"].is_array() || value["data"].is_null())))
    {
        return Err(unreadable());
    }
    Ok(if catalog && value["data"].is_null() {
        json!([])
    } else {
        value["data"].clone()
    })
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate(project)?;
    let root = format!("eds_project/{project}/eds_solar");
    match command {
        Command::ReadMedia { city, reference } => {
            let snapshot =
                crate::authorized::solar_snapshot(profile, transport, authority, project, city)?;
            snapshot.download_media(transport, reference)
        }
        Command::Portfolio(command) => {
            crate::solar_portfolio::run(profile, transport, authority, project, command)
        }
        Command::ImportMap {
            city,
            role,
            file_name,
            bytes,
        } => crate::solar_media::upload(
            profile, transport, authority, project, city, role, file_name, bytes,
        ),
        Command::Reference { request } => {
            use base64::Engine;
            let bundle = match transport.solar_reference(crate::SolarReferenceCall {
                profile,
                authority: authority.call(),
                request,
            }) {
                Ok(bundle) => bundle,
                Err(error) => {
                    return Ok(
                        json!({"project":project,"reference_error":{"code":error.code.as_str(),"message":error.message.chars().take(800).collect::<String>()}}),
                    )
                }
            };
            let artifacts = [
                ("weather.json", bundle.weather_json),
                ("reference-unit.json", bundle.reference_unit_json),
                ("reference-unit.parquet", bundle.reference_unit_parquet),
                ("manifest.json", bundle.manifest_json),
            ].into_iter().map(|(name, bytes)| {
                if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 { return Err(unreadable()); }
                Ok(json!({"name":name,"sha256":format!("{:x}", Sha256::digest(&bytes)),
                    "bytes":bytes.len(),"base64":base64::engine::general_purpose::STANDARD.encode(bytes)}))
            }).collect::<Result<Vec<_>, ClientError>>()?;
            Ok(json!({"project":project,"artifacts":artifacts}))
        }
        Command::ListCities => {
            let mut result = call(
                profile,
                transport,
                authority,
                false,
                json!({"action":"list_templates","root":root}),
            )?;
            // The service's empty list is historically null; normalize it once.
            if result["count"] == 0 && result["templates"].is_null() {
                result["templates"] = json!([]);
            }
            let rows = result["templates"].as_array().ok_or_else(unreadable)?;
            if rows.len() > 1000
                || result["count"].as_u64() != Some(rows.len() as u64)
                || rows
                    .iter()
                    .any(|r| !r["template_id"].as_str().is_some_and(id))
            {
                return Err(unreadable());
            }
            result["project"] = json!(project);
            Ok(result)
        }
        Command::Seed {
            source,
            cities,
            overwrite,
            digest,
        } => {
            let mut payload = json!({"action":if digest.is_some() {"seed_apply"} else {"seed_preview"},"root":root});
            if let Some(source) = source {
                payload["seed_source_root"] = json!(source);
            }
            if !cities.is_empty() {
                payload["cities"] = json!(cities);
            }
            if *overwrite {
                payload["seed_overwrite"] = json!(true);
            }
            if let Some(digest) = digest {
                payload["seed_digest"] = json!(digest);
            }
            let result = call(profile, transport, authority, false, payload)?;
            let plan = &result["plan"];
            if plan["root"] != root
                || plan["ds_project"] != project
                || !plan["seed_digest"].as_str().is_some_and(sha)
                || !plan["cities"].is_array()
                || plan["cities"].as_array().is_some_and(|v| v.len() > 64)
                || (digest.is_none() && plan["mutated"] != false)
                || (digest.is_some() && result["seed_digest"].as_str() != digest.as_deref())
                || (*overwrite && plan["overwrite"] != true)
            {
                return Err(unreadable());
            }
            if let Some(source) = source {
                if plan["seed_source_root"] != *source {
                    return Err(unreadable());
                }
            }
            if !cities.is_empty() {
                let got = plan["cities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|r| r["city_id"].as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                let want = cities
                    .iter()
                    .map(String::as_str)
                    .collect::<std::collections::BTreeSet<_>>();
                if got != want {
                    return Err(unreadable());
                }
            }
            Ok(result)
        }
        Command::CommitCity {
            city,
            expected_base,
            snapshot_json,
            fingerprint,
        } => {
            let receipt = call(
                profile,
                transport,
                authority,
                false,
                json!({"action":"project_commit","root":root,"template_id":city,"project_commit":{"expected_base":expected_base,"snapshot_json":snapshot_json}}),
            )?;
            if receipt["root"] != root
                || receipt["city_id"] != *city
                || receipt["client_snapshot_sha256"] != *fingerprint
                || !receipt["input_base_fingerprint"].as_str().is_some_and(sha)
            {
                return Err(unreadable());
            }
            Ok(receipt)
        }
        Command::Publish {
            city,
            client_run_id,
            engine_version,
            build_manifest,
            fingerprint,
            calculation,
            reports,
        } => {
            let snapshot =
                crate::authorized::solar_snapshot(profile, transport, authority, project, city)?;
            if snapshot.input_base_fingerprint() != fingerprint {
                return Err(conflict());
            }
            let calculate = publish(
                profile,
                transport,
                authority,
                project,
                client_run_id,
                engine_version,
                build_manifest,
                &format!("calculate-{city}"),
                "default",
                fingerprint,
                Some((snapshot.snapshot_sha256(), snapshot.snapshot_receipt_id())),
                &[calculation],
            )?;
            let mut receipts = vec![calculate];
            let report_fingerprint = format!("{:x}", Sha256::digest(&calculation.bytes));
            for draft in reports.iter().filter(|o| draft_id(&o.id)) {
                let mut outputs = vec![draft];
                outputs.extend(reports.iter().filter(|o| o.id.starts_with("plot-")));
                receipts.push(publish(
                    profile,
                    transport,
                    authority,
                    project,
                    client_run_id,
                    engine_version,
                    build_manifest,
                    &format!("report-{city}"),
                    &draft.id,
                    &report_fingerprint,
                    None,
                    &outputs,
                )?);
            }
            Ok(json!({"project_id":project,"city_id":city,"publications":receipts}))
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    run: &str,
    version: &str,
    build: &str,
    operation: &str,
    variant: &str,
    base: &str,
    snapshot: Option<(&str, &str)>,
    outputs: &[&Output],
) -> Result<Value, ClientError> {
    let declarations:Vec<_>=outputs.iter().map(|o|json!({"output_id":o.id,"format":o.format,"content_type":o.content_type,"sha256":format!("{:x}",Sha256::digest(&o.bytes)),"size_bytes":o.bytes.len()})).collect();
    let mut request = json!({"action":"open","project_id":project,"client_run_id":run,"engine":"solar","operation":operation,"variant":variant,"engine_version":version,"engine_build_manifest_sha256":build,"input_base_fingerprint":base,"outputs":declarations});
    if let Some((sha, receipt)) = snapshot {
        request["source_snapshot_sha256"] = json!(sha);
        request["snapshot_receipt_id"] = json!(receipt);
    }
    let opened = call(profile, transport, authority, true, request)?;
    let work = opened["work_id"]
        .as_str()
        .filter(|s| id(s))
        .ok_or_else(unreadable)?;
    if opened["state"] == "stored_stale" {
        return Err(conflict());
    }
    if opened["state"] == "published" && opened["head_revision"].as_i64().is_some_and(|n| n > 0) {
        return Ok(
            json!({"work_id":work,"operation":operation,"variant":variant,"state":"published"}),
        );
    }
    if opened["state"] != "pending" {
        return Err(unreadable());
    }
    let uploads = opened["outputs"].as_array().ok_or_else(unreadable)?;
    if uploads.len() != outputs.len() {
        return Err(unreadable());
    }
    let mut seen = std::collections::BTreeSet::new();
    for upload in uploads {
        let output_id = upload["output_id"].as_str().ok_or_else(unreadable)?;
        if !seen.insert(output_id) {
            return Err(unreadable());
        }
        let output = outputs
            .iter()
            .find(|o| o.id == output_id)
            .ok_or_else(unreadable)?;
        if upload["object_generation"].as_i64().is_some_and(|n| n > 0) {
            continue;
        }
        let uri = upload["session_uri"].as_str().ok_or_else(unreadable)?;
        validate_upload_uri(uri)?;
        let mut reader = std::io::Cursor::new(&output.bytes);
        let response = transport.upload_bytes(UploadBytesCall {
            uri,
            size: output.bytes.len() as u64,
            reader: &mut reader,
        })?;
        crate::project_data::uploaded(response)?;
    }
    let finalized = call(
        profile,
        transport,
        authority,
        true,
        json!({"action":"finalize","project_id":project,"client_run_id":run,"work_id":work}),
    )?;
    if finalized["work_id"] != work {
        return Err(unreadable());
    }
    if finalized["state"] != "published"
        || !finalized["head_revision"].as_i64().is_some_and(|n| n > 0)
    {
        return Err(conflict());
    }
    Ok(json!({"work_id":work,"operation":operation,"variant":variant,"state":"published"}))
}
pub(crate) fn validate_upload_uri(uri: &str) -> Result<(), ClientError> {
    let u = url::Url::parse(uri).map_err(|_| unreadable())?;
    if uri.len() > 8192
        || u.scheme() != "https"
        || u.host_str() != Some("storage.googleapis.com")
        || u.port().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || !u.path().starts_with("/upload/storage/v1/b/")
        || !u.path().ends_with("/o")
        || !u
            .query_pairs()
            .any(|(k, v)| k == "upload_id" && !v.is_empty())
    {
        return Err(unreadable());
    }
    Ok(())
}

fn draft_id(id: &str) -> bool {
    matches!(
        id,
        "apd-draft-fr"
            | "apd-draft-en"
            | "network-draft-fr"
            | "network-draft-en"
            | "plant-draft-fr"
            | "plant-draft-en"
            | "financial-draft-fr"
            | "financial-draft-en"
    )
}
