//! How a Kernel flow reaches storage, on every host. A flow that interleaves
//! storage with gateway exchanges holds one [`StorePort`] and issues typed
//! calls; the port decides nothing. In the page the port is the host's
//! StoreHost (the storage worker on the Web, `core.store` on the Desktop);
//! natively it is a connection to the protected Store.
use super::{execute, Call, Identity, Request};
use rusqlite::Connection;
use serde_json::Value;
use std::cell::RefCell;

/// One answer: the Answer JSON (store fields, `generation`, `owner`,
/// `effects`) and its payload bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub answer: Value,
    pub bytes: Option<Vec<u8>>,
}
impl Reply {
    /// The `owner` this answer carries, for the flow's later `expect_owner`.
    pub fn owner(&self) -> Option<&str> {
        self.answer.get("owner").and_then(Value::as_str)
    }
}

/// A flow's storage door. `expect_owner` is the `owner` of the flow's first
/// answer, so a write after an identity change is refused, never misfiled.
#[allow(async_fn_in_trait)]
pub trait StorePort {
    async fn call(
        &self,
        request: Request,
        expect_owner: Option<String>,
        bytes: Option<Vec<u8>>,
    ) -> Result<Reply, String>;
}

/// The native port: one protected connection and the identity its host binds.
pub struct Direct<'a> {
    connection: RefCell<&'a mut Connection>,
    identity: Identity,
}
impl<'a> Direct<'a> {
    pub fn new(connection: &'a mut Connection, identity: Identity) -> Self {
        Self {
            connection: RefCell::new(connection),
            identity,
        }
    }
}
impl StorePort for Direct<'_> {
    async fn call(
        &self,
        request: Request,
        expect_owner: Option<String>,
        bytes: Option<Vec<u8>>,
    ) -> Result<Reply, String> {
        // The statement-protocol stores carry their callers' complete scopes.
        let identity = match request {
            Request::Status { .. } | Request::GridProjectCatalog { .. } => None,
            _ => Some(&self.identity),
        };
        let answer = execute(
            &mut self.connection.borrow_mut(),
            identity,
            Call {
                request,
                expect_owner,
            },
            bytes,
        )?;
        Ok(Reply {
            bytes: answer.bytes.clone(),
            answer: serde_json::to_value(&answer).map_err(|e| e.to_string())?,
        })
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
pub use page::{PageStore, StoreHost};

/// The page's port: the host object `$lib/storage/kernel-store.ts` passes
/// beside GatewayHost.
#[cfg(all(target_arch = "wasm32", feature = "wasm"))]
mod page {
    use super::{Reply, StorePort};
    use crate::durable_store::{Call, Request};
    use js_sys::{Reflect, Uint8Array};
    use wasm_bindgen::{prelude::*, JsCast};
    use wasm_bindgen_futures::JsFuture;

    #[wasm_bindgen]
    extern "C" {
        /// `transaction(call: Uint8Array, bytes: Uint8Array | null):
        /// Promise<{answer: Uint8Array; bytes: Uint8Array | null}>`.
        pub type StoreHost;
        #[wasm_bindgen(method, catch)]
        fn transaction(
            this: &StoreHost,
            call: Uint8Array,
            bytes: Option<Uint8Array>,
        ) -> Result<js_sys::Promise, JsValue>;
    }

    fn message(error: JsValue) -> String {
        error
            .as_string()
            .or_else(|| {
                error
                    .dyn_ref::<js_sys::Error>()
                    .map(|error| String::from(error.message()))
            })
            .unwrap_or_else(|| "the store host failed".into())
    }

    pub struct PageStore {
        host: StoreHost,
    }
    impl PageStore {
        pub fn new(host: StoreHost) -> Self {
            Self { host }
        }
    }
    impl StorePort for PageStore {
        async fn call(
            &self,
            request: Request,
            expect_owner: Option<String>,
            bytes: Option<Vec<u8>>,
        ) -> Result<Reply, String> {
            let call = serde_json::to_vec(&Call {
                request,
                expect_owner,
            })
            .map_err(|e| e.to_string())?;
            let promise = self
                .host
                .transaction(
                    Uint8Array::from(call.as_slice()),
                    bytes.as_deref().map(Uint8Array::from),
                )
                .map_err(message)?;
            let reply = JsFuture::from(promise).await.map_err(message)?;
            let answer = Reflect::get(&reply, &"answer".into())
                .ok()
                .and_then(|value| value.dyn_into::<Uint8Array>().ok())
                .ok_or("the store host answered no answer bytes")?;
            let bytes = Reflect::get(&reply, &"bytes".into())
                .ok()
                .and_then(|value| value.dyn_into::<Uint8Array>().ok())
                .map(|bytes| bytes.to_vec());
            Ok(Reply {
                answer: serde_json::from_slice(&answer.to_vec()).map_err(|e| e.to_string())?,
                bytes,
            })
        }
    }
}
