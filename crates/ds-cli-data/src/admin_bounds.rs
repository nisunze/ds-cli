//! Exact national boundary reads, and hierarchy attachment on a point file.
//!
//! The two reads are national reference data — no project, no window, nothing
//! to render — so they call the gateway as the restored native user through
//! `ds-client-core::admin_bounds`. Until 2026-09-18 they asked the paired
//! desktop to make the same request, which meant a machine without a window
//! could not read a boundary at all.
//!
//! `attach` is different and still paired: it needs the ACTIVE PROJECT's
//! digest-pinned Rwanda reference asset, which only the desktop's component
//! manager installs and verifies today.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, NOT_PAIRED, PAIRING_REJECTED, PROJECT_NOT_OPEN, REFUSED,
    SIGNED_OUT, UNREACHABLE, UNREADABLE as DESKTOP_UNREADABLE, UNSUPPORTED as DESKTOP_UNSUPPORTED,
    classify_signed_out, invoke, paired, paired_availability,
};
use ds_client_core::admin_bounds::{Answer, Command as Read, Country, Level};
use serde_json::{Map, Value, json};

pub const OPERATION: BridgeOp = BridgeOp {
    operation: "data.admin_bounds.attach",
    arguments: &["source", "out", "longitude_column", "latitude_column"],
};

const COUNTRY_ARG: Arg = Arg {
    name: "country",
    kind: ArgKind::Value,
    value: "<country>",
    required: false,
    default: Some("rwanda"),
    choices: &["rwanda"],
    summary: "Declared national boundary authority. Rwanda is the only installed authority.",
};
const LEVEL_ARG: Arg = Arg {
    name: "level",
    kind: ArgKind::Value,
    value: "<level>",
    required: true,
    default: None,
    choices: &["province", "district", "sector", "cell", "village"],
    summary: "Exact hierarchy level to list.",
};
const PARENT_CODE_ARG: Arg = Arg::value(
    "parent-code",
    "<code>",
    "Exact immediate parent code. Required below province.",
);
const CODE_ARG: Arg = Arg {
    name: "code",
    kind: ArgKind::Value,
    value: "<code>",
    required: true,
    default: None,
    choices: &[],
    summary: "Exact 1, 2, 4, 6, or 8 digit Rwanda administrative code.",
};
const GEOMETRY_OUT_ARG: Arg = Arg::value(
    "geometry-out",
    "<path.geojson>",
    "Also write the exact polygon here, as a one-feature GeoJSON layer. Existing files are never overwritten.",
);
const LANE_ARG: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);

const INVALID_ADMIN_SCOPE: Refusal = Refusal {
    code: "invalid_admin_scope",
    when: "the country, level, code, or immediate parent relationship is not exact",
    remedy: "use country rwanda and the declared 1/2/4/6/8-digit hierarchy",
};
const GEOMETRY_OUT_REFUSED: Refusal = Refusal {
    code: "output_refused",
    when: "--geometry-out already exists, or its directory cannot be written",
    remedy: "choose a new --geometry-out path; an exact boundary never overwrites a file",
};

/// This domain's own refusals, then every refusal the native user path can
/// return, composed so a new one reaches these reads rather than going
/// undocumented. The authority's own failures arrive through that list:
/// `auth_transient` when it cannot answer, `auth_response_unreadable` when it
/// answers outside its exact hierarchy contract.
const AUTH_REFUSALS: usize = ds_cli_auth::PROJECT_LIST_COMMAND.refusals.len();
const fn with_native_refusals<const N: usize, const TOTAL: usize>(
    own: [Refusal; N],
) -> [Refusal; TOTAL] {
    let mut all = [INVALID_ADMIN_SCOPE; TOTAL];
    let mut index = 0;
    while index < N {
        all[index] = own[index];
        index += 1;
    }
    index = 0;
    while index < AUTH_REFUSALS {
        all[N + index] = ds_cli_auth::PROJECT_LIST_COMMAND.refusals[index];
        index += 1;
    }
    all
}
const LIST_REFUSAL_SET: [Refusal; 1 + AUTH_REFUSALS] =
    with_native_refusals::<1, { 1 + AUTH_REFUSALS }>([INVALID_ADMIN_SCOPE]);
