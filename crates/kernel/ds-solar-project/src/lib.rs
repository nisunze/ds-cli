//! The local Solar project owns inputs, closed local runs and authored-input publication.
//! This crate has no HTTP client, UI, map, credential or Solar engine dependency;
//! a local run's calculation is supplied by its host. A separate authenticated
//! sender acknowledges exact outbox rows after server acceptance.
use anyhow::{Result, ensure};
use ds_solar_contracts::SolarCityInput;
pub use ds_solar_contracts::is_solar_id as identifier;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub mod clean;
pub mod control;
pub mod files;
pub mod intake;
pub mod network;
pub mod preparation;
mod records;
pub mod reference;
pub mod run;
pub mod sync;

const SCHEMA: &str = "ds-solar.project-workspace/v1";
pub const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_CITIES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectIdentity {
    pub project_id: String,
    pub root: String,
}

impl ProjectIdentity {
    pub fn validate(&self) -> Result<()> {
        ensure!(identifier(&self.project_id), "invalid Solar project id");
        ensure!(
            self.root == format!("eds_project/{}/eds_solar", self.project_id),
            "Solar root must belong to the project"
        );
        Ok(())
    }
}

pub fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub struct Workspace {
    pub(crate) db: Connection,
    pub(crate) path: PathBuf,
    pub identity: ProjectIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingUpload {
    pub sequence: i64,
    pub kind: String,
    pub identity: String,
    pub digest: String,
    pub payload: Value,
    pub attempts: u32,
    pub error: Option<String>,
}

impl Workspace {
    /// A local project identity is attribution, never permission to publish.
    /// Publication separately binds to a restored user's selected project.
    pub fn initialize(path: &Path, identity: ProjectIdentity) -> Result<Self> {
        identity.validate()?;
        ensure!(
            fs::symlink_metadata(path).is_err(),
            "Solar workspace already exists"
        );
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let staging = tempfile::Builder::new()
            .prefix(".solar-project-")
            .tempdir_in(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(staging.path(), fs::Permissions::from_mode(0o700))?;
        }
        let db = Self::connection(staging.path(), true)?;
        db.execute_batch("BEGIN IMMEDIATE;
            CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE cities(id TEXT PRIMARY KEY, digest TEXT NOT NULL, input TEXT NOT NULL);
            CREATE TABLE runs(id TEXT PRIMARY KEY, status TEXT NOT NULL, request TEXT NOT NULL, receipt TEXT, error TEXT);
            CREATE TABLE outbox(sequence INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, identity TEXT NOT NULL, digest TEXT NOT NULL, payload TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending', attempts INTEGER NOT NULL DEFAULT 0, error TEXT, receipt TEXT);
            CREATE INDEX outbox_pending ON outbox(status,sequence);
")?;
        db.execute(
            "INSERT INTO meta VALUES('schema',?1),('identity',?2)",
            params![SCHEMA, serde_json::to_string(&identity)?],
        )?;
        db.execute_batch("COMMIT;")?;
        files::private_dir(&staging.path().join("runs"))?;
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(db);
        files::sync_dir(staging.path())?;
        fs::rename(staging.path(), path)?;
        files::sync_dir(parent)?;
        Self::open(path)
    }

    pub fn open(path: &Path) -> Result<Self> {
        let db = Self::connection(path, false)?;
        let schema: String =
            db.query_row("SELECT value FROM meta WHERE key='schema'", [], |r| {
                r.get(0)
            })?;
        ensure!(schema == SCHEMA, "unsupported Solar workspace schema");
        let raw: String = db.query_row("SELECT value FROM meta WHERE key='identity'", [], |r| {
            r.get(0)
        })?;
        let identity: ProjectIdentity = serde_json::from_str(&raw)?;
        identity.validate()?;
        Ok(Self {
            db,
            path: fs::canonicalize(path)?,
            identity,
        })
    }

    fn connection(path: &Path, create: bool) -> Result<Connection> {
        files::ordinary_dir(path)?;
        let file = path.join("solar.sqlite");
        for suffix in ["solar.sqlite", "solar.sqlite-wal", "solar.sqlite-shm"] {
            let p = path.join(suffix);
            if fs::symlink_metadata(&p).is_ok() {
                files::ordinary_file(&p)?;
            }
        }
        let flags = OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_READ_WRITE
            | if create {
                OpenFlags::SQLITE_OPEN_CREATE
            } else {
                OpenFlags::empty()
            };
        let db = Connection::open_with_flags(&file, flags)?;
        files::private_file(&file)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
        )?;
        Ok(db)
    }

    /// Seed complete locally held city records atomically with their upload
    /// intents. Existing records require their exact previous content digest.
    pub fn seed(
        &mut self,
        inputs: &[SolarCityInput],
        expected: &std::collections::BTreeMap<String, String>,
    ) -> Result<Value> {
        ensure!(
            !inputs.is_empty() && inputs.len() <= MAX_CITIES,
            "seed requires 1..64 city records"
        );
        let mut unique = BTreeSet::new();
        let mut encoded = Vec::new();
        for input in inputs {
            input.validate()?;
            ensure!(
                identifier(&input.identity.city_id)
                    && unique.insert(input.identity.city_id.clone()),
                "duplicate or invalid Solar city id"
            );
            ensure!(
                input.identity.project_id == self.identity.project_id
                    && input.identity.root == self.identity.root,
                "seed city belongs to another project"
            );
            let body = serde_json::to_string(input)?;
            ensure!(body.len() <= MAX_INPUT_BYTES, "seed city exceeds 16 MiB");
            encoded.push(body);
        }
        ensure!(
            expected.keys().all(|id| unique.contains(id)),
            "expected revisions include an unselected city"
        );
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut changed = Vec::new();
        for (input, body) in inputs.iter().zip(encoded) {
            let id = &input.identity.city_id;
            let prior: Option<String> = tx
                .query_row("SELECT digest FROM cities WHERE id=?1", [id], |r| r.get(0))
                .optional()?;
            if prior.as_deref() == Some(&input.content_digest) {
                continue;
            }
            ensure!(
                prior.as_ref() == expected.get(id),
                "Solar city revision conflict: {id}"
            );
            tx.execute("INSERT INTO cities VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET digest=excluded.digest,input=excluded.input", params![id,input.content_digest,body])?;
            tx.execute("INSERT OR IGNORE INTO outbox(kind,identity,digest,payload) VALUES('city',?1,?2,?3)", params![id,input.content_digest,body])?;
            changed.push(json!({"city_id":id,"content_digest":input.content_digest}));
        }
        tx.commit()?;
        Ok(
            json!({"project_id":self.identity.project_id,"changed":changed,"local":true,"publication":"pending"}),
        )
    }

    pub fn cities(&self, requested: &[String]) -> Result<Vec<SolarCityInput>> {
        ensure!(requested.len() <= MAX_CITIES, "city selection exceeds 64");
        let wanted: BTreeSet<_> = requested.iter().collect();
        ensure!(
            wanted.len() == requested.len() && requested.iter().all(|s| identifier(s)),
            "duplicate or invalid city selection"
        );
        let mut statement = self.db.prepare("SELECT id,input FROM cities ORDER BY id")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut result = Vec::new();
        for row in rows {
            let (id, body) = row?;
            if !wanted.is_empty() && !wanted.contains(&id) {
                continue;
            }
            ensure!(body.len() <= MAX_INPUT_BYTES, "stored city exceeds 16 MiB");
            let input = records::Record::decode(serde_json::from_str(&body)?)?.input()?;
            ensure!(
                input.identity.city_id == id
                    && input.identity.project_id == self.identity.project_id
                    && input.identity.root == self.identity.root,
                "stored city identity mismatch"
            );
            result.push(input);
            ensure!(
                result.len() <= MAX_CITIES,
                "select at most 64 cities explicitly"
            );
        }
        if !wanted.is_empty() {
            ensure!(
                result.len() == wanted.len(),
                "selected city is not seeded locally"
            );
        }
        Ok(result)
    }

    pub fn pending(&self, limit: usize) -> Result<Vec<PendingUpload>> {
        self.pending_scope(limit, false)
    }
    pub fn pending_scope(&self, limit: usize, inputs_only: bool) -> Result<Vec<PendingUpload>> {
        self.pending_selected(limit, inputs_only, None)
    }
    pub fn pending_selected(
        &self,
        limit: usize,
        inputs_only: bool,
        run_id: Option<&str>,
    ) -> Result<Vec<PendingUpload>> {
        ensure!(
            !(inputs_only && run_id.is_some()),
            "choose inputs-only or one run"
        );
        if let Some(id) = run_id {
            self.run_result(id)?;
        }
        ensure!((1..=100).contains(&limit), "outbox limit must be 1..100");
        let mut stmt = self.db.prepare("SELECT sequence,kind,identity,digest,payload,attempts,error FROM outbox WHERE status='pending' AND (?2=0 OR kind='city') AND (?3 IS NULL OR kind='city' OR identity=?3) ORDER BY sequence LIMIT ?1")?;
        let raw = stmt.query_map(params![limit, inputs_only, run_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get::<_, String>(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?;
        raw.map(|row| {
            let (sequence, kind, identity, digest, payload, attempts, error) = row?;
            Ok(PendingUpload {
                sequence,
                kind,
                identity,
                digest,
                payload: serde_json::from_str(&payload)?,
                attempts,
                error,
            })
        })
        .collect()
    }

    /// The sender retains this non-credential identity across retries. It must
    /// reauthorize access on every remote request, including after a restart.
    pub fn bind_sender(
        &mut self,
        project: &str,
        lane: &str,
        principal: &str,
        audience: &str,
    ) -> Result<()> {
        ensure!(
            project == self.identity.project_id && matches!(lane, "stable" | "canary"),
            "Solar sender project or lane mismatch"
        );
        ensure!(
            [principal, audience].iter().all(|s| s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
            "invalid Solar sender identity"
        );
        let binding = serde_json::to_string(
            &json!({"project":project,"lane":lane,"principal":principal,"audience":audience}),
        )?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let prior: Option<String> = tx
            .query_row("SELECT value FROM meta WHERE key='sender'", [], |r| {
                r.get(0)
            })
            .optional()?;
        ensure!(
            prior.as_ref().is_none_or(|v| v == &binding),
            "Solar workspace belongs to another sender"
        );
        tx.execute("INSERT OR IGNORE INTO meta VALUES('sender',?1)", [binding])?;
        tx.commit()?;
        Ok(())
    }

    pub fn acknowledge(&self, row: &PendingUpload, receipt: &Value) -> Result<()> {
        let receipt = serde_json::to_string(receipt)?;
        ensure!(
            receipt.len() <= 1024 * 1024,
            "Solar publication receipt exceeds 1 MiB"
        );
        ensure!(self.db.execute("UPDATE outbox SET status='published',receipt=?1,error=NULL WHERE sequence=?2 AND digest=?3 AND status='pending'",params![receipt,row.sequence,row.digest])?==1,"Solar outbox acknowledgement no longer names a pending row");
        Ok(())
    }

    pub fn failed(&self, row: &PendingUpload, code: &str) -> Result<()> {
        ensure!(
            identifier(code),
            "upload failures must use a bounded code, never credentials or URLs"
        );
        self.db.execute("UPDATE outbox SET attempts=attempts+1,error=?1 WHERE sequence=?2 AND digest=?3 AND status='pending'",params![code,row.sequence,row.digest])?;
        Ok(())
    }

    pub fn status(&self) -> Result<Value> {
        let mut stmt = self
            .db
            .prepare("SELECT id,digest FROM cities ORDER BY id LIMIT 65")?;
        let cities = stmt
            .query_map([], |r| {
                Ok(json!({"city_id":r.get::<_,String>(0)?,"content_digest":r.get::<_,String>(1)?}))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let pending: i64 = self.db.query_row(
            "SELECT count(*) FROM outbox WHERE status='pending'",
            [],
            |r| r.get(0),
        )?;
        let mut runs = self
            .db
            .prepare("SELECT id,status,error FROM runs ORDER BY rowid DESC LIMIT 20")?;
        let runs=runs.query_map([],|r|Ok(json!({"run_id":r.get::<_,String>(0)?,"status":r.get::<_,String>(1)?,"error":r.get::<_,Option<String>>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(
            json!({"schema_version":SCHEMA,"project":self.identity,"cities":cities.iter().take(64).collect::<Vec<_>>(),"more_cities":cities.len()>64,"runs":runs,"pending_uploads":pending}),
        )
    }
}

pub mod dashboard;
pub mod portfolio;
