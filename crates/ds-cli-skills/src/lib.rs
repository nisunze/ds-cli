//! Discovery and verification for the agent guidance shipped beside `ds`.
//!
//! Skills are documents, not another executable surface. Each desktop or
//! headless package carries a closed, hashed bundle whose receipt binds those
//! documents to the exact `ds` source SHA they describe. `ds doctor` verifies
//! that local bundle and any user-level copies without executing an installer
//! or an engine.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const RECEIPT_CONTRACT: &str = "ds-cli-skills-bundle/v3";
pub const RECEIPT_SOURCE: &str = "ds-cli";
const INSTALL_CONTRACT: &str = "ds-cli-skills-install/v1";
const OWNER: &str = "nisunze/ds-cli";
const OWNER_MARKER: &str = ".ds-cli-skills-owner";
const INVENTORY: &str = ".ds-cli-skills-owned";
const INSTALLED_RECEIPT: &str = ".ds-cli-skills-receipt.json";
const MAX_RECEIPT_BYTES: u64 = 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_FILES: usize = 1_000;
const MAX_SKILL_DOCUMENT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Receipt {
    source_sha: String,
    skills: Vec<String>,
    files: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
struct Bundle {
    root: PathBuf,
    receipt: Receipt,
}

/// One packaged bundle whose bounded receipt metadata matches the exact CLI
/// source identity supplied by the caller. File inventory and content digests
/// are deliberately verified only when a document is read.
#[derive(Clone, Debug)]
pub struct IndexedBundle {
    bundle: Bundle,
}

impl IndexedBundle {
    pub fn root(&self) -> &Path {
        &self.bundle.root
    }

    pub fn source_sha(&self) -> &str {
        &self.bundle.receipt.source_sha
    }

    pub fn skills(&self) -> &[String] {
        &self.bundle.receipt.skills
    }

    /// Read only the selected skill's entry document. The name must be one
    /// of the receipt identifiers; callers never supply a path. Re-validate
    /// the complete bundle and the selected digest at read time so a prior
    /// resources/list result cannot become a filesystem escape or stale-byte
    /// authority. This is the first point that reads skill document content.
    pub fn read_skill(&self, name: &str) -> Result<String, String> {
        if !valid_skill_name(name) || !self.bundle.receipt.skills.iter().any(|item| item == name) {
            return Err(format!("unknown shipped skill `{name}`"));
        }
        let current = validate_bundle(&self.bundle.root, self.source_sha())?;
        if current != self.bundle.receipt {
            return Err("skill bundle changed after it was selected".to_string());
        }
        let relative = format!("skills/{name}/SKILL.md");
        let expected = current
            .files
            .get(&relative)
            .ok_or_else(|| format!("receipt omits {relative}"))?;
        let path = self.bundle.root.join(&relative);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("skill document is unreadable: {error}"))?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_SKILL_DOCUMENT_BYTES {
            return Err("skill document is not a bounded regular file".to_string());
        }
        if &sha256(&path)? != expected {
            return Err("skill document digest changed after bundle verification".to_string());
        }
        fs::read_to_string(path).map_err(|error| format!("skill document is not UTF-8: {error}"))
    }
}

/// Locate one unambiguous release-matched packaged bundle without consulting
/// or requiring any user-level agent skills directory. This reads bounded
/// receipt metadata only; [`IndexedBundle::read_skill`] performs the complete
/// inventory and digest verification lazily.
pub fn indexed_bundle(expected_cli_sha: &str) -> Result<IndexedBundle, String> {
    let candidates = bundle_candidates();
    let existing = candidates
        .iter()
        .filter(|path| path.exists())
        .collect::<Vec<_>>();
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for root in existing {
        match index_bundle_root(root, expected_cli_sha) {
            Ok(bundle) => valid.push(bundle),
            Err(reason) => invalid.push(format!("{}: {reason}", root.display())),
        }
    }
    let Some(first) = valid.first().cloned() else {
        return Err(invalid
            .into_iter()
            .next()
            .unwrap_or_else(|| "no packaged ds-cli-skills bundle was found".to_string()));
    };
    if valid
        .iter()
        .skip(1)
        .any(|candidate| candidate.receipt != first.receipt)
    {
        return Err("multiple release-matched skill bundles contain different skills".to_string());
    }
    Ok(IndexedBundle { bundle: first })
}

