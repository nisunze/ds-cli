//! Survey photo bytes to local files, headlessly: the photos a survey report
//! shows, fetched under the user's JWT (feedback ee3f371f, 2026-09-25).
//!
//! References come from `ds survey entries read`, one by one (`--path`) or a
//! whole GeoJSON it wrote (`--from`). One report export grant covers the
//! fetch; each photo lands at `<out-dir>/<object path>`, and a photo already
//! there is kept, so a fetch that stopped part-way resumes by running again.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::survey_entries_read::media_address;
use serde_json::{Value, json};

/// Photos in one fetch; a whole form's photos fit, a whole project may not.
const MAX_PHOTOS: usize = 5_000;
const MAX_FROM_BYTES: u64 = 256 * 1024 * 1024;
/// Photos read at once. Each worker holds at most one photo in memory.
const WORKERS: usize = 8;

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "survey_photo_invalid",
        when: "no reference was given, one is not a survey media address, or the photos span more than 32 projects",
        remedy: "pass references exactly as `ds survey entries read` reports them",
    },
    Refusal {
        code: "survey_photo_not_found",
        when: "the project is not visible to this user (a missing photo is listed under `failed` instead)",
        remedy: "verify --project; list a form's photos with `ds survey entries read`",
    },
    Refusal {
        code: "survey_photo_source",
        when: "--from is missing, oversized, or not a GeoJSON written by `ds survey entries read --out`",
        remedy: "write it with `ds survey entries read --out <file.geojson>`",
    },
    Refusal {
        code: "survey_photo_output",
        when: "--out-dir cannot be created or written",
        remedy: "choose a writable directory",
    },
    Refusal {
        code: "survey_photo_forbidden",
        when: "the user lacks reports.export on a project the photos belong to (photo access is the export grant)",
        remedy: "ask a project manager for reports.export on every project the photos belong to",
    },
    Refusal {
        code: "survey_photo_unavailable",
        when: "this deployment cannot sign media links",
        remedy: "report it with `ds feedback submit`",
    },
    Refusal {
        code: "survey_photo_transient",
        when: "the grant service is temporarily unavailable",
        remedy: "run the same fetch again",
    },
    Refusal {
        code: "survey_photo_unreadable",
        when: "the grant broke its contract",
        remedy: "retry once, then report it with `ds feedback submit`",
    },
    ds_cli_auth::SIGNED_OUT_REFUSAL,
    Refusal {
        code: "project_required",
        when: "--project is absent, blank or untrimmed",
        remedy: "pass one exact ds_project value from ds auth project list",
    },
    Refusal {
        code: "auth_transient",
        when: "native identity restoration is temporarily unavailable",
        remedy: "retry without changing local state",
    },
    Refusal {
        code: "native_profile_not_configured",
        when: "the exact packaged native profile is unavailable",
        remedy: "install one complete ds release",
    },
];