const LIST_REFUSALS: &[Refusal] = &LIST_REFUSAL_SET;
const READ_REFUSAL_SET: [Refusal; 2 + AUTH_REFUSALS] =
    with_native_refusals::<2, { 2 + AUTH_REFUSALS }>([INVALID_ADMIN_SCOPE, GEOMETRY_OUT_REFUSED]);
const READ_REFUSALS: &[Refusal] = &READ_REFUSAL_SET;

const OUT_ARG: Arg = Arg {
    name: "out",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "New same-format output path. Existing files are never overwritten.",
};
const SOURCE_ARG: Arg = Arg {
    name: "source",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "Local CSV, TSV, GeoJSON, or JSON point file carrying elevation data.",
};
const LONGITUDE_ARG: Arg = Arg::value(
    "longitude-column",
    "<name>",
    "Longitude column for CSV/TSV input. Omit for GeoJSON geometry.",
);
const LATITUDE_ARG: Arg = Arg::value(
    "latitude-column",
    "<name>",
    "Latitude column for CSV/TSV input. Omit for GeoJSON geometry.",
);
const ADMIN_UNSUPPORTED: Refusal = Refusal {
    code: "source_unsupported",
    when: "the source is not CSV, TSV, or GeoJSON, or table coordinate columns were omitted",
    remedy: "use GeoJSON geometry, or pass both coordinate columns for CSV/TSV",
};
const NO_OVERWRITE: Refusal = Refusal {
    code: "output_refused",
    when: "the output already exists, is the source, or cannot be created safely",
    remedy: "choose a new same-format --out path; this command never overwrites",
};