fn index_bundle_root(root: &Path, expected_cli_sha: &str) -> Result<Bundle, String> {
    let root_meta = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if !root_meta.file_type().is_dir() {
        return Err("bundle root is not a regular directory".to_string());
    }
    let receipt = parse_receipt(&root.join("receipt.json"), expected_cli_sha)?;
    Ok(Bundle {
        root: root.to_path_buf(),
        receipt,
    })
}

pub fn doctor_report(expected_cli_sha: &str) -> Value {
    let candidates = bundle_candidates();
    let existing: Vec<PathBuf> = candidates
        .iter()
        .filter(|path| path.exists())
        .cloned()
        .collect();
    let mut valid = Vec::new();
    let mut invalid = Vec::new();
    for root in &existing {
        match validate_bundle(root, expected_cli_sha) {
            Ok(receipt) => valid.push(Bundle {
                root: root.clone(),
                receipt,
            }),
            Err(reason) => invalid.push(format!("{}: {reason}", root.display())),
        }
    }

    let (status, bundle, reason, remedy) = if valid.is_empty() {
        if existing.is_empty() {
            (
                "missing",
                None,
                Some("no packaged ds-cli-skills bundle was found".to_string()),
                Some("reinstall the complete ds release; its package carries the skills matched to this ds build".to_string()),
            )
        } else {
            (
                "invalid",
                None,
                invalid.first().cloned(),
                Some("reinstall ds from one complete verified release".to_string()),
            )
        }
    } else {
        let first = &valid[0];
        let differs = valid
            .iter()
            .skip(1)
            .any(|candidate| candidate.receipt != first.receipt);
        if differs {
            (
                "ambiguous",
                None,
                Some("multiple release-matched skill bundles contain different skills".to_string()),
                Some(
                    "set DS_CLI_SKILLS_BUNDLE to the bundle installed with this ds executable"
                        .to_string(),
                ),
            )
        } else {
            ("ready", Some(first.clone()), None, None)
        }
    };

    let agents = [
        ("codex", codex_skills_dir()),
        ("claude", claude_skills_dir()),
        ("copilot", copilot_skills_dir()),
    ]
    .into_iter()
    .map(|(agent, target)| inspect_install(agent, target, bundle.as_ref(), expected_cli_sha))
    .collect::<Vec<_>>();

    let mut report = json!({
        "status": status,
        "bundle_path": bundle.as_ref().map(|found| found.root.display().to_string()),
        "source_sha": bundle.as_ref().map(|found| found.receipt.source_sha.as_str()),
        "skills": bundle.as_ref().map(|found| found.receipt.skills.clone()).unwrap_or_default(),
        "agents": agents,
        "reason": reason,
        "remedy": remedy,
    });
    if let Some(found) = bundle {
        report["installers"] = json!({
            "shell": {
                "path": found.root.join("scripts/install-skills.sh").display().to_string(),
                "args": ["install"],
            },
            "powershell": {
                "path": found.root.join("scripts/install-skills.ps1").display().to_string(),
                "args": ["install"],
            },
        });
    }
    report
}

fn bundle_candidates() -> Vec<PathBuf> {
    if let Some(override_path) = std::env::var_os("DS_CLI_SKILLS_BUNDLE") {
        return vec![PathBuf::from(override_path)];
    }

    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe()
        && let Some(bin) = executable.parent()
    {
        push_unique(&mut candidates, bin.join("ds-cli-skills"));
        if let Some(prefix) = bin.parent() {
            push_unique(
                &mut candidates,
                prefix.join("Resources").join("ds-cli-skills"),
            );
            push_unique(
                &mut candidates,
                prefix
                    .join("lib")
                    .join("DS GridDesign Canary")
                    .join("ds-cli-skills"),
            );
            push_unique(
                &mut candidates,
                prefix
                    .join("lib")
                    .join("DS GridDesign")
                    .join("ds-cli-skills"),
            );
            push_unique(
                &mut candidates,
                prefix.join("lib").join("ds").join("ds-cli-skills"),
            );
        }
    }
    push_unique(
        &mut candidates,
        PathBuf::from("/usr/lib/DS GridDesign Canary/ds-cli-skills"),
    );
    push_unique(
        &mut candidates,
        PathBuf::from("/usr/lib/DS GridDesign/ds-cli-skills"),
    );
    push_unique(&mut candidates, PathBuf::from("/usr/lib/ds/ds-cli-skills"));
    candidates
}

