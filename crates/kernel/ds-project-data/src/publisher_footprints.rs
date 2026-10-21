//! Pinned publisher objects -> one retained native analytical source.
//! No socket, credential, provider substitution or inferred coverage exists here.
use crate::local_source::{DigestAlgorithm, Kind, Origin, ProducerSource, Source};
use ds_command_kernel::project_dataset_cache::{self as policy, Coverage, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
};

const MAX_COMPRESSED: u64 = 128 * 1024 * 1024;
const MAX_EXPANDED: usize = 512 * 1024 * 1024;
const MAX_RECORD: usize = 2 * 1024 * 1024;
const MAX_FEATURES: usize = 500_000;
const BATCH: usize = 512;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    /// Publisher tile/partition identity, independent of local filename.
    pub resource_id: String,
    pub path: PathBuf,
    pub sha256: String,
    /// Explicit publisher object coverage, not a feature-derived extent.
    pub coverage: Coverage,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema: String,
    pub project_id: String,
    pub asset_id: String,
    pub revision: String,
    pub actual_source: Origin,
    pub publisher_revision: String,
    pub coverage: Coverage,
    pub resources: Vec<Resource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Invalid(String),
    Required(String),
    Malformed(String),
    Unsupported(String),
    TooLarge(String),
    Store(String),
}
impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "publisher_input_invalid",
            Self::Required(_) => "project_dataset_local_source_required",
            Self::Malformed(_) => "publisher_input_malformed",
            Self::Unsupported(_) => "publisher_geometry_unsupported",
            Self::TooLarge(_) => "publisher_input_too_large",
            Self::Store(_) => "project_dataset_store_failed",
        }
    }
    pub fn message(&self) -> &str {
        match self {
            Self::Invalid(m)
            | Self::Required(m)
            | Self::Malformed(m)
            | Self::Unsupported(m)
            | Self::TooLarge(m)
            | Self::Store(m) => m,
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}
impl std::error::Error for Error {}
fn store(e: impl std::fmt::Display) -> Error {
    Error::Store(e.to_string())
}
fn malformed(e: impl std::fmt::Display) -> Error {
    Error::Malformed(e.to_string())
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn text(v: &str) -> bool {
    !v.is_empty() && v.len() <= 200 && !v.chars().any(char::is_control)
}
fn digest(v: &str) -> bool {
    v.len() == 64
        && v.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

impl Input {
    pub fn validate(&self, scope: &Scope) -> Result<(), Error> {
        if self.schema != "ds.publisher-footprints/v1"
            || self.project_id != scope.project
            || !text(&self.asset_id)
            || !text(&self.revision)
            || !text(&self.publisher_revision)
            || !matches!(
                self.actual_source,
                Origin::GoogleOpenBuildings | Origin::MicrosoftBuildings
            )
            || self.resources.is_empty()
            || self.resources.len() > 16
            || self.coverage.is_empty()
        {
            return Err(Error::Invalid("select one exact project/asset/revision and Google CSV.gz or Microsoft GeoJSONL.gz publisher input (1–16 objects)".into()));
        }
        self.coverage.rects().map_err(malformed)?;
        if self.coverage.cells.len() > 256 {
            return Err(Error::TooLarge(
                "publisher extraction exceeds 256 coverage cells".into(),
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut available = Coverage::default();
        for r in &self.resources {
            if !text(&r.resource_id)
                || !ids.insert(&r.resource_id)
                || !r.path.is_absolute()
                || !digest(&r.sha256)
                || r.coverage.is_empty()
                || r.coverage.cells.len() > 256
            {
                return Err(Error::Invalid("each publisher object needs a unique stable id, absolute path, exact SHA-256 and explicit coverage".into()));
            }
            r.coverage.rects().map_err(malformed)?;
            available.cells.extend(&r.coverage.cells);
        }
        if !policy::assess(&available, &self.coverage)
            .map_err(malformed)?
            .covered
        {
            return Err(Error::Required("selected publisher objects do not cover the requested extract; supply the missing pinned objects explicitly".into()));
        }
        Ok(())
    }
    /// Paths are transport addresses. Source family/version, stable object ids,
    /// byte pins and extraction coverage alone identify this conversion.
    pub fn identity(&self) -> String {
        sha(self.pins().to_string().as_bytes())
    }
    fn pins(&self) -> Value {
        let mut resources: Vec<_> = self
            .resources
            .iter()
            .map(|r| json!({"resource_id":r.resource_id,"sha256":r.sha256,"coverage":r.coverage}))
            .collect();
        resources.sort_by_key(|r| r["resource_id"].as_str().unwrap_or("").to_owned());
        json!({"schema":self.schema,"project_id":self.project_id,"asset_id":self.asset_id,"revision":self.revision,
            "actual_source":self.actual_source,"publisher_revision":self.publisher_revision,"coverage":self.coverage,"resources":resources,
            "conversion":"ds.publisher-footprints/polygon-2d-v1"})
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Produced {
    pub source: Source,
    pub source_manifest: PathBuf,
    pub receipt: Value,
    pub reused: bool,
}

/// Explicitly acquire from already admitted publisher objects. A repeated
/// unchanged acquisition consumes its retained manifest without opening the
/// original objects or checking any remote freshness.
pub fn produce(root: &Path, scope: &Scope, input: &Input) -> Result<Produced, Error> {
    input.validate(scope)?;
    let destination = root.join("publisher-footprints").join(input.identity());
    crate::private::create_dir_all(
        destination
            .parent()
            .ok_or_else(|| store("publisher destination has no parent"))?,
    )
    .map_err(store)?;
    // The store owns advisory locks. Concurrent consumers of the same pinned
    // conversion join its completed output rather than re-reading objects.
    let _lease =
        ds_layer_store::exclusive_lock(&destination.with_extension("lock")).map_err(store)?;
    if destination.exists() {
        return retained(&destination, scope, input);
    }
    crate::private::create_dir_all(
        destination
            .parent()
            .ok_or_else(|| store("publisher destination has no parent"))?,
    )
    .map_err(store)?;
    let stage = tempfile::tempdir_in(destination.parent().unwrap()).map_err(store)?;
    let artifact = stage.path().join("footprints.parquet");
    let pins = input.pins();
    let producer_digest = sha(pins.to_string().as_bytes());
    let provenance = ds_columnar::geoparquet::Provenance {
        source_digest: Some(producer_digest.clone()),
        conversion: Some(input.identity()),
        ..Default::default()
    };
    let mut writer = ds_columnar::ConversionStream::create(&artifact, provenance).map_err(store)?;
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut batch = Vec::new();
    let mut raw_rows = 0usize;
    let mut duplicates = 0usize;
    let mut compressed_bytes = 0u64;
    let mut resources: Vec<_> = input.resources.iter().collect();
    resources.sort_by_key(|r| &r.resource_id);
    for (i, resource) in resources.into_iter().enumerate() {
        let local = stage.path().join(format!("object-{i}.gz"));
        compressed_bytes += copy_verified(resource, &local)?;
        let decoder = flate2::read::MultiGzDecoder::new(File::open(&local).map_err(store)?);
        let mut reader = std::io::BufReader::new(decoder.take(MAX_EXPANDED as u64 + 1));
        let mut expanded = 0usize;
        let mut header = None;
        while let Some(line) = line(&mut reader, &mut expanded)? {
            let feature = match input.actual_source {
                Origin::GoogleOpenBuildings => {
                    let row = csv_row(&line)?;
                    if header.is_none() {
                        validate_headers(&row)?;
                        header = Some(row);
                        continue;
                    }
                    google_feature(header.as_ref().unwrap(), &row)?
                }
                Origin::MicrosoftBuildings => microsoft_feature(&line)?,
                _ => unreachable!("source validated before reading"),
            };
            raw_rows += 1;
            if raw_rows > 5_000_000 {
                return Err(Error::TooLarge(
                    "publisher input exceeds five million rows".into(),
                ));
            }
            // Parse/validate even rows outside coverage; corruption never
            // becomes evidence of a complete extraction.
            let bounds = polygon_bounds(&feature["geometry"])?;
            if !input.coverage.cells.iter().any(|c| intersects(*c, bounds)) {
                continue;
            }
            let id = feature["id"]
                .as_str()
                .ok_or_else(|| malformed("publisher identity missing"))?
                .to_owned();
            let hash = sha(feature.to_string().as_bytes());
            if let Some(previous) = seen.get(&id) {
                if previous != &hash {
                    return Err(Error::Malformed(
                        "publisher objects contain conflicting rows for one stable footprint id"
                            .into(),
                    ));
                }
                duplicates += 1;
                continue;
            }
            seen.insert(id, hash);
            if seen.len() > MAX_FEATURES {
                return Err(Error::TooLarge(
                    "publisher extract exceeds 500000 footprints; select a smaller coverage".into(),
                ));
            }
            batch.push(feature);
            if batch.len() == BATCH {
                writer.append(&batch, &[]).map_err(malformed)?;
                batch.clear();
            }
        }
        if input.actual_source == Origin::GoogleOpenBuildings && header.is_none() {
            return Err(malformed("Google publisher CSV has no header"));
        }
        std::fs::remove_file(&local).map_err(store)?;
    }
    writer.append(&batch, &[]).map_err(malformed)?;
    let conversion = writer.finish(&input.asset_id).map_err(store)?;
    crate::private::prepare_file(&artifact).map_err(store)?;
    let source = Source {
        schema: "ds.local-analytical-source/v1".into(),
        project_id: input.project_id.clone(),
        asset_id: input.asset_id.clone(),
        revision: input.revision.clone(),
        actual_source: input.actual_source,
        producer_source: ProducerSource {
            revision: input.publisher_revision.clone(),
            digest_algorithm: DigestAlgorithm::Sha256,
            digest: producer_digest,
        },
        content: Kind::Footprints,
        artifact_path: destination.join("footprints.parquet"),
        artifact_sha256: ds_columnar::digest_file(&artifact).map_err(store)?,
        completed_coverage: input.coverage.clone(),
    };
    source.validate(scope).map_err(malformed)?;
    let receipt = json!({"schema":"ds.publisher-footprints-receipt/v1","pins":pins,"source":source,
        "raw_rows":raw_rows,"feature_count":conversion.feature_count,"duplicates":duplicates,"compressed_bytes":compressed_bytes,
        "artifact_bytes":conversion.byte_count,"network_requests":0,"brain_compute_requests":0});
    write_json(
        &stage.path().join("source.json"),
        &serde_json::to_value(&source).map_err(store)?,
    )?;
    write_json(&stage.path().join("receipt.json"), &receipt)?;
    // Never replace an existing producer revision. The receipt is published
    // last; interrupted output cannot claim an admitted source.
    match crate::private::create_dir(&destination) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return retained(&destination, scope, input)
        }
        Err(e) => return Err(store(e)),
    }
    for name in ["footprints.parquet", "source.json", "receipt.json"] {
        std::fs::hard_link(stage.path().join(name), destination.join(name)).map_err(store)?;
    }
    Ok(Produced {
        source,
        source_manifest: destination.join("source.json"),
        receipt,
        reused: false,
    })
}
fn retained(dir: &Path, scope: &Scope, input: &Input) -> Result<Produced, Error> {
    let metadata = std::fs::symlink_metadata(dir).map_err(store)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(store("publisher output is not a private directory"));
    }
    let receipt = read_json(&dir.join("receipt.json"))?;
    if receipt["schema"] != "ds.publisher-footprints-receipt/v1" || receipt["pins"] != input.pins()
    {
        return Err(store(
            "retained publisher output has no matching completed receipt",
        ));
    }
    let source: Source =
        serde_json::from_value(read_json(&dir.join("source.json"))?).map_err(store)?;
    source.validate(scope).map_err(malformed)?;
    if receipt["source"] != serde_json::to_value(&source).map_err(store)?
        || source.artifact_path != dir.join("footprints.parquet")
    {
        return Err(store(
            "retained publisher manifest differs from its completed receipt",
        ));
    }
    Ok(Produced {
        source,
        source_manifest: dir.join("source.json"),
        receipt,
        reused: true,
    })
}
fn write_json(path: &Path, value: &Value) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(value).map_err(store)?;
    if bytes.len() > 128 * 1024 {
        return Err(Error::TooLarge(
            "publisher receipt exceeds 128 KiB; use fewer coverage cells".into(),
        ));
    }
    let mut file = crate::private::open_options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(store)?;
    file.write_all(&bytes).map_err(store)?;
    file.sync_all().map_err(store)
}
fn read_json(path: &Path) -> Result<Value, Error> {
    let meta = std::fs::symlink_metadata(path).map_err(store)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 128 * 1024 {
        return Err(store(
            "publisher receipt must be a regular bounded JSON file",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(store)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(store)?;
    if bytes.len() > 128 * 1024 {
        return Err(store("publisher receipt grew beyond its bound"));
    }
    serde_json::from_slice(&bytes).map_err(store)
}
fn copy_verified(resource: &Resource, destination: &Path) -> Result<u64, Error> {
    let meta = std::fs::symlink_metadata(&resource.path).map_err(|_| {
        Error::Required(format!(
            "publisher object {} is missing; explicitly acquire its pinned revision",
            resource.resource_id
        ))
    })?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() == 0
        || meta.len() > MAX_COMPRESSED
    {
        return Err(Error::TooLarge(
            "publisher object must be a regular nonempty gzip file of at most 128 MiB".into(),
        ));
    }
    #[cfg(test)]
    counts::read();
    let mut input = File::open(&resource.path).map_err(store)?;
    let mut output = crate::private::open_options()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(store)?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buf).map_err(store)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > MAX_COMPRESSED {
            return Err(Error::TooLarge(
                "publisher gzip grew beyond its byte bound".into(),
            ));
        }
        hash.update(&buf[..n]);
        output.write_all(&buf[..n]).map_err(store)?;
    }
    if format!("{:x}", hash.finalize()) != resource.sha256 {
        return Err(Error::Invalid(format!(
            "publisher object {} differs from its exact SHA-256 pin",
            resource.resource_id
        )));
    }
    output.sync_all().map_err(store)?;
    Ok(bytes)
}
fn line(reader: &mut impl BufRead, total: &mut usize) -> Result<Option<String>, Error> {
    let mut line = String::new();
    let n = reader
        .take(MAX_RECORD as u64 + 1)
        .read_line(&mut line)
        .map_err(malformed)?;
    *total += n;
    if n > MAX_RECORD || *total > MAX_EXPANDED {
        return Err(Error::TooLarge(
            "publisher expanded gzip exceeds its 512 MiB/object or 2 MiB/record bound".into(),
        ));
    }
    Ok((n != 0).then_some(line))
}
fn csv_row(line: &str) -> Result<csv::StringRecord, Error> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(line.as_bytes());
    let mut rows = reader.records();
    let row = rows
        .next()
        .ok_or_else(|| malformed("Google CSV contains an empty record"))?
        .map_err(malformed)?;
    if rows.next().is_some() {
        return Err(malformed("Google CSV requires one bounded record per line"));
    }
    Ok(row)
}
fn validate_headers(headers: &csv::StringRecord) -> Result<(), Error> {
    let mut seen = std::collections::BTreeSet::new();
    if headers.iter().any(|h| h.is_empty() || !seen.insert(h))
        || !headers.iter().any(|h| h == "geometry")
    {
        return Err(malformed(
            "Google CSV needs unique named columns and its geometry WKT column",
        ));
    }
    Ok(())
}
fn google_feature(headers: &csv::StringRecord, row: &csv::StringRecord) -> Result<Value, Error> {
    if headers.len() != row.len() {
        return Err(malformed(
            "Google CSV row differs from its declared columns",
        ));
    }
    let properties: BTreeMap<String, String> = headers
        .iter()
        .zip(row.iter())
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    let geometry = polygon_wkt(
        properties
            .get("geometry")
            .ok_or_else(|| malformed("Google geometry missing"))?,
    )?;
    let publisher_id = properties
        .get("building_id")
        .or_else(|| properties.get("id"))
        .filter(|id| !id.is_empty())
        .cloned();
    feature(
        Origin::GoogleOpenBuildings,
        geometry,
        publisher_id,
        serde_json::to_value(properties).map_err(malformed)?,
    )
}
fn microsoft_feature(line: &str) -> Result<Value, Error> {
    let row: Value = serde_json::from_str(line).map_err(malformed)?;
    if row["type"] != "Feature" || !row["properties"].is_object() {
        return Err(malformed(
            "Microsoft publisher row must be one GeoJSON Feature with properties",
        ));
    }
    if row.get("crs").is_some() || row["geometry"].get("crs").is_some() {
        return Err(Error::Unsupported(
            "publisher GeoJSON must use its declared WGS84 coordinates without a CRS override"
                .into(),
        ));
    }
    polygon_bounds(&row["geometry"])?;
    let id = match row.get("id").filter(|v| !v.is_null()) {
        Some(Value::String(id)) => Some(id.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(_) => {
            return Err(malformed(
                "Microsoft publisher id must be a string or number",
            ))
        }
        None => None,
    };
    feature(
        Origin::MicrosoftBuildings,
        row["geometry"].clone(),
        id,
        row["properties"].clone(),
    )
}
fn feature(
    origin: Origin,
    geometry: Value,
    publisher_id: Option<String>,
    properties: Value,
) -> Result<Value, Error> {
    polygon_bounds(&geometry)?;
    if publisher_id
        .as_ref()
        .is_some_and(|id| id.is_empty() || id.len() > 512 || id.chars().any(char::is_control))
    {
        return Err(malformed("publisher footprint id is invalid"));
    }
    let key = publisher_id
        .clone()
        .unwrap_or_else(|| sha(geometry.to_string().as_bytes()));
    let prefix = if origin == Origin::GoogleOpenBuildings {
        "google"
    } else {
        "microsoft"
    };
    // Lossless publisher properties as one declared string column avoid a
    // later publisher record silently changing an already pinned Arrow schema.
    Ok(
        json!({"type":"Feature","id":format!("{prefix}:{key}"),"geometry":geometry,
        "properties":{"actual_source":origin,"publisher_id":publisher_id.unwrap_or_default(),"publisher_properties":properties.to_string()}}),
    )
}
fn intersects(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[2] && a[2] >= b[0] && a[1] <= b[3] && a[3] >= b[1]
}
fn polygon_bounds(geometry: &Value) -> Result<[f64; 4], Error> {
    let mut nodes = 0usize;
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    let mut polygon = |value: &Value| -> Result<(), Error> {
        let rings = value
            .as_array()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| malformed("polygon needs nonempty rings"))?;
        for ring in rings {
            let points = ring
                .as_array()
                .filter(|v| v.len() >= 4)
                .ok_or_else(|| malformed("polygon ring needs at least four positions"))?;
            if points.first() != points.last() {
                return Err(malformed(
                    "publisher polygon rings must be explicitly closed",
                ));
            }
            for p in points {
                nodes += 1;
                if nodes > 100_000 {
                    return Err(Error::TooLarge(
                        "one publisher polygon exceeds 100000 positions".into(),
                    ));
                }
                let p = p.as_array().filter(|p| p.len() == 2).ok_or_else(|| {
                    Error::Unsupported(
                        "publisher polygons support two-dimensional positions only".into(),
                    )
                })?;
                let x = p[0]
                    .as_f64()
                    .ok_or_else(|| malformed("polygon longitude must be finite"))?;
                let y = p[1]
                    .as_f64()
                    .ok_or_else(|| malformed("polygon latitude must be finite"))?;
                if !x.is_finite()
                    || !y.is_finite()
                    || !(-180.0..=180.0).contains(&x)
                    || !(-90.0..=90.0).contains(&y)
                {
                    return Err(malformed(
                        "publisher polygon coordinates must be finite WGS84 longitude/latitude",
                    ));
                }
                bounds[0] = bounds[0].min(x);
                bounds[1] = bounds[1].min(y);
                bounds[2] = bounds[2].max(x);
                bounds[3] = bounds[3].max(y);
            }
        }
        Ok(())
    };
    match geometry["type"].as_str() {
        Some("Polygon") => polygon(&geometry["coordinates"])?,
        Some("MultiPolygon") => {
            for p in geometry["coordinates"]
                .as_array()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| malformed("multipolygon needs polygons"))?
            {
                polygon(p)?;
            }
        }
        _ => {
            return Err(Error::Unsupported(
                "publisher footprints admit Polygon and MultiPolygon only".into(),
            ))
        }
    }
    Ok(bounds)
}

/// Strict bounded 2D publisher WKT; no geometry rewriting or CRS guessing.
pub fn polygon_wkt(text: &str) -> Result<Value, Error> {
    if text.len() > MAX_RECORD {
        return Err(Error::TooLarge(
            "publisher WKT exceeds its record bound".into(),
        ));
    }
    struct Parser<'a> {
        bytes: &'a [u8],
        at: usize,
        nodes: usize,
    }
    impl Parser<'_> {
        fn ws(&mut self) {
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                self.at += 1;
            }
        }
        fn byte(&mut self, c: u8) -> Result<(), Error> {
            self.ws();
            if self.bytes.get(self.at) != Some(&c) {
                return Err(malformed("malformed polygon WKT delimiter"));
            }
            self.at += 1;
            Ok(())
        }
        fn next(&mut self, c: u8) -> bool {
            self.ws();
            if self.bytes.get(self.at) == Some(&c) {
                self.at += 1;
                true
            } else {
                false
            }
        }
        fn number(&mut self) -> Result<f64, Error> {
            self.ws();
            let start = self.at;
            while self.bytes.get(self.at).is_some_and(|c| {
                c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.' | b'e' | b'E')
            }) {
                self.at += 1;
            }
            std::str::from_utf8(&self.bytes[start..self.at])
                .map_err(malformed)?
                .parse::<f64>()
                .map_err(|_| malformed("malformed polygon WKT number"))
        }
        fn ring(&mut self) -> Result<Value, Error> {
            self.byte(b'(')?;
            let mut points = Vec::new();
            loop {
                let x = self.number()?;
                if !self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
                    return Err(malformed("WKT ordinates need whitespace"));
                }
                let y = self.number()?;
                self.nodes += 1;
                if self.nodes > 100_000 {
                    return Err(Error::TooLarge("WKT exceeds 100000 positions".into()));
                }
                self.ws();
                if self
                    .bytes
                    .get(self.at)
                    .is_some_and(|c| !matches!(c, b',' | b')'))
                {
                    return Err(Error::Unsupported(
                        "publisher WKT supports two dimensions only".into(),
                    ));
                }
                points.push(json!([x, y]));
                if self.next(b',') {
                    continue;
                }
                self.byte(b')')?;
                break;
            }
            Ok(json!(points))
        }
        fn polygon(&mut self) -> Result<Value, Error> {
            self.byte(b'(')?;
            let mut rings = Vec::new();
            loop {
                rings.push(self.ring()?);
                if self.next(b',') {
                    continue;
                }
                self.byte(b')')?;
                break;
            }
            Ok(json!(rings))
        }
    }
    let mut p = Parser {
        bytes: text.as_bytes(),
        at: 0,
        nodes: 0,
    };
    p.ws();
    let start = p.at;
    while p.bytes.get(p.at).is_some_and(u8::is_ascii_alphabetic) {
        p.at += 1;
    }
    let kind = std::str::from_utf8(&p.bytes[start..p.at])
        .map_err(malformed)?
        .to_ascii_uppercase();
    p.ws();
    if matches!(kind.as_str(), "POLYGON" | "MULTIPOLYGON")
        && p.bytes.get(p.at).is_some_and(u8::is_ascii_alphabetic)
    {
        return Err(Error::Unsupported(
            "publisher WKT does not admit Z/M/EMPTY modifiers".into(),
        ));
    }
    let coordinates = match kind.as_str() {
        "POLYGON" => p.polygon()?,
        "MULTIPOLYGON" => {
            p.byte(b'(')?;
            let mut polygons = Vec::new();
            loop {
                polygons.push(p.polygon()?);
                if p.next(b',') {
                    continue;
                }
                p.byte(b')')?;
                break;
            }
            json!(polygons)
        }
        _ => {
            return Err(Error::Unsupported(
                "publisher WKT admits POLYGON and MULTIPOLYGON without SRID/Z/M/EMPTY only".into(),
            ))
        }
    };
    p.ws();
    if p.at != p.bytes.len() {
        return Err(malformed("trailing content after polygon WKT"));
    }
    let geometry =
        json!({"type":if kind=="POLYGON"{"Polygon"}else{"MultiPolygon"},"coordinates":coordinates});
    polygon_bounds(&geometry)?;
    Ok(geometry)
}