pub static FETCH_COMMAND: Command = Command {
    id: "survey.photo.fetch",
    path: &["survey", "photo", "fetch"],
    contract: 1,
    chapter: Chapter::Survey,
    summary: "Fetch survey photo thumbnails (or originals) to local files.",
    purpose: "Use to look at survey photos or put them in a report. Thumbnails by default, as the map previews them: 320 px, fast to fetch and to read; one never uploaded is made from the original, as the map does on hover. Add --original only for photos whose detail matters. References come from `ds survey entries read`, one by one with --path or all at once with --from its --out GeoJSON. Each lands at <out-dir>/<object path>; one already there is kept, so an interrupted fetch resumes by running again. Access is the report export grant (reports.export on each project the photos belong to), checked once per fetch.",
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::PROJECT,
        Arg::repeated(
            "path",
            "<reference>",
            "A photo reference as `survey entries read` reports it; repeat for more.",
        ),
        Arg::value(
            "from",
            "<file.geojson>",
            "Every photo in a GeoJSON written by `survey entries read --out`.",
        ),
        Arg::switch(
            "original",
            "Fetch the full-size originals instead of thumbnails.",
        ),
        Arg::value(
            "out-dir",
            "<dir>",
            "Write here instead of the core's held photos (see `survey local status`).",
        ),
        Arg::value(
            "lane",
            "<stable|canary>",
            "Deployment lane; stable is the default.",
        )
        .default("stable")
        .choices(&["stable", "canary"]),
    ],
    output: "The project and output directory; each photo fetched (object path, file, bytes, media type, and whether a thumbnail was generated), each already present, and each that failed with its code and reason; `complete` is true only when none failed.",
    examples: &[
        Example {
            command: "ds survey photo fetch --project <exact-id> --from entries.geojson --out-dir photos --output json",
            note: "Every photo's thumbnail for a form read with `ds survey entries read --out entries.geojson`.",
            runnable: false,
        },
        Example {
            command: "ds survey photo fetch --project <exact-id> --path <object-path> --original --out-dir photos",
            note: "One full-size photo, when its thumbnail is not enough; the object path is an entry's `media[].object_path`.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/survey.md"),
    search: &["photo download", "photo files", "report photos"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn fetch(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let thumbnail = !inputs.switch("original");
    let (wanted, owners) = wanted(inputs, thumbnail)?;
    // Each photo's project is the kernel address's, not a string prefix.
    let projects: Vec<&str> = owners
        .iter()
        .map(String::as_str)
        .filter(|other| *other != project)
        .collect();
    let headless = ds_cli_auth::survey_media_grant(inputs.require("lane")?, project, &projects)?;
    let grants = headless.result();
    // By default the photos join the core's copy of the project, where
    // `survey local status` counts them and a report finds them.
    let held = inputs.value("out-dir").is_none();
    let out_dir = match inputs.value("out-dir") {
        Some(dir) => PathBuf::from(dir),
        None => {
            let root = ds_layer_store::default_root()
                .map_err(|message| Failure::unavailable("survey_photo_output", message))?;
            ds_project_data::survey_hold::media_dir(
                &root,
                &ds_command_kernel::project_dataset_cache::Scope {
                    principal: headless.identity().uid().to_owned(),
                    project: project.to_owned(),
                },
            )
        }
    };
    if held {
        ds_layer_store::private::create_dir_all(&out_dir).map_err(output)?;
    } else {
        std::fs::create_dir_all(&out_dir).map_err(output)?;
    }

    let (mut fetched, mut present, mut failed) = (Vec::new(), Vec::new(), Vec::new());
    let mut missing = Vec::new();
    for (object_path, original) in &wanted {
        let file = out_dir.join(object_path);
        if file.is_file() {
            present.push(json!({"object_path": object_path, "file": file}));
        } else {
            missing.push((original.as_str(), file));
        }
    }
    // Each photo is a resolver redirect then a storage read; one at a time a
    // form's hundred photos took nine minutes on 2026-09-25.
    let next = std::sync::atomic::AtomicUsize::new(0);
    let outcomes = std::sync::Mutex::new(Vec::with_capacity(missing.len()));
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(missing.len()) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some((original, file)) = missing.get(index) else {
                        break;
                    };
                    let outcome = one(grants, original, file, thumbnail, held);
                    outcomes
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push(outcome);
                }
            });
        }
    });
    let mut outcomes = outcomes
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    outcomes.sort_by(|left, right| {
        left.1["object_path"]
            .as_str()
            .cmp(&right.1["object_path"].as_str())
    });
    for (ok, outcome) in outcomes {
        if ok {
            fetched.push(outcome);
        } else {
            failed.push(outcome);
        }
    }
    Ok(json!({
        "lane": headless.lane(),
        "project": {"ds_project": headless.project_id()},
        "out_dir": out_dir,
        "held": held,
        "thumbnail": thumbnail,
        "requested": wanted.len(),
        "complete": failed.is_empty(),
        "grant": {
            "projects": grants.grants().iter().map(|grant| grant.project()).collect::<Vec<_>>(),
            "expires_at": grants.grants().first().map(|grant| grant.expires_at()),
        },
        "fetched": fetched,
        "present": present,
        "failed": failed,
    }))
}

/// One photo fetched and written: `(true, receipt)` or `(false, failure)`.
/// A thumbnail the store never received is made from the original.
fn one(
    grants: &ds_client_core::MediaGrants,
    original: &str,
    file: &Path,
    thumbnail: bool,
    held: bool,
) -> (bool, Value) {
    let fetched = if thumbnail {
        ds_cli_auth::survey_thumbnail_bytes(grants, original)
            .map(|made| (made.photo, made.generated))
    } else {
        ds_cli_auth::survey_photo_bytes(grants, original).map(|photo| (photo, false))
    };
    match fetched {
        Ok((photo, generated)) => match write(file, &photo.bytes, held) {
            Ok(()) => (
                true,
                json!({
                    "object_path": photo.object_path,
                    "file": file,
                    "bytes": photo.bytes.len(),
                    "media_type": photo.media_type,
                    "generated": generated,
                }),
            ),
            Err(error) => (
                false,
                json!({
                    "object_path": photo.object_path,
                    "code": "survey_photo_output",
                    "message": error.to_string(),
                }),
            ),
        },
        Err(failure) => (
            false,
            json!({
                "object_path": original,
                "code": failure.code(),
                "message": failure.message(),
            }),
        ),
    }
}

