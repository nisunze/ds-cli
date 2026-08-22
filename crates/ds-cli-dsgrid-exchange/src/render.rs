//! Shared human-output fragments.
//!
//! Human output is always a projection of the machine result — never a
//! parallel computation — so these helpers read the same JSON the `--output
//! json` caller receives. `plan` and `convert` both print blockers, warnings
//! and losses, and they must print them the same way: an operator comparing a
//! plan against the run that followed it is comparing two screens, and a
//! difference in shape reads as a difference in substance.

use serde_json::Value;

/// Print a titled list, or nothing at all when it is empty.
///
/// Empty sections are omitted rather than printed as "none". A conversion
/// with no losses should not spend three lines saying so — the absence of a
/// LOSSES heading is the answer, and it keeps the common case short.
pub fn list(out: &mut String, title: &str, value: &Value) {
    let Some(items) = value.as_array().filter(|items| !items.is_empty()) else {
        return;
    };

    out.push_str(&format!("\n{title}\n"));
    for item in items {
        match item.as_str() {
            Some(text) => out.push_str(&format!("  {text}\n")),
            None => out.push_str(&format!("  {item}\n")),
        }
    }
}
