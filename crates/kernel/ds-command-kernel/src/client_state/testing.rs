//! The client state's test host, natively and in the browser's WASM: a
//! scripted gateway that runs the real step machine and holds every request
//! until the scenario answers it, and a preference store kept in memory per
//! identity. Scenarios run on futures' local pool natively and on
//! wasm-bindgen-futures in WASM.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use serde_json::{json, Value};

use super::{ClientState, Effects};
use crate::durable_store::Request as StoreRequest;
use crate::gateway_exchange::{resume, start, Exchange, Facts, Observed, Refusal, Request, Step};
use crate::ui_preferences::{Key, Operation};

/// One request the scripted gateway holds until the scenario answers it.
pub(crate) struct Held {
    request: Request,
    answer: futures_channel::oneshot::Sender<(u16, Value)>,
}

impl Held {
    pub fn body(&self) -> Value {
        self.request.body.clone().unwrap_or(Value::Null)
    }

    pub fn answer(self, status: u16, body: Value) {
        self.answer.send((status, body)).expect("the read awaits its answer");
    }
}

#[derive(Clone, Default)]
pub(crate) struct Host {
    held: Rc<RefCell<VecDeque<Held>>>,
    /// Preference values by identity and key name; `None` is anonymous.
    preferences: Rc<RefCell<BTreeMap<(Option<String>, String), Value>>>,
    /// The identity the store binds, as a host binds the signed-in account.
    pub identity: Rc<RefCell<Option<String>>>,
    store_calls: Rc<RefCell<usize>>,
    /// Every other store: the real durable store on an in-memory SQLite.
    db: Rc<RefCell<Option<rusqlite::Connection>>>,
    nonces: Rc<RefCell<u64>>,
}

impl Host {
    /// One call of the real durable store under the bound identity.
    pub fn call(&self, request: StoreRequest) -> Value {
        self.durable(request, None).expect("the store answers")
    }

    /// Hold a picked file's bytes for its upload, as the page does.
    pub fn admit(&self, path: &str) -> Result<(), Refusal> {
        let project = path.split('/').next().unwrap_or_default();
        let admit = json!({"store": "media_outbox", "operation": {"action": "admit", "project": project, "path": path,
            "surface": "survey-media", "owner_ref": "entry", "content_type": "image/jpeg", "size": 4,
            "generation": "gen-00000001", "payload": {"byte_length": 4}, "now": 1}});
        let request = serde_json::from_value(admit).expect("an admission");
        self.durable(request, Some(vec![1, 2, 3, 4])).map(|_| ())
    }

    fn durable(&self, request: StoreRequest, bytes: Option<Vec<u8>>) -> Result<Value, Refusal> {
        use crate::durable_store::{execute, Call, Identity, Principal};
        let identity = Identity {
            principal: self
                .identity
                .borrow()
                .clone()
                .map_or(Principal::Anonymous, Principal::Account),
            lane: "canary".into(),
            audience: "firebase:ds-test;gateway:https://gateway.example.test".into(),
        };
        let mut db = self.db.borrow_mut();
        let connection = db.get_or_insert_with(|| rusqlite::Connection::open_in_memory().expect("in-memory SQLite"));
        let call = Call {
            request,
            expect_owner: None,
        };
        let answer = execute(connection, Some(&identity), call, bytes)
            .map_err(crate::gateway_exchange::retained::store_refusal)?;
        Ok(serde_json::to_value(&answer).expect("a store answer serializes"))
    }
}

impl Host {
    /// The first held request to `path`.
    pub fn take(&self, path: &str) -> Held {
        let mut held = self.held.borrow_mut();
        let index = held
            .iter()
            .position(|held| held.request.path == path)
            .unwrap_or_else(|| panic!("a request to {path} is held"));
        held.remove(index).expect("the position is held")
    }

    pub fn held(&self) -> usize {
        self.held.borrow().len()
    }

    pub fn sign_in(&self, principal: Option<&str>) {
        *self.identity.borrow_mut() = principal.map(str::to_owned);
    }

    /// What the store holds for `principal` under `key`.
    pub fn preference(&self, principal: Option<&str>, key: &str) -> Option<Value> {
        self.preferences
            .borrow()
            .get(&(principal.map(str::to_owned), key.to_owned()))
            .cloned()
    }

    pub fn hold(&self, principal: Option<&str>, key: &str, value: Value) {
        self.preferences
            .borrow_mut()
            .insert((principal.map(str::to_owned), key.to_owned()), value);
    }

