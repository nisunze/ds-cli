//! The core's copy of a project's survey forms, on disk.
//!
//! Every decision is the kernel's (`ds_command_kernel::survey::hold`): which
//! filter a copy was read under, reuse | delta | replace, the merge of a
//! verified stream, the view, the status. This module only keeps the bytes:
//! one directory per account and project under the layer root (beside the
//! working-area form choice and the dataset rooms), one folder per form with
//! `rows.ndjson` and `manifest.json`, and a `media/` tree of held photos at
//! their object paths. Writes are whole-file and renamed into place under a
//! room lock, so a reader sees the previous copy or the next, never half.
//!
//! The host injects the fetch; this crate never opens a socket.

use ds_command_kernel::project_dataset_cache::Scope;
use ds_command_kernel::survey::hold::{
    self, Applied, Decision, Filter, HeldMedia, Manifest, Refresh, View,
};
use ds_command_kernel::time::OffsetDateTime;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};

const ROOT: &str = "survey-hold";
const VERSION: &str = "v1";
const ROWS: &str = "rows.ndjson";
const MANIFEST: &str = "manifest.json";

/// Why a held read could not be answered.
#[derive(Debug)]
pub enum HoldError<E> {
    /// The host's fetch failed; its own error, unchanged.
    Fetch(E),
    /// `Refresh::Local` and nothing covering is held.
    NotHeld,
    /// The kernel refused the filter or the stream; the held copy is intact.
    Refused(String),
    /// The local store could not be read or written.
    Store(String),
}

/// Where one read's answer came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The held copy, unchanged.
    Held,
    /// The held copy, refreshed with the changes since its cursor.
    Delta,
    /// A fresh read of the form under the filter.
    Replace,
}

impl Source {
    pub const fn as_str(self) -> &'static str {
        match self {
            Source::Held => "held",
            Source::Delta => "delta",
            Source::Replace => "replace",
        }
    }
}

/// The copy one read answered from.
pub struct Held {
    pub source: Source,
    pub manifest: Manifest,
}

/// The account's and project's directory. The scope is digested so no
/// identity appears in a path.
pub fn room_dir(root: &Path, scope: &Scope) -> PathBuf {
    let mut digest = Sha256::new();
    digest.update(hold::SCHEMA.as_bytes());
    digest.update([0]);
    digest.update(scope.principal.as_bytes());
    digest.update([0]);
    digest.update(scope.project.as_bytes());
    root.join(ROOT)
        .join(VERSION)
        .join(format!("{:x}", digest.finalize()))
}

/// Where held photos live: `<media>/<object path>`, thumbnails beside them.
pub fn media_dir(root: &Path, scope: &Scope) -> PathBuf {
    room_dir(root, scope).join("media")
}

fn form_dir(root: &Path, scope: &Scope, form: &str) -> Result<PathBuf, String> {
    if form.is_empty()
        || form.len() > 128
        || !form
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err("the form is not one exact survey form slug".into());
    }
    Ok(room_dir(root, scope).join("forms").join(form))
}

fn lock(root: &Path, scope: &Scope) -> Result<fs::File, String> {
    ds_layer_store::exclusive_lock(&room_dir(root, scope).join(".lock"))
        .map_err(|_| "the survey hold lock could not be taken".into())
}

/// The manifest of one held form, if held.
pub fn manifest(root: &Path, scope: &Scope, form: &str) -> Result<Option<Manifest>, String> {
    let path = form_dir(root, scope, form)?.join(MANIFEST);
    match fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "a held survey manifest is unreadable".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("a held survey manifest could not be read".into()),
    }
}

/// Every held form's manifest for the scope.
pub fn manifests(root: &Path, scope: &Scope) -> Result<Vec<Manifest>, String> {
    let dir = room_dir(root, scope).join("forms");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let Some(form) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if let Some(held) = manifest(root, scope, &form)? {
            out.push(held);
        }
    }
    out.sort_by(|a, b| a.form.cmp(&b.form));
    Ok(out)
}

/// The held rows of one form, verified against the manifest's digest.
pub fn rows(root: &Path, scope: &Scope, held: &Manifest) -> Result<Vec<Value>, String> {
    let bytes = fs::read(form_dir(root, scope, &held.form)?.join(ROWS))
        .map_err(|_| "the held survey rows could not be read")?;
    if format!("{:x}", Sha256::digest(&bytes)) != held.rows_sha256 {
        return Err("the held survey rows do not match their manifest".into());
    }
    BufReader::new(bytes.as_slice())
        .lines()
        .map(|line| {
            let line = line.map_err(|_| "the held survey rows are unreadable")?;
            serde_json::from_str(&line).map_err(|_| "a held survey row is unreadable".into())
        })
        .collect()
}

