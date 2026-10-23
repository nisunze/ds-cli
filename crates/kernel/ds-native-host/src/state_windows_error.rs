//! Pure classification of Windows DPAPI access/context failures.
//!
//! The adapter maps these to the existing unavailable protected-store cause.
//! This does not prove that stored data is intact or that a token is restricted.
//! Unknown decrypt failures retain the unsafe/unreadable refusal: DPAPI can
//! report corruption through several different errors.

// WinError values are kept portable so the classification runs in Linux tests.
// https://learn.microsoft.com/windows/win32/debug/system-error-codes--0-499-
// https://learn.microsoft.com/windows/win32/debug/system-error-codes--1000-1299-
// https://learn.microsoft.com/windows/win32/debug/system-error-codes--1300-1699-
// https://learn.microsoft.com/windows/win32/com/com-error-codes-4
pub(crate) fn dpapi_context_unavailable(code: u32) -> bool {
    matches!(
        code,
        5 // ERROR_ACCESS_DENIED
        | 1244 // ERROR_NOT_AUTHENTICATED
        | 1245 // ERROR_NOT_LOGGED_ON
        | 1253 // ERROR_BAD_USER_PROFILE
        | 1309 // ERROR_NO_IMPERSONATION_TOKEN
        | 1312 // ERROR_NO_SUCH_LOGON_SESSION
        | 1314 // ERROR_PRIVILEGE_NOT_HELD
        | 1325 // ERROR_PASSWORD_RESTRICTION (DPAPI UI forbidden)
        | 0x8009_000b // NTE_BAD_KEY_STATE
        | 0x8009_000d // NTE_NO_KEY
        | 0x8009_0010 // NTE_PERM
        | 0x8009_0016 // NTE_BAD_KEYSET (also denied key-container access)
        | 0x8009_0022 // NTE_SILENT_CONTEXT
        | 0x8009_0024 // NTE_TEMPORARY_PROFILE
        | 0x8009_002e // NTE_UI_REQUIRED
    )
}

#[cfg(test)]
#[path = "state_windows_error_tests.rs"]
mod tests;
