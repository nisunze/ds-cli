//! Source-history reads must remain bounded, exact and independent of import.

mod common;

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

const CRI: &str = "TYPE='CRI FILE' VERSION='89' UNITS='SI'\n; weather\n2\n'EDT' 0\n'High wind' 0\n'EDT' ; Weather Case for Final After Creep\n; WC for final after load\n2\n'High wind'\n'EDT'\n\n0 ; m_iCSAWantConductorSpecificAfterLoadWC\n";

fn read(path: &Path, limit: &str) -> (Value, i32) {
    common::json(&[
        "dsgrid-exchange",
        "inspect",
        "--source",
        path.to_str().unwrap(),
        "--material-history",
        "--limit",
        limit,
        "--output",
        "json",
    ])
}

#[test]
fn exact_native_history_is_bounded_and_source_bytes_unchanged() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.cri");
    std::fs::write(&path, CRI).unwrap();
    let (answer, status) = read(&path, "1");
    assert_eq!(status, 0, "{answer}");
    let history = &answer["data"]["material_history"];
    let source = &history["sources"][0];
    assert_eq!(
        source["source_sha256"],
        format!("{:x}", Sha256::digest(CRI.as_bytes()))
    );
    let criterion = &source["backups"][0]["criteria"][0];
    assert_eq!(criterion["sha256"], source["source_sha256"]);
    assert_eq!(criterion["history"]["finalAfterCreep"]["rawValue"], "'EDT'");
    assert_eq!(criterion["final_after_load_total"], 2);
    assert_eq!(criterion["final_after_load_omitted"], 1);
    assert_eq!(
        criterion["history"]["finalAfterLoad"]["selectors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        criterion["history"]["finalAfterLoad"]["selectors"][0]["rawValue"],
        "'High wind'"
    );
    assert_eq!(std::fs::read(&path).unwrap(), CRI.as_bytes());
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn malformed_or_undefined_native_history_refuses_even_beyond_limit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("source.cri");
    for text in [
        CRI.replace("\n2\n'High wind'", "\n3\n'High wind'"),
        CRI.replace("'EDT'\n\n0", "'Undeclared'\n\n0"),
        CRI.replace("TYPE='CRI FILE'", "TYPE='DON FILE'"),
    ] {
        std::fs::write(&path, &text).unwrap();
        let (answer, status) = read(&path, "1");
        assert_ne!(status, 0);
        assert_eq!(
            answer["error"]["code"], "native_history_parse_failed",
            "{answer}"
        );
        assert!(
            answer["error"]["remedy"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
        assert_eq!(std::fs::read(&path).unwrap(), text.as_bytes());
    }
    let (answer, status) = read(&path, "0");
    assert_ne!(status, 0);
    assert_eq!(answer["error"]["code"], "invalid_limit");
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
}
