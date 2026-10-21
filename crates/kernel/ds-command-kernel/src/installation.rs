//! Desktop installation policy. These decisions never govern browser access.
use serde::{Deserialize, Serialize};
pub const OFFLINE_LEASE_SECONDS: i64 = 7 * 24 * 60 * 60;
pub const CHECK_IN_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DesktopAccess {
    Allowed,
    Blocked,
    UpgradeRequired,
    HandshakeRequired,
    ClockRollback,
}
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLeaseState {
    pub access: DesktopAccess,
    pub handshake_due: bool,
    pub expires_at: i64,
    pub remaining_seconds: i64,
}
/// Called only after the host verifies the signed installation capability.
/// A previous denial cannot become an allowance merely because time passes.
pub fn evaluate(
    status: &str,
    issued_at: i64,
    expires_at: i64,
    now: i64,
    clock_floor: i64,
) -> DesktopLeaseState {
    let access = if status == "blocked" {
        DesktopAccess::Blocked
    } else if status == "upgrade_required" {
        DesktopAccess::UpgradeRequired
    } else if now < clock_floor || now < issued_at {
        DesktopAccess::ClockRollback
    } else if status != "active"
        || expires_at <= issued_at
        || expires_at.saturating_sub(issued_at) > OFFLINE_LEASE_SECONDS
        || now >= expires_at
    {
        DesktopAccess::HandshakeRequired
    } else {
        DesktopAccess::Allowed
    };
    DesktopLeaseState {
        handshake_due: now >= issued_at.saturating_add(CHECK_IN_SECONDS)
            || access != DesktopAccess::Allowed,
        access,
        expires_at,
        remaining_seconds: expires_at.saturating_sub(now).max(0),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_lease_has_exact_daily_and_weekly_boundaries() {
        let start = 1_000_000;
        let end = start + OFFLINE_LEASE_SECONDS;
        assert!(!evaluate("active", start, end, start + CHECK_IN_SECONDS - 1, start).handshake_due);
        assert!(evaluate("active", start, end, start + CHECK_IN_SECONDS, start).handshake_due);
        assert_eq!(
            evaluate("active", start, end, end - 1, start).access,
            DesktopAccess::Allowed
        );
        assert_eq!(
            evaluate("active", start, end, end, start).access,
            DesktopAccess::HandshakeRequired
        );
        assert_eq!(
            evaluate("active", start, end + 1, start, start).access,
            DesktopAccess::HandshakeRequired
        );
        assert_eq!(
            evaluate("active", start, end, start, start + 1).access,
            DesktopAccess::ClockRollback
        );
        assert_eq!(
            evaluate("blocked", start, end, end + 100, start).access,
            DesktopAccess::Blocked
        );
    }
}
