use super::*;

#[test]
fn device_protected_routes_keep_their_public_methods_paths_and_bearer() {
    let session =
        DeviceAccessSession::new("fixture-memory-bearer".into(), 1_900_000_600, 1_900_000_000)
            .unwrap();
    let list = session.protected_call(DeviceProtectedOperation::List);
    assert_eq!(
        (list.method(), list.path().unwrap()),
        ("GET", "/api/v1/auth/devices".into())
    );
    assert_eq!(list.bearer_token(), "fixture-memory-bearer");
    // These are the exact IDs admitted by the previous closed public getter.
    for id in ["device-a", "a.b_1-2", ".", ".."] {
        for (operation, method) in [
            (DeviceProtectedOperation::Read(id), "GET"),
            (DeviceProtectedOperation::Revoke(id), "DELETE"),
        ] {
            let call = session.protected_call(operation);
            assert_eq!(call.method(), method);
            assert_eq!(call.path().unwrap(), format!("/api/v1/auth/devices/{id}"));
            assert_eq!(call.bearer_token(), "fixture-memory-bearer");
        }
    }
}

#[test]
fn device_route_migration_preserves_identifier_refusals_before_any_host_effect() {
    let session =
        DeviceAccessSession::new("fixture-memory-bearer".into(), 1_900_000_600, 1_900_000_000)
            .unwrap();
    for id in [
        "",
        " ",
        " device",
        "device/other",
        "device?other",
        "device\nother",
        "é",
    ] {
        for operation in [
            DeviceProtectedOperation::Read(id),
            DeviceProtectedOperation::Revoke(id),
        ] {
            assert!(matches!(
                session.protected_call(operation).path(),
                Err(DeviceError::Input)
            ));
        }
    }
}
