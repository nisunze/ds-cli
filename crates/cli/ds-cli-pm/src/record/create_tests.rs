use super::*;

fn parse(tokens: &[&str]) -> Inputs {
    let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
    ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
}

const RECORD: [&str; 10] = [
    "--project",
    "p",
    "--category",
    "project_note",
    "--channel",
    "other",
    "--direction",
    "internal",
    "--subject",
    "April reference",
];

/// Every file is read and bounded before the record is filed: a missing,
/// empty or duplicated file refuses with no record and no upload.
#[test]
fn attachments_are_checked_before_anything_is_filed() {
    let root = std::env::temp_dir().join(format!("pm-record-create-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let good = root.join("EPCGIS_1.RAR");
    std::fs::write(&good, b"Rar!\x1a\x07\x01\x00").unwrap();
    let other = root.join("index.md");
    std::fs::write(&other, b"# index").unwrap();
    let good = good.to_str().unwrap().to_owned();
    let other = other.to_str().unwrap().to_owned();
    let absent = root.join("absent.pdf").to_str().unwrap().to_owned();

    let mut two = RECORD.to_vec();
    two.extend(["--attach", good.as_str(), "--attach", other.as_str()]);
    let files = attachments(&parse(&two)).expect("two distinct files");
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].name(), "EPCGIS_1.RAR");

    let mut duplicate = RECORD.to_vec();
    duplicate.extend(["--attach", good.as_str(), "--attach", good.as_str()]);
    let mut missing = RECORD.to_vec();
    missing.extend(["--attach", absent.as_str()]);
    for tokens in [duplicate, missing] {
        let error = attachments(&parse(&tokens)).unwrap_err();
        assert_eq!(error.code(), crate::task::attach::FILE_INVALID.code);
    }
    assert!(attachments(&parse(&RECORD)).unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_partial_upload_is_a_declared_refusal_that_names_the_resume_command() {
    let codes: Vec<&str> = COMMAND
        .refusals
        .iter()
        .map(|refusal| refusal.code)
        .collect();
    assert!(codes.contains(&ATTACH_PARTIAL.code));
    assert!(ATTACH_PARTIAL.remedy.contains("ds pm record attach"));
    assert!(
        COMMAND
            .args
            .iter()
            .any(|arg| arg.name == "attach" && arg.kind == ArgKind::Repeated)
    );
}