pub static COMMAND: Command = Command {
    id: "data.admin-bounds.attach",
    path: &["data", "admin-bounds", "attach"],
    contract: 1,
    summary: "Attach Rwanda province-to-village fields to local elevation points.",
    purpose: "Uses the active project's digest-pinned Rwanda boundary resource and the bundled native reporter to write a new CSV, TSV, or GeoJSON file. The source geometry and elevation fields are unchanged; existing operator-supplied admin values win. This needs the paired desktop for its governed installed resource, but it does not need the map to be open.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        SOURCE_ARG,
        OUT_ARG,
        LONGITUDE_ARG,
        LATITUDE_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, output path, matched/outside counts, output digest, reference digest, and attached columns.",
    examples: &[
        Example {
            command: "ds data admin-bounds attach --source ./elevation.csv --out ./elevation-admin.csv --longitude-column longitude --latitude-column latitude",
            note: "Streams table rows locally; no map is opened.",
            runnable: false,
        },
        Example {
            command: "ds data admin-bounds attach --source ./elevation.geojson --out ./elevation-admin.geojson",
            note: "GeoJSON reads coordinates from feature geometry.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::UNREADABLE,
        ADMIN_UNSUPPORTED,
        NO_OVERWRITE,
        NOT_PAIRED,
        PROJECT_NOT_OPEN,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        SIGNED_OUT,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    requires: Requires::Window,
    availability: paired_availability,
};

pub static LIST_COMMAND: Command = Command {
    id: "data.admin-bounds.list",
    path: &["data", "admin-bounds", "list"],
    contract: 1,
    summary: "List exact Rwanda administrative units under one immediate parent.",
    purpose: "Reads the same authenticated national hierarchy Desktop Search place reads, as the signed-in native user. Province is the root; every lower level requires its exact immediate parent code, which keeps the result bounded and prevents a guessed hierarchy. This is country-scoped reference data, not project data: no project is selected, no project is fenced, and no window is involved.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[COUNTRY_ARG, LEVEL_ARG, PARENT_CODE_ARG, LANE_ARG],
    output: "Country and reference scope, level, parent code, exact count, and bounded code/name rows. Geometry is not included.",
    examples: &[
        Example {
            command: "ds data admin-bounds list --country rwanda --level province --output json",
            note: "Lists the hierarchy root; needs no project and no window.",
            runnable: false,
        },
        Example {
            command: "ds data admin-bounds list --country rwanda --level village --parent-code 110101 --output json",
            note: "Lists only the exact villages of one cell.",
            runnable: false,
        },
    ],
    refusals: LIST_REFUSALS,
    reference: Some("docs/reference/data.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static READ_COMMAND: Command = Command {
    id: "data.admin-bounds.read",
    path: &["data", "admin-bounds", "read"],
    contract: 1,
    summary: "Read one exact Rwanda boundary and bounded geometry evidence.",
    purpose: "Reads one code from the same authenticated geometry authority Desktop Search place reads. The polygon is not printed: hundreds of kilometres of coastline are not a terminal answer, so the receipt reports identity, type, bounds, coordinate count and digest. Pass --geometry-out to keep the exact bytes as a one-feature GeoJSON layer, which `ds map local register` takes as it stands.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[COUNTRY_ARG, CODE_ARG, GEOMETRY_OUT_ARG, LANE_ARG],
    output: "National reference scope, exact boundary identity, geometry type, bounds, coordinate count and SHA-256, and the written path when --geometry-out was given.",
    examples: &[
        Example {
            command: "ds data admin-bounds read --country rwanda --code 11010102 --output json",
            note: "Bounded evidence for one exact village.",
            runnable: false,
        },
        Example {
            command: "ds data admin-bounds read --country rwanda --code 11010102 --geometry-out ./gihanga.geojson --output json",
            note: "Keeps the exact authority polygon as a layer file.",
            runnable: false,
        },
    ],
    refusals: READ_REFUSALS,
    reference: Some("docs/reference/data.md"),
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid_admin_scope(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_admin_scope", message).remedy(INVALID_ADMIN_SCOPE.remedy)
}

/// A refused vocabulary is this command's own refusal, not the authority's:
/// nothing has been sent yet when it is raised.
fn scope(error: ds_client_core::ClientError) -> Failure {
    invalid_admin_scope(error.to_string())
}

fn country(inputs: &Inputs) -> Result<Country, Failure> {
    Country::parse(inputs.require("country")?).map_err(scope)
}

/// Validate here, before a profile is loaded or a socket is opened, so an
/// impossible hierarchy is answered on any machine — signed in or not.
fn validated(command: Read) -> Result<Read, Failure> {
    command.validate().map_err(scope)?;
    Ok(command)
}

pub fn run_list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let country = country(inputs)?;
    let command = validated(Read::Children {
        country,
        level: Level::parse(inputs.require("level")?).map_err(scope)?,
        parent_code: inputs.value("parent-code").map(str::to_owned),
    })?;
    let answer = ds_cli_auth::admin_bounds(inputs.require("lane")?, &command)?;
    Ok(answer.receipt(country))
}

pub fn run_read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let country = country(inputs)?;
    let command = validated(Read::Boundary {
        country,
        code: inputs.require("code")?.to_owned(),
    })?;
    // The output path is settled before the read, so a boundary is never
    // fetched only to be dropped on a path that was never writable.
    let out = inputs
        .value("geometry-out")
        .map(|value| geometry_out(Path::new(value)))
        .transpose()?;
    let answer = ds_cli_auth::admin_bounds(inputs.require("lane")?, &command)?;
    let mut receipt = answer.receipt(country);
    if let (Some(out), Answer::Boundary(boundary)) = (out, &answer) {
        let bytes = serde_json::to_vec(&boundary.feature_collection())
            .map_err(|error| Failure::internal("output_refused", error.to_string()))?;
        // `create_new` rather than `write`: the path was free when it was
        // checked, and it must still be free now, or "never overwrites" is a
        // claim with a window in it.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out)
            .and_then(|mut file| file.write_all(&bytes))
            .map_err(|error| {
                Failure::conflict(
                    "output_refused",
                    format!("Could not write the boundary geometry: {error}"),
                )
                .remedy(GEOMETRY_OUT_REFUSED.remedy)
            })?;
        receipt["geometry"]["written_to"] = json!(out.to_string_lossy());
    }
    Ok(receipt)
}

/// Resolve `--geometry-out` and refuse before anything is read.
fn geometry_out(raw: &Path) -> Result<PathBuf, Failure> {
    let out = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| Failure::internal("output_refused", error.to_string()))?
            .join(raw)
    };
    if out.exists() {
        return Err(
            Failure::conflict("output_refused", "The --geometry-out file already exists.")
                .remedy(GEOMETRY_OUT_REFUSED.remedy),
        );
    }
    Ok(out)
}

