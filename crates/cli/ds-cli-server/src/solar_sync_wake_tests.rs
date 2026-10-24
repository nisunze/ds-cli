use super::*;

#[test]
fn failed_startup_waits_for_the_next_recovery_boundary() {
    let mut wake = SyncWake::at_startup();
    assert!(wake.needs_observation(100, true));
    wake.failed();
    assert!(!wake.needs_observation(101, false));
    assert_eq!(wake.trigger(&inventory("held"), 101, false), None);
    assert!(wake.needs_observation(30_100, true));
    assert_eq!(
        wake.trigger(&inventory("held"), 30_100, true),
        Some(ds_sync_runtime::Trigger::Startup)
    );
}

fn inventory(value: &str) -> crate::server_reports::Inventory {
    crate::server_reports::Inventory {
        fingerprint: value.into(),
    }
}

#[test]
fn event_wake_does_not_rescan_an_idle_report_inventory() {
    let wake = SyncWake {
        startup: false,
        fingerprint: Some("same".into()),
        retry_eligible: false,
        offline: false,
        reconnect_pending: false,
        wake_at_ms: None,
    };
    assert!(!wake.needs_observation(10, false));
    assert!(wake.needs_observation(10, true));
}

#[test]
fn startup_is_once_then_idle_report_inventory_never_creates_a_gateway_trigger() {
    let mut wake = SyncWake::at_startup();
    assert_eq!(
        wake.trigger(&inventory("first"), 10, true),
        Some(ds_sync_runtime::Trigger::Startup)
    );
    wake.startup = false;
    wake.fingerprint = Some("first".into());
    assert_eq!(wake.trigger(&inventory("first"), 11, true), None);
}

#[test]
fn sealed_inventory_change_and_retained_work_are_local_change_triggers() {
    let mut wake = SyncWake {
        startup: false,
        fingerprint: Some("old".into()),
        retry_eligible: false,
        offline: false,
        reconnect_pending: false,
        wake_at_ms: None,
    };
    assert_eq!(
        wake.trigger(&inventory("new"), 10, true),
        Some(ds_sync_runtime::Trigger::LocalChange)
    );
    wake.fingerprint = Some("new".into());
    wake.retry_eligible = true;
    assert_eq!(
        wake.trigger(&inventory("new"), 11, true),
        Some(ds_sync_runtime::Trigger::LocalChange)
    );
}

#[test]
fn reconnect_requires_a_successful_post_offline_observation() {
    let mut wake = SyncWake {
        startup: false,
        fingerprint: Some("same".into()),
        retry_eligible: false,
        offline: true,
        reconnect_pending: false,
        wake_at_ms: None,
    };
    wake.applied(crate::server_reports::Pass {
        inventory: inventory("same"),
        refreshed_remote: true,
        retry_eligible: false,
        offline: false,
        wake_at_ms: None,
        reclaimed: ds_sync_runtime::Reclaimed::default(),
        idle: None,
        summary: ds_sync_runtime::kernel_sync::Summary::default(),
        receipts: Vec::new(),
    });
    assert_eq!(
        wake.trigger(&inventory("same"), 10, false),
        Some(ds_sync_runtime::Trigger::Reconnect)
    );
}

#[test]
fn a_proven_gateway_return_triggers_reconnect_without_waiting_for_recovery() {
    let wake = SyncWake {
        startup: false,
        fingerprint: Some("same".into()),
        retry_eligible: true,
        offline: true,
        reconnect_pending: true,
        wake_at_ms: None,
    };
    assert!(wake.needs_observation(10, false));
    assert_eq!(
        wake.trigger(&inventory("same"), 10, false),
        Some(ds_sync_runtime::Trigger::Reconnect)
    );
}

#[test]
fn offline_recovery_and_kernel_deadline_are_explicit_typed_triggers() {
    let wake = SyncWake {
        startup: false,
        fingerprint: Some("same".into()),
        retry_eligible: false,
        offline: true,
        reconnect_pending: false,
        wake_at_ms: None,
    };
    assert_eq!(
        wake.trigger(&inventory("same"), 10, true),
        Some(ds_sync_runtime::Trigger::LocalChange)
    );
    let wake = SyncWake {
        startup: false,
        fingerprint: Some("same".into()),
        retry_eligible: false,
        offline: false,
        reconnect_pending: false,
        wake_at_ms: Some(10),
    };
    assert_eq!(
        wake.trigger(&inventory("same"), 10, false),
        Some(ds_sync_runtime::Trigger::LocalChange)
    );
}
