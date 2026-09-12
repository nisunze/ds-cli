//! Who owns this host — answered by the credential this machine holds.
//!
//! The Server is the desktop's core, so it authorizes the way the desktop
//! does: from the protected native state on this machine. Nothing on a
//! request path talks to the gateway. `serve` reads the credential from disk,
//! binds its port, and from then on every route and every worker asks the
//! same local question — *is this still the credential that started me?*
//!
//! There is exactly one answer that stops the host: a LOCAL answer that the
//! owner changed (a different credential on disk, or the credential removed).
//! "I could not tell right now" is not that answer and never refuses a
//! request — that conflation is what made a Server unusable within fifteen
//! seconds of losing its upstream.
//!
//! The gateway refresh still happens, because a device credential that is
//! never refreshed eventually stops being useful for publication and sync.
//! It happens [off the request path](CredentialRefresh): once at start if the
//! upstream is reachable, then in the background at a bounded interval. Its
//! failure is logged and nothing else. There is no online/offline conditional
//! here beyond "refresh if it works".

use ds_compute_runtime::{Authorizer, digest};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

/// How often the local observation is re-read, at most. A protected-state
/// read per request would serialize the door behind a file lock; a stale
/// window of fifteen seconds is the same window this host has always had for
/// noticing an account change, and it is a LOCAL read either way.
pub const OBSERVE_INTERVAL: Duration = Duration::from_secs(15);

/// How often the held credential is refreshed with the gateway, off every
/// request path. Short enough that a device access token is renewed long
/// before anything needs it, long enough that a host with no upstream spends
/// nothing on rediscovering that.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(300);

/// What the protected state on this machine says about its owner right now.
/// Read with no network at all: this is the disk's answer, not the gateway's.
#[derive(Clone, PartialEq, Eq)]
pub enum OwnerAnswer {
    /// A credential is held: this owner fence, and this exact binding.
    Held { owner: String, credential: String },
    /// No credential for this lane is on this machine any more.
    SignedOut,
}

/// The credential a host authorizes from. Two methods, and only the first is
/// ever reached from a request: [`held`](OwnerCredential::held) reads this
/// machine, [`refresh`](OwnerCredential::refresh) talks to the gateway.
pub trait OwnerCredential: Send + Sync + 'static {
    /// The protected state on this machine. MUST NOT touch the network.
    fn held(&self) -> Result<OwnerAnswer, String>;
    /// Renew the held credential with the gateway. Called only by
    /// [`CredentialRefresh`], never while a request or a claim waits.
    fn refresh(&self) -> Result<(), String>;
}

/// The production credential: this Linux user's protected native state for
/// one lane.
pub struct NativeCredential {
    lane: String,
}

impl NativeCredential {
    pub const fn new(lane: String) -> Self {
        Self { lane }
    }
}

impl OwnerCredential for NativeCredential {
    fn held(&self) -> Result<OwnerAnswer, String> {
        held_locally(&self.lane)
    }
    fn refresh(&self) -> Result<(), String> {
        ds_cli_auth::refresh_runtime_identity(&self.lane)
            .map(|_| ())
            .map_err(|error| error.message().to_owned())
    }
}

/// The owner fence: the digest of the held credential's (uid, lane, audience).
/// The same three fields the durable rows are fenced by, so the host's own
/// identity and its jobs' identity cannot drift apart.
fn fence(uid: &str, lane: &str, audience: &str) -> Result<String, String> {
    Ok(digest(
        &serde_json::to_vec(&(uid, lane, audience)).map_err(|error| error.to_string())?,
    ))
}

