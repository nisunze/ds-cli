//! What the host can spare for parallel spotting.
//!
//! The engine performs no I/O: it admits searches against a byte budget its
//! host supplies. This is that host's half. The budget is the physical
//! memory the operating system reports available, less a reserve for the
//! user's other work, so a whole-model batch runs in parallel by default and
//! still never freezes the computer.

/// Kept free for everything else on the machine: the larger of this floor and
/// a fifth of physical memory.
const RESERVE_FLOOR_BYTES: u64 = 2 << 30;
const RESERVE_FRACTION_DIVISOR: u64 = 5;

/// The spotting batch budget in bytes, or `None` when the operating system
/// reports no memory figures.
pub fn spotting_memory_budget_bytes() -> Option<u64> {
    let (available, total) = imp::available_and_total_bytes()?;
    Some(budget_from(available, total))
}

/// Available memory less the reserve, and never zero: a machine with nothing
/// to spare still runs one search at a time, the engine's admission floor.
fn budget_from(available: u64, total: u64) -> u64 {
    let reserve = RESERVE_FLOOR_BYTES.max(total / RESERVE_FRACTION_DIVISOR);
    available.saturating_sub(reserve).max(1)
}

#[cfg(target_os = "linux")]
mod imp {
    pub fn available_and_total_bytes() -> Option<(u64, u64)> {
        let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
        let field = |name: &str| {
            meminfo.lines().find_map(|line| {
                let kib = line
                    .strip_prefix(name)?
                    .strip_prefix(':')?
                    .trim()
                    .strip_suffix("kB")?
                    .trim();
                kib.parse::<u64>().ok().map(|kib| kib * 1024)
            })
        };
        Some((field("MemAvailable")?, field("MemTotal")?))
    }
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    pub fn available_and_total_bytes() -> Option<(u64, u64)> {
        // SAFETY: MEMORYSTATUSEX is plain data; all-zero is a valid value.
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `status` is writable and its dwLength is set, as the API
        // requires.
        let reported = unsafe { GlobalMemoryStatusEx(&mut status) } != 0;
        reported.then_some((status.ullAvailPhys, status.ullTotalPhys))
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    pub fn available_and_total_bytes() -> Option<(u64, u64)> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1 << 30;

    #[test]
    fn the_reserve_is_the_larger_of_two_gib_and_a_fifth_of_memory() {
        // 64 GiB machine, 40 GiB free: a fifth (12.8 GiB) stays reserved.
        assert_eq!(budget_from(40 * GIB, 64 * GIB), 40 * GIB - 64 * GIB / 5);
        // 8 GiB laptop, 6 GiB free: the 2 GiB floor governs.
        assert_eq!(budget_from(6 * GIB, 8 * GIB), 4 * GIB);
    }

    #[test]
    fn a_machine_with_nothing_to_spare_still_admits_one_search() {
        assert_eq!(budget_from(GIB, 8 * GIB), 1);
    }

    #[test]
    fn this_host_reports_its_memory() {
        if cfg!(any(target_os = "linux", windows)) {
            let (available, total) = imp::available_and_total_bytes().expect("reported");
            assert!(available > 0 && available <= total);
        }
    }
}
