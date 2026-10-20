//! The installed and published release of the main application or DS Core.
//! Core reads the lane's catalog member once through its captured release
//! authority, retains the Kernel's proof beside the module receipts, and
//! returns the Kernel's decision. Installs, downloads and activates nothing.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_command_kernel::desktop_update::installation::{
    self, Application, CatalogRead, ReleaseIdentityInput,
};
use ds_command_kernel::time::{OffsetDateTime, format_description::well_known::Rfc3339};
use ds_update_runtime::modules::release_identity::InstallationStore;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Read;

const INVALID: Refusal = Refusal {
    code: "release_identity_invalid",
    when: "the component, lane or main application build identity is malformed or crossed",
    remedy: "name shell or core, this Core's own lane for core, and the running main application's exact build",
};

pub static INSPECT: Command = Command {
    id: "core.release-identity",
    path: &["core", "release-identity"],
    contract: 1,
    summary: "Show installed and published releases of the app or DS Core.",
    purpose: "Read the lane's catalog member for the main application (shell) or DS Core once, natively; the Kernel admits it and decides version, build number and release date for both rows. The installed row is proven when the member publishes exactly this build, or recalled from that proof retained beside the module receipts, so it survives later publications and offline reads. Core names itself; the main application names its build through the installed-* inputs, otherwise its last proven build stands in. Installs, downloads and activates nothing.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "component",
            "<shell|core>",
            "Main application (shell) or DS Core.",
        )
        .choices(&["shell", "core"])
        .required(),
        Arg::value(
            "lane",
            "<stable|canary>",
            "Installed lane; defaults to this Core's.",
        )
        .choices(&["stable", "canary"]),
        Arg::value(
            "installed-version",
            "<semver>",
            "Shell only: running main application's version.",
        ),
        Arg::value(
            "installed-build",
            "<sha>",
            "Shell only: its exact build identity.",
        ),
        Arg::value(
            "installed-built-at",
            "<rfc3339>",
            "Shell only: its exact build time.",
        ),
        Arg::switch(
            "offline",
            "Answer from retained facts only; read no catalog.",
        ),
    ],
    output: "component, channel, installed and published {version, buildNumber, releaseDate} (null: not recorded), admitted release, update (main application only), installation {source: catalog|recorded|unrecorded, recordedAt}, catalog {state: read|unpublished|unavailable|offline|local, message}.",
    examples: &[],
    refusals: &[INVALID],
    reference: Some("docs/reference/core.md"),
    search: &[
        "version",
        "build",
        "release",
        "date",
        "installed",
        "published",
    ],
    requires: Requires::Server,
    availability: || Availability::Available,
};

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(INVALID.code, message).remedy(INVALID.remedy)
}

/// The main application's own build, as it names itself. All or nothing.
fn shell_claim(inputs: &Inputs) -> Result<Value, Failure> {
    let fields = [
        inputs.value("installed-version"),
        inputs.value("installed-build"),
        inputs.value("installed-built-at"),
    ];
    match fields {
        [None, None, None] => Ok(Value::Null),
        [Some(version), Some(build), Some(built_at)] => {
            Ok(json!({"version": version, "sourceSha": build, "builtAt": built_at}))
        }
        _ => Err(invalid(
            "name the main application's version, build and build time together, or none",
        )),
    }
}

/// This executable's own bytes; unknown bytes bind no record.
fn executable_sha256() -> Option<String> {
    let path = std::env::current_exe().ok()?;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 1 << 16];
    loop {
        match file.read(&mut buffer).ok()? {
            0 => return Some(format!("{:x}", hash.finalize())),
            count => hash.update(&buffer[..count]),
        }
    }
}

fn platform() -> Result<&'static str, Failure> {
    if cfg!(target_os = "windows") {
        Ok("windows")
    } else if cfg!(target_os = "macos") {
        Ok("macos")
    } else if cfg!(target_os = "linux") {
        Ok("linux")
    } else {
        Err(invalid("this host has no release platform"))
    }
}

/// `core` is this Core's build identity (`ds version`), supplied by the binary.
pub fn inspect(inputs: &Inputs, _: &Context, core: Value) -> Result<Value, Failure> {
    let application = Application::parse(inputs.require("component")?).map_err(invalid)?;
    let own = ds_cli_auth::reliability::release_lane();
    let channel = match (inputs.value("lane"), application) {
        (Some(lane), Application::Core) if lane != own => {
            return Err(invalid(format!(
                "DS Core reads its own {own} lane; it cannot be the {lane} release"
            )));
        }
        (Some(lane), _) => lane,
        (None, _) => own,
    };
    let installed = match application {
        Application::Core => {
            if inputs.value("installed-version").is_some()
                || inputs.value("installed-build").is_some()
                || inputs.value("installed-built-at").is_some()
            {
                return Err(invalid(
                    "DS Core names its own build; installed-* are for shell",
                ));
            }
            let mut identity = core;
            // Only a public lane retains a record, so only it binds the bytes.
            if channel != "local" {
                if let (Some(object), Some(digest)) =
                    (identity.as_object_mut(), executable_sha256())
                {
                    object.insert("executable_sha256".into(), json!(digest));
                }
            }
            identity
        }
        Application::Shell => shell_claim(inputs)?,
    };
    // Local development retains nothing; a public lane's record is read
    // before any catalog request, and an unreadable store is never written.
    let (store, mut record_error) = match channel {
        "local" => (None, None),
        lane => match InstallationStore::managed(application, lane) {
            Ok(store) => (Some(store), None),
            Err(error) => (None, Some(error)),
        },
    };
    let record = match store.as_ref().map(InstallationStore::read).transpose() {
        Ok(record) => record.flatten(),
        Err(error) => {
            record_error = Some(error);
            None
        }
    };
    let catalog = if channel == "local" {
        CatalogRead::Local
    } else if inputs.switch("offline") {
        CatalogRead::Offline
    } else {
        read_catalog(application, channel)
    };
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| Failure::internal("release_identity_clock", e.to_string()))?;
    let answer = installation::project(
        application,
        channel,
        ReleaseIdentityInput {
            catalog,
            installed,
            platform: platform()?.into(),
            record,
            now,
        },
    )
    .map_err(invalid)?;
    // A record that could not be read is never overwritten blindly.
    if let (Some(store), Some(record), None) = (&store, &answer.record, &record_error) {
        if let Err(error) = store.write(record) {
            record_error = Some(error);
        }
    }
    let mut value = serde_json::to_value(&answer)
        .map_err(|e| Failure::internal("release_identity_invalid", e.to_string()))?;
    if let Some(error) = record_error {
        value["installation"]["recordError"] = json!(error.chars().take(300).collect::<String>());
    }
    Ok(value)
}

/// One intentional read through the packaged lane's release authority. A
/// missing authority or failed read is shown; the installed row stays.
fn read_catalog(application: Application, channel: &str) -> CatalogRead {
    let lane = match ds_cli_auth::Lane::parse(channel) {
        Ok(lane) => lane,
        Err(failure) => return CatalogRead::Unavailable(failure.to_string()),
    };
    let authority = match ds_cli_auth::module_updates::capture_release_authority(lane) {
        Ok(authority) => authority,
        Err(failure) => return CatalogRead::Unavailable(failure.to_string()),
    };
    match authority.read_application_release(application) {
        Ok(release) => CatalogRead::Read(release),
        Err(message) => CatalogRead::Unavailable(message),
    }
}

#[cfg(test)]
#[path = "release_identity_tests.rs"]
mod tests;
