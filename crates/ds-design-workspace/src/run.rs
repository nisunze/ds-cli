use super::*;
use ds_command_kernel::design::SourceRole;
use ds_network::network::{
    native_fast_lv::decode_native_fast_lv_request, process_lv_transformer, LvProcessInput,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct CapturedResult<'a> {
    #[serde(borrow)]
    output: CapturedOutput<'a>,
}
#[derive(Deserialize)]
struct CapturedOutput<'a> {
    #[serde(borrow)]
    voltage_drop: Option<&'a serde_json::value::RawValue>,
}

/// Read the producer's retained member without serializing it again. Admission
/// hashes the captured final layers/settings; it never runs network processing.
fn saved_voltage_drop(
    bytes: &[u8],
    result: &Value,
    transformer: &str,
    revision: &str,
) -> Result<Vec<u8>> {
    if result["input_revision"] != revision {
        return Err(invalid("voltage_drop_result_stale: result input revision differs from the independently captured job revision"));
    }
    let captured: CapturedResult<'_> = serde_json::from_slice(bytes)?;
    let raw = captured.output.voltage_drop.ok_or_else(|| {
        invalid("voltage_drop_result_missing: captured run has no saved producer analysis")
    })?;
    if raw.get().len() > 64 * 1024 * 1024 {
        return Err(invalid(
            "voltage_drop_result_invalid: saved analysis exceeds 64 MiB",
        ));
    }
    let analysis: ds_network::network::voltage_drop::VoltageDropAnalysis =
        serde_json::from_str(raw.get()).map_err(|error| {
            invalid(format!(
                "voltage_drop_result_invalid: saved analysis: {error}"
            ))
        })?;
    if result["schema"] != "ds.design.process-result/v1"
        || result["transformer"] != transformer
        || analysis.schema != ds_network::network::voltage_drop::VOLTAGE_DROP_ANALYSIS_SCHEMA_V1
        || [
            &analysis.inputs_digest,
            &analysis.analysis_inputs_digest,
            &analysis.processed_digest,
        ]
        .iter()
        .any(|digest| digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(invalid("voltage_drop_result_invalid: saved result identity, schema or binding digests are invalid"));
    }
    let layers = result["output"]["gdfs"]
        .as_object()
        .ok_or_else(|| invalid("voltage_drop_result_invalid: saved result has no final layers"))?;
    let settings =
        serde_json::from_value(result["output"]["settings_used"].clone()).map_err(|error| {
            invalid(format!(
                "voltage_drop_result_invalid: saved settings: {error}"
            ))
        })?;
    if !analysis.matches_processed_layers(layers, &settings) {
        return Err(invalid("voltage_drop_result_stale: saved analysis does not bind the captured final layers/settings"));
    }
    let tr = result["output"]["gdfs"]["tr"]["features"]
        .as_array()
        .filter(|features| features.len() == 1)
        .ok_or_else(|| invalid("voltage_drop_result_invalid: saved layers need one transformer"))?;
    if tr[0]["properties"]
        .get("transfo")
        .is_some_and(|name| name != transformer)
    {
        return Err(invalid(
            "voltage_drop_result_invalid: saved transformer differs from the report identity",
        ));
    }
    if tr[0]["properties"]["vd_summary"] != analysis.summary {
        return Err(invalid(
            "voltage_drop_result_stale: saved transformer summary differs from producer analysis",
        ));
    }
    Ok(raw.get().as_bytes().to_vec())
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RunReceipt {
    pub run_id: String,
    pub workers: usize,
    pub completed: usize,
    pub failed: usize,
    pub pending: usize,
}

pub(crate) fn compute_input(snapshot: &Snapshot) -> Result<LvProcessInput> {
    // Reuse the engine's closed validation of settings, config and layer bounds.
    let wire = json!({"schema":"ds.fast-lv.request/v1","jobs":[{"transformer_name":snapshot.transformer,
        "gdfs":snapshot.layers,"settings":snapshot.settings,"config_dfs":snapshot.network_config.sheets}]});
    let mut decoded =
        decode_native_fast_lv_request(&serde_json::to_vec(&wire)?).map_err(invalid)?;
    let job = decoded.jobs.remove(0);
    let mut customer_sources = Vec::new();
    let mut pole_sources = Vec::new();
    let mut tables = BTreeMap::new();
    for source in &snapshot.sources {
        let address = format!("snapshot:{}", source.id);
        tables.insert(address.clone(), source.collection.clone());
        match source.role {
            SourceRole::Customers|SourceRole::Buildings=>customer_sources.push(serde_json::from_value(json!({"address":address, "endpoint":if matches!(source.role,SourceRole::Buildings){"customers_from_buildings"}else{"customers_from_table"},"label":source.id}))?),
            SourceRole::Poles=>pole_sources.push(serde_json::from_value(json!({"address":address,"label":source.id}))?),
        }
    }
    Ok(LvProcessInput {
        ds_project: "__offline_design__".into(),
        transformer_name: job.transformer_name,
        gdfs: job.gdfs,
        settings: job.settings,
        config_dfs: job.config_dfs,
        customer_sources,
        pole_sources,
        include_design_customers: snapshot.include_design_customers,
        local_tables: tables,
        differential: None,
    })
}

impl Workspace {
    /// Capture the exact input revisions before a worker is launched. Resuming
    /// the same run never substitutes edits made after this transaction.
    pub fn prepare_run(&mut self, id: &str, transformers: &[String]) -> Result<Value> {
        design::identifier(id).map_err(invalid)?;
        if transformers.is_empty() || transformers.len() > 32 {
            return Err(invalid("select 1..32 explicit transformers"));
        }
        let unique: std::collections::BTreeSet<_> = transformers.iter().collect();
        if unique.len() != transformers.len() {
            return Err(invalid("transformers must be unique"));
        }
        let selection = serde_json::to_string(transformers)?;
        let current_engine = engine_identity()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<(String, String)> = tx
            .query_row("SELECT selection,engine FROM runs WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        if let Some((old, engine)) = existing {
            if old != selection || engine != current_engine {
                return Err(Error::Conflict(
                    "run id already pins different selection or engine".into(),
                ));
            }
            return Ok(json!({"run_id":id,"captured":true,"resumed":true}));
        }
        tx.execute(
            "INSERT INTO runs(id,selection,engine) VALUES(?1,?2,?3)",
            params![id, selection, current_engine],
        )?;
        let mut total = 0_i64;
        for (ordinal, name) in transformers.iter().enumerate() {
            let (revision,size):(String,i64)=tx.query_row("SELECT h.revision,length(r.document) FROM heads h JOIN revisions r ON r.revision=h.revision WHERE h.transformer=?1",[name],|r|Ok((r.get(0)?,r.get(1)?)))
                .optional()?.ok_or_else(||invalid(format!("transformer missing: {name}")))?;
            total += size;
            if total > design::MAX_BYTES as i64 {
                return Err(invalid("run input exceeds 64 MiB; split the selection"));
            }
            tx.execute("INSERT INTO jobs(run,ordinal,transformer,revision,state) VALUES(?1,?2,?3,?4,'pending')",params![id,ordinal as i64,name,revision])?;
        }
        tx.commit()?;
        Ok(
            json!({"run_id":id,"captured":true,"resumed":false,"jobs":transformers.len(),"input_bytes":total}),
        )
    }
    pub fn cancel(&mut self, id: &str) -> Result<Value> {
        if self
            .db
            .execute("UPDATE runs SET cancel=1 WHERE id=?1", [id])?
            != 1
        {
            return Err(invalid("run does not exist"));
        }
        Ok(json!({"run_id":id,"cancellation":"requested_at_job_boundaries"}))
    }
    pub fn run_status(&self, id: &str) -> Result<Value> {
        let cancel: i64 = self
            .db
            .query_row("SELECT cancel FROM runs WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| invalid("run does not exist"))?;
        let mut statement=self.db.prepare("SELECT transformer,revision,state,result_sha256 FROM jobs WHERE run=?1 ORDER BY ordinal")?;
        let rows=statement.query_map([id],|r|Ok(json!({"transformer":r.get::<_,String>(0)?,"input_revision":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"result_sha256":r.get::<_,Option<String>>(3)?})))?
            .collect::<std::result::Result<Vec<_>,_>>()?;
        Ok(json!({"run_id":id,"cancel_requested":cancel!=0,"jobs":rows}))
    }
    pub fn process(&mut self, id: &str, workers: usize) -> Result<RunReceipt> {
        let available = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        if workers == 0 || workers > available {
            return Err(invalid(format!("workers must be 1..{available}")));
        }
        let lock_path = self.root.join("compute.lock");
        if lock_path.symlink_metadata().is_ok() {
            regular(&lock_path)?;
        }
        let lock = private_options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock_exclusive()
            .map_err(|_| Error::Busy("another workspace compute worker is running".into()))?;
        let engine: String = self
            .db
            .query_row("SELECT engine FROM runs WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| invalid("run does not exist"))?;
        if engine != engine_identity()? {
            return Err(Error::Conflict(
                "captured run requires its original engine".into(),
            ));
        }
        // Holding the compute lock proves a previous worker is no longer alive.
        self.db.execute(
            "UPDATE jobs SET state='pending' WHERE run=?1 AND state='running'",
            [id],
        )?;
        let mut stmt=self.db.prepare("SELECT transformer,revision FROM jobs WHERE run=?1 AND state='pending' ORDER BY ordinal")?;
        let jobs = stmt
            .query_map([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);
        let width = workers.min(jobs.len().max(1));
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(width)
            .build()
            .map_err(invalid)?;
        let root = &self.root;
        pool.install(|| {
            jobs.par_iter()
                .try_for_each(|(name, revision)| process_job(root, id, name, revision))
        })?;
        let count = |state: &str| -> Result<usize> {
            Ok(self.db.query_row(
                "SELECT count(*) FROM jobs WHERE run=?1 AND state=?2",
                params![id, state],
                |r| r.get::<_, i64>(0),
            )? as usize)
        };
        Ok(RunReceipt {
            run_id: id.into(),
            workers: width,
            completed: count("completed")?,
            failed: count("failed")?,
            pending: count("pending")?,
        })
    }
    pub fn export_result(&self, id: &str, transformer: &str, out: &Path) -> Result<Value> {
        let (bytes, sha, _) = self.result(id, transformer)?;
        write_new(out, &bytes)?;
        Ok(
            json!({"run_id":id,"transformer":transformer,"out":out,"sha256":sha,"bytes":bytes.len()}),
        )
    }
    fn result(&self, id: &str, transformer: &str) -> Result<(Vec<u8>, String, String)> {
        let result:Option<(Vec<u8>,String,String)>=self.db.query_row("SELECT result,result_sha256,revision FROM jobs WHERE run=?1 AND transformer=?2 AND result IS NOT NULL",params![id,transformer],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let (bytes, sha, revision) = result.ok_or_else(|| invalid("job has no result"))?;
        if format!("{:x}", Sha256::digest(&bytes)) != sha {
            return Err(invalid("result digest mismatch"));
        }
        Ok((bytes, sha, revision))
    }
    /// Publish an immutable report-input directory. This deliberately does not
    /// accept the current room: reporting always consumes a captured run result.
    pub fn report_inputs(&self, id: &str, transformer: &str, out: &Path) -> Result<Value> {
        if out.symlink_metadata().is_ok() {
            return Err(Error::Conflict(
                "report input directory already exists".into(),
            ));
        }
        let (bytes, sha, captured_revision) = self.result(id, transformer)?;
        let result: Value = serde_json::from_slice(&bytes)?;
        let analysis = saved_voltage_drop(&bytes, &result, transformer, &captured_revision)?;
        let analysis_sha = format!("{:x}", Sha256::digest(&analysis));
        let layers = result["output"]["gdfs"]
            .as_object()
            .ok_or_else(|| invalid("failed job cannot produce reports"))?;
        let revision = result["input_revision"]
            .as_str()
            .ok_or_else(|| invalid("missing input revision"))?;
        let (_, snapshot) = self.read(transformer, Some(revision))?;
        let parent = out
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let stage = tempfile::tempdir_in(parent)?;
        let files = [
            (
                "transformer.json",
                serde_json::to_vec(&json!({"layers":layers}))?,
            ),
            (
                "network-config.json",
                serde_json::to_vec(&snapshot.network_config)?,
            ),
            ("voltage-drop.json", analysis),
        ];
        let mut hashes = BTreeMap::new();
        for (name, bytes) in files {
            write_new(&stage.path().join(name), &bytes)?;
            hashes.insert(name, format!("{:x}", Sha256::digest(&bytes)));
        }
        let receipt = json!({"schema":"ds.design.report-inputs/v1","run_id":id,"transformer":transformer,
            "input_revision":revision,"result_sha256":sha,"files":hashes,"input_shape":"plain_local",
            "layers_sha256":design::digest(layers).map_err(invalid)?,
            "voltage_drop_result":{"path":out.join("voltage-drop.json"),"sha256":analysis_sha}});
        write_new(
            &stage.path().join("manifest.json"),
            &serde_json::to_vec(&receipt)?,
        )?;
        fs::rename(stage.path(), out)?;
        sync_dir(parent)?;
        Ok(receipt)
    }

    /// Retain exact successful report bytes locally before queuing publication.
    /// A partial export stays partial; it never becomes a complete delivery.
    pub fn retain_report(
        &mut self,
        id: &str,
        transformer: &str,
        root: &Path,
        report: &Value,
    ) -> Result<Value> {
        if report["transformer"] != transformer || report["status"] != "completed" {
            return Err(invalid(
                "only a completed matching report can be queued; inspect partial blockers",
            ));
        }
        let (result_bytes, result_sha, captured_revision) = self.result(id, transformer)?;
        let result: Value = serde_json::from_slice(&result_bytes)?;
        let analysis = saved_voltage_drop(&result_bytes, &result, transformer, &captured_revision)?;
        if read_file(&root.join("voltage-drop.json"), 64 * 1024 * 1024)? != analysis {
            return Err(invalid(
                "voltage_drop_result_stale: report analysis changed after capture",
            ));
        }
        let revision = result["input_revision"]
            .as_str()
            .ok_or_else(|| invalid("missing input revision"))?;
        let (_, snapshot) = self.read(transformer, Some(revision))?;
        for (name, expected) in [
            (
                "transformer.json",
                json!({"layers":result["output"]["gdfs"]}),
            ),
            (
                "network-config.json",
                serde_json::to_value(&snapshot.network_config)?,
            ),
        ] {
            let bytes = read_file(&root.join(name), 256 * 1024 * 1024)?;
            if bytes != serde_json::to_vec(&expected)? {
                return Err(invalid("report input changed after capture"));
            }
        }
        let requested: Vec<String> = serde_json::from_value(report["formats_requested"].clone())
            .map_err(|_| invalid("report has no exact requested formats"))?;
        let expected = ds_command_kernel::report_formats::normalize(&requested);
        let expected: std::collections::BTreeSet<_> = expected.iter().map(String::as_str).collect();
        let requested_set: std::collections::BTreeSet<_> =
            requested.iter().map(String::as_str).collect();
        if requested.len() != requested_set.len() || requested_set != expected {
            return Err(invalid(
                "report selection must include the mandatory saved voltage-drop JSON",
            ));
        }
        let root = fs::canonicalize(root.join("artifacts"))?;
        let artifacts = report["artifacts"]
            .as_array()
            .ok_or_else(|| invalid("report has no artifact list"))?;
        if artifacts.is_empty() || artifacts.len() > 8 {
            return Err(invalid("expected 1..8 report artifacts"));
        }
        let mut files = Vec::new();
        let mut total = 0;
        let mut names = std::collections::BTreeSet::new();
        let mut formats = std::collections::BTreeSet::new();
        let mut records = Vec::new();
        for artifact in artifacts {
            let format = artifact["format"]
                .as_str()
                .ok_or_else(|| invalid("artifact format missing"))?;
            if !expected.contains(format) || !formats.insert(format) {
                return Err(invalid(
                    "report has an unrequested or duplicate artifact format",
                ));
            }
            let filename = artifact["filename"]
                .as_str()
                .ok_or_else(|| invalid("artifact filename missing"))?;
            design::identifier(filename).map_err(invalid)?;
            if ds_command_kernel::report_formats::report_filename(transformer, format)
                .map_err(invalid)?
                != filename
            {
                return Err(invalid(
                    "artifact filename differs from its transformer and format",
                ));
            }
            if !names.insert(filename) {
                return Err(invalid("duplicate report artifact filename"));
            }
            let path = Path::new(
                artifact["path"]
                    .as_str()
                    .ok_or_else(|| invalid("artifact path missing"))?,
            );
            regular(path)?;
            if fs::canonicalize(path)? != root.join(filename) {
                return Err(invalid("artifact escapes report directory"));
            }
            let bytes = read_file(path, 256 * 1024 * 1024 - total)?;
            total += bytes.len();
            let sha = format!("{:x}", Sha256::digest(&bytes));
            if artifact["sha256"] != sha || artifact["bytes"].as_u64() != Some(bytes.len() as u64) {
                return Err(invalid("report artifact digest or size mismatch"));
            }
            if format == ds_command_kernel::report_formats::MANDATORY_INDIVIDUAL_OUTPUT
                && bytes != analysis
            {
                return Err(invalid("voltage_drop_result_stale: delivered JSON differs from the saved producer source"));
            }
            files.push((sha, bytes));
            records.push(json!({"filename":filename,"format":artifact["format"],"bytes":artifact["bytes"],"sha256":artifact["sha256"]}));
        }
        if formats != expected {
            return Err(invalid("completed report is missing a requested artifact or mandatory saved voltage-drop JSON"));
        }
        let manifest = json!({"schema":"ds.design.delivery/v1","run_id":id,"transformer":transformer,"input_revision":revision,
            "result_sha256":result_sha,"country":report["country"],"formats_requested":report["formats_requested"],"artifacts":records});
        let digest = design::digest(&manifest).map_err(invalid)?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (sha, bytes) in files {
            tx.execute(
                "INSERT OR IGNORE INTO artifacts VALUES(?1,?2)",
                params![sha, bytes],
            )?;
        }
        let inserted = tx.execute(
            "INSERT OR IGNORE INTO deliveries VALUES(?1,?2,?3,?4)",
            params![digest, id, transformer, serde_json::to_vec(&manifest)?],
        )?;
        if inserted != 0 {
            tx.execute("INSERT INTO outbox(operation_id,transformer,base_revision,revision,kind) VALUES(?1,?2,?3,?4,'report_artifacts')",params![format!("report:{digest}"),transformer,revision,digest])?;
        }
        tx.commit()?;
        Ok(
            json!({"run_id":id,"transformer":transformer,"delivery_sha256":digest,"artifact_count":artifacts.len(),"retained_bytes":total,"publication":"pending_transport"}),
        )
    }
}

// A package version alone is not a computation identity: two engine builds
// can share 0.1.0. Pin the exact native host executable before capture/resume.
fn executable_digest() -> Result<String> {
    static ID: std::sync::OnceLock<std::result::Result<String, String>> =
        std::sync::OnceLock::new();
    ID.get_or_init(|| {
        let path = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut file = File::open(path).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 65536];
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        Ok(format!("{:x}", hash.finalize()))
    })
    .clone()
    .map_err(Error::Io)
}
fn engine_identity() -> Result<String> {
    Ok(format!(
        "{}:{}",
        ds_network::CORE_VERSION,
        executable_digest()?
    ))
}

fn process_job(root: &Path, id: &str, name: &str, revision: &str) -> Result<()> {
    let mut workspace = Workspace::open(root)?;
    let cancel: i64 = workspace
        .db
        .query_row("SELECT cancel FROM runs WHERE id=?1", [id], |r| r.get(0))?;
    if cancel != 0 {
        return Ok(());
    }
    let (_, snapshot) = workspace.read(name, Some(revision))?;
    workspace.db.execute(
        "UPDATE jobs SET state='running' WHERE run=?1 AND transformer=?2",
        params![id, name],
    )?;
    let result = process_lv_transformer(compute_input(&snapshot)?);
    let (state, document) = match result {
        Ok(output) => (
            "completed",
            json!({"schema":"ds.design.process-result/v1","transformer":name,"input_revision":revision,
                    "engine_version":ds_network::CORE_VERSION,"worker_executable_sha256":executable_digest()?,"output":output}),
        ),
        Err(error) => (
            "failed",
            json!({"schema":"ds.design.process-result/v1","transformer":name,"input_revision":revision,"error":error.to_string()}),
        ),
    };
    let bytes = serde_json::to_vec(&document)?;
    if bytes.len() > 256 * 1024 * 1024 {
        return Err(invalid("processed result exceeds 256 MiB"));
    }
    let sha = format!("{:x}", Sha256::digest(&bytes));
    let tx = workspace
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "UPDATE jobs SET state=?1,result=?2,result_sha256=?3 WHERE run=?4 AND transformer=?5",
        params![state, bytes, sha, id, name],
    )?;
    // A computation is retained independently of the user's current edits.
    // Publication must explicitly reconcile its pinned input revision.
    if state == "completed" {
        let op = format!(
            "run:{}",
            design::digest(&json!([id, name])).map_err(invalid)?
        );
        tx.execute("INSERT INTO outbox(operation_id,transformer,base_revision,revision,kind) VALUES(?1,?2,?3,?4,'process_result')",params![op,name,revision,sha])?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod report_source_tests {
    use super::*;

    fn captured() -> (tempfile::TempDir, Workspace, Value) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        Workspace::init(&root, "test-project").unwrap();
        let mut workspace = Workspace::open(&root).unwrap();
        let snapshot = json!({"schema":"ds.design.snapshot/v1","transformer":"T1","crs":"EPSG:4326",
            "layers":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"tr","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"name":"T1","names":"T1"}}]},
                "lv_lines":{"type":"FeatureCollection","features":[{"type":"Feature","id":"line","geometry":{"type":"LineString","coordinates":[[30.0,-2.0],[30.0004,-2.0]]},"properties":{}}]}},
            "settings":{},"network_config":{"sheets":{}},"sources":[],"include_design_customers":true});
        workspace
            .write(&serde_json::to_vec(&snapshot).unwrap(), None, "create")
            .unwrap();
        workspace.prepare_run("run", &["T1".into()]).unwrap();
        assert_eq!(workspace.process("run", 1).unwrap().completed, 1);
        let (bytes, _, _) = workspace.result("run", "T1").unwrap();
        let result = serde_json::from_slice(&bytes).unwrap();
        (temp, workspace, result)
    }

    // A synthetic retained transport fixture, including historical/damaged
    // variants. No test repair or export reruns the producer.
    fn store(workspace: &Workspace, bytes: &[u8]) {
        workspace
            .db
            .execute(
                "UPDATE jobs SET result=?1,result_sha256=?2 WHERE run='run' AND transformer='T1'",
                params![bytes, format!("{:x}", Sha256::digest(bytes))],
            )
            .unwrap();
    }

    #[test]
    fn report_capture_keeps_exact_saved_json_member_and_final_layer_pin() {
        let (temp, workspace, result) = captured();
        let bytes = serde_json::to_vec_pretty(&result).unwrap();
        store(&workspace, &bytes);
        let raw: CapturedResult<'_> = serde_json::from_slice(&bytes).unwrap();
        let source = raw.output.voltage_drop.unwrap().get().as_bytes();
        assert!(source.contains(&b'\n'));
        let before = workspace.outbox(0, 100).unwrap();
        let out = temp.path().join("report");
        let receipt = workspace.report_inputs("run", "T1", &out).unwrap();
        assert_eq!(fs::read(out.join("voltage-drop.json")).unwrap(), source);
        assert_eq!(
            receipt["voltage_drop_result"]["sha256"],
            format!("{:x}", Sha256::digest(source))
        );
        assert_eq!(
            receipt["files"]["voltage-drop.json"],
            receipt["voltage_drop_result"]["sha256"]
        );
        assert_eq!(
            receipt["layers_sha256"],
            design::digest(&result["output"]["gdfs"]).unwrap()
        );
        assert_eq!(workspace.result("run", "T1").unwrap().0, bytes);
        assert_eq!(workspace.outbox(0, 100).unwrap(), before);
    }

    #[test]
    fn missing_invalid_and_stale_saved_analysis_refuse_before_staging() {
        let (temp, mut workspace, original) = captured();
        let changed = workspace.edit(&serde_json::to_vec(&json!({
            "schema":"ds.design.edit/v1","transformer":"T1","expected_revision":original["input_revision"],
            "operation_id":"later-edit","mutations":[{"kind":"set_properties","layer":"lv_lines","ids":["line"],"values":{"note":"later held revision"}}]
        })).unwrap()).unwrap();
        assert!(workspace.read("T1", changed["revision"].as_str()).is_ok());
        let before = workspace.outbox(0, 100).unwrap();
        for (case, code) in [
            ("missing", "voltage_drop_result_missing"),
            ("null", "voltage_drop_result_missing"),
            ("schema", "voltage_drop_result_invalid"),
            ("state", "voltage_drop_result_invalid"),
            ("identity", "voltage_drop_result_invalid"),
            ("digest", "voltage_drop_result_stale"),
            ("layers", "voltage_drop_result_stale"),
            ("revision", "voltage_drop_result_stale"),
            ("settings", "voltage_drop_result_stale"),
            ("summary", "voltage_drop_result_stale"),
        ] {
            let mut result = original.clone();
            match case {
                "missing" => {
                    result["output"]
                        .as_object_mut()
                        .unwrap()
                        .remove("voltage_drop");
                }
                "null" => result["output"]["voltage_drop"] = Value::Null,
                "schema" => result["output"]["voltage_drop"]["schema"] = json!("unknown"),
                "state" => result["output"]["voltage_drop"]["status"] = json!("held"),
                "identity" => result["transformer"] = json!("T2"),
                "digest" => {
                    result["output"]["voltage_drop"]["processed_digest"] = json!("0".repeat(64))
                }
                "layers" => {
                    result["output"]["gdfs"]["tr"]["features"][0]["geometry"]["coordinates"] =
                        json!([30.1, -2.0])
                }
                "revision" => result["input_revision"] = changed["revision"].clone(),
                "settings" => {
                    let previous = result["output"]["settings_used"]["keep_lv_lines_topology"]
                        .as_bool()
                        .unwrap();
                    result["output"]["settings_used"]["keep_lv_lines_topology"] = json!(!previous);
                }
                "summary" => {
                    result["output"]["voltage_drop"]["summary"]["calculation"] =
                        json!("changed after capture")
                }
                _ => unreachable!(),
            }
            let bytes = serde_json::to_vec(&result).unwrap();
            store(&workspace, &bytes);
            let out = temp.path().join(case);
            let error = workspace.report_inputs("run", "T1", &out).unwrap_err();
            assert!(error.to_string().starts_with(code), "{case}: {error}");
            assert!(!out.exists(), "{case}");
            if case == "revision" {
                let report = json!({"transformer":"T1","status":"completed"});
                let error = workspace
                    .retain_report("run", "T1", &out, &report)
                    .unwrap_err();
                assert!(error.to_string().starts_with(code), "retention: {error}");
            }
            assert_eq!(workspace.result("run", "T1").unwrap().0, bytes);
            assert_eq!(workspace.outbox(0, 100).unwrap(), before);
        }
    }

    fn artifact(root: &Path, format: &str, bytes: &[u8]) -> Value {
        let filename = ds_command_kernel::report_formats::report_filename("T1", format).unwrap();
        let path = root.join("artifacts").join(&filename);
        fs::write(&path, bytes).unwrap();
        json!({"format":format,"filename":filename,"path":path,"bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(bytes))})
    }

    #[test]
    fn delivery_requires_complete_selection_and_exact_saved_json_before_outbox() {
        let (temp, mut workspace, _) = captured();
        let root = temp.path().join("report");
        workspace.report_inputs("run", "T1", &root).unwrap();
        fs::create_dir(root.join("artifacts")).unwrap();
        let source = fs::read(root.join("voltage-drop.json")).unwrap();
        let layers = fs::read(root.join("transformer.json")).unwrap();
        let report = json!({"transformer":"T1","status":"completed","country":"Test",
            "formats_requested":["xlsx","voltage_drop"],
            "artifacts":[artifact(&root,"xlsx",b"test workbook bytes"),artifact(&root,"voltage_drop",&source)]});
        let before = workspace.outbox(0, 100).unwrap();
        for case in [
            "no_mandatory_selection",
            "missing_artifact",
            "duplicate",
            "extra",
            "identity",
            "source_artifact",
            "source_input",
            "layers",
            "digest",
        ] {
            let mut bad = report.clone();
            match case {
                "no_mandatory_selection" => bad["formats_requested"] = json!(["xlsx"]),
                "missing_artifact" => {
                    bad["artifacts"].as_array_mut().unwrap().pop();
                }
                "duplicate" => {
                    let duplicate = bad["artifacts"][0].clone();
                    bad["artifacts"].as_array_mut().unwrap().push(duplicate);
                }
                "extra" => bad["artifacts"]
                    .as_array_mut()
                    .unwrap()
                    .push(artifact(&root, "geojson", b"extra")),
                "identity" => bad["transformer"] = json!("T2"),
                "source_artifact" => {
                    let mut altered = source.clone();
                    altered.push(b'\n');
                    bad["artifacts"][1] = artifact(&root, "voltage_drop", &altered);
                }
                "source_input" => fs::write(root.join("voltage-drop.json"), b"{}").unwrap(),
                "layers" => fs::write(root.join("transformer.json"), b"{}").unwrap(),
                "digest" => bad["artifacts"][0]["sha256"] = json!("0".repeat(64)),
                _ => unreachable!(),
            }
            assert!(
                workspace.retain_report("run", "T1", &root, &bad).is_err(),
                "{case}"
            );
            assert_eq!(workspace.outbox(0, 100).unwrap(), before, "{case}");
            fs::write(root.join("voltage-drop.json"), &source).unwrap();
            fs::write(root.join("transformer.json"), &layers).unwrap();
            artifact(&root, "voltage_drop", &source);
        }
        assert_eq!(
            workspace
                .retain_report("run", "T1", &root, &report)
                .unwrap()["artifact_count"],
            2
        );
        let after = workspace.outbox(0, 100).unwrap();
        assert_eq!(
            after["rows"].as_array().unwrap().len(),
            before["rows"].as_array().unwrap().len() + 1
        );
        workspace
            .retain_report("run", "T1", &root, &report)
            .unwrap();
        assert_eq!(workspace.outbox(0, 100).unwrap(), after);
    }
}