fn push_unique(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.contains(&path) {
        paths.push(path);
    }
}

fn codex_skills_dir() -> Option<PathBuf> {
    explicit_or_home("CODEX_SKILLS_DIR", "CODEX_HOME", ".codex", "skills")
}

fn claude_skills_dir() -> Option<PathBuf> {
    if let Some(path) = nonempty_env("CLAUDE_SKILLS_DIR") {
        return Some(PathBuf::from(path));
    }
    nonempty_env("HOME").map(|home| PathBuf::from(home).join(".claude").join("skills"))
}

fn copilot_skills_dir() -> Option<PathBuf> {
    if let Some(path) = nonempty_env("COPILOT_SKILLS_DIR") {
        return Some(PathBuf::from(path));
    }
    nonempty_env("HOME").map(|home| PathBuf::from(home).join(".copilot").join("skills"))
}

fn explicit_or_home(
    explicit: &str,
    product_home: &str,
    default_home: &str,
    leaf: &str,
) -> Option<PathBuf> {
    if let Some(path) = nonempty_env(explicit) {
        return Some(PathBuf::from(path));
    }
    if let Some(path) = nonempty_env(product_home) {
        return Some(PathBuf::from(path).join(leaf));
    }
    nonempty_env("HOME").map(|home| PathBuf::from(home).join(default_home).join(leaf))
}

fn nonempty_env(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

fn validate_bundle(root: &Path, expected_cli_sha: &str) -> Result<Receipt, String> {
    let root_meta = fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if !root_meta.file_type().is_dir() {
        return Err("bundle root is not a regular directory".to_string());
    }
    let receipt = parse_receipt(&root.join("receipt.json"), expected_cli_sha)?;
    let actual = collect_files(root, Some("receipt.json"))?;
    let actual_digests = digest_files(&actual)?;
    if actual_digests != receipt.files {
        return Err("receipt file inventory or digest does not match the bundle".to_string());
    }

    let skills_root = root.join("skills");
    let mut actual_skills = Vec::new();
    for entry in fs::read_dir(&skills_root)
        .map_err(|error| format!("skills directory is unreadable: {error}"))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let meta = fs::symlink_metadata(entry.path()).map_err(|error| error.to_string())?;
        if !meta.file_type().is_dir() {
            return Err("skills directory contains a non-directory entry".to_string());
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "skill name is not UTF-8".to_string())?;
        if !valid_skill_name(&name) {
            return Err(format!("invalid skill directory name: {name}"));
        }
        actual_skills.push(name);
    }
    actual_skills.sort();
    if actual_skills != receipt.skills {
        return Err("receipt skill inventory does not match the bundle".to_string());
    }
    Ok(receipt)
}

