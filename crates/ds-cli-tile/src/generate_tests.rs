use super::*;

#[test]
fn generation_refuses_without_reaching_any_service() {
    let inputs = ds_cli_contract::parse(
        &COMMAND,
        &[
            "--project".into(),
            "p-test".into(),
            "--type".into(),
            "design".into(),
        ],
    )
    .expect("declared arguments parse");
    let context = Context {
        confirmed: true,
        output: ds_cli_contract::Output::resolve(ds_cli_contract::Format::Json, false, true),
    };
    let failure = run(&inputs, &context).expect_err("hosted tiling is retired");
    assert_eq!(failure.code(), "native_compute_required");
    assert!(
        failure
            .remedy_text()
            .is_some_and(|remedy| remedy.contains("ds core tile submit"))
    );
    assert!(
        COMMAND
            .refusals
            .iter()
            .any(|refusal| refusal.code == failure.code()),
        "the refusal is declared"
    );
}
