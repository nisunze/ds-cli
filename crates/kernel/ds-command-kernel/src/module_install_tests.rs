use super::*;
use serde_json::json;

#[test]
fn removed_record_is_closed_lane_bound_and_refused_by_installed_v1_reader() {
    let raw = json!({"schema":REMOVED_SELECTION_SCHEMA,"component":"network","channel":"canary","generation":2,"state":"removed"});
    let record: ModuleSelectionRecord = serde_json::from_value(raw.clone()).unwrap();
    assert_eq!(record.generation(), 2);
    validate_selection_record(&record, ModuleComponent::Network, "canary", "linux").unwrap();
    assert!(record
        .clone()
        .installed()
        .unwrap_err()
        .starts_with("module_removed:"));
    assert!(serde_json::from_value::<ModuleSelection>(raw.clone()).is_err());
    for (key, value) in [
        ("schema", json!(SELECTION_SCHEMA)),
        ("generation", json!(0)),
        ("component", json!("solar")),
        ("channel", json!("stable")),
    ] {
        let mut changed = raw.clone();
        changed[key] = value;
        let record = serde_json::from_value(changed).unwrap();
        assert!(
            validate_selection_record(&record, ModuleComponent::Network, "canary", "linux")
                .is_err()
        );
    }
    for (key, value) in [
        ("state", json!("disabled")),
        ("active", json!(null)),
        ("previous", json!(null)),
        ("unexpected", json!(true)),
    ] {
        let mut changed = raw.clone();
        changed[key] = value;
        assert!(serde_json::from_value::<ModuleSelectionRecord>(changed).is_err());
    }
}