fn parse_receipt(path: &Path, expected_cli_sha: &str) -> Result<Receipt, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("receipt is missing or unreadable: {error}"))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_RECEIPT_BYTES {
        return Err("receipt is not a bounded regular file".to_string());
    }
    let body = fs::read(path).map_err(|error| format!("receipt is unreadable: {error}"))?;
    let value: Value = serde_json::from_slice(&body)
        .map_err(|error| format!("receipt is invalid JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "receipt is not a JSON object".to_string())?;
    exact_keys(
        object.keys().map(String::as_str),
        &[
            "contract",
            "dirty",
            "files",
            "skills",
            "source",
            "source_sha",
        ],
        "receipt",
    )?;
    if value["contract"] != RECEIPT_CONTRACT || value["source"] != RECEIPT_SOURCE {
        return Err("receipt contract or source is not ds-cli v3".to_string());
    }
    if value["dirty"] != false {
        return Err("receipt describes dirty source".to_string());
    }
    let source_sha = required_sha(&value["source_sha"], "source_sha")?;
    if source_sha != expected_cli_sha {
        return Err(format!(
            "receipt targets ds-cli {source_sha}, but this binary is {expected_cli_sha}"
        ));
    }

    let mut skills = value["skills"]
        .as_array()
        .ok_or_else(|| "receipt skills is not an array".to_string())?
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|name| valid_skill_name(name))
                .map(str::to_string)
                .ok_or_else(|| "receipt contains an invalid skill name".to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    if skills.is_empty() || !skills.iter().any(|name| name == "ds") {
        return Err("receipt does not contain the ds entry skill".to_string());
    }
    let original_skills = skills.clone();
    skills.sort();
    skills.dedup();
    if skills != original_skills {
        return Err("receipt skills must be unique and sorted".to_string());
    }

    let records = value["files"]
        .as_array()
        .ok_or_else(|| "receipt files is not an array".to_string())?;
    if records.is_empty() || records.len() > MAX_FILES {
        return Err("receipt file count is outside its bound".to_string());
    }
    let mut files = BTreeMap::new();
    for record in records {
        let object = record
            .as_object()
            .ok_or_else(|| "receipt contains a non-object file record".to_string())?;
        exact_keys(
            object.keys().map(String::as_str),
            &["path", "sha256"],
            "file record",
        )?;
        let relative = record["path"]
            .as_str()
            .filter(|path| safe_relative(path))
            .ok_or_else(|| "receipt contains an unsafe file path".to_string())?;
        let digest = record["sha256"]
            .as_str()
            .filter(|digest| lower_hex(digest, 64))
            .ok_or_else(|| "receipt contains an invalid file digest".to_string())?;
        if files
            .insert(relative.to_string(), digest.to_string())
            .is_some()
        {
            return Err("receipt contains a duplicate file path".to_string());
        }
    }
    Ok(Receipt {
        source_sha,
        skills,
        files,
    })
}

fn inspect_install(
    agent: &str,
    target: Option<PathBuf>,
    bundle: Option<&Bundle>,
    expected_cli_sha: &str,
) -> Value {
    let Some(target) = target else {
        return json!({
            "agent": agent,
            "path": null,
            "status": "unavailable",
            "reason": "no user home directory is available",
        });
    };
    let path_text = target.display().to_string();
    let inventory_path = target.join(INVENTORY);
    if !inventory_path.exists() {
        let unmanaged = target.join("ds").join(OWNER_MARKER).exists()
            || target.join(INSTALLED_RECEIPT).exists();
        return json!({
            "agent": agent,
            "path": path_text,
            "status": if unmanaged { "stale" } else { "not_installed" },
            "reason": if unmanaged { Some("managed files exist without an owned inventory") } else { None },
        });
    }

    let result = verify_install(&target, bundle, expected_cli_sha);
    match result {
        Ok(receipt) => json!({
            "agent": agent,
            "path": path_text,
            "status": "current",
            "source_sha": receipt.source_sha,
        }),
        Err(reason) => json!({
            "agent": agent,
            "path": path_text,
            "status": "stale",
            "reason": reason,
        }),
    }
}

fn verify_install(
    target: &Path,
    bundle: Option<&Bundle>,
    expected_cli_sha: &str,
) -> Result<Receipt, String> {
    let inventory_path = target.join(INVENTORY);
    let metadata = fs::symlink_metadata(&inventory_path)
        .map_err(|error| format!("install inventory is unreadable: {error}"))?;
    if !metadata.file_type().is_file() || metadata.len() > 64 * 1024 {
        return Err("install inventory is not a bounded regular file".to_string());
    }
    let inventory = fs::read_to_string(&inventory_path)
        .map_err(|error| format!("install inventory is unreadable: {error}"))?;
    let mut lines = inventory.lines();
    if lines.next() != Some(INSTALL_CONTRACT) {
        return Err("install inventory has the wrong contract".to_string());
    }
    let names = lines.map(str::to_string).collect::<Vec<_>>();
    if names.is_empty()
        || names.iter().any(|name| !valid_skill_name(name))
        || names.iter().collect::<BTreeSet<_>>().len() != names.len()
    {
        return Err("install inventory contains invalid skill names".to_string());
    }

    let receipt = parse_receipt(&target.join(INSTALLED_RECEIPT), expected_cli_sha)?;
    if names != receipt.skills {
        return Err("installed skill inventory does not match its receipt".to_string());
    }
    if let Some(expected) = bundle
        && receipt != expected.receipt
    {
        return Err("installed receipt does not match the packaged bundle".to_string());
    }

    let mut actual = BTreeMap::new();
    for name in &names {
        let skill_root = target.join(name);
        let marker = skill_root.join(OWNER_MARKER);
        let marker_meta = fs::symlink_metadata(&marker)
            .map_err(|_| format!("installed skill {name} has no ownership marker"))?;
        if !marker_meta.file_type().is_file()
            || !matches!(fs::read_to_string(&marker), Ok(text) if text.trim() == OWNER)
        {
            return Err(format!(
                "installed skill {name} is not owned by this bundle"
            ));
        }
        for (relative, path) in collect_files(&skill_root, Some(OWNER_MARKER))? {
            actual.insert(format!("skills/{name}/{relative}"), path);
        }
    }
    let actual_digests = digest_files(&actual)?;
    let expected = receipt
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("skills/"))
        .map(|(path, digest)| (path.clone(), digest.clone()))
        .collect::<BTreeMap<_, _>>();
    if actual_digests != expected {
        return Err("installed skill bytes do not match their receipt".to_string());
    }
    Ok(receipt)
}

