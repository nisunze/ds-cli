//! Every skill must fit the conditional-load budget the desktop release lane
//! enforces (`ds-web/scripts/desktop/prepare-ds-cli-skills.sh`: 4096 bytes
//! for the root `ds` skill, 8192 for every other). The lane refuses to package
//! an oversized skill and the refusal arrives after the source sync, on the
//! release host — 2026-09-13 (`ds-printout`) and 2026-09-20 (`ds`, `ds-printout`
//! again) each cost a release attempt. This pins the budget where the skills
//! are edited, so a commit that grows one fails here first.

use std::fs;
use std::path::PathBuf;

fn skills_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

#[test]
fn every_skill_fits_its_conditional_load_budget() {
    let root = skills_root();
    let mut checked = 0usize;
    let mut oversized = Vec::new();
    for entry in fs::read_dir(&root).expect("skills directory") {
        let entry = entry.expect("skills entry");
        let skill = entry.path().join("SKILL.md");
        if !skill.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let budget = if name == "ds" { 4096 } else { 8192 };
        let bytes = fs::metadata(&skill).expect("skill metadata").len();
        checked += 1;
        if bytes > budget {
            oversized.push(format!(
                "skills/{name}/SKILL.md is {bytes} bytes; budget {budget}"
            ));
        }
    }
    assert!(
        checked > 10,
        "expected the skills directory beside the crates, found {checked} skills"
    );
    assert!(
        oversized.is_empty(),
        "the desktop release lane refuses these skills; trim them before pushing:\n{}",
        oversized.join("\n")
    );
}