/// Read this machine, with no network.
///
/// The observation deliberately does NOT read the saved project selection: a
/// host that admits a project per operation must not be stopped by two
/// providers disagreeing about a value it never uses.
fn held_locally(lane: &str) -> Result<OwnerAnswer, String> {
    let identity = ds_cli_auth::probe_headless_identity_for_named_project(lane)
        .map_err(|error| error.message().to_owned())?;
    let Some(identity) = identity else {
        return Ok(OwnerAnswer::SignedOut);
    };
    let credential = match ds_cli_auth::runtime_credential_binding(lane) {
        Ok(credential) => credential,
        // The one error that is an ANSWER: nothing is held for this lane.
        Err(error) if error.code() == "headless_signed_out" => {
            return Ok(OwnerAnswer::SignedOut);
        }
        Err(error) => return Err(error.message().to_owned()),
    };
    Ok(OwnerAnswer::Held {
        owner: fence(
            identity.uid(),
            identity.lane(),
            identity.credential_audience_sha256(),
        )?,
        credential,
    })
}

/// The owner this machine's credential names, for a caller that needs the
/// fence itself rather than a decision about it. Local, no network.
pub fn owner_fence(lane: &str) -> Result<String, String> {
    match held_locally(lane)? {
        OwnerAnswer::Held { owner, .. } => Ok(owner),
        OwnerAnswer::SignedOut => Err(SIGNED_OUT.to_owned()),
    }
}

const SIGNED_OUT: &str = "this machine holds no native credential for this lane; sign in under the server's Linux account";
const OWNER_CHANGED: &str = "server identity changed; old jobs are fenced";
const CREDENTIAL_CHANGED: &str = "server credential changed; restart the host explicitly to resume retained work under the new login";

/// Whether the last gateway refresh answered at all.
///
/// One Server, one owner, one lane, one gateway: this is a single fact about
/// the process, written only by [`CredentialRefresh`] and read by the Sync
/// Center session, whose passes genuinely do need an upstream. Nothing on an
/// admission, queue, execution or job-read path reads it — those are local
/// and stay local — and it defaults to "reachable" so nothing waits on a
/// refresher that has not run yet.
static GATEWAY_REACHABLE: AtomicBool = AtomicBool::new(true);

pub fn gateway_reachable() -> bool {
    GATEWAY_REACHABLE.load(Ordering::Acquire)
}

fn record_reachable(reachable: bool) {
    GATEWAY_REACHABLE.store(reachable, Ordering::Release);
}

/// One authenticated owner, observed locally.
///
/// Constructed from the credential on disk — which is why a host starts with
/// no upstream present — and bound to that exact credential, so a revoked
/// device cannot silently fall back to another login for the same UID.
pub struct NativeAuthorizer {
    source: Arc<dyn OwnerCredential>,
    owner: String,
    credential: String,
    interval: Duration,
    observed: Mutex<Option<(Instant, OwnerAnswer)>>,
    /// When an unreadable machine was last reported, so protected state that
    /// is briefly unreadable costs one log line rather than one per request.
    reported: Mutex<Option<Instant>>,
}

impl NativeAuthorizer {
    /// The production authorizer for one lane. Reads the protected state on
    /// this machine once and nothing else: no network, no saved selection.
    pub fn new(lane: String) -> Result<Self, String> {
        Self::from_source(Arc::new(NativeCredential::new(lane)), OBSERVE_INTERVAL)
    }

    pub fn from_source(
        source: Arc<dyn OwnerCredential>,
        interval: Duration,
    ) -> Result<Self, String> {
        match source.held()? {
            OwnerAnswer::Held { owner, credential } => Ok(Self {
                source,
                owner,
                credential,
                interval,
                observed: Mutex::new(None),
                reported: Mutex::new(None),
            }),
            OwnerAnswer::SignedOut => Err(SIGNED_OUT.to_owned()),
        }
    }

    /// The fence this host runs under: the digest `connection.json` records
    /// and every durable row is owned by.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// The local observation, at most one read per interval.
    fn observe(&self) -> Result<OwnerAnswer, String> {
        let mut cached = self
            .observed
            .lock()
            .map_err(|_| "authorization lock poisoned".to_string())?;
        if let Some((at, answer)) = cached.as_ref()
            && at.elapsed() < self.interval
        {
            return Ok(answer.clone());
        }
        let answer = self.source.held()?;
        *cached = Some((Instant::now(), answer.clone()));
        Ok(answer)
    }

