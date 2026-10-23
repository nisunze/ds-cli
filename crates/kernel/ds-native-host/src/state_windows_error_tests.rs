use super::dpapi_context_unavailable;

#[test]
fn access_token_logon_and_profile_failures_are_context_unavailable() {
    for code in [5, 1244, 1245, 1253, 1309, 1312, 1314, 1325] {
        assert!(dpapi_context_unavailable(code), "WinError {code}");
    }
}

#[test]
fn provider_key_access_and_noninteractive_context_failures_are_unavailable() {
    for code in [
        0x8009_000b,
        0x8009_000d,
        0x8009_0010,
        0x8009_0016,
        0x8009_0022,
        0x8009_0024,
        0x8009_002e,
    ] {
        assert!(dpapi_context_unavailable(code), "WinError {code:#x}");
    }
}

#[test]
fn malformed_data_and_integrity_failures_never_become_access_failures() {
    for code in [
        13,          // ERROR_INVALID_DATA
        87,          // ERROR_INVALID_PARAMETER
        0x8009_0005, // NTE_BAD_DATA
        0x8009_0006, // NTE_BAD_SIGNATURE
        0x8009_001a, // NTE_KEYSET_ENTRY_BAD
        0x8009_0027, // NTE_INVALID_PARAMETER
        0x8009_002c, // NTE_DECRYPTION_FAILURE
    ] {
        assert!(!dpapi_context_unavailable(code), "WinError {code:#x}");
    }
}

#[test]
fn unknown_and_missing_error_codes_fail_closed() {
    for code in [0, 1, 6, 0x8009_0020, 0xffff_ffff] {
        assert!(!dpapi_context_unavailable(code), "WinError {code:#x}");
    }
}