pub fn render_list(data: &Value) -> String {
    let mut out = format!(
        "{} {}{}\n",
        data["country"].as_str().unwrap_or("Rwanda"),
        data["level"].as_str().unwrap_or("boundaries"),
        data["parent_code"]
            .as_str()
            .map(|code| format!(" under {code}"))
            .unwrap_or_default(),
    );
    for row in data["boundaries"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<10} {}\n",
            row["code"].as_str().unwrap_or("?"),
            row["name"].as_str().unwrap_or("?"),
        ));
    }
    out
}

pub fn render_read(data: &Value) -> String {
    let boundary = &data["boundary"];
    let geometry = &data["geometry"];
    let mut out = format!(
        "{} {} {}\n  {} · {} coordinate positions · sha256 {}\n",
        boundary["level"].as_str().unwrap_or("boundary"),
        boundary["code"].as_str().unwrap_or("?"),
        boundary["name"].as_str().unwrap_or("?"),
        geometry["type"].as_str().unwrap_or("geometry"),
        geometry["coordinate_positions"].as_u64().unwrap_or(0),
        geometry["sha256"].as_str().unwrap_or("?"),
    );
    if let Some(path) = data["geometry"]["written_to"].as_str() {
        out.push_str(&format!("  geometry written to {path}\n"));
    }
    out
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = std::fs::canonicalize(inputs.require("source")?).map_err(|error| {
        Failure::invalid(
            "source_unreadable",
            format!("Could not resolve the source: {error}"),
        )
        .remedy("Check the path exists and is readable.")
    })?;
    let raw_out = std::path::PathBuf::from(inputs.require("out")?);
    let out = if raw_out.is_absolute() {
        raw_out
    } else {
        std::env::current_dir()
            .map_err(|error| Failure::internal("output_refused", error.to_string()))?
            .join(raw_out)
    };
    if out.exists() {
        return Err(
            Failure::conflict("output_refused", "The output file already exists.")
                .remedy("Choose another --out path; admin attachment never overwrites."),
        );
    }
    let longitude = inputs.value("longitude-column");
    let latitude = inputs.value("latitude-column");
    if longitude.is_some() != latitude.is_some() {
        return Err(Failure::invalid(
            "source_unsupported",
            "--longitude-column and --latitude-column must be supplied together.",
        ));
    }
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "csv" | "tsv") && longitude.is_none() {
        return Err(Failure::invalid(
            "source_unsupported",
            "CSV/TSV admin attachment requires explicit coordinate columns.",
        )
        .remedy("Pass --longitude-column and --latitude-column from `ds data inspect`."));
    }
    if !matches!(extension.as_str(), "csv" | "tsv" | "geojson" | "json") {
        return Err(Failure::invalid(
            "source_unsupported",
            "Admin attachment supports CSV, TSV, and GeoJSON point files.",
        ));
    }
    let mut arguments = Map::new();
    arguments.insert("source".into(), json!(source.to_string_lossy()));
    arguments.insert("out".into(), json!(out.to_string_lossy()));
    if let Some(value) = longitude {
        arguments.insert("longitude_column".into(), json!(value));
    }
    if let Some(value) = latitude {
        arguments.insert("latitude_column".into(), json!(value));
    }
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    invoke(
        &descriptor,
        &OPERATION,
        Value::Object(arguments),
        Duration::from_secs(20 * 60),
    )
    .map_err(classify_signed_out)
}