/// What to write (the target object path: the original, or its thumbnail)
/// mapped to the original it comes from, each once, in a stable order.
#[allow(clippy::type_complexity)]
fn wanted(
    inputs: &Inputs,
    thumbnail: bool,
) -> Result<(BTreeMap<String, String>, BTreeSet<String>), Failure> {
    let mut references: Vec<String> = inputs.repeated("path").to_vec();
    if let Some(from) = inputs.value("from") {
        references.extend(from_file(from)?);
    }
    if references.is_empty() {
        return Err(invalid("pass --path or --from with at least one photo"));
    }
    let mut wanted = BTreeMap::new();
    let mut owners = BTreeSet::new();
    for reference in &references {
        let (address, _) = media_address(reference).ok_or_else(|| {
            invalid(format!(
                "`{}` is not a survey media reference",
                reference.chars().take(160).collect::<String>()
            ))
        })?;
        let path = if thumbnail {
            address.thumbnail_object_path
        } else {
            address.object_path.clone()
        };
        // The kernel admits only plain segments; a written path must not
        // climb out of --out-dir even if that ever changed.
        if !Path::new(&path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        {
            return Err(invalid("a photo path is not a plain relative path"));
        }
        owners.insert(address.project.clone());
        wanted.insert(path, address.object_path);
    }
    if wanted.len() > MAX_PHOTOS {
        return Err(invalid(
            "more than 5000 photos; fetch a narrower read (--bbox, --updated-after) at a time",
        ));
    }
    Ok((wanted, owners))
}

/// Every `media[].object_path` in a GeoJSON written by `entries read --out`.
fn from_file(raw: &str) -> Result<Vec<String>, Failure> {
    let source = |message: String| {
        Failure::invalid("survey_photo_source", message)
            .remedy("write it with `ds survey entries read --out <file.geojson>`")
    };
    let file = std::fs::File::open(raw).map_err(|error| source(format!("`{raw}`: {error}")))?;
    let mut body = String::new();
    file.take(MAX_FROM_BYTES + 1)
        .read_to_string(&mut body)
        .map_err(|error| source(format!("`{raw}`: {error}")))?;
    if body.len() as u64 > MAX_FROM_BYTES {
        return Err(source(format!("`{raw}` is larger than 256 MiB")));
    }
    let document: Value =
        serde_json::from_str(&body).map_err(|_| source(format!("`{raw}` is not JSON")))?;
    let features = document
        .get("features")
        .and_then(Value::as_array)
        .filter(|_| document["type"] == "FeatureCollection")
        .ok_or_else(|| source(format!("`{raw}` is not a FeatureCollection")))?;
    Ok(features
        .iter()
        .filter_map(|feature| feature.get("media").and_then(Value::as_array))
        .flatten()
        .filter_map(|media| media.get("object_path").and_then(Value::as_str))
        .map(str::to_owned)
        .collect())
}

/// Written beside its final name and renamed into place, so a file that
/// exists is always a whole photo. A held photo is the core's copy of
/// respondents' media: its directories are 0700 and the file 0600
/// (`ds_layer_store::private`); a photo written to the user's `--out-dir`
/// keeps ordinary modes.
fn write(file: &Path, bytes: &[u8], held: bool) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        if held {
            ds_layer_store::private::create_dir_all(parent)?;
        } else {
            std::fs::create_dir_all(parent)?;
        }
    }
    let partial = file.with_extension("part");
    let created = if held {
        ds_layer_store::private::create_file(&partial)
    } else {
        std::fs::File::create(&partial)
    };
    let result = created
        .and_then(|mut handle| handle.write_all(bytes).and_then(|()| handle.sync_all()))
        .and_then(|()| std::fs::rename(&partial, file));
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result
}

fn output(error: std::io::Error) -> Failure {
    Failure::invalid("survey_photo_output", error.to_string()).remedy("choose a writable directory")
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("survey_photo_invalid", message)
        .remedy("pass references exactly as `ds survey entries read` reports them")
}

