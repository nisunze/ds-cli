//! Public release reads: the latest desktop shell or component release a lane
//! publishes (`GET /api/v1/app_releases`), and the installation manager's
//! publication on a lane's Hosting origin. The reads are anonymous and
//! best-effort — a public download page never shows an error because a
//! release lookup failed — so every failure reads as no release. The page
//! names the URL; nothing here sends a credential.

use serde_json::{Map, Value};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

/// A release answer's `release` member: ds-brain's flat wire, or the
/// standard envelope a proxy may keep. `None` for a refusal or an answer
/// without the member.
pub fn release_member(body: &Value) -> Option<Value> {
    if body.get("success") == Some(&Value::Bool(false)) {
        return None;
    }
    let answer = body
        .get("data")
        .filter(|data| data.is_object())
        .unwrap_or(body);
    answer.as_object()?.get("release").cloned()
}

/// The most bytes a release answer may hold.
pub const ANSWER_CAP: usize = 1024 * 1024;
/// The most bytes the installation manager's publication may hold.
pub const MANAGER_CAP: usize = 64 * 1024;
const MANAGER_SCHEMA: &str = "ds.installation-manager-publication/v1";
/// The most bytes one manager file may hold, and its package receipt.
const MANAGER_FILE_MAX: u64 = 128 * 1024;
const MANAGER_RECEIPT_MAX: u64 = 64 * 1024;

fn exactly(object: &Map<String, Value>, names: &[&str]) -> bool {
    object.len() == names.len() && names.iter().all(|name| object.contains_key(*name))
}

fn hex(value: &Value, len: usize) -> bool {
    value.as_str().is_some_and(|text| {
        text.len() == len && text.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

fn bounded(value: &Value, max: u64) -> bool {
    value.as_u64().is_some_and(|size| size > 0 && size <= max)
}

/// One file of a manager package: one of `names`, at its package's
/// content-addressed path, with an exact digest and a bounded size.
fn manager_file<'a>(value: &'a Value, names: &[&str], receipt: &str) -> Option<&'a str> {
    let file = value.as_object()?;
    if !exactly(file, &["filename", "path", "sha256", "size_bytes"])
        || !hex(&file["sha256"], 64)
        || !bounded(&file["size_bytes"], MANAGER_FILE_MAX)
    {
        return None;
    }
    let name = file["filename"].as_str().filter(|name| names.contains(name))?;
    if file["path"].as_str()? != format!("/installation-manager/{receipt}/{name}") {
        return None;
    }
    if name == "package.receipt.json"
        && (file["sha256"].as_str() != Some(receipt) || !bounded(&file["size_bytes"], MANAGER_RECEIPT_MAX))
    {
        return None;
    }
    Some(name)
}

/// One platform's package: its fixed files, exact source revisions and a
/// bootstrap at its own content-addressed path.
fn manager_unit(value: &Value, platform: &str) -> Option<()> {
    let unit = value.as_object()?;
    if !exactly(unit, &["arch", "package_receipt_sha256", "source_revisions", "assets", "bootstrap"])
        || unit["arch"] != "x86_64"
        || !hex(&unit["package_receipt_sha256"], 64)
    {
        return None;
    }
    let sources = unit["source_revisions"].as_object()?;
    if !exactly(sources, &["ds-cli", "ds-web"]) || !sources.values().all(|revision| hex(revision, 40)) {
        return None;
    }
    let receipt = unit["package_receipt_sha256"].as_str()?;
    let (names, extension): (&[&str], _) = if platform == "windows" {
        (&["package.receipt.json", "ds-install-manager.ps1", "install-ds-core-windows.ps1"], "ps1")
    } else {
        (&["package.receipt.json", "ds-install-manager.sh", "install-core.sh", "install-desktop.sh"], "sh")
    };
    let assets = unit["assets"].as_array()?;
    let mut seen = std::collections::BTreeSet::new();
    for asset in assets {
        seen.insert(manager_file(asset, names, receipt)?);
    }
    if assets.len() != names.len() || seen.len() != names.len() {
        return None;
    }
    let bootstrap = unit["bootstrap"].as_object()?;
    let filename = format!("install-manager.{extension}");
    if !exactly(bootstrap, &["filename", "path", "sha256", "size_bytes"])
        || bootstrap["filename"].as_str() != Some(filename.as_str())
        || !hex(&bootstrap["sha256"], 64)
        || !bounded(&bootstrap["size_bytes"], MANAGER_FILE_MAX)
        || bootstrap["path"].as_str()?
            != format!("/installation-manager/{}/{filename}", bootstrap["sha256"].as_str()?)
    {
        return None;
    }
    Some(())
}

/// The installation manager's publication for `channel` (`stable` or
/// `canary`), admitted only when it is exactly what the publisher writes:
/// both platforms, each with its fixed files at content-addressed paths,
/// exact digests and bounded sizes. Anything else reads as no publication.
pub fn installation_manager(body: &Value, channel: &str) -> Option<Value> {
    let product = match channel {
        "stable" => "rw.datasolutions.installation-manager",
        "canary" => "rw.datasolutions.installation-manager.canary",
        _ => return None,
    };
    let publication = body.as_object()?;
    if !exactly(publication, &["schema", "channel", "product_id", "published_at", "platforms"])
        || publication["schema"] != MANAGER_SCHEMA
        || publication["channel"] != channel
        || publication["product_id"] != product
        || !publication["published_at"]
            .as_str()
            .is_some_and(|at| OffsetDateTime::parse(at, &Rfc3339).is_ok())
    {
        return None;
    }
    let platforms = publication["platforms"].as_object()?;
    if !exactly(platforms, &["windows", "linux"]) {
        return None;
    }
    for (platform, unit) in platforms {
        manager_unit(unit, platform)?;
    }
    Some(body.clone())
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod exports {
    use crate::gateway_exchange::browser::{facts, GatewayHost};
    use futures_util::StreamExt;

    /// One anonymous JSON read of at most `cap` bytes; `None` on any failure.
    pub(crate) async fn read(
        host: &GatewayHost,
        url: &str,
        cap: usize,
    ) -> Option<serde_json::Value> {
        let facts = facts(host).ok()?;
        if facts.offline_mode || !facts.link_up {
            return None;
        }
        let parsed = url::Url::parse(url).ok()?;
        if !matches!(parsed.scheme(), "https" | "http") {
            return None;
        }
        let response = reqwest::Client::new()
            .get(parsed)
            .header("Accept", "application/json")
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.ok()?;
            if bytes.len() + chunk.len() > cap {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).ok()
    }
}

#[cfg(test)]
#[path = "releases_tests.rs"]
mod tests;
