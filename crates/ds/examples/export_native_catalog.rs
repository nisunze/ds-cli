//! Regenerate or check the product artifact from the exact compiled Registry.
//! This example never writes target/debug/ds or executes a registered command.
#![allow(dead_code)] // The complete Registry includes handlers this exporter never runs.
#[path = "../src/build.rs"]
mod build;
#[path = "../src/meta.rs"]
mod meta;
#[path = "../src/native_catalog.rs"]
mod native_catalog;
#[path = "../src/registry.rs"]
mod registry;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or("Use --out <path> or --check <path>")?;
    let path = args.next().ok_or("A declared artifact path is required")?;
    if args.next().is_some() || !["--out", "--check"].contains(&mode.as_str()) {
        return Err("Use exactly --out <path> or --check <path>".into());
    }
    let value = native_catalog::export().map_err(std::io::Error::other)?;
    let bytes = serde_json::to_vec_pretty(&value)?;
    if mode == "--check" {
        let actual: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        // Source revision/dirty status changes across commits; the exact
        // declaration fingerprint and commands are the drift authority.
        if actual["schema"] != value["schema"]
            || actual["commands"] != value["commands"]
            || actual["provenance"]["descriptor_sha256"] != value["provenance"]["descriptor_sha256"]
        {
            return Err("Native tools artifact drifted; regenerate it from this Registry".into());
        }
    } else {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
    }
    println!(
        "{} commands; descriptor SHA {}; {}",
        value["commands"].as_array().unwrap().len(),
        value["provenance"]["descriptor_sha256"].as_str().unwrap(),
        path
    );
    Ok(())
}
