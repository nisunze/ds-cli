//! The page's one session ([`super::Session`]) and its typed exports.
//!
//! ds-web calls `authSessionStart` with the profile's StoreHost (the browser's
//! storage worker on every surface, never Core), subscribes to
//! `authSessionSignal` and renders it. Sign-in, sign-up, Google's redirect and
//! sign-out are intents; every request's bearer is read here by the gateway
//! executor ([`credentials`]), refreshed first when it is due, so no token
//! crosses to TypeScript.

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Function, Promise, Reflect};
use reactive_graph::computed::ArcMemo;
use reactive_graph::signal::ArcRwSignal;
use reactive_graph::traits::{Get, Set};
use serde::Serialize;
use tsify::Tsify;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{future_to_promise, spawn_local, JsFuture};

use super::google::{self, Taken};
use super::store::{Operation, ENDED};
use super::{
    restore, Answer, AuthSessionView, Credentials, Persist, Plan, Record, RefreshCall, Session,
    NOT_STARTED,
};
use crate::client_state::browser::StateSignal;
use crate::client_state::{Demand, OnDemand};
use crate::durable_store::port::{PageStore, StoreHost, StorePort};
use crate::durable_store::Request;
use crate::gateway_exchange::browser::{typed, GatewayCredentials};
use crate::gateway_exchange::firebase_auth::exports::{
    now_epoch_seconds, open, refresh, rejection,
};
use crate::gateway_exchange::firebase_auth::{AuthRefusal, Call};
use crate::gateway_exchange::{session_changed_refusal, Refusal};
use crate::gateway_target::{FIREBASE_PROJECT, GOOGLE_CLIENT_ID};

/// A refresh in flight: its session generation, the promise every waiting
/// request awaits, and what it decided.
struct InFlight {
    generation: u64,
    promise: Promise,
    answer: Rc<RefCell<Option<Answer>>>,
}

struct Page {
    store: PageStore,
    session: RefCell<Session>,
    restored: RefCell<Option<Promise>>,
    refreshing: RefCell<Option<InFlight>>,
}

thread_local! {
    static PAGE: RefCell<Option<Rc<Page>>> = const { RefCell::new(None) };
    static VIEW: ArcRwSignal<AuthSessionView> = ArcRwSignal::new(AuthSessionView::Restoring);
}

fn page() -> Option<Rc<Page>> {
    PAGE.with(|held| held.borrow().clone())
}

fn started() -> Result<Rc<Page>, JsValue> {
    page().ok_or_else(|| {
        rejection(
            &AuthRefusal {
                code: NOT_STARTED.into(),
                message: "This page has not started its session yet.".into(),
            },
            None,
        )
    })
}

fn publish(view: AuthSessionView) {
    VIEW.with(|signal| signal.set(view));
}

/// 32 lowercase hex digits of fresh randomness.
fn token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn warn(message: &str) {
    web_sys::console::warn_1(&JsValue::from_str(message));
}

fn google_unavailable() -> AuthRefusal {
    AuthRefusal {
        code: google::UNAVAILABLE.into(),
        message: "Google sign-in needs this browser's storage, which is unavailable.".into(),
    }
}

/// The page's origin (`location.origin`).
fn page_origin() -> Option<String> {
    Reflect::get(&js_sys::global(), &"location".into())
        .and_then(|location| Reflect::get(&location, &"origin".into()))
        .ok()?
        .as_string()
}

/// The callback's fragment, removed from the address bar and the history
/// entry before anything else reads it.
fn take_fragment() -> String {
    let global = js_sys::global();
    let Ok(location) = Reflect::get(&global, &"location".into()) else {
        return String::new();
    };
    let text = |key: &str| {
        Reflect::get(&location, &key.into())
            .ok()
            .and_then(|value| value.as_string())
            .unwrap_or_default()
    };
    let hash = text("hash");
    if hash.is_empty() {
        return hash;
    }
    let path = format!("{}{}", text("pathname"), text("search"));
    if let Ok(history) = Reflect::get(&global, &"history".into()) {
        let replace = Reflect::get(&history, &"replaceState".into())
            .ok()
            .and_then(|function| function.dyn_into::<Function>().ok());
        if let Some(replace) = replace {
            let state = Reflect::get(&history, &"state".into()).unwrap_or(JsValue::NULL);
            let _ = replace.call3(
                &history,
                &state,
                &JsValue::from_str(""),
                &JsValue::from_str(&path),
            );
        }
    }
    hash
}