pub fn render(data: &Value) -> String {
    format!(
        "attached Rwanda admin bounds to {}\n  {} of {} points matched · sha256 {}\n",
        data["out"].as_str().unwrap_or("?"),
        data["features_matched"].as_u64().unwrap_or(0),
        data["features_read"].as_u64().unwrap_or(0),
        data["output_sha256"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_scope_validation_has_one_stable_code() {
        // Every way a caller can name a hierarchy that does not exist is the
        // same refusal, and all of them are raised here — before a profile is
        // loaded, before a socket is opened, and therefore on any machine.
        let refusals = [
            Country::parse("kenya").map(|_| ()).map_err(scope),
            Level::parse("county").map(|_| ()).map_err(scope),
            validated(Read::Boundary {
                country: Country::Rwanda,
                code: "110".into(),
            })
            .map(|_| ()),
            validated(Read::Children {
                country: Country::Rwanda,
                level: Level::Village,
                parent_code: None,
            })
            .map(|_| ()),
            validated(Read::Children {
                country: Country::Rwanda,
                level: Level::Province,
                parent_code: Some("1".into()),
            })
            .map(|_| ()),
            validated(Read::Children {
                country: Country::Rwanda,
                level: Level::Sector,
                parent_code: Some("1".into()),
            })
            .map(|_| ()),
        ];
        for refusal in refusals {
            let failure = refusal.expect_err("an inexact scope is refused");
            assert_eq!(failure.code(), "invalid_admin_scope");
            assert_eq!(failure.class().token(), "invalid_input");
            assert!(failure.remedy_text().is_some());
        }
    }

    #[test]
    fn an_exact_leg_of_the_hierarchy_is_accepted() {
        // The mirror of the refusals: the exact prefix hierarchy passes, so the
        // check above is not simply refusing everything.
        for command in [
            Read::Children {
                country: Country::Rwanda,
                level: Level::Province,
                parent_code: None,
            },
            Read::Children {
                country: Country::Rwanda,
                level: Level::Village,
                parent_code: Some("110101".into()),
            },
            Read::Boundary {
                country: Country::Rwanda,
                code: "11010102".into(),
            },
        ] {
            assert!(validated(command).is_ok());
        }
    }

    #[test]
    fn the_two_reads_need_no_window() {
        for command in [&LIST_COMMAND, &READ_COMMAND] {
            assert_eq!(command.authority, Authority::HeadlessUser);
            assert!(
                command
                    .args
                    .iter()
                    .all(|arg| arg.name != "desktop-descriptor"),
                "{} still takes a descriptor",
                command.id
            );
            assert!(
                command.args.iter().any(|arg| arg.name == "lane"),
                "{} cannot choose its native lane",
                command.id
            );
            // Composed, not copied: a new native refusal reaches these reads.
            for refusal in ds_cli_auth::PROJECT_LIST_COMMAND.refusals {
                assert!(
                    command.refusals.iter().any(|own| own.code == refusal.code),
                    "{} does not document {}",
                    command.id,
                    refusal.code
                );
            }
            // And no desktop refusal survives on a command with no door.
            assert!(
                !command
                    .refusals
                    .iter()
                    .any(|refusal| refusal.code.starts_with("desktop_")
                        || refusal.code == "not_paired"),
                "{} documents a refusal it can no longer raise",
                command.id
            );
        }
        // Attachment is the one paired command left here, and it is the only
        // operation this crate may still send.
        assert_eq!(crate::BRIDGE_OPS.len(), 4);
        assert!(
            crate::BRIDGE_OPS
                .iter()
                .all(|op| !op.operation.starts_with("data.admin_bounds.")
                    || op.operation == "data.admin_bounds.attach")
        );
    }

    #[test]
    fn a_boundary_never_overwrites_a_file() {
        let dir = std::env::temp_dir().join(format!(
            "ds-admin-bounds-{}-{}",
            std::process::id(),
            "geometry-out"
        ));
        std::fs::create_dir_all(&dir).expect("a temporary directory");
        let taken = dir.join("existing.geojson");
        std::fs::write(&taken, b"{}").expect("a file in the way");
        let failure = geometry_out(&taken).expect_err("an existing file is refused");
        assert_eq!(failure.code(), "output_refused");
        assert_eq!(
            geometry_out(&dir.join("new.geojson")).expect("a free path resolves"),
            dir.join("new.geojson")
        );
        // A relative path is resolved against the caller's own directory, so
        // the receipt names a path the caller can find again.
        assert!(
            geometry_out(Path::new("boundary.geojson"))
                .expect("relative")
                .is_absolute()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