pub fn render(data: &Value) -> String {
    let count = |key: &str| data[key].as_array().map_or(0, Vec::len);
    let mut text = format!(
        "{} photos under {}: {} fetched, {} already present, {} failed\n",
        data["requested"].as_u64().unwrap_or(0),
        data["out_dir"].as_str().unwrap_or("."),
        count("fetched"),
        count("present"),
        count("failed"),
    );
    for failure in data["failed"].as_array().into_iter().flatten().take(20) {
        text.push_str(&format!(
            "  FAILED {}  {}: {}\n",
            failure["object_path"].as_str().unwrap_or(""),
            failure["code"].as_str().unwrap_or(""),
            failure["message"].as_str().unwrap_or(""),
        ));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_cli_contract::args::parse;

    fn inputs(arguments: &[&str]) -> Inputs {
        parse(
            &FETCH_COMMAND,
            &arguments
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
        .expect("closed arguments")
    }

    #[test]
    fn references_resolve_before_auth_to_canonical_paths_once() {
        let dir = std::env::temp_dir().join(format!("ds-photo-fetch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let from = dir.join("entries.geojson");
        std::fs::write(
            &from,
            json!({"type":"FeatureCollection","features":[
                {"type":"Feature","media":[{"object_path":"projects/p1/forms/f/e1/photo/a.jpg"}]},
                {"type":"Feature","media":[]}]})
            .to_string(),
        )
        .unwrap();
        let arguments = [
            "--project",
            "p1",
            "--out-dir",
            "x",
            "--path",
            "gs://bucket/projects/p1/forms/f/e1/photo/a.jpg",
            "--path",
            "projects/p0/forms/f/e2/b.jpg",
            "--from",
            from.to_str().unwrap(),
        ];
        let (wanted, owners) = wanted(&inputs(&arguments), false).unwrap();
        assert_eq!(owners.into_iter().collect::<Vec<_>>(), ["p0", "p1"]);
        assert_eq!(
            wanted.into_keys().collect::<Vec<_>>(),
            [
                "projects/p0/forms/f/e2/b.jpg",
                "projects/p1/forms/f/e1/photo/a.jpg"
            ]
        );
        let (thumbnails, _) = super::wanted(&inputs(&arguments), true).unwrap();
        assert!(
            thumbnails
                .iter()
                .all(|(path, original)| path.ends_with("_thunder.jpeg")
                    && original.ends_with(".jpg"))
        );
        let _ = std::fs::remove_dir_all(&dir);

        for bad in [
            &["--project", "p1", "--out-dir", "x"][..],
            &["--project", "p1", "--out-dir", "x", "--path", "not a photo"],
            &[
                "--project",
                "p1",
                "--out-dir",
                "x",
                "--path",
                "../../etc/passwd",
            ],
        ] {
            assert_eq!(
                super::wanted(&inputs(bad), false).unwrap_err().code(),
                "survey_photo_invalid"
            );
        }
    }

    #[test]
    fn failures_are_listed_by_name() {
        let text = render(&json!({"requested":3,"out_dir":"photos",
            "fetched":[{}],"present":[{}],
            "failed":[{"object_path":"projects/p/forms/f/e/x.jpg","code":"survey_photo_not_found","message":"the photo is not stored"}]}));
        assert!(text.contains("3 photos under photos: 1 fetched, 1 already present, 1 failed"));
        assert!(text.contains("FAILED projects/p/forms/f/e/x.jpg  survey_photo_not_found"));
    }

    /// A held photo is private to the account (owner rule); one the user
    /// asked for in their own `--out-dir` keeps the platform's ordinary modes.
    #[cfg(unix)]
    #[test]
    fn held_photos_are_private_and_user_output_is_not() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir = std::env::temp_dir().join(format!("ds-photo-modes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let held = dir.join("media/projects/p1/forms/f/e1/photo/a.jpg");
        write(&held, b"jpeg", true).unwrap();
        assert_eq!(mode(&held), 0o600);
        for folder in held.ancestors().skip(1).take(6) {
            assert_eq!(mode(folder), 0o700, "{}", folder.display());
        }
        let exported = dir.join("out/a.jpg");
        write(&exported, b"jpeg", false).unwrap();
        let control = dir.join("out/control");
        std::fs::File::create(&control).unwrap();
        assert_eq!(
            mode(&exported),
            mode(&control),
            "a user export keeps ordinary modes"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