impl Page {
    async fn call(&self, operation: Operation) -> Result<serde_json::Value, String> {
        self.store
            .call(Request::AuthSession { operation }, None, None)
            .await
            .map(|reply| reply.answer)
    }

    /// Wait for the held record to be read.
    async fn ready(&self) {
        let restored = self.restored.borrow().clone();
        if let Some(restored) = restored {
            let _ = JsFuture::from(restored).await;
        }
    }

    /// Read the held record and adopt it.
    async fn restore(self: Rc<Self>) {
        let stored = match self.call(Operation::Read).await {
            Ok(answer) => serde_json::from_value::<Option<Record>>(answer["session"].clone())
                .ok()
                .flatten(),
            Err(error) => {
                warn(&format!(
                    "auth session: the held record could not be read: {error}"
                ));
                None
            }
        };
        let plan = restore(stored, FIREBASE_PROJECT);
        if let Some(session) = plan.forget {
            let _ = self.call(Operation::SignOut { session }).await;
        }
        let stored = plan.adopt.is_some();
        let view = {
            let mut session = self.session.borrow_mut();
            session.restored(plan.adopt, stored);
            session.view()
        };
        publish(view);
    }

    /// Keep what a decision changed. A refresh the store refuses as ended
    /// means another tab signed out or in: this page follows.
    async fn persist(self: Rc<Self>, generation: u64, persist: Persist) {
        let (operation, establishing) = match persist {
            Persist::SignIn(record) => (Operation::SignIn { record }, true),
            Persist::Refresh(record) => (Operation::Refresh { record }, false),
            Persist::SignOut(session) => (Operation::SignOut { session }, false),
        };
        match self.call(operation).await {
            Ok(_) if establishing => self.session.borrow_mut().stored(generation),
            Ok(_) => {}
            Err(error) if error.starts_with(ENDED) => {
                let view = {
                    let mut session = self.session.borrow_mut();
                    session.ended_elsewhere(generation).then(|| session.view())
                };
                if let Some(view) = view {
                    publish(view);
                }
            }
            Err(error) => warn(&format!(
                "auth session: the record could not be kept: {error}"
            )),
        }
    }

    /// Bind a validated sign-in as this page's session and keep it.
    async fn bind(self: &Rc<Self>, login: crate::ValidatedLogin) -> AuthSessionView {
        // A restore still running must finish before this sign-in.
        self.ready().await;
        let record = Record::from_login(&login, &token(), now_epoch_seconds());
        let (persist, generation, view) = {
            let mut session = self.session.borrow_mut();
            let persist = session.sign_in(record);
            (persist, session.generation(), session.view())
        };
        publish(view.clone());
        Rc::clone(self).persist(generation, persist).await;
        view
    }

    /// The bearer for the next request: none when signed out, the held token
    /// when fresh, otherwise a refresh's (one per session at a time).
    async fn bearer(self: &Rc<Self>, mut force: bool) -> Result<Option<Credentials>, Refusal> {
        self.ready().await;
        for _ in 0..4 {
            let plan = self.session.borrow().plan(now_epoch_seconds(), force);
            match plan {
                Plan::SignedOut => return Ok(None),
                Plan::Use(credentials) => return Ok(Some(credentials)),
                Plan::Refresh(call) => match self.refresh(call).await {
                    Answer::Bearer(credentials) => return Ok(Some(credentials)),
                    Answer::Refused(refusal) => return Err(*refusal),
                    Answer::Superseded => force = false,
                },
            }
        }
        Err(session_changed_refusal())
    }

