use super::*;

#[test]
fn local_report_defaults_to_stable_and_forwards_requested_addon_lane() {
    for lane in [None, Some("canary")] {
        let mut args: Vec<String> = [
            "--workspace",
            "offline-workspace",
            "--run-id",
            "run-1",
            "--transformer",
            "T1",
            "--out-dir",
            "delivery",
            "--country",
            "Rwanda",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        if let Some(lane) = lane {
            args.extend(["--lane".into(), lane.into()]);
        }
        let inputs = ds_cli_contract::args::parse(&REPORT, &args).unwrap();
        let expected = lane.unwrap_or("stable");
        assert_eq!(inputs.require("lane").unwrap(), expected);
        let forwarded = report_export_inputs(
            Path::new("delivery/report-request.json"),
            Path::new("delivery"),
            inputs.require("lane").unwrap(),
        )
        .unwrap();
        assert_eq!(forwarded.require("lane").unwrap(), expected);
        assert_eq!(forwarded.require("task").unwrap(), "transformer");
        assert_eq!(
            forwarded.require("request").unwrap(),
            "delivery/report-request.json"
        );
        assert_eq!(
            Path::new(forwarded.require("result").unwrap()),
            Path::new("delivery").join("report-result.json")
        );
    }
    assert_eq!(REPORT.authority, Authority::None);
    assert_eq!(REPORT.contract, 2);
    assert_eq!(INIT.contract, 1);
}

#[test]
fn saved_analysis_refusals_keep_exact_codes_and_unknown_codes_stay_invalid() {
    for code in [
        "voltage_drop_result_missing",
        "voltage_drop_result_invalid",
        "voltage_drop_result_stale",
    ] {
        let message = format!("{code}: actual saved producer diagnostic");
        let failure = report_input_error(Error::Invalid(message.clone()));
        assert_eq!(failure.code(), code);
        assert_eq!(failure.message(), message);
        assert!(
            failure
                .remedy_text()
                .unwrap()
                .contains("cannot calculate or reconstruct")
        );
    }
    for message in [
        "voltage_drop_result_future: unknown",
        " voltage_drop_result_missing: padded",
    ] {
        let failure = report_input_error(Error::Invalid(message.into()));
        assert_eq!(failure.code(), "design_workspace_invalid");
        assert_eq!(failure.message(), message);
        assert!(failure.remedy_text().is_none());
    }
}