    /// Report an unreadable machine at most once a minute. It is not a
    /// refusal, so it must not be silent either.
    fn report(&self, message: &str) {
        let Ok(mut reported) = self.reported.lock() else {
            return;
        };
        if reported.is_some_and(|at| at.elapsed() < Duration::from_secs(60)) {
            return;
        }
        *reported = Some(Instant::now());
        eprintln!("DS server: could not read this machine's credential ({message}); continuing");
    }
}

impl Authorizer for NativeAuthorizer {
    /// Stop only for a local answer that the owner changed.
    ///
    /// * a different owner or a different credential on disk — stop;
    /// * the credential removed (signed out here) — stop;
    /// * the machine could not answer — carry on, and say so once.
    ///
    /// Nothing here can fail because an upstream is unreachable, because
    /// nothing here asks one.
    fn authorize(&self, owner: &str) -> Result<(), String> {
        if owner != self.owner {
            return Err(OWNER_CHANGED.to_owned());
        }
        match self.observe() {
            Ok(OwnerAnswer::Held { owner, credential }) => {
                if owner != self.owner {
                    return Err(OWNER_CHANGED.to_owned());
                }
                if credential != self.credential {
                    return Err(CREDENTIAL_CHANGED.to_owned());
                }
                Ok(())
            }
            Ok(OwnerAnswer::SignedOut) => Err(CREDENTIAL_CHANGED.to_owned()),
            Err(message) => {
                self.report(&message);
                Ok(())
            }
        }
    }
}

