//! Project Work attachment reservations whose bytes were never verified are
//! not documents (2026-10-21 Project Work stress: an interrupted upload was
//! listed as a fresh asset with its declared size, and every read of it was
//! refused "attachment is not ready").
//!
//! The recorded answer is outside the parity corpus on purpose: ds-brain
//! pins that corpus to the Kernel its print-layout validator is built from.
//! ds-brain holds a copy of this pair under
//! `internal/services/testdata/assets-index-pm-upload-state.*.json` and answers it
//! the same way.

use std::fs;

use ds_command_kernel::assets::{tree::build_value, TreeRequest};
use serde_json::Value;

const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/assets-tree-pm-upload-state"
);

fn read(name: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(format!("{DIR}/{name}")).unwrap()).unwrap()
}

#[test]
fn uploading_and_failed_reservations_stay_out_of_the_tree() {
    let request: TreeRequest = serde_json::from_value(read("request.json")).unwrap();
    let answer = build_value(&request).unwrap();
    let text = answer.to_string();
    for listed in ["sys:pm_attachment:pa_ready", "sys:pm_attachment:pa_legacy"] {
        assert!(text.contains(listed), "{listed} is listed");
    }
    for hidden in ["pa_uploading", "pa_failed", "interrupted.bin", "reclaimed.bin"] {
        assert!(!text.contains(hidden), "{hidden} is left out");
    }
    if std::env::var_os("DS_ASSETS_TREE_CORPUS_WRITE").is_some() {
        let pretty = serde_json::to_string_pretty(&answer).unwrap();
        fs::write(format!("{DIR}/response.json"), pretty + "\n").unwrap();
    }
    assert_eq!(answer, read("response.json"));
}
