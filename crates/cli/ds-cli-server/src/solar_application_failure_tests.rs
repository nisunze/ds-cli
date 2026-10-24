use super::*;
use ds_solar_host::process::final_lint::Error;

#[test]
fn a_named_solar_capability_refusal_survives_server_and_cli_transport() {
    let error = application_failure(Error::Task(Box::new(
        Failure::unavailable("solar_engine_update_required", "fixture capability refused")
            .detail(serde_json::json!({"task":"final.lint"})),
    )));
    assert_eq!(error.code(), "solar_engine_update_required");
    assert_eq!(error.detail_value().unwrap()["task"], "final.lint");
    let (status, Json(body)) = crate::host::typed(&error);
    let received = crate::typed_refusal(status.as_u16(), &serde_json::to_vec(&body).unwrap());
    assert_eq!(received.code(), "solar_engine_update_required");
    assert_eq!(received.class(), ds_cli_contract::ExitClass::Unavailable);
    assert_eq!(received.detail_value().unwrap()["task"], "final.lint");
    // Arbitrary source prose never becomes a product refusal discriminator.
    let local = application_failure(Error::Local(
        "solar_engine_update_required: authored text".into(),
    ));
    assert_eq!(local.code(), "server_refused");
}
