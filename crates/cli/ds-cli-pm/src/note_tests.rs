use super::*;

#[test]
fn creation_has_one_canonical_required_input() {
    assert_eq!(CREATE.effect, Effect::GlobalWrite);
    assert!(CREATE.args.iter().any(|a| a.name == "input" && a.required));
    assert!(!CREATE.args.iter().any(|a| matches!(
        a.name,
        "title" | "body" | "body-file" | "id" | "transformer"
    )));
    assert!(ds_cli_contract::args::parse(&CREATE, &["--project".into(), "p".into()]).is_err());
}

/// A note takes its files through the record upload; a partial upload is a
/// declared refusal that keeps the note and names the resume command.
#[test]
fn note_creation_declares_its_attachments_and_their_partial_refusal() {
    assert!(
        CREATE
            .args
            .iter()
            .any(|a| a.name == "attach" && a.kind == ArgKind::Repeated && !a.required)
    );
    let codes: Vec<&str> = CREATE.refusals.iter().map(|r| r.code).collect();
    for code in [
        ATTACH_PARTIAL.code,
        crate::task::attach::FILE_INVALID.code,
        crate::record::attach::UNVERIFIED.code,
    ] {
        assert!(codes.contains(&code), "{code}");
    }
    assert!(ATTACH_PARTIAL.remedy.contains("ds pm record attach"));
}
