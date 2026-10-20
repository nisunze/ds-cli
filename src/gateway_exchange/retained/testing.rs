//! A test edge: the real durable store on an in-memory SQLite, and a gateway
//! that answers from a script while recording every request a flow sends.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::future::Future;
use std::task::{Context, Poll, Waker};

use rusqlite::Connection;
use serde_json::Value;

use super::{store_refusal, Edge};
use crate::durable_store::{execute, Call, Identity, Principal};
use crate::gateway_exchange::{
    offline_refusal, resume, start, Exchange, Facts, Failure, Observed, Refusal, Request, Step,
};

/// One scripted gateway answer.
#[derive(Clone, Debug)]
pub(crate) enum Answer {
    /// A JSON body with this status.
    Json(u16, Value),
    /// No answer arrived.
    Failed(Failure),
}

pub(crate) struct TestEdge {
    pub db: RefCell<Connection>,
    pub identity: RefCell<Identity>,
    pub online: Cell<bool>,
    pub now: Cell<u64>,
    pub answers: RefCell<VecDeque<Answer>>,
    pub sent: RefCell<Vec<Request>>,
}

pub(crate) fn account(principal: &str) -> Identity {
    Identity {
        principal: Principal::Account(principal.into()),
        lane: "canary".into(),
        audience: "firebase:ds-test;gateway:https://gateway.example.test".into(),
    }
}

impl TestEdge {
    pub fn new() -> Self {
        Self {
            db: RefCell::new(Connection::open_in_memory().expect("in-memory SQLite")),
            identity: RefCell::new(account("owner")),
            online: Cell::new(true),
            now: Cell::new(1_760_000_000_000),
            answers: RefCell::new(VecDeque::new()),
            sent: RefCell::new(Vec::new()),
        }
    }
    pub fn answer(&self, answer: Answer) -> &Self {
        self.answers.borrow_mut().push_back(answer);
        self
    }
    pub fn json(&self, body: Value) -> &Self {
        self.answer(Answer::Json(200, body))
    }
    /// Every request sent so far.
    pub fn requests(&self) -> Vec<Request> {
        self.sent.borrow().clone()
    }
    pub fn sign_in(&self, principal: &str) {
        *self.identity.borrow_mut() = account(principal);
    }
    fn facts(&self) -> Facts {
        Facts {
            now_ms: self.now.get(),
            has_session: true,
            offline_mode: !self.online.get(),
        }
    }
}

impl Edge for TestEdge {
    async fn fetch<C: Exchange>(&self, call: C) -> Result<C::Output, Refusal> {
        let mut step = start(call);
        loop {
            step = match step {
                Step::Done(value) => return Ok(value),
                Step::Refused(refusal) => return Err(refusal),
                Step::Wait { continuation, .. } => {
                    resume(continuation, Observed::Waited, &[], &self.facts())
                }
                Step::Fetch {
                    request,
                    continuation,
                } => {
                    if !self.online.get() {
                        return Err(offline_refusal());
                    }
                    self.sent.borrow_mut().push(request);
                    let answer = self
                        .answers
                        .borrow_mut()
                        .pop_front()
                        .expect("the flow sent an unscripted request");
                    let (observed, body) = match answer {
                        Answer::Json(status, body) => (
                            Observed::Response {
                                status,
                                content_type: Some("application/json".into()),
                                retry_after: None,
                            },
                            body.to_string().into_bytes(),
                        ),
                        Answer::Failed(cause) => (
                            Observed::Failure {
                                cause,
                                message: None,
                            },
                            Vec::new(),
                        ),
                    };
                    resume(continuation, observed, &body, &self.facts())
                }
            };
        }
    }

    async fn store(&self, call: Call) -> Result<Value, Refusal> {
        let answer = execute(
            &mut self.db.borrow_mut(),
            Some(&self.identity.borrow()),
            call,
            None,
        )
        .map_err(store_refusal)?;
        Ok(serde_json::to_value(&answer).expect("a store answer serializes"))
    }

    fn now_ms(&self) -> u64 {
        self.now.get()
    }

    fn online(&self) -> bool {
        self.online.get()
    }
}

/// Run a flow on a test edge to its end. A test edge never pends, so one
/// poll finishes the flow.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("a flow on a test edge never waits"),
    }
}
