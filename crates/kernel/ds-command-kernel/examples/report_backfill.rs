//! Offline dry-run entry point. No database/object mutation or credentials.
//! Capture a revision-pinned Snapshot through the authorized project API first.
use std::io::{Read, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().len() != 1 {
        return Err("report_migration_dry_run_only: this executable only plans stdin snapshots; application belongs to the revision-checked database/object host".into());
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("report_migration_invalid: snapshot exceeds 64 MiB".into());
    }
    let snapshot =
        serde_json::from_slice::<ds_command_kernel::report_migrations::Snapshot>(&bytes)?;
    let plan = ds_command_kernel::report_migrations::plan(&snapshot)?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &plan)?;
    std::io::stdout().write_all(b"\n")?;
    Ok(())
}
