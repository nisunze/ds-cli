use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn sources(dir: &Path, paths: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).expect("kernel source directory") {
        let path = entry.expect("kernel source entry").path();
        if path.is_dir() {
            if path
                .file_name()
                .is_some_and(|n| n == "target" || n == ".git")
            {
                continue;
            }
            sources(&path, paths);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "toml")
        ) {
            paths.push(path);
        }
    }
}

fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut paths = vec![root.join("Cargo.toml"), root.join("build.rs")];
    for dir in ["src", "crates"] {
        println!("cargo:rerun-if-changed={dir}");
        sources(&root.join(dir), &mut paths);
    }
    paths.sort();
    let mut hash = Sha256::new();
    for path in paths {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path
            .strip_prefix(&root)
            .unwrap()
            .to_str()
            .unwrap()
            .as_bytes();
        let bytes = fs::read(&path).expect("kernel source bytes");
        hash.update((name.len() as u64).to_be_bytes());
        hash.update(name);
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    println!(
        "cargo:rustc-env=DS_KERNEL_SOURCE_SHA256={:x}",
        hash.finalize()
    );
}
