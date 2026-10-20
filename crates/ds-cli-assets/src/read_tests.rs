use ds_cli_contract::args::parse;
use ds_cli_contract::output::{Format, Output};

use super::*;

fn context() -> Context {
    Context {
        confirmed: false,
        output: Output::resolve(Format::Json, false, true),
    }
}

/// A destination that does not exist, under a directory that does.
fn new_file(label: &str) -> String {
    std::env::temp_dir()
        .join(format!(
            "ds-cli-assets-read-{}-{label}.bin",
            std::process::id()
        ))
        .display()
        .to_string()
}

/// Every local refusal below is proved on the arguments alone: nothing
/// is fetched and no file is written by this test module.
fn unpaired() -> [String; 0] {
    []
}

fn refusal(flags: &[&str]) -> String {
    let mut tokens: Vec<String> = flags.iter().map(|flag| (*flag).to_string()).collect();
    tokens.extend(["--project".to_string(), "test_project".to_string()]);
    let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
    arguments(&inputs)
        .expect_err("a malformed read is refused before any round trip")
        .code()
        .to_string()
}

#[test]
fn the_asset_and_the_destination_are_refused_by_name_before_any_round_trip() {
    assert_eq!(
        refusal(&["--asset", "a_7Kq3nR2v", "--out", &new_file("shape")]),
        "invalid_asset_id"
    );
    for bad in [
        "",
        "  ",
        "Downloads/lot3.pdf",
        "./lot3.pdf",
        "~/lot3.pdf",
        "/home/me/Downloads/",
        "/home/me/../me/lot3.pdf",
    ] {
        assert_eq!(
            refusal(&["--asset", "a_7kq3nr2v0b1c", "--out", bad]),
            "invalid_out_path",
            "`{bad}` was accepted as a destination"
        );
    }
}

#[test]
fn an_existing_destination_or_a_missing_directory_is_refused_here_not_after_a_round_trip() {
    // The desktop shares this filesystem, so what exists here exists
    // there. An existing file — and an existing directory named as the
    // file, and a dangling link — is refused by name; so is a directory
    // that is not there to write into. Both would otherwise cost a
    // project round trip, and possibly a project switch, to learn.
    let existing = new_file("existing");
    std::fs::write(&existing, b"already here").expect("temp file is writable");
    let failure = {
        let mut tokens = ["--asset", "a_7kq3nr2v0b1c", "--out", existing.as_str()]
            .map(str::to_string)
            .to_vec();
        tokens.extend(unpaired());
        tokens.extend(["--project".to_string(), "test_project".to_string()]);
        let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
        arguments(&inputs).expect_err("an existing file is never overwritten")
    };
    let _ = std::fs::remove_file(&existing);
    assert_eq!(failure.code(), "invalid_out_path");
    assert!(
        failure.message().contains("already exists"),
        "the refusal must say the file exists: {}",
        failure.message()
    );

    let directory = std::env::temp_dir().display().to_string();
    assert_eq!(
        refusal(&["--asset", "a_7kq3nr2v0b1c", "--out", &directory]),
        "invalid_out_path",
        "an existing directory named as the file was accepted"
    );

    let missing_parent = std::env::temp_dir()
        .join(format!(
            "ds-cli-assets-read-{}-no-such-dir",
            std::process::id()
        ))
        .join("lot3.pdf")
        .display()
        .to_string();
    assert_eq!(
        refusal(&["--asset", "a_7kq3nr2v0b1c", "--out", &missing_parent]),
        "invalid_out_path",
        "a destination under a missing directory was accepted"
    );
}

#[test]
fn a_well_formed_read_passes_every_local_check() {
    // A member may be named; the only thing left is the catalogue, which
    // a unit test does not reach. A projected `sys:` id passes the local
    // checks too and is refused by name only when its bytes are asked for.
    let out = new_file("well-formed");
    let tokens: Vec<String> = [
        "--asset",
        "sys:design_attachment:att_1:rev_2",
        "--member",
        "Lot3/gis/poles.shp",
        "--out",
        &out,
        "--project",
        "test_project",
    ]
    .map(str::to_string)
    .to_vec();
    let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
    assert!(arguments(&inputs).is_ok());
    let _ = context();
}

#[test]
fn the_payload_carries_exactly_the_keys_the_operation_declares() {
    // The handler is held against its own declaration, with every flag
    // set.
    let out = new_file("payload");
    let mut tokens = [
        "--asset",
        "a_7kq3nr2v0b1c",
        "--member",
        " Lot3/gis/poles.shp ",
        "--out",
        out.as_str(),
    ]
    .map(str::to_string)
    .to_vec();
    tokens.extend(unpaired());
    tokens.extend(["--project".to_string(), "test_project".to_string()]);
    let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
    let payload = arguments(&inputs).expect("valid");
    let mut keys: Vec<&str> = payload
        .as_object()
        .expect("object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, ["asset", "member", "out"]);
    assert_eq!(payload["member"], json!("Lot3/gis/poles.shp"));

    // A whole-asset read sends no member at all rather than an empty one.
    let mut tokens = [
        "--asset",
        "a_7kq3nr2v0b1c",
        "--member",
        "  ",
        "--out",
        out.as_str(),
    ]
    .map(str::to_string)
    .to_vec();
    tokens.extend(unpaired());
    tokens.extend(["--project".to_string(), "test_project".to_string()]);
    let inputs = parse(&COMMAND, &tokens).expect("declared tokens parse");
    let whole = arguments(&inputs).expect("valid");
    assert!(whole.get("member").is_none());
}

#[test]
fn the_receipt_renders_the_path_bytes_and_digest() {
    let out = render(&json!({
        "path": "/home/me/Downloads/EPC-Lot3-signed.pdf",
        "bytes": 811_233,
        "digest": "b1946ac92492d2347c6235b4d2611184",
        "asset_id": "a_7kq3nr2v0b1c",
        "member": null,
    }));
    assert_eq!(
        out,
        "wrote /home/me/Downloads/EPC-Lot3-signed.pdf\n  \
         811233 bytes · sha256 b1946ac92492d2347c6235b4d2611184\n  \
         from a_7kq3nr2v0b1c\n"
    );
    let member = render(&json!({
        "path": "/home/me/Downloads/poles.shp",
        "bytes": 1,
        "digest": "d1",
        "asset_id": "a_7kq3nr2v0b1c",
        "member": "Lot3/gis/poles.shp",
    }));
    assert!(member.contains("1 byte · sha256 d1"));
    assert!(member.ends_with("from a_7kq3nr2v0b1c · Lot3/gis/poles.shp\n"));
}