    pub fn store_calls(&self) -> usize {
        *self.store_calls.borrow()
    }

    fn name(key: Key) -> String {
        serde_json::to_value(key)
            .ok()
            .and_then(|name| name.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    fn answer_store(&self, request: StoreRequest) -> Result<Value, Refusal> {
        *self.store_calls.borrow_mut() += 1;
        let identity = self.identity.borrow().clone();
        let StoreRequest::UiPreferences { operation } = request else {
            return self.durable(request, None);
        };
        let mut preferences = self.preferences.borrow_mut();
        match operation {
            Operation::Get { keys, .. } => {
                let values: serde_json::Map<String, Value> = keys
                    .into_iter()
                    .map(|key| {
                        let name = Self::name(key);
                        let value = preferences
                            .get(&(identity.clone(), name.clone()))
                            .cloned()
                            .unwrap_or(Value::Null);
                        (name, value)
                    })
                    .collect();
                Ok(json!({"values": values, "generation": 0, "owner": identity}))
            }
            Operation::Set { key, value, .. } => {
                let name = Self::name(key);
                preferences.insert((identity.clone(), name.clone()), value.clone());
                Ok(json!({"values": {name: value}, "generation": 1, "owner": identity}))
            }
            _ => Err(Refusal::new("unscripted_store", "only get and set are scripted")),
        }
    }
}

impl Effects for Host {
    fn exchange<C: Exchange + 'static>(
        &self,
        call: C,
    ) -> impl Future<Output = Result<C::Output, Refusal>> + use<C> {
        let held = Rc::clone(&self.held);
        async move {
            let facts = Facts {
                now_ms: 0,
                has_session: true,
                offline_mode: false,
            };
            let mut step = start(call);
            loop {
                step = match step {
                    Step::Done(value) => return Ok(value),
                    Step::Refused(refusal) => return Err(refusal),
                    Step::Wait { continuation, .. } => {
                        resume(continuation, Observed::Waited, &[], &facts)
                    }
                    Step::Fetch {
                        request,
                        continuation,
                    } => {
                        let (answer, answered) = futures_channel::oneshot::channel();
                        held.borrow_mut().push_back(Held { request, answer });
                        let (status, body) = answered.await.expect("the scenario answers");
                        let observed = Observed::Response {
                            status,
                            content_type: Some("application/json".into()),
                            retry_after: None,
                        };
                        resume(continuation, observed, body.to_string().as_bytes(), &facts)
                    }
                }
            }
        }
    }

    fn store(&self, request: StoreRequest) -> impl Future<Output = Result<Value, Refusal>> + use<> {
        let answer = self.answer_store(request);
        async move { answer }
    }

    fn now_ms(&self) -> u64 {
        1_760_000_000_000
    }

    fn nonce(&self) -> String {
        let mut nonces = self.nonces.borrow_mut();
        *nonces += 1;
        format!("{:032x}", *nonces)
    }
}

pub(crate) fn state() -> (ClientState<Host>, Host) {
    let host = Host::default();
    (ClientState::new(host.clone()), host)
}

/// Lets every task the last change woke run to its next pause.
pub(crate) async fn settle() {
    struct Yield(bool);
    impl Future for Yield {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                return Poll::Ready(());
            }
            self.0 = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
    for _ in 0..16 {
        Yield(false).await;
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run(scenario: impl Future<Output = ()> + 'static) {
    use any_spawner::Executor;
    use std::cell::Cell;
    let _ = Executor::init_futures_executor();
    let done = Rc::new(Cell::new(false));
    Executor::spawn_local({
        let done = Rc::clone(&done);
        async move {
            scenario.await;
            done.set(true);
        }
    });
    Executor::poll_local();
    assert!(done.get(), "the scenario stalled");
}

/// A test natively and under wasm-bindgen-test.
macro_rules! scenario {
    ($(#[$meta:meta])* async fn $name:ident() $body:block) => {
        $(#[$meta])*
        #[cfg(not(target_arch = "wasm32"))]
        #[test]
        fn $name() {
            $crate::client_state::testing::run(async $body);
        }

        $(#[$meta])*
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen_test::wasm_bindgen_test]
        async fn $name() {
            let _ = any_spawner::Executor::init_wasm_bindgen();
            async $body.await;
        }
    };
}
pub(crate) use scenario;