    async fn refresh(self: &Rc<Self>, call: RefreshCall) -> Answer {
        let generation = call.generation;
        let joined = self
            .refreshing
            .borrow()
            .as_ref()
            .filter(|flight| flight.generation == generation)
            .map(|flight| (flight.promise.clone(), Rc::clone(&flight.answer)));
        let (promise, answer) = match joined {
            Some(joined) => joined,
            None => {
                let answer = Rc::new(RefCell::new(None));
                let (page, slot) = (Rc::clone(self), Rc::clone(&answer));
                let promise = future_to_promise(async move {
                    let outcome = refresh(&call.refresh_token, &call.uid, &call.email).await;
                    let (decided, persist, view) = {
                        let mut session = page.session.borrow_mut();
                        let (decided, persist) =
                            session.refreshed(generation, outcome, now_epoch_seconds());
                        (decided, persist, session.view())
                    };
                    {
                        let mut flight = page.refreshing.borrow_mut();
                        if flight
                            .as_ref()
                            .is_some_and(|flight| flight.generation == generation)
                        {
                            *flight = None;
                        }
                    }
                    *slot.borrow_mut() = Some(decided);
                    publish(view);
                    if let Some(persist) = persist {
                        spawn_local(Rc::clone(&page).persist(generation, persist));
                    }
                    Ok(JsValue::UNDEFINED)
                });
                *self.refreshing.borrow_mut() = Some(InFlight {
                    generation,
                    promise: promise.clone(),
                    answer: Rc::clone(&answer),
                });
                (promise, answer)
            }
        };
        let _ = JsFuture::from(promise).await;
        let decided = answer.borrow().clone();
        decided.unwrap_or(Answer::Superseded)
    }
}

impl From<Credentials> for GatewayCredentials {
    fn from(credentials: Credentials) -> Self {
        Self {
            token: Some(credentials.token.clone()),
            email: credentials.email.clone(),
        }
    }
}

/// The session's bearer and email for the gateway executor's next request:
/// none when the page never started a session or nobody is signed in.
pub(crate) async fn credentials() -> Result<GatewayCredentials, Refusal> {
    let Some(page) = page() else {
        return Ok(GatewayCredentials::default());
    };
    Ok(page
        .bearer(false)
        .await?
        .map(Into::into)
        .unwrap_or_default())
}

/// Start this page's session with the profile's store host: read the held
/// record. A second call answers the first call's promise.
#[wasm_bindgen(js_name = authSessionStart, unchecked_return_type = "Promise<void>")]
pub fn start(store: StoreHost) -> Promise {
    if let Some(page) = page() {
        let restored = page.restored.borrow().clone();
        return restored.unwrap_or_else(|| Promise::resolve(&JsValue::UNDEFINED));
    }
    let page = Rc::new(Page {
        store: PageStore::new(store),
        session: RefCell::new(Session::new()),
        restored: RefCell::new(None),
        refreshing: RefCell::new(None),
    });
    PAGE.with(|held| *held.borrow_mut() = Some(Rc::clone(&page)));
    let restoring = Rc::clone(&page);
    let promise = future_to_promise(async move {
        restoring.restore().await;
        Ok(JsValue::UNDEFINED)
    });
    *page.restored.borrow_mut() = Some(promise.clone());
    promise
}

/// The session as the page renders it: restoring, signed out (with why the
/// last session ended, when the person did not end it) or signed in.
#[wasm_bindgen(js_name = authSessionSignal, unchecked_return_type = "StateSignal<AuthSessionView>")]
pub fn signal() -> StateSignal {
    let view = VIEW.with(Clone::clone);
    StateSignal::of(&OnDemand {
        value: ArcMemo::new(move |_: Option<&AuthSessionView>| view.get()),
        demand: Demand::default(),
    })
}

/// Sign in with an email and password; rejects as `FirebaseAuthRefusal`.
#[wasm_bindgen(js_name = authSessionSignIn, unchecked_return_type = "AuthSessionView")]
pub async fn sign_in(email: String, password: String) -> Result<JsValue, JsValue> {
    let page = started()?;
    let login = open(
        Call::SignIn {
            email: &email,
            password: &password,
        },
        Some(&email),
    )
    .await?;
    typed(&page.bind(login).await)
}

/// Create an account and sign in to it; rejects as `FirebaseAuthRefusal`.
#[wasm_bindgen(js_name = authSessionSignUp, unchecked_return_type = "AuthSessionView")]
pub async fn sign_up(email: String, password: String) -> Result<JsValue, JsValue> {
    let page = started()?;
    let login = open(
        Call::SignUp {
            email: &email,
            password: &password,
        },
        Some(&email),
    )
    .await?;
    typed(&page.bind(login).await)
}