fn collect_files(
    root: &Path,
    skip_root_file: Option<&str>,
) -> Result<BTreeMap<String, PathBuf>, String> {
    let mut files = BTreeMap::new();
    let mut total_bytes = 0u64;
    collect_files_at(root, root, skip_root_file, &mut files, &mut total_bytes)?;
    Ok(files)
}

fn collect_files_at(
    root: &Path,
    current: &Path,
    skip_root_file: Option<&str>,
    files: &mut BTreeMap<String, PathBuf>,
    total_bytes: &mut u64,
) -> Result<(), String> {
    for entry in
        fs::read_dir(current).map_err(|error| format!("directory is unreadable: {error}"))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!("bundle contains a symlink: {}", path.display()));
        }
        if metadata.file_type().is_dir() {
            collect_files_at(root, &path, skip_root_file, files, total_bytes)?;
            continue;
        }
        if !metadata.file_type().is_file() {
            return Err(format!(
                "bundle contains a non-regular path: {}",
                path.display()
            ));
        }
        let relative = relative_utf8(root, &path)?;
        if path.parent() == Some(root) && skip_root_file == Some(relative.as_str()) {
            continue;
        }
        *total_bytes = total_bytes.saturating_add(metadata.len());
        if *total_bytes > MAX_BUNDLE_BYTES || files.len() >= MAX_FILES {
            return Err("bundle file count or size exceeds its diagnostic bound".to_string());
        }
        files.insert(relative, path);
    }
    Ok(())
}

fn digest_files(files: &BTreeMap<String, PathBuf>) -> Result<BTreeMap<String, String>, String> {
    files
        .iter()
        .map(|(relative, path)| Ok((relative.clone(), sha256(path)?)))
        .collect()
}

fn sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn relative_utf8(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "file escaped bundle root".to_string())?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(
                part.to_str()
                    .ok_or_else(|| "bundle path is not UTF-8".to_string())?,
            ),
            _ => return Err("bundle contains an unsafe relative path".to_string()),
        }
    }
    Ok(parts.join("/"))
}

fn exact_keys<'a>(
    actual: impl Iterator<Item = &'a str>,
    expected: &[&str],
    label: &str,
) -> Result<(), String> {
    let actual = actual.collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(format!("{label} has unexpected or missing fields"));
    }
    Ok(())
}

