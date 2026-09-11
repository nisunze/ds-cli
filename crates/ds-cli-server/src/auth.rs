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
fn observe(lane: &str) -> Result<(String, String), String> {
    let id = ds_cli_auth::probe_headless_identity(lane)
        .map_err(|e| e.message().to_owned())?
        .ok_or("server signed out")?
        .0;
    let owner = digest(
        &serde_json::to_vec(&(id.uid(), id.lane(), id.credential_audience_sha256()))
            .map_err(|e| e.to_string())?,
    );
    let credential =
        ds_cli_auth::runtime_credential_binding(lane).map_err(|e| e.message().to_owned())?;
    Ok((owner, credential))
}
/// One refresh cache across workers. Bind to the originating credential so
/// device revocation cannot silently fall back to another login for the UID.
pub struct NativeAuthorizer {
    lane: String,
    credential: String,
    cached: Mutex<Option<(Instant, Result<String, String>)>>,
}
impl NativeAuthorizer {
    pub fn new(lane: String) -> Result<Self, String> {
        let (_, credential) = observe(&lane)?;
        Ok(Self {
            lane,
            credential,
            cached: Mutex::new(None),
        })
    }
    fn check(&self, owner: &str, observed: (String, String)) -> Result<(), String> {
        if observed.0 != owner {
            return Err("server identity changed; old jobs are fenced".into());
        }
        if observed.1 != self.credential {
            return Err("server credential changed; restart the host explicitly to resume retained work under the new login".into());
        }
        Ok(())
    }
    fn authorize_with(
        &self,
        owner: &str,
        mut probe: impl FnMut() -> Result<(String, String), String>,
        mut refresh: impl FnMut() -> Result<String, String>,
    ) -> Result<(), String> {
        self.check(owner, probe()?)?;
        let mut cache = self
            .cached
            .lock()
            .map_err(|_| "authorization lock poisoned")?;
        if cache
            .as_ref()
            .is_none_or(|(time, _)| time.elapsed() >= Duration::from_secs(15))
        {
            let refreshed = refresh();
            // A refresh can clear a revoked device while leaving a different
            // provider signed in. Check the credential again before admission.
            self.check(owner, probe()?)?;
            *cache = Some((Instant::now(), refreshed));
        }
        match &cache.as_ref().ok_or("authorization unavailable")?.1 {
            Ok(actual) if actual == owner => Ok(()),
            Ok(_) => Err("server identity changed".into()),
            Err(e) => Err(e.clone()),
        }
    }
}
impl Authorizer for NativeAuthorizer {
    fn authorize(&self, owner: &str) -> Result<(), String> {
        self.authorize_with(owner, || observe(&self.lane), || identity(&self.lane))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn authorizer() -> NativeAuthorizer {
        NativeAuthorizer {
            lane: "stable".into(),
            credential: "device:first".into(),
            cached: Mutex::new(None),
        }
    }
    #[test]
    fn refresh_cannot_switch_to_password_or_another_device_for_the_same_uid() {
        for replacement in ["firebase", "device:second"] {
            let auth = authorizer();
            let calls = Cell::new(0);
            let result = auth.authorize_with(
                "owner",
                || {
                    let credential = if calls.get() == 0 {
                        "device:first"
                    } else {
                        replacement
                    };
                    calls.set(calls.get() + 1);
                    Ok(("owner".into(), credential.into()))
                },
                || Ok("owner".into()),
            );
            assert!(result.unwrap_err().contains("credential changed"));
            assert!(auth.cached.lock().unwrap().is_none());
            assert!(
                auth.authorize_with(
                    "owner",
                    || Ok(("owner".into(), replacement.into())),
                    || panic!("replacement must not be refreshed")
                )
                .is_err()
            );
        }
    }
    #[test]
    fn password_relogin_for_the_same_uid_fences_the_old_server() {
        let mut auth = authorizer();
        auth.credential = "firebase:first-login".into();
        auth.authorize_with(
            "owner",
            || Ok(("owner".into(), "firebase:first-login".into())),
            || Ok("owner".into()),
        )
        .unwrap();
        assert!(
            auth.authorize_with(
                "owner",
                || Ok(("owner".into(), "firebase:second-login".into())),
                || panic!("a replacement session must not refresh old work"),
            )
            .unwrap_err()
            .contains("credential changed")
        );
    }
    #[test]
    fn cached_authority_still_checks_logout_and_credential_changes() {
        let auth = authorizer();
        auth.authorize_with(
            "owner",
            || Ok(("owner".into(), "device:first".into())),
            || Ok("owner".into()),
        )
        .unwrap();
        auth.authorize_with(
            "owner",
            || Ok(("owner".into(), "device:first".into())),
            || panic!("fresh cache must be reused"),
        )
        .unwrap();
        assert!(
            auth.authorize_with(
                "owner",
                || Err("signed out".into()),
                || panic!("signed out must not refresh")
            )
            .is_err()
        );
        assert!(
            auth.authorize_with(
                "owner",
                || Ok(("owner".into(), "firebase".into())),
                || panic!("provider switch must not refresh")
            )
            .is_err()
        );
        assert!(
            auth.authorize_with(
                "owner",
                || Ok(("another-owner".into(), "device:first".into())),
                || panic!("account switch must not refresh")
            )
            .is_err()
        );
    }
}
