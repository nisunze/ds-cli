//! Managed engine records shared by the Kernel and the process boundary.
//!
//! The Kernel's addon store, the typed process client (`ds-cli-exec`) and the
//! engines that link that client read the same component catalogue, engine
//! build admission, release catalogue and native selection receipts. They live
//! here, types and pure decisions only, so an engine that calls Core's process
//! client does not compile the Kernel. The Kernel re-exports these modules at
//! their old paths (`ds_command_kernel::module_{engine,install,release}`).
pub mod module_engine;
pub mod module_install;
pub mod module_release;