/// The session a sign-out ended, held only for the cleanup that still speaks
/// as it (`pushRevoke`). The page can read nothing from it.
#[wasm_bindgen]
pub struct EndedSession {
    credentials: GatewayCredentials,
}

impl EndedSession {
    pub(crate) fn credentials(&self) -> &GatewayCredentials {
        &self.credentials
    }
}

/// The person signs out: the view turns signed out before this returns and
/// the profile's record is removed. Answers the ended session, if one was.
#[wasm_bindgen(js_name = authSessionSignOut)]
pub fn sign_out() -> Option<EndedSession> {
    let page = page()?;
    let (ended, generation, view) = {
        let mut session = page.session.borrow_mut();
        let ended = session.sign_out();
        (ended, session.generation(), session.view())
    };
    publish(view);
    let record = ended?;
    spawn_local(Rc::clone(&page).persist(generation, Persist::SignOut(record.session.clone())));
    Some(EndedSession {
        credentials: GatewayCredentials {
            token: Some(record.id_token.clone()),
            email: record.user.email.clone(),
        },
    })
}

/// A request was refused as unauthenticated: refresh `expected_uid`'s token
/// even though it looked fresh. `true` when a new bearer resulted.
#[wasm_bindgen(js_name = authSessionForceRefresh)]
pub async fn force_refresh(expected_uid: String) -> Result<bool, JsValue> {
    let Some(page) = page() else {
        return Ok(false);
    };
    page.ready().await;
    if page.session.borrow().uid() != Some(expected_uid.as_str()) {
        return Ok(false);
    }
    Ok(matches!(page.bearer(true).await, Ok(Some(credentials)) if credentials.uid == expected_uid))
}

/// Start Google's redirect sign-in: hold a fresh state and nonce for this
/// profile and answer the authorization URL the page navigates to.
/// `callback_path` is this site's `/auth/google` route below its mount;
/// `return_path` is the page the person returns to.
#[wasm_bindgen(js_name = authGoogleBegin)]
pub async fn google_begin(callback_path: String, return_path: String) -> Result<String, JsValue> {
    let page = started()?;
    page.ready().await;
    let origin = page_origin().unwrap_or_default();
    let (redirect, url) = google::begin(
        GOOGLE_CLIENT_ID,
        &origin,
        &callback_path,
        &return_path,
        &token(),
        &token(),
        now_epoch_seconds(),
    )
    .map_err(|refused| rejection(&refused, None))?;
    page.call(Operation::BeginRedirect { redirect })
        .await
        .map_err(|_| rejection(&google_unavailable(), None))?;
    Ok(url)
}

/// A finished Google sign-in: the session and the page to return to.
#[derive(Serialize, Tsify)]
#[serde(rename_all = "camelCase")]
pub struct AuthGoogleCompletion {
    pub view: AuthSessionView,
    pub return_path: String,
}

/// Complete Google's redirect on its callback route: take the answer from
/// the address, check it against the pending redirect, exchange it with
/// Firebase and bind the session. Rejects as `FirebaseAuthRefusal`.
#[wasm_bindgen(js_name = authGoogleComplete, unchecked_return_type = "AuthGoogleCompletion")]
pub async fn google_complete() -> Result<JsValue, JsValue> {
    let page = started()?;
    page.ready().await;
    let callback =
        google::parse_callback(&take_fragment()).map_err(|refused| rejection(&refused, None))?;
    let answer = page
        .call(Operation::TakeRedirect {
            state: callback.state.clone(),
        })
        .await
        .map_err(|_| rejection(&google_unavailable(), None))?;
    let taken: Taken =
        serde_json::from_value(answer).map_err(|_| rejection(&google_unavailable(), None))?;
    let (id_token, pending) =
        google::verify(taken, callback, GOOGLE_CLIENT_ID, now_epoch_seconds())
            .map_err(|refused| rejection(&refused, None))?;
    let request_uri = google::request_uri(&pending);
    let login = open(
        Call::Google {
            id_token: &id_token,
            request_uri: &request_uri,
        },
        None,
    )
    .await?;
    let view = page.bind(login).await;
    typed(&AuthGoogleCompletion {
        view,
        return_path: pending.return_path.clone(),
    })
}
