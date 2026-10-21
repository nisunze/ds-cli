//! Core's protected Store runs the Kernel durable stores. `core.store` hands a
//! call here with the protected identity; the Kernel owns the call
//! vocabulary, schema, SQL and transaction (`ds_command_kernel::durable_store`).
use crate::{Error, Result, Store};
use ds_command_kernel::durable_store::{self as kernel, Answer, Call, Identity};

impl Store {
    /// One Kernel durable-store call in one transaction of this Store. Stores
    /// with their own Core command are refused by name.
    pub fn durable_store(
        &mut self,
        identity: &Identity,
        call: Call,
        bytes: Option<Vec<u8>>,
    ) -> Result<Answer> {
        call.request.core_homed().map_err(Error::Kernel)?;
        kernel::execute(&mut self.connection, Some(identity), call, bytes).map_err(Error::Kernel)
    }
}
