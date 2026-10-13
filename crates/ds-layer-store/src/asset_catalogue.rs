//! Native Assets index holdings. Private files/locks belong to this existing
//! persistence owner; scope, freshness and completion admission stay in the kernel.
//! Retained metadata is an authenticated capture, not proof of current permission.
use crate::{exclusive_lock, private};
use ds_command_kernel::assets::index_read::{self, Held, Request, Scope};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

const SCHEMA: &str = "ds.assets.native-page/v1";
// The admitted value is bounded at 8 MiB; allow its bounded scope/query wrapper.
const MAX_BYTES: usize = 8 * 1024 * 1024 + 32 * 1024;
const MAX_PAGES: usize = 64;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub uid: String,
    pub lane: String,
    pub audience: String,
    pub credential_instance: String,
    pub project: String,
    /// Present only if protected identity itself carries an email (device).
    pub protected_email: Option<String>,
}
impl Binding {
    fn directory(&self, root: &Path) -> Result<PathBuf, String> {
        // The email remains part of every page's semantic identity. The outer
        // address binds the protected provider; Firebase's probe has no email.
        let ok = |s: &str| {
            !s.is_empty() && s.len() <= 2048 && s.trim() == s && !s.chars().any(char::is_control)
        };
        if !["stable", "canary"].contains(&self.lane.as_str())
            || ![
                &self.uid,
                &self.audience,
                &self.credential_instance,
                &self.project,
            ]
            .into_iter()
            .all(|s| ok(s))
        {
            return Err("assets_retained_scope_invalid".into());
        }
        Ok(root.join("assets-catalogue/v1").join(hash(self)?))
    }
    fn accepts(&self, scope: &Scope) -> bool {
        scope.uid == self.uid
            && scope.lane == self.lane
            && scope.audience == self.audience
            && scope.project == self.project
            && self
                .protected_email
                .as_ref()
                .is_none_or(|e| e.eq_ignore_ascii_case(&scope.email))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    schema: String,
    binding: Binding,
    scope: Scope,
    key: String,
    query: Value,
    epoch: u64,
    serial: u64,
    saved_at_ms: i64,
    value_sha256: String,
    value: Value,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    epoch: u64,
    serial: u64,
    pages: BTreeMap<String, u64>,
    denied: bool,
}
pub struct Capture {
    pub scope: Scope,
    pub value: Value,
}
pub trait Source {
    type Error;
    /// Protected local identity only: no refresh, directory or source acquisition.
    fn check_binding(&mut self) -> Result<(), Self::Error>;
    /// One explicit, authenticated index acquisition through the admitted client.
    fn capture(&mut self) -> Result<Capture, Self::Error>;
    fn access_refused(&self, error: &Self::Error) -> bool;
    fn verify_page(&self, value: &Value) -> Result<(), Self::Error>;
}
#[derive(Debug)]
pub enum Error<E> {
    Source(E),
    Local(String),
    AcquisitionRequired,
    Superseded,
}
fn local<E>(s: impl ToString) -> Error<E> {
    Error::Local(s.to_string())
}
fn hash(v: &impl Serialize) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(v).map_err(|_| "assets_retained_unreadable")?)
    ))
}
fn read<T: for<'a> Deserialize<'a>>(p: &Path, cap: usize) -> Result<Option<T>, String> {
    let file = match File::open(p) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("assets_retained_unreadable".into()),
    };
    let mut bytes = Vec::new();
    file.take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "assets_retained_unreadable")?;
    if bytes.len() > cap {
        return Err("assets_retained_unreadable".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| "assets_retained_unreadable".into())
}
fn write<T: Serialize>(p: &Path, v: &T, cap: usize) -> Result<(), String> {
    let bytes = serde_json::to_vec(v).map_err(|_| "assets_retained_unreadable")?;
    if bytes.len() > cap {
        return Err("assets_retained_unreadable".into());
    }
    private::write_atomic(p, bytes).map_err(|_| "assets_retained_unavailable".into())
}
fn identity(scope: &Scope, query: &Value) -> Result<Value, String> {
    index_read::evaluate(Request::Identity {
        scope: scope.clone(),
        query: query.clone(),
    })
}
fn held(page: &Page, state: &State) -> Held {
    Held {
        key: page.key.clone(),
        saved_at_ms: page.saved_at_ms,
        invalidated: page.epoch != state.epoch,
    }
}
fn validate(page: &Page, binding: &Binding, query: &Value) -> Result<(), String> {
    if page.schema != SCHEMA
        || page.binding != *binding
        || !binding.accepts(&page.scope)
        || page.query != *query
        || page.saved_at_ms < 0
        || page.value_sha256 != hash(&page.value)?
        || identity(&page.scope, query)?["key"] != page.key
    {
        return Err("assets_retained_unreadable".into());
    }
    Ok(())
}
fn result(page: Page, mode: &str, stale: bool) -> Value {
    let mut value = page.value;
    // Acquiring an authorized index snapshot does not prove every source is
    // currently fresh: its probe may be buffered or unwatermarked. Preserve
    // source qualifications; only capture-time authority is verified here.
    value["retained"] = json!({
        "schema":"ds.assets.retained-read/v1", "mode":mode, "stale":stale,
        "capture_started_at_ms":page.saved_at_ms, "captured_scope":page.scope,
        "page_sha256":page.value_sha256, "current_authorization_verified":mode == "acquired",
        "current_source_verified":false,
        "permission_freshness":if mode == "acquired" { "verified_at_capture" } else { "captured_only" }
    });
    value
}
/// Read one page. The query lock spans only this requested acquisition; the
/// scope marker lock is short and prevents stale completion publishing after a
/// known mutation. Concurrent native processes share the same locks and bytes.
pub fn consume<S: Source>(
    root: &Path,
    binding: &Binding,
    mut query: Value,
    offline: bool,
    now_ms: i64,
    source: &mut S,
) -> Result<Value, Error<S::Error>> {
    if now_ms < 0 {
        return Err(local("assets_retained_time_invalid"));
    }
    let refresh = query["refresh"] == true;
    if offline && refresh {
        return Err(local("assets_offline_refresh_conflict"));
    }
    query
        .as_object_mut()
        .ok_or_else(|| local("assets_retained_query_invalid"))?
        .remove("refresh");
    let dir = binding.directory(root).map_err(local)?;
    source.check_binding().map_err(Error::Source)?;
    let address = hash(&query).map_err(local)?;
    let path = dir.join(format!("{address}.json"));
    if offline && !path.try_exists().map_err(local)? {
        return Err(Error::AcquisitionRequired);
    }
    let observed = if refresh {
        read::<Page>(&path, MAX_BYTES)
            .map_err(local)?
            .map(|p| p.serial)
    } else {
        None
    };
    // Offline reads never create a missing catalogue or lock.
    let _query_lock = if offline {
        use fs2::FileExt;
        let f = private::open_options()
            .read(true)
            .write(true)
            .open(dir.join(format!("{address}.lock")))
            .map_err(local)?;
        f.lock_exclusive().map_err(local)?;
        f
    } else {
        exclusive_lock(&dir.join(format!("{address}.lock"))).map_err(local)?
    };
    let mut policy_query = query.clone();
    let (state, page) = {
        let _lock = exclusive_lock(&dir.join("state.lock")).map_err(local)?;
        let state = read::<State>(&dir.join("state.json"), 16384)
            .map_err(local)?
            .unwrap_or_default();
        // Register first intentional use before acquisition so concurrent
        // mutation can supersede the capture without creating unused stores.
        if !offline && !dir.join("state.json").exists() {
            write(&dir.join("state.json"), &state, 16384).map_err(local)?;
        }
        let page = if state.denied {
            None
        } else {
            read::<Page>(&path, MAX_BYTES).map_err(local)?
        };
        if let Some(p) = &page {
            validate(p, binding, &query).map_err(local)?;
            source.verify_page(&p.value).map_err(Error::Source)?;
            if p.serial > state.serial || state.pages.get(&address) != Some(&p.serial) {
                return Err(local("assets_retained_unreadable"));
            }
        }
        (state, page)
    };
    // A refresh that waited for the same page's refresh consumes that capture.
    let coalesced_refresh = refresh
        && page
            .as_ref()
            .is_some_and(|p| Some(p.serial) != observed && p.epoch == state.epoch);
    policy_query["refresh"] = json!(refresh && !coalesced_refresh);
    if let Some(p) = page.as_ref().filter(|_| !state.denied) {
        let plan = index_read::evaluate(Request::Plan {
            scope: p.scope.clone(),
            query: policy_query.clone(),
            epoch: state.epoch,
            held: Some(held(p, &state)),
            pending: None,
            online: !offline,
            now_ms,
        })
        .map_err(local)?;
        if plan["decision"] == "retained" {
            source.check_binding().map_err(Error::Source)?;
            return Ok(result(
                page.unwrap(),
                if offline {
                    "offline_capture"
                } else {
                    "retained_capture"
                },
                plan["stale"] == true,
            ));
        }
    }
    if offline {
        return Err(Error::AcquisitionRequired);
    }
    let capture = match source.capture() {
        Ok(c) => c,
        Err(error) => {
            if source.access_refused(&error) {
                mark(root, binding, true).map_err(local)?;
            }
            // No implicit fallback disguises a denied or unavailable acquisition.
            return Err(Error::Source(error));
        }
    };
    source.check_binding().map_err(Error::Source)?;
    if !binding.accepts(&capture.scope) {
        return Err(Error::Superseded);
    }
    source.verify_page(&capture.value).map_err(Error::Source)?;
    let id = identity(&capture.scope, &query).map_err(local)?;
    let _lock = exclusive_lock(&dir.join("state.lock")).map_err(local)?;
    let mut current = read::<State>(&dir.join("state.json"), 16384)
        .map_err(local)?
        .unwrap_or_default();
    if current.epoch != state.epoch {
        return Err(Error::Superseded);
    }
    source.check_binding().map_err(Error::Source)?;
    current.denied = false;
    current.serial = current
        .serial
        .checked_add(1)
        .ok_or_else(|| local("assets_retained_epoch_invalid"))?;
    let page = Page {
        schema: SCHEMA.into(),
        binding: binding.clone(),
        scope: capture.scope,
        key: id["key"]
            .as_str()
            .ok_or_else(|| local("assets_retained_unreadable"))?
            .into(),
        query,
        epoch: current.epoch,
        serial: current.serial,
        saved_at_ms: now_ms,
        value_sha256: hash(&capture.value).map_err(local)?,
        value: capture.value,
    };
    current.pages.insert(address.clone(), page.serial);
    if current.pages.len() > MAX_PAGES {
        if let Some(oldest) = current
            .pages
            .iter()
            .filter(|(k, _)| **k != address)
            .min_by_key(|(_, v)| **v)
            .map(|(k, _)| k.clone())
        {
            current.pages.remove(&oldest);
            let _ = std::fs::remove_file(dir.join(format!("{oldest}.json")));
        }
    }
    write(&path, &page, MAX_BYTES).map_err(local)?;
    write(&dir.join("state.json"), &current, 16384).map_err(local)?;
    Ok(result(page, "acquired", false))
}
/// A known, verified mutation marks existing holdings stale. Unused catalogue:
/// no directory, no marker, no source call. Failed/no-op writes never call this.
pub fn invalidate(root: &Path, binding: &Binding) -> Result<bool, String> {
    mark(root, binding, false)
}
fn mark(root: &Path, binding: &Binding, denied: bool) -> Result<bool, String> {
    let dir = binding.directory(root)?;
    if !dir
        .join("state.json")
        .try_exists()
        .map_err(|_| "assets_retained_unavailable")?
    {
        return Ok(false);
    }
    let _lock = exclusive_lock(&dir.join("state.lock"))?;
    let Some(mut state) = read::<State>(&dir.join("state.json"), 16384)? else {
        return Ok(false);
    };
    state.denied |= denied;
    let retired = if denied {
        // Publish the denial fence before cleanup. A missing/unremovable old
        // body must never leave another scoped page admissible offline.
        std::mem::take(&mut state.pages)
    } else {
        BTreeMap::new()
    };
    state.epoch = state
        .epoch
        .checked_add(1)
        .filter(|n| *n <= 9_007_199_254_740_991)
        .ok_or("assets_retained_epoch_invalid")?;
    write(&dir.join("state.json"), &state, 16384)?;
    for address in retired.keys() {
        match std::fs::remove_file(dir.join(format!("{address}.json"))) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("assets_retained_unavailable".into()),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    fn binding() -> Binding {
        Binding {
            uid: "u1".into(),
            lane: "canary".into(),
            audience: "credential:a1".into(),
            credential_instance: "login1".into(),
            project: "p1".into(),
            protected_email: Some("owner@example.com".into()),
        }
    }
    fn scope(b: &Binding) -> Scope {
        Scope {
            uid: b.uid.clone(),
            email: "owner@example.com".into(),
            lane: b.lane.clone(),
            audience: b.audience.clone(),
            project: b.project.clone(),
        }
    }
    struct Fake {
        b: Binding,
        calls: Arc<AtomicUsize>,
        checks: usize,
        reject: bool,
        change: bool,
        during: Option<Box<dyn FnOnce() + Send>>,
    }
    impl Fake {
        fn new(b: Binding, calls: Arc<AtomicUsize>) -> Self {
            Self {
                b,
                calls,
                checks: 0,
                reject: false,
                change: false,
                during: None,
            }
        }
    }
    impl Source for Fake {
        type Error = &'static str;
        fn check_binding(&mut self) -> Result<(), Self::Error> {
            self.checks += 1;
            if self.change {
                Err("changed")
            } else {
                Ok(())
            }
        }
        fn capture(&mut self) -> Result<Capture, Self::Error> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(during) = self.during.take() {
                during();
            }
            if self.reject {
                return Err("denied");
            }
            Ok(Capture {
                scope: scope(&self.b),
                value: json!({"assets":[],"more":false,"index":{"generation":n},"call":n}),
            })
        }
        fn access_refused(&self, e: &&str) -> bool {
            *e == "denied"
        }
        fn verify_page(&self, v: &Value) -> Result<(), Self::Error> {
            if v["assets"].is_array() {
                Ok(())
            } else {
                Err("invalid")
            }
        }
    }
    fn query() -> Value {
        json!({"order":"recent","limit":50})
    }
    #[test]
    fn cold_warm_offline_and_changed_have_exact_acquisition_counts() {
        let root = tempfile::tempdir().unwrap();
        let b = binding();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(b.clone(), calls.clone());
        let cold = consume(root.path(), &b, query(), false, 1000, &mut source).unwrap();
        assert_eq!(cold["retained"]["mode"], "acquired");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let warm = consume(root.path(), &b, query(), false, 2000, &mut source).unwrap();
        assert_eq!(warm["retained"]["current_authorization_verified"], false);
        assert_eq!(warm["retained"]["mode"], "retained_capture");
        let offline = consume(root.path(), &b, query(), true, 900000, &mut source).unwrap();
        assert_eq!(offline["retained"]["stale"], true);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(invalidate(root.path(), &b).unwrap());
        let stale = consume(root.path(), &b, query(), true, 2000, &mut source).unwrap();
        assert_eq!(stale["retained"]["stale"], true);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            consume(root.path(), &b, query(), false, 3000, &mut source).unwrap()["call"],
            2
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        #[cfg(unix)]
        private::assert_private_tree(&root.path().join("assets-catalogue"));
    }
    #[test]
    fn unused_invalidation_and_missing_offline_create_nothing_and_acquire_zero() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("unused");
        let b = binding();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(b.clone(), calls.clone());
        assert!(!invalidate(&path, &b).unwrap());
        assert!(matches!(
            consume(&path, &b, query(), true, 1000, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert!(!path.exists());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(source.checks, 1);
    }
    #[test]
    fn native_instances_coalesce_a_requested_page() {
        let root = tempfile::tempdir().unwrap();
        let b = binding();
        let calls = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|threads| {
            for _ in 0..8 {
                let b = b.clone();
                let calls = calls.clone();
                let path = root.path();
                threads.spawn(move || {
                    let mut source = Fake::new(b.clone(), calls);
                    consume(path, &b, query(), false, 1000, &mut source).unwrap();
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // A new process-equivalent source owns no in-memory cache and reuses disk.
        let mut source = Fake::new(b.clone(), calls.clone());
        consume(root.path(), &b, query(), true, 2000, &mut source).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn known_mutation_during_cold_capture_refuses_stale_publish() {
        let root = tempfile::tempdir().unwrap();
        let b = binding();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(b.clone(), calls.clone());
        let path = root.path().to_owned();
        let during_b = b.clone();
        source.during = Some(Box::new(move || {
            assert!(invalidate(&path, &during_b).unwrap());
        }));
        assert!(matches!(
            consume(root.path(), &b, query(), false, 1000, &mut source),
            Err(Error::Superseded)
        ));
        assert!(matches!(
            consume(root.path(), &b, query(), true, 2000, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn denied_refresh_cannot_be_hidden_by_retained_or_offline_data() {
        let root = tempfile::tempdir().unwrap();
        let b = binding();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(b.clone(), calls.clone());
        consume(root.path(), &b, query(), false, 1000, &mut source).unwrap();
        source.reject = true;
        let mut refresh = query();
        refresh["refresh"] = json!(true);
        assert!(matches!(
            consume(root.path(), &b, refresh, false, 2000, &mut source),
            Err(Error::Source("denied"))
        ));
        assert!(matches!(
            consume(root.path(), &b, query(), true, 3000, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        source.reject = false;
        consume(root.path(), &b, query(), false, 4000, &mut source).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn denial_fence_survives_missing_old_body_and_clears_other_queries() {
        let root = tempfile::tempdir().unwrap();
        let b = binding();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(b.clone(), calls.clone());
        consume(root.path(), &b, query(), false, 1000, &mut source).unwrap();
        let mut other = query();
        other["folder"] = json!("other");
        consume(root.path(), &b, other.clone(), false, 1000, &mut source).unwrap();
        let path = b
            .directory(root.path())
            .unwrap()
            .join(format!("{}.json", hash(&query()).unwrap()));
        std::fs::remove_file(path).unwrap();
        source.reject = true;
        let mut refresh = other.clone();
        refresh["refresh"] = json!(true);
        assert!(matches!(
            consume(root.path(), &b, refresh, false, 2000, &mut source),
            Err(Error::Source("denied"))
        ));
        assert!(matches!(
            consume(root.path(), &b, other, true, 3000, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn acquired_index_does_not_claim_verification_of_stale_or_bounded_sources() {
        let b = binding();
        let page = |index| Page {
            schema: SCHEMA.into(),
            binding: b.clone(),
            scope: scope(&b),
            key: "key".into(),
            query: query(),
            epoch: 0,
            serial: 1,
            saved_at_ms: 1000,
            value_sha256: "digest".into(),
            value: json!({"index":index}),
        };
        for index in [
            json!({"stale_sources":["grid_exports"],"truncated_sources":[]}),
            json!({"stale_sources":[],"truncated_sources":["survey_media"]}),
            json!({}),
        ] {
            let value = result(page(index), "acquired", false);
            assert_eq!(value["retained"]["current_source_verified"], false);
            assert_eq!(value["retained"]["current_authorization_verified"], true);
        }
        assert_eq!(
            result(
                page(json!({"stale_sources":[],"truncated_sources":[]})),
                "acquired",
                false
            )["retained"]["current_source_verified"],
            false
        );
        assert_eq!(
            result(
                page(json!({"stale_sources":[],"truncated_sources":[]})),
                "offline_capture",
                false
            )["retained"]["current_source_verified"],
            false
        );
    }
    #[test]
    fn exact_principal_lane_audience_credential_project_email_and_query_are_fenced() {
        let root = tempfile::tempdir().unwrap();
        let b = binding();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(b.clone(), calls.clone());
        consume(root.path(), &b, query(), false, 1000, &mut source).unwrap();
        for edit in 0..6 {
            let mut other = b.clone();
            match edit {
                0 => other.uid = "u2".into(),
                1 => other.lane = "stable".into(),
                2 => other.audience = "a2".into(),
                3 => other.credential_instance = "login2".into(),
                4 => other.project = "p2".into(),
                _ => other.protected_email = Some("other@example.com".into()),
            };
            let mut source = Fake::new(other.clone(), calls.clone());
            assert!(matches!(
                consume(root.path(), &other, query(), true, 2000, &mut source),
                Err(Error::AcquisitionRequired)
            ));
        }
        let mut q = query();
        q["folder"] = json!("different");
        assert!(matches!(
            consume(root.path(), &b, q, true, 2000, &mut source),
            Err(Error::AcquisitionRequired)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn changed_identity_and_corrupt_bytes_never_publish_or_acquire_offline() {
        let root = tempfile::tempdir().unwrap();
        let b = binding();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut source = Fake::new(b.clone(), calls.clone());
        source.change = true;
        assert!(matches!(
            consume(root.path(), &b, query(), false, 1000, &mut source),
            Err(Error::Source("changed"))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        source.change = false;
        consume(root.path(), &b, query(), false, 1000, &mut source).unwrap();
        let path = b
            .directory(root.path())
            .unwrap()
            .join(format!("{}.json", hash(&query()).unwrap()));
        let mut page: Page = read(&path, MAX_BYTES).unwrap().unwrap();
        page.value["call"] = json!(999);
        write(&path, &page, MAX_BYTES).unwrap();
        assert!(matches!(
            consume(root.path(), &b, query(), true, 2000, &mut source),
            Err(Error::Local(_))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
