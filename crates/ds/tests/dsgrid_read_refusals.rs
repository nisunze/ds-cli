//! Shared read refusals stay truthful when the mutation vocabulary grows.
use serde_json::Value;
mod common;

#[test]
fn report_and_gap_descriptors_declare_their_actual_read_failures() {
    let directory = tempfile::tempdir().unwrap();
    let absent = directory.path().join("absent.dsgrid");
    let output = directory.path().join("staking.xlsx");
    for (id, path, writes) in [
        (
            "dsgrid.report.staking",
            vec!["dsgrid", "report", "staking"],
            true,
        ),
        (
            "dsgrid.report.structures",
            vec!["dsgrid", "report", "structures"],
            false,
        ),
        (
            "dsgrid.alignment.gap.show",
            vec!["dsgrid", "alignment", "gap", "show"],
            false,
        ),
    ] {
        let (descriptor, code) = common::json(&["capabilities", id, "--output", "json"]);
        assert_eq!(code, 0);
        let declarations = descriptor["data"]["command"]["refusals"]
            .as_array()
            .unwrap();
        let declared = |code: &str| declarations.iter().any(|row| row["code"] == code);
        for package in [None, Some(absent.to_str().unwrap())] {
            let mut args = path.clone();
            if writes {
                args.extend(["--out", output.to_str().unwrap()]);
            }
            if let Some(package) = package {
                args.extend(["--package", package]);
            }
            args.extend(["--output", "json"]);
            let (result, code) = common::json(&args);
            assert_ne!(code, 0, "{result}");
            let expected = if package.is_some() {
                "model_not_found"
            } else {
                "target_required"
            };
            assert_eq!(result["error"]["code"], expected, "{id}: {result}");
            assert!(declared(expected), "{id} omits its emitted {expected}");
            assert_ne!(result["error"]["remedy"], Value::Null);
        }
        assert!(!output.exists());
    }
}
