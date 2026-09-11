use ds_compute_runtime::{Authorizer, digest};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

pub fn identity(lane: &str) -> Result<String, String> {
    let id = ds_cli_auth::refresh_runtime_identity(lane).map_err(|e| e.message().to_owned())?;
    Ok(digest(
        &serde_json::to_vec(&(id.uid(), id.lane(), id.credential_audience_sha256()))
            .map_err(|e| e.to_string())?,
    ))
}
/// One bounded refresh cache across parallel workers. Account changes are
/// checked locally on every request; upstream revocation is checked every 15s.
pub struct NativeAuthorizer {
    pub lane: String,
    cached: Mutex<Option<(Instant, Result<String, String>)>>,
}
impl NativeAuthorizer {
    pub fn new(lane: String) -> Self {
        Self {
            lane,
            cached: Mutex::new(None),
        }
    }
}
impl Authorizer for NativeAuthorizer {
    fn authorize(&self, owner: &str) -> Result<(), String> {
        let observed = ds_cli_auth::probe_headless_identity(&self.lane)
            .map_err(|e| e.message().to_owned())?
            .ok_or("server signed out")?
            .0;
        let observed = digest(
            &serde_json::to_vec(&(
                observed.uid(),
                observed.lane(),
                observed.credential_audience_sha256(),
            ))
            .map_err(|e| e.to_string())?,
        );
        if observed != owner {
            return Err("server identity changed; old jobs are fenced".into());
        }
        let mut cache = self
            .cached
            .lock()
            .map_err(|_| "authorization lock poisoned")?;
        if cache
            .as_ref()
            .is_none_or(|(time, _)| time.elapsed() >= Duration::from_secs(15))
        {
            *cache = Some((Instant::now(), identity(&self.lane)));
        }
        match &cache.as_ref().ok_or("authorization unavailable")?.1 {
            Ok(actual) if actual == owner => Ok(()),
            Ok(_) => Err("server identity changed".into()),
            Err(e) => Err(e.clone()),
        }
    }
}