/// The gateway refresh, running beside the host and never in front of it.
///
/// It is a plain thread: it attempts a refresh at once (so a host started
/// with an upstream present is refreshed immediately), then once per
/// interval. A failure is recorded and logged; it refuses nothing, fences
/// nothing and stops nothing. Dropping the handle stops the thread.
pub struct CredentialRefresh {
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl CredentialRefresh {
    pub fn start(source: Arc<dyn OwnerCredential>, interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let worker = thread::Builder::new()
            .name("ds-credential-refresh".into())
            .spawn(move || {
                refresh_loop(source.as_ref(), &flag, interval, &mut |message| {
                    eprintln!("DS server: {message}");
                });
            })
            .ok();
        Self { stop, worker }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }
}

impl Drop for CredentialRefresh {
    fn drop(&mut self) {
        self.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// One slice of waiting, so stopping the host does not wait out an interval.
const REFRESH_SLICE: Duration = Duration::from_millis(100);

/// One attempt. The outcome is recorded and, when it failed, said once —
/// and that is the whole of it: nothing here refuses, fences or stops.
fn refresh_once(source: &dyn OwnerCredential, log: &mut dyn FnMut(&str)) {
    match source.refresh() {
        Ok(()) => record_reachable(true),
        Err(message) => {
            record_reachable(false);
            log(&format!(
                "credential refresh did not reach the gateway ({message}); the host is unaffected"
            ));
        }
    }
}

fn refresh_loop(
    source: &dyn OwnerCredential,
    stop: &AtomicBool,
    interval: Duration,
    log: &mut dyn FnMut(&str),
) {
    while !stop.load(Ordering::Acquire) {
        refresh_once(source, log);
        let mut waited = Duration::ZERO;
        while waited < interval && !stop.load(Ordering::Acquire) {
            let slice = REFRESH_SLICE.min(interval - waited);
            thread::sleep(slice);
            waited += slice;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A credential source that is a machine: it says what is on disk, and
    /// every refresh fails unless a test says otherwise. Counting refreshes
    /// is how a test proves a request path never reaches for a gateway.
    pub(crate) struct FixtureCredential {
        answer: Mutex<Result<OwnerAnswer, String>>,
        refreshes: AtomicUsize,
        refreshable: AtomicBool,
    }

    impl FixtureCredential {
        pub(crate) fn held(owner: &str, credential: &str) -> Arc<Self> {
            Arc::new(Self {
                answer: Mutex::new(Ok(OwnerAnswer::Held {
                    owner: owner.to_owned(),
                    credential: credential.to_owned(),
                })),
                refreshes: AtomicUsize::new(0),
                refreshable: AtomicBool::new(false),
            })
        }
        pub(crate) fn set(&self, answer: Result<OwnerAnswer, String>) {
            *self.answer.lock().expect("fixture answer") = answer;
        }
        pub(crate) fn refreshes(&self) -> usize {
            self.refreshes.load(Ordering::SeqCst)
        }
    }

    impl OwnerCredential for FixtureCredential {
        fn held(&self) -> Result<OwnerAnswer, String> {
            self.answer.lock().expect("fixture answer").clone()
        }
        fn refresh(&self) -> Result<(), String> {
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            if self.refreshable.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err("no route to the gateway".into())
            }
        }
    }

    /// A host with no upstream at all authorizes from what it holds, and the
    /// missing gateway reaches no part of the answer.
    #[test]
    fn a_held_credential_authorizes_with_no_gateway_and_no_refresh() {
        let source = FixtureCredential::held("owner", "device:first");
        let auth = NativeAuthorizer::from_source(source.clone(), Duration::ZERO).expect("held");
        assert_eq!(auth.owner(), "owner");
        for _ in 0..10 {
            auth.authorize("owner")
                .expect("the held credential is the owner");
        }
        assert_eq!(
            source.refreshes(),
            0,
            "authorization must never reach for the gateway"
        );
    }

    /// The one thing that stops a host: this machine now says somebody else.
    #[test]
    fn a_changed_or_removed_credential_on_disk_stops_the_host() {
        for (replacement, expected) in [
            (
                OwnerAnswer::Held {
                    owner: "owner".into(),
                    credential: "device:second".into(),
                },
                CREDENTIAL_CHANGED,
            ),
            (
                OwnerAnswer::Held {
                    owner: "owner".into(),
                    credential: "firebase".into(),
                },
                CREDENTIAL_CHANGED,
            ),
            (
                OwnerAnswer::Held {
                    owner: "somebody-else".into(),
                    credential: "device:first".into(),
                },
                OWNER_CHANGED,
            ),
            (OwnerAnswer::SignedOut, CREDENTIAL_CHANGED),
        ] {
            let source = FixtureCredential::held("owner", "device:first");
            let auth = NativeAuthorizer::from_source(source.clone(), Duration::ZERO).expect("held");
            auth.authorize("owner").expect("the starting credential");
            source.set(Ok(replacement));
            assert_eq!(auth.authorize("owner").expect_err("stopped"), expected);
        }
    }

    /// A machine that cannot answer right now is not a machine that changed
    /// hands. This is the whole difference the second pass asked for.
    #[test]
    fn an_unreadable_machine_is_not_an_owner_change() {
        let source = FixtureCredential::held("owner", "device:first");
        let auth = NativeAuthorizer::from_source(source.clone(), Duration::ZERO).expect("held");
        source.set(Err("protected state is locked".into()));
        auth.authorize("owner")
            .expect("could not tell is not an answer that the owner changed");
        // And when the machine can answer again, the answer is honoured.
        source.set(Ok(OwnerAnswer::Held {
            owner: "owner".into(),
            credential: "device:second".into(),
        }));
        assert!(auth.authorize("owner").is_err());
    }

    /// A connection owned by another fence is refused whatever the disk says.
    #[test]
    fn a_foreign_owner_is_refused_without_reading_anything() {
        let source = FixtureCredential::held("owner", "device:first");
        let auth = NativeAuthorizer::from_source(source, Duration::ZERO).expect("held");
        assert_eq!(auth.authorize("another-owner").unwrap_err(), OWNER_CHANGED);
    }

    /// The local read is cached, so the door is not a protected-state lock.
    #[test]
    fn the_local_observation_is_read_at_most_once_an_interval() {
        let source = FixtureCredential::held("owner", "device:first");
        let auth =
            NativeAuthorizer::from_source(source.clone(), Duration::from_secs(60)).expect("held");
        auth.authorize("owner").expect("first");
        source.set(Ok(OwnerAnswer::SignedOut));
        auth.authorize("owner")
            .expect("within the interval the host does not re-read the machine");
    }

    /// A host with no credential at all does not start. It is a local answer,
    /// and it is the only startup answer authorization has.
    #[test]
    fn a_machine_with_no_credential_cannot_host() {
        let source = FixtureCredential::held("owner", "device:first");
        source.set(Ok(OwnerAnswer::SignedOut));
        // `.err()` rather than `expect_err`: an authorizer holds a credential
        // binding, and a panic message must never carry one, so it has no
        // `Debug` to print.
        let refused = NativeAuthorizer::from_source(source, Duration::ZERO)
            .err()
            .expect("a machine with no credential cannot host");
        assert_eq!(refused, SIGNED_OUT);
    }

    /// Reachability is one fact about the process, so the two tests that
    /// write it take turns. Everything else here is per-test state.
    static REACHABILITY: Mutex<()> = Mutex::new(());

    /// The refresher fails forever without ever refusing anything, and its
    /// failure is said rather than swallowed. Nothing on a request path is
    /// affected, and the host's answers do not move.
    #[test]
    fn a_refresh_that_never_reaches_the_gateway_neither_stops_nor_refuses() {
        let _turn = REACHABILITY.lock().expect("reachability turn");
        let before = gateway_reachable();
        let source = FixtureCredential::held("owner", "device:first");
        let auth = NativeAuthorizer::from_source(source.clone(), Duration::ZERO).expect("held");
        let mut logged = Vec::new();
        for _ in 0..3 {
            refresh_once(source.as_ref(), &mut |message| {
                logged.push(message.to_owned())
            });
            // Between attempts the host answers exactly as it did.
            auth.authorize("owner").expect("unaffected by the gateway");
        }
        assert_eq!(source.refreshes(), 3);
        assert_eq!(logged.len(), 3, "a failure is said, not swallowed");
        assert!(
            logged
                .iter()
                .all(|line| line.contains("the host is unaffected")),
            "{logged:?}"
        );
        assert!(!gateway_reachable(), "a failed refresh is recorded");
        // And a reachable gateway is recorded the same way.
        source.refreshable.store(true, Ordering::SeqCst);
        refresh_once(source.as_ref(), &mut |_| panic!("a success says nothing"));
        assert!(gateway_reachable());
        record_reachable(before);
    }

    /// The loop attempts, keeps attempting, and stops when it is told to —
    /// which is what makes it safe to leave beside a host for months.
    #[test]
    fn the_background_refresher_runs_beside_the_host_and_stops_with_it() {
        let _turn = REACHABILITY.lock().expect("reachability turn");
        let before = gateway_reachable();
        let source = FixtureCredential::held("owner", "device:first");
        let refresh = CredentialRefresh::start(source.clone(), Duration::from_millis(1));
        let started = Instant::now();
        while source.refreshes() < 2 && started.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(source.refreshes() >= 2, "the refresher kept attempting");
        // Dropping the handle joins the thread; a hung one would hang here.
        drop(refresh);
        let attempts = source.refreshes();
        thread::sleep(Duration::from_millis(20));
        assert_eq!(
            attempts,
            source.refreshes(),
            "a stopped refresher is stopped"
        );
        record_reachable(before);
    }
}