#[cfg(test)]
mod counts {
    use std::cell::Cell;
    thread_local! {static READS:Cell<usize>=const{Cell::new(0)};}
    pub fn read() {
        READS.with(|r| r.set(r.get() + 1));
    }
    pub fn take() -> usize {
        READS.with(|r| r.replace(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Acquisition, BundleReceipt, DeclaredSource, Failure, Hosts, Mode, Provider};
    fn scope() -> Scope {
        Scope {
            principal: "fixture-user".into(),
            project: "fixture-project".into(),
        }
    }
    fn coverage() -> Coverage {
        Coverage {
            cells: vec![[29.9, -2.1, 30.2, -1.9]],
        }
    }
    fn polygon(x: f64) -> Value {
        json!({"type":"Polygon","coordinates":[[[x,-2.],[x+0.0001,-2.],[x+0.0001,-1.9999],[x,-1.9999],[x,-2.]]]})
    }
    fn gzip(dir: &Path, name: &str, text: &str) -> Resource {
        let path = dir.join(name);
        let file = File::create(&path).unwrap();
        let mut writer = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        writer.write_all(text.as_bytes()).unwrap();
        writer.finish().unwrap();
        Resource {
            resource_id: name.into(),
            sha256: ds_columnar::digest_file(&path).unwrap(),
            path,
            coverage: coverage(),
        }
    }
    fn input(dir: &Path, source: Origin, text: &str) -> Input {
        Input {
            schema: "ds.publisher-footprints/v1".into(),
            project_id: scope().project,
            asset_id: "publisher-city".into(),
            revision: "asset-r1".into(),
            actual_source: source,
            publisher_revision: if source == Origin::GoogleOpenBuildings {
                "google-v3"
            } else {
                "microsoft-2026-08-13"
            }
            .into(),
            coverage: coverage(),
            resources: vec![gzip(dir, "tile.gz", text)],
        }
    }
    fn microsoft(id: &str, x: f64) -> String {
        json!({"type":"Feature","id":id,"geometry":polygon(x),"properties":{"height":8.5}})
            .to_string()
            + "\n"
    }
    fn google() -> String {
        "latitude,longitude,area_in_meters,confidence,geometry,full_plus_code,building_id\n-2,30,40,0.9,\"POLYGON ((30.0005 -2,30.0006 -2,30.0006 -1.9999,30.0005 -1.9999,30.0005 -2))\",6G9G2222+22,published-building-1\n".into()
    }
    struct NoRemote(usize);
    impl Provider for NoRemote {
        fn acquire(&mut self, _: &policy::Dataset, _: &Value) -> Result<Acquisition, Failure> {
            self.0 += 1;
            Err(Failure::ProviderUnavailable("network forbidden".into()))
        }
    }
    fn seed(
        root: &Path,
        source: &Source,
        extents: &[policy::Extent],
        read: bool,
    ) -> crate::EnsureOutcome {
        let mut provider = NoRemote(0);
        let mut downloads = 0;
        let mut fetch = |_: &BundleReceipt, _: &Path| {
            downloads += 1;
            Err("download forbidden".into())
        };
        let mut hosts = Hosts {
            provider: &mut provider,
            fetch: &mut fetch,
        };
        let result = crate::ensure(
            root,
            &scope(),
            &source.dataset(),
            &DeclaredSource::LocalAnalytical(Box::new(source.clone())),
            &policy::buffer_policy(&[200.]).unwrap(),
            extents,
            &[],
            if read {
                Mode::Read
            } else {
                Mode::Acquire(&mut hosts)
            },
        )
        .unwrap();
        assert_eq!(
            (provider.0, downloads),
            (0, 0),
            "actual producer and room operations have no Brain or bundle transport calls"
        );
        result
    }
    #[test]
    fn both_publishers_produce_canonical_indexed_rooms_and_reuse_offline() {
        for origin in [Origin::GoogleOpenBuildings, Origin::MicrosoftBuildings] {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("held");
            let text = if origin == Origin::GoogleOpenBuildings {
                google()
            } else {
                microsoft("published-building-1", 30.0005)
            };
            let input = input(dir.path(), origin, &text);
            counts::take();
            let cold = produce(&root, &scope(), &input).unwrap();
            assert_eq!(counts::take(), 1);
            assert_eq!(cold.source.actual_source, origin);
            assert_eq!(
                cold.source.producer_source.revision,
                input.publisher_revision
            );
            let mut reader =
                ds_columnar::ArtifactReader::open(&cold.source.artifact_path, 16).unwrap();
            assert_eq!(reader.summary().feature_count, 1);
            assert!(reader.summary().conversion.is_some());
            let batch = reader.next_batch().unwrap().unwrap();
            let features = ds_columnar::batch_to_features(&batch, 1000).unwrap();
            assert!(features[0]["id"]
                .as_str()
                .unwrap()
                .ends_with(":published-building-1"));
            let props: Value = serde_json::from_str(
                features[0]["properties"]["publisher_properties"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            if origin == Origin::GoogleOpenBuildings {
                assert_eq!(props["confidence"], "0.9");
            } else {
                assert_eq!(props["height"], 8.5);
            }
            let extents = vec![policy::Extent {
                name: "first-transformer".into(),
                bounds: Some([30., -2., 30.001, -1.999]),
            }];
            let first = seed(&root, &cold.source, &extents, false);
            assert!(first.covered && first.acquired > 0);
            assert_eq!(first.features, 1);
            std::fs::remove_file(&input.resources[0].path).unwrap();
            let retained = produce(&root, &scope(), &input).unwrap();
            assert!(retained.reused);
            assert_eq!(counts::take(), 0);
            assert_eq!(retained.source, cold.source);
            assert_eq!(seed(&root, &retained.source, &extents, false).acquired, 0);
            assert!(seed(&root, &retained.source, &extents, true).covered);
            let mut expanded = extents;
            expanded.push(policy::Extent {
                name: "second-transformer".into(),
                bounds: Some([30.06, -2., 30.061, -1.999]),
            });
            assert!(seed(&root, &retained.source, &expanded, false).covered);
            assert_eq!(
                counts::take(),
                0,
                "coverage expansion queries retained analytical input, not raw publisher data"
            );
        }
    }
    #[test]
    fn empty_completed_extract_is_a_reusable_source() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = input(dir.path(), Origin::MicrosoftBuildings, "");
        // A verified empty compressed object is valid publisher evidence.
        input.resources[0].coverage = coverage();
        let root = dir.path().join("held");
        let out = produce(&root, &scope(), &input).unwrap();
        assert_eq!(out.receipt["feature_count"], 0);
        let extents = vec![policy::Extent {
            name: "empty".into(),
            bounds: Some([30., -2., 30.001, -1.999]),
        }];
        let result = seed(&root, &out.source, &extents, false);
        assert!(result.covered);
        assert_eq!(result.features, 0);
        std::fs::remove_file(&input.resources[0].path).unwrap();
        assert!(produce(&root, &scope(), &input).unwrap().reused);
        assert_eq!(seed(&root, &out.source, &extents, false).acquired, 0);
    }
    #[test]
    fn wkt_preserves_holes_multipolygons_and_refuses_unsupported_or_malformed_input() {
        let geometry = polygon_wkt(
            "POLYGON ((30 -2,31 -2,31 -1,30 -2),(30.1 -1.9,30.2 -1.9,30.2 -1.8,30.1 -1.9))",
        )
        .unwrap();
        assert_eq!(geometry["coordinates"].as_array().unwrap().len(), 2);
        let multi =
            polygon_wkt("MULTIPOLYGON (((30 -2,31 -2,31 -1,30 -2)),((32 -2,33 -2,33 -1,32 -2)))")
                .unwrap();
        assert_eq!(multi["coordinates"].as_array().unwrap().len(), 2);
        for text in [
            "POINT (30 -2)",
            "SRID=4326;POLYGON ((30 -2,31 -2,31 -1,30 -2))",
            "POLYGON Z ((30 -2 0,31 -2 0,31 -1 0,30 -2 0))",
            "POLYGON EMPTY",
        ] {
            assert!(
                matches!(polygon_wkt(text), Err(Error::Unsupported(_))),
                "{text}"
            );
        }
        for text in [
            "POLYGON ((30 -2,31 -2,31 -1,30 -1))",
            "POLYGON ((181 -2,31 -2,31 -1,181 -2))",
            "POLYGON ((30 -2,31 -2,31 -1,30 -2)) garbage",
        ] {
            assert!(
                matches!(polygon_wkt(text), Err(Error::Malformed(_))),
                "{text}"
            );
        }
    }
    #[test]
    fn bad_pins_malformed_rows_and_conflicting_ids_publish_nothing() {
        for case in ["digest", "malformed", "unsupported", "conflict"] {
            let dir = tempfile::tempdir().unwrap();
            let text=match case {"malformed"=>"{invalid}\n".into(),"unsupported"=>json!({"type":"Feature","properties":{},"geometry":{"type":"Point","coordinates":[30.,-2.]}}).to_string(),
                "conflict"=>microsoft("same",30.0005)+&microsoft("same",30.0006),_=>microsoft("valid",30.0005)};
            let mut input = input(dir.path(), Origin::MicrosoftBuildings, &text);
            if case == "digest" {
                input.resources[0].sha256 = "a".repeat(64);
            }
            let root = dir.path().join("held");
            let err = produce(&root, &scope(), &input).unwrap_err();
            assert_eq!(
                err.code(),
                match case {
                    "digest" => "publisher_input_invalid",
                    "unsupported" => "publisher_geometry_unsupported",
                    _ => "publisher_input_malformed",
                }
            );
            assert!(!root
                .join("publisher-footprints")
                .join(input.identity())
                .exists());
        }
    }
    #[test]
    fn gaps_and_wrong_project_are_refused_before_any_publisher_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = input(
            dir.path(),
            Origin::MicrosoftBuildings,
            &microsoft("a", 30.0005),
        );
        counts::take();
        input.coverage.cells.push([31., -2.1, 31.2, -1.9]);
        assert!(matches!(
            produce(&dir.path().join("held"), &scope(), &input),
            Err(Error::Required(_))
        ));
        assert_eq!(counts::take(), 0);
        input.coverage = coverage();
        let mut other = scope();
        other.project = "other-project".into();
        assert!(matches!(
            produce(&dir.path().join("held"), &other, &input),
            Err(Error::Invalid(_))
        ));
        assert_eq!(counts::take(), 0);
    }
    #[test]
    fn overlapping_tiles_deduplicate_stable_ids_and_paths_do_not_change_identity() {
        let dir = tempfile::tempdir().unwrap();
        let text = microsoft("a", 30.0005);
        let mut input = input(dir.path(), Origin::MicrosoftBuildings, &text);
        input.resources.push(gzip(dir.path(), "second.gz", &text));
        let identity = input.identity();
        let out = produce(&dir.path().join("held"), &scope(), &input).unwrap();
        assert_eq!(out.receipt["feature_count"], 1);
        assert_eq!(out.receipt["duplicates"], 1);
        input.resources.reverse();
        for r in &mut input.resources {
            r.path = dir.path().join("moved-but-unneeded.gz");
        }
        assert_eq!(input.identity(), identity);
        assert!(
            produce(&dir.path().join("held"), &scope(), &input)
                .unwrap()
                .reused
        );
        input.actual_source = Origin::GoogleOpenBuildings;
        assert_ne!(input.identity(), identity);
    }

    #[test]
    fn concurrent_consumers_share_one_actual_publisher_read() {
        let dir = tempfile::tempdir().unwrap();
        let input = input(
            dir.path(),
            Origin::MicrosoftBuildings,
            &microsoft("one", 30.0005),
        );
        let root = dir.path().join("held");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let jobs: Vec<_> = (0..2)
            .map(|_| {
                let input = input.clone();
                let root = root.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    counts::take();
                    barrier.wait();
                    let out = produce(&root, &scope(), &input).unwrap();
                    (out.reused, counts::take())
                })
            })
            .collect();
        let results: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
        assert_eq!(results.iter().map(|r| r.1).sum::<usize>(), 1);
        assert_eq!(results.iter().filter(|r| r.0).count(), 1);
    }
    #[test]
    fn oversized_records_and_missing_objects_are_typed_needs_without_completion() {
        let dir = tempfile::tempdir().unwrap();
        let mut input = input(
            dir.path(),
            Origin::MicrosoftBuildings,
            &"x".repeat(MAX_RECORD + 1),
        );
        assert!(matches!(
            produce(&dir.path().join("held"), &scope(), &input),
            Err(Error::TooLarge(_))
        ));
        input.resources[0].path = dir.path().join("missing.gz");
        assert!(matches!(
            produce(&dir.path().join("held"), &scope(), &input),
            Err(Error::Required(_))
        ));
    }
}
