fn main() {
    println!("cargo:rerun-if-env-changed=DS_DESKTOP_LANE");

    let lane = match std::env::var("DS_DESKTOP_LANE").ok().as_deref() {
        Some("stable") => "stable",
        Some("canary") => "canary",
        Some("headless") => "headless",
        Some("local") => "local",
        None => "development",
        Some(other) => {
            panic!("DS_DESKTOP_LANE must be stable, canary, headless, or local; got {other:?}")
        }
    };
    println!("cargo:rustc-env=DS_MCP_RELEASE_LANE={lane}");
}
