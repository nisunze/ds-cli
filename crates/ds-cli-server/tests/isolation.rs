mod fixtures;
use fixtures::*;

#[test]
fn probe() {
    let host = Host::start(&[A, B, C], limits(), A);
    // No project named at all, on the wire: the sealed envelope names its own.
    let answer = host.raw("POST", "/v1/solar-processing/s1", Some(&solar_submission()));
    eprintln!("status {} ctx {}", answer.status, answer.json()["job"]["context"]);
    let sealed = host.input("solar.json", &solar_submission());
    let solar = ds_cli_server::solar_submit(
        &host.args(&ds_cli_server::SOLAR_SUBMIT, &["--key", "s2", "--input", &sealed, "--project", A]),
        &context(),
    )
    .expect("solar submitted");
    eprintln!("cli solar context: {}", solar["job"]["context"]);
    let mismatch = ds_cli_server::solar_submit(
        &host.args(&ds_cli_server::SOLAR_SUBMIT, &["--key", "s3", "--input", &sealed, "--project", C]),
        &context(),
    )
    .expect_err("sealed project wins");
    eprintln!("mismatch: {} / {}", mismatch.code(), mismatch.message());
    // Layers, on the wire.
    let hide = host.raw("POST", &format!("/v1/layers/visibility?project={A}"),
        Some(br#"{"layers":["survey/poles"],"visible":false}"#));
    eprintln!("layer hide {} {}", hide.status, String::from_utf8_lossy(&hide.body));
    let other = host.raw("POST", &format!("/v1/layers/visibility?project={B}"),
        Some(br#"{"layers":["survey/poles"],"visible":false}"#));
    eprintln!("layer other {} {}", other.status, String::from_utf8_lossy(&other.body));
}
