//! Regression (24 Oct, 2c544caa): `ds report plan-profile --model` refused
//! `mv_print_asset_missing` unless the engineer had hand-downloaded every
//! approved front-matter PDF. Omitted, Core now reads them through DS assets
//! and admits only the bytes the adopted setup pins.
use super::*;
use sha2::{Digest, Sha256};
use std::cell::RefCell;

fn pinned(index: usize, asset_id: &str, bytes: &[u8]) -> PreservedPage {
    PreservedPage {
        page_index: index,
        asset_id: asset_id.into(),
        sha256: format!("sha256:{:x}", Sha256::digest(bytes)),
    }
}

#[test]
fn omitted_assets_are_read_once_each_and_satisfy_the_setup() {
    let pages = [
        pinned(0, "a_cover", b"%PDF cover"),
        pinned(1, "a_naming", b"%PDF naming"),
        pinned(2, "a_cover", b"%PDF cover"),
    ];
    let reads = RefCell::new(Vec::new());
    let acquired = acquire_publication_assets(&pages, |id| {
        reads.borrow_mut().push(id.to_owned());
        Ok(match id {
            "a_cover" => b"%PDF cover".to_vec(),
            _ => b"%PDF naming".to_vec(),
        })
    })
    .expect("pinned bytes");
    assert_eq!(*reads.borrow(), ["a_cover", "a_naming"]);
    check_publication_assets(&pages, &acquired.map).expect("exactly the setup's assets");
    for (id, bytes) in [
        ("a_cover", &b"%PDF cover"[..]),
        ("a_naming", b"%PDF naming"),
    ] {
        let path = acquired.map[id].as_str().expect("path");
        assert!(std::path::Path::new(path).is_absolute());
        assert_eq!(std::fs::read(path).expect("held"), bytes);
    }
    assert_eq!(acquired.receipt["source"], "ds_assets");
    assert_eq!(acquired.receipt["assets"].as_array().map(Vec::len), Some(2));
}

#[test]
fn bytes_that_differ_from_the_pin_refuse_and_a_read_failure_passes_through() {
    let pages = [pinned(0, "a_cover", b"%PDF approved")];
    let failure = acquire_publication_assets(&pages, |_| Ok(b"%PDF edited".to_vec()))
        .err()
        .expect("digest mismatch refuses");
    assert_eq!(failure.code(), "mv_print_asset_missing");
    assert_eq!(
        failure.detail_value().expect("detail")["asset_id"],
        "a_cover"
    );
    let failure = acquire_publication_assets(&pages, |_| {
        Err(Failure::invalid("asset_not_found", "no such asset"))
    })
    .err()
    .expect("read failure");
    assert_eq!(failure.code(), "asset_not_found");
}

#[test]
fn an_asset_id_never_names_a_path_outside_the_private_directory() {
    let pages = [pinned(0, "../../etc/passwd", b"%PDF x")];
    let acquired = acquire_publication_assets(&pages, |_| Ok(b"%PDF x".to_vec())).expect("ok");
    let path = std::path::PathBuf::from(acquired.map["../../etc/passwd"].as_str().unwrap());
    assert!(
        !path
            .components()
            .any(|c| c == std::path::Component::ParentDir)
    );
}