/// Answer one read of one form: reuse, refresh or read it, as the kernel
/// decides, then hold the result. `fetch` receives the whole `query_entries`
/// request body and returns the route's NDJSON bytes.
#[allow(clippy::too_many_arguments)]
pub fn refresh<E>(
    root: &Path,
    scope: &Scope,
    form: &str,
    filter: &Filter,
    policy: Refresh,
    now: OffsetDateTime,
    fetch: impl FnOnce(&Value) -> Result<Vec<u8>, E>,
) -> Result<Held, HoldError<E>> {
    let store = HoldError::Store;
    let filter = filter.clone().canonical().map_err(HoldError::Refused)?;
    let _lock = lock(root, scope).map_err(store)?;
    let held = manifest(root, scope, form).map_err(HoldError::Store)?;
    let decision = hold::decide(&scope.project, form, held.as_ref(), &filter, policy, now)
        .map_err(HoldError::Refused)?;
    let (mode, body) = match decision {
        Decision::NotHeld => return Err(HoldError::NotHeld),
        Decision::Reuse => {
            return Ok(Held {
                source: Source::Held,
                manifest: held.ok_or_else(|| HoldError::Store("the held copy vanished".into()))?,
            })
        }
        Decision::Query { mode, body } => (mode, body),
    };
    let bytes = fetch(&body).map_err(HoldError::Fetch)?;
    // A delta merges onto the held copy under its own filter; a replace is
    // the requested filter from nothing.
    let (held_rows, filter) = match (mode, held.as_ref()) {
        (hold::ReadMode::Delta, Some(manifest)) => (
            Some((
                manifest,
                rows(root, scope, manifest).map_err(HoldError::Store)?,
            )),
            manifest.filter.clone(),
        ),
        (_, manifest) => (manifest.map(|m| (m, Vec::new())), filter),
    };
    let Applied { rows, mut manifest } =
        hold::apply(&scope.project, form, held_rows, &filter, mode, &bytes, now)
            .map_err(HoldError::Refused)?;
    let dir = form_dir(root, scope, form).map_err(HoldError::Store)?;
    fs::create_dir_all(&dir)
        .map_err(|_| HoldError::Store("the survey hold could not be created".into()))?;
    let mut encoded = Vec::new();
    for row in &rows {
        serde_json::to_writer(&mut encoded, row)
            .map_err(|_| HoldError::Store("a survey row could not be encoded".into()))?;
        encoded.push(b'\n');
    }
    manifest.rows_sha256 = format!("{:x}", Sha256::digest(&encoded));
    write_whole(&dir.join(ROWS), &encoded).map_err(HoldError::Store)?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|_| HoldError::Store("the survey manifest could not be encoded".into()))?;
    write_whole(&dir.join(MANIFEST), &manifest_bytes).map_err(HoldError::Store)?;
    Ok(Held {
        source: match mode {
            hold::ReadMode::Delta => Source::Delta,
            hold::ReadMode::Replace => Source::Replace,
        },
        manifest,
    })
}

/// The held rows of one form inside a view.
pub fn view(
    root: &Path,
    scope: &Scope,
    held: &Manifest,
    view: &View,
) -> Result<Vec<Value>, String> {
    Ok(rows(root, scope, held)?
        .into_iter()
        .filter(|row| view.admits(row))
        .collect())
}

/// Held photos per form, counted from the media tree by the kernel's grammar:
/// a reference that is its own thumbnail companion is a thumbnail.
pub fn held_media(root: &Path, scope: &Scope) -> BTreeMap<String, HeldMedia> {
    let base = media_dir(root, scope);
    let mut counts: BTreeMap<String, HeldMedia> = BTreeMap::new();
    let mut stack = vec![base.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(object) = path
                .strip_prefix(&base)
                .ok()
                .and_then(|relative| relative.to_str())
            else {
                continue;
            };
            let Ok(address) = ds_command_kernel::survey_media_address::parse(object) else {
                continue;
            };
            let slot = counts.entry(address.form.clone()).or_default();
            if address.thumbnail_object_path == address.object_path {
                slot.thumbnails += 1;
            } else {
                slot.originals += 1;
            }
        }
    }
    counts
}

/// Where one held photo lives, if the object path is a plain relative path.
pub fn media_file(root: &Path, scope: &Scope, object_path: &str) -> Option<PathBuf> {
    let relative = Path::new(object_path);
    relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
        .then(|| media_dir(root, scope).join(relative))
}

/// What is held for the project, against the forms the project names.
pub fn status(
    root: &Path,
    scope: &Scope,
    forms: &[String],
    now: OffsetDateTime,
) -> Result<Value, String> {
    let held = manifests(root, scope)?;
    Ok(hold::status(
        &scope.project,
        forms,
        &held,
        &held_media(root, scope),
        now,
    ))
}

fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let partial = path.with_extension("part");
    let result = fs::File::create(&partial)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&partial, path));
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result.map_err(|_| "the survey hold could not be written".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::time::format_description::well_known::Rfc3339;
    use serde_json::json;

    fn at(text: &str) -> OffsetDateTime {
        OffsetDateTime::parse(text, &Rfc3339).unwrap()
    }

    fn body(rows: &[Value]) -> Vec<u8> {
        let mut out = String::new();
        for row in rows {
            out.push_str(&row.to_string());
            out.push('\n');
        }
        out.push_str(
            &json!({"__type":"summary","form_count":1,"errors":[],"total_features":rows.len()})
                .to_string(),
        );
        out.into_bytes()
    }

    fn feature(id: &str, updated: &str, deleted: bool) -> Value {
        json!({"type":"Feature","geometry":{"type":"Point","coordinates":[29.5,-2.0]},
               "properties":{"id":id,"metadata":{"firestore_updated_at":updated,"is_deleted":deleted}}})
    }

    #[test]
    fn a_form_is_read_held_reused_refreshed_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let scope = Scope {
            principal: "uid-1".into(),
            project: "p1".into(),
        };
        let whole = Filter::default();
        let auto = Refresh::Auto {
            max_age_seconds: 300,
        };

        // Nothing held and Local: refused without a fetch.
        let never = refresh(
            dir.path(),
            &scope,
            "f",
            &whole,
            Refresh::Local,
            at("2026-09-25T10:00:00Z"),
            |_| -> Result<Vec<u8>, ()> { panic!("a local read must not fetch") },
        );
        assert!(matches!(never, Err(HoldError::NotHeld)));

        let first = refresh(
            dir.path(),
            &scope,
            "f",
            &whole,
            auto,
            at("2026-09-25T10:00:00Z"),
            |body| {
                assert_eq!(body["action"], "query_entries");
                Ok::<_, ()>(body_of(&[feature("a", "2026-09-25T09:00:00Z", false)]))
            },
        )
        .unwrap();
        assert_eq!(first.source, Source::Replace);
        assert_eq!(first.manifest.rows, 1);

        // Young copy: reused, no fetch.
        let again = refresh(
            dir.path(),
            &scope,
            "f",
            &whole,
            auto,
            at("2026-09-25T10:01:00Z"),
            |_| -> Result<Vec<u8>, ()> { panic!("a young copy must be reused") },
        )
        .unwrap();
        assert_eq!(again.source, Source::Held);

        // Old copy: a delta from the cursor.
        let delta = refresh(
            dir.path(),
            &scope,
            "f",
            &whole,
            auto,
            at("2026-09-25T11:00:00Z"),
            |body| {
                assert_eq!(body["updated_after"], "2026-09-25T09:00:00Z");
                Ok::<_, ()>(body_of(&[feature("b", "2026-09-25T10:30:00Z", false)]))
            },
        )
        .unwrap();
        assert_eq!(delta.source, Source::Delta);
        assert_eq!(delta.manifest.rows, 2);
        assert_eq!(rows(dir.path(), &scope, &delta.manifest).unwrap().len(), 2);

        // A broken stream leaves the copy as it was.
        let broken = refresh(
            dir.path(),
            &scope,
            "f",
            &whole,
            Refresh::Delta,
            at("2026-09-25T11:05:00Z"),
            |_| Ok::<_, ()>(b"{\"type\":\"Feature\"".to_vec()),
        );
        assert!(matches!(broken, Err(HoldError::Refused(_))));
        assert_eq!(manifest(dir.path(), &scope, "f").unwrap().unwrap().rows, 2);

        // Photos held, counted by the kernel's grammar.
        let photo = media_file(dir.path(), &scope, "projects/p1/forms/f/a/photo/1.jpg").unwrap();
        fs::create_dir_all(photo.parent().unwrap()).unwrap();
        fs::write(&photo, b"x").unwrap();
        fs::write(photo.with_file_name("1_thunder.jpeg"), b"x").unwrap();
        let report = status(
            dir.path(),
            &scope,
            &["f".into(), "g".into()],
            at("2026-09-25T11:06:00Z"),
        )
        .unwrap();
        assert_eq!(report["forms"][0]["rows"], 2);
        assert_eq!(report["forms"][0]["photos"]["originals"], 1);
        assert_eq!(report["forms"][0]["photos"]["thumbnails"], 1);
        assert_eq!(report["forms"][1]["held"], false);
        assert!(media_file(dir.path(), &scope, "../etc/passwd").is_none());
    }

    fn body_of(rows: &[Value]) -> Vec<u8> {
        body(rows)
    }
}
