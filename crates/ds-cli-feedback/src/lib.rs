//! `ds feedback` records product gaps in the same authenticated backlog as the
//! DS GridDesign `fb` shortcut.
//!
//! This is deliberately a paired-application domain. `ds` never receives a
//! Firebase token and never invents another issue store; the running app sends
//! one typed report through its existing feedback client under the user it has
//! already authenticated. The adapter pins `reporter_kind` to `agent`.

pub mod submit;

use ds_cli_contract::spec::Domain;
use ds_cli_desktop::ops::BridgeOp;

pub static DOMAIN: Domain = Domain {
    id: "feedback",
    summary: "Product feedback: report a CLI gap to the shared backlog.",
    commands: &[&submit::COMMAND],
};

pub const SUBMIT: BridgeOp = BridgeOp {
    operation: "feedback.submit",
    arguments: &[
        "title",
        "detail",
        "component",
        "kind",
        "severity",
        "agent",
        "model",
        "client",
        "evidence",
        "context",
    ],
};

pub const BRIDGE_OPS: &[&BridgeOp] = &[&SUBMIT];
