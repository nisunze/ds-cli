//! Step lines `install-latest` writes to stderr for a shell to project.
//!
//! The progress line `<component>: <step> <percent>% (<done>/<total>)` is a
//! fixed grammar older shells parse strictly, so it never grows. The target
//! release version travels on its own line, `<component>: version <release>`,
//! which an older shell reads as an unknown step and ignores.
use ds_command_kernel::module_release::ui::UiComponent;
use std::time::{Duration, Instant};

/// Progress lines of one step never come faster than this.
const MIN_GAP: Duration = Duration::from_millis(250);

pub(crate) fn progress_line(
    component: UiComponent,
    step: &str,
    progress: u8,
    done: u64,
    total: u64,
) -> String {
    format!(
        "{}: {step} {progress}% ({done}/{total})",
        component.as_str()
    )
}

pub(crate) fn version_line(component: UiComponent, version: &str) -> Option<String> {
    let clean = !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+' | b'_'));
    clean.then(|| format!("{}: version {version}", component.as_str()))
}

/// Lets a step change and the end of a download through at once, and spaces
/// the rest so a fast link does not flood the shell.
pub(crate) struct Pace {
    step: Option<String>,
    at: Option<Instant>,
}
impl Pace {
    pub(crate) fn new() -> Self {
        Self {
            step: None,
            at: None,
        }
    }
    pub(crate) fn admit(&mut self, step: &str, done: u64, total: u64, now: Instant) -> bool {
        let changed = self.step.as_deref() != Some(step);
        let due = self.at.is_none_or(|at| now.duration_since(at) >= MIN_GAP);
        if changed || due || (total > 0 && done >= total) {
            self.step = Some(step.to_owned());
            self.at = Some(now);
            return true;
        }
        false
    }
}

#[cfg(test)]
#[path = "ui_install_progress_tests.rs"]
mod tests;