fn required_sha(value: &Value, field: &str) -> Result<String, String> {
    value
        .as_str()
        .filter(|sha| lower_hex(sha, 40))
        .map(str::to_string)
        .ok_or_else(|| format!("receipt {field} is not a lowercase Git SHA"))
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ds-skills-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write_bundle(root: &Path, cli_sha: &str) -> Receipt {
        fs::create_dir_all(root.join("skills/ds/agents")).unwrap();
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("skills/ds/SKILL.md"), "---\nname: ds\n---\n").unwrap();
        fs::write(root.join("skills/ds/agents/openai.yaml"), "interface: ds\n").unwrap();
        fs::write(root.join("scripts/install-skills.sh"), "#!/bin/sh\n").unwrap();
        let files = digest_files(&collect_files(root, Some("receipt.json")).unwrap()).unwrap();
        let receipt = Receipt {
            source_sha: cli_sha.to_string(),
            skills: vec!["ds".to_string()],
            files,
        };
        let records = receipt
            .files
            .iter()
            .map(|(path, sha256)| json!({ "path": path, "sha256": sha256 }))
            .collect::<Vec<_>>();
        fs::write(
            root.join("receipt.json"),
            serde_json::to_vec(&json!({
                "contract": RECEIPT_CONTRACT,
                "source": RECEIPT_SOURCE,
                "source_sha": receipt.source_sha,
                "dirty": false,
                "skills": receipt.skills,
                "files": records,
            }))
            .unwrap(),
        )
        .unwrap();
        receipt
    }

    #[test]
    fn bundle_is_closed_and_cli_bound() {
        let temp = TestDir::new();
        let cli_sha = "2222222222222222222222222222222222222222";
        let expected = write_bundle(&temp.0, cli_sha);
        assert_eq!(validate_bundle(&temp.0, cli_sha).unwrap(), expected);
        assert!(
            validate_bundle(&temp.0, "3333333333333333333333333333333333333333")
                .unwrap_err()
                .contains("targets ds-cli")
        );
    }

    #[test]
    fn headless_release_bundle_is_a_closed_candidate() {
        assert!(bundle_candidates().contains(&PathBuf::from("/usr/lib/ds/ds-cli-skills")));
    }

    #[test]
    fn bundle_tampering_is_rejected() {
        let temp = TestDir::new();
        let cli_sha = "2222222222222222222222222222222222222222";
        write_bundle(&temp.0, cli_sha);
        fs::write(temp.0.join("skills/ds/SKILL.md"), "changed\n").unwrap();
        assert!(validate_bundle(&temp.0, cli_sha).is_err());
    }

    #[test]
    fn startup_index_reads_receipt_metadata_and_defers_content_verification() {
        let temp = TestDir::new();
        let cli_sha = "2222222222222222222222222222222222222222";
        write_bundle(&temp.0, cli_sha);
        fs::write(temp.0.join("skills/ds/SKILL.md"), "changed\n").unwrap();
        let indexed = IndexedBundle {
            bundle: index_bundle_root(&temp.0, cli_sha).expect("receipt metadata remains readable"),
        };
        assert!(indexed.read_skill("ds").is_err());
    }

    #[test]
    fn installed_skills_are_verified_against_the_receipt() {
        let temp = TestDir::new();
        let bundle_root = temp.0.join("bundle");
        let target = temp.0.join("installed");
        let cli_sha = "2222222222222222222222222222222222222222";
        let receipt = write_bundle(&bundle_root, cli_sha);
        fs::create_dir_all(target.join("ds/agents")).unwrap();
        fs::copy(
            bundle_root.join("skills/ds/SKILL.md"),
            target.join("ds/SKILL.md"),
        )
        .unwrap();
        fs::copy(
            bundle_root.join("skills/ds/agents/openai.yaml"),
            target.join("ds/agents/openai.yaml"),
        )
        .unwrap();
        fs::write(target.join("ds").join(OWNER_MARKER), format!("{OWNER}\n")).unwrap();
        fs::write(target.join(INVENTORY), format!("{INSTALL_CONTRACT}\nds\n")).unwrap();
        fs::copy(
            bundle_root.join("receipt.json"),
            target.join(INSTALLED_RECEIPT),
        )
        .unwrap();
        let bundle = Bundle {
            root: bundle_root,
            receipt: receipt.clone(),
        };
        assert_eq!(
            verify_install(&target, Some(&bundle), cli_sha).unwrap(),
            receipt
        );
        fs::write(target.join("ds/SKILL.md"), "changed\n").unwrap();
        assert!(verify_install(&target, Some(&bundle), cli_sha).is_err());
    }
}
