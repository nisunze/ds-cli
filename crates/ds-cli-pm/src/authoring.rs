//! Explicit, voluntary per-request PM authoring labels. No ambient state.
use ds_cli_contract::{Failure, Inputs, spec::Arg};
use ds_client_core::{project_correspondence::Action, project_management::Command};
pub use ds_command_kernel::project_management::authoring_model::AuthoringModel;
use serde_json::{Value, json};
pub const MODEL: Arg = Arg::value(
    "authoring-model",
    "<known-label>",
    "Optional known model label (at most 200 UTF-8 bytes); omit unknown.",
);
pub const VERSION: Arg = Arg::value(
    "authoring-version",
    "<known-label>",
    "Optional known model version (at most 200 bytes); omit unknown.",
);
pub const STRENGTH: Arg = Arg::value(
    "authoring-strength",
    "<known-label>",
    "Optional known model strength (at most 200 bytes); omit unknown.",
);
#[macro_export]
macro_rules! authored_args {($($arg:expr),* $(,)?)=>{&[$($arg,)*$crate::authoring::MODEL,$crate::authoring::VERSION,$crate::authoring::STRENGTH]};}
fn invalid(e: impl std::fmt::Display) -> Failure {
    Failure::invalid(crate::PM_REFUSED.code,e.to_string()).detail(json!({"source":"native_authoring_validation"})).remedy("Declare only known trimmed model/version/strength labels within 200 bytes; leave unknown fields absent")
}
pub fn declaration(i: &Inputs) -> Result<Option<AuthoringModel>, Failure> {
    let d = AuthoringModel {
        model: i.value("authoring-model").map(str::to_owned),
        version: i.value("authoring-version").map(str::to_owned),
        strength: i.value("authoring-strength").map(str::to_owned),
    };
    if d == AuthoringModel::default() {
        return Ok(None);
    }
    d.validate().map_err(invalid)?;
    Ok(Some(d))
}
pub fn draft(
    i: &Inputs,
    draft: &mut ds_command_kernel::project_management::project_notes::Draft,
) -> Result<(), Failure> {
    if let Some(model) = declaration(i)? {
        if draft.authoring_model.as_ref().is_some_and(|d| d != &model) {
            return Err(invalid(
                "Explicit authoring flags differ from draft.authoring_model",
            ));
        }
        draft.authoring_model = Some(model);
    }
    Ok(())
}
pub fn project(
    i: &Inputs,
    lane: &str,
    project: &str,
    command: Command,
) -> Result<ds_cli_auth::HeadlessNamedProject<Value>, Failure> {
    let command = command.with_authoring(declaration(i)?).map_err(invalid)?;
    ds_cli_auth::project_management_for_project(lane, project, &command)
}
pub fn correspondence(
    i: &Inputs,
    lane: &str,
    project: &str,
    action: Action,
) -> Result<ds_cli_auth::HeadlessNamedProject<Value>, Failure> {
    let action = action.with_authoring(declaration(i)?).map_err(invalid)?;
    crate::correspondence(lane, project, &action)
}
pub fn commit(
    i: &Inputs,
    lane: &str,
    project: &str,
    prepared: &ds_command_kernel::project_management::writes::PreparedCommand,
) -> Result<ds_command_kernel::project_management::writes::OperationResult, Failure> {
    let command = Command::Commit {
        command_id: crate::command_id()?,
        base_revision: prepared.base_revision,
        command: prepared.command.clone(),
    };
    let report = self::project(i, lane, project, command)?;
    crate::accepted(
        ds_command_kernel::project_management::writes::decode_operation_result(
            &report.into_result(),
        ),
    )
}
pub fn commit_batch(
    i: &Inputs,
    lane: &str,
    project: &str,
    prepared: &ds_command_kernel::project_management::writes::PreparedBatch,
) -> Result<ds_command_kernel::project_management::writes::OperationResult, Failure> {
    let command = Command::CommitBatch {
        command_id: crate::command_id()?,
        base_revision: prepared.base_revision,
        commands: prepared.commands.clone(),
    };
    let report = self::project(i, lane, project, command)?;
    crate::accepted(
        ds_command_kernel::project_management::writes::decode_operation_result(
            &report.into_result(),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parse(c: &ds_cli_contract::spec::Command, args: &[&str]) -> Inputs {
        ds_cli_contract::args::parse(c, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            .unwrap()
    }
    #[test]
    fn authoring_descriptors_are_only_on_supported_writes() {
        let writes = [
            &crate::note::CREATE,
            &crate::note_memory::UPDATE,
            &crate::note_memory::BACKUP,
            &crate::note_memory::RETIRE,
            &crate::note_memory::RESTORE,
            &crate::note_memory::CONSOLIDATE,
            &crate::task::create::COMMAND,
            &crate::task::update::COMMAND,
            &crate::task::assign::COMMAND,
            &crate::task::respond::COMMAND,
            &crate::record::create::COMMAND,
            &crate::record::reply::COMMAND,
            &crate::record::update::COMMAND,
        ];
        for c in writes {
            for flag in ["authoring-model", "authoring-version", "authoring-strength"] {
                assert!(
                    c.args.iter().any(|a| a.name == flag),
                    "{} missing {}",
                    c.id,
                    flag
                );
            }
        }
        for c in [
            &crate::note_memory::READ,
            &crate::note_memory::QUERY,
            &crate::note_memory::CONTEXT,
            &crate::note_memory::VERSIONS,
        ] {
            assert!(!c.args.iter().any(|a| a.name.starts_with("authoring-")));
            assert!(
                ds_cli_contract::args::parse(
                    c,
                    &[
                        "--project".into(),
                        "p".into(),
                        "--authoring-model".into(),
                        "GPT-6".into()
                    ]
                )
                .is_err()
            );
        }
    }
    #[test]
    fn explicit_invalid_labels_refuse_before_any_transport() {
        let plain = parse(&crate::note::CREATE, &["--project", "p"]);
        assert_eq!(declaration(&plain).unwrap(), None);
        for label in ["", " ", "GPT-6 ", "GPT\n6"] {
            let i = parse(
                &crate::note::CREATE,
                &["--project", "p", "--authoring-model", label],
            );
            assert!(declaration(&i).is_err());
        }
        let i = parse(
            &crate::note::CREATE,
            &["--project", "p", "--authoring-model", "GPT-6"],
        );
        let model = declaration(&i).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(model).unwrap(),
            json!({"model":"GPT-6"})
        );
        for extra in ["source", "verified", "kind", "signature"] {
            let mut value = json!({"model":"GPT-6"});
            value[extra] = json!("self_declared");
            assert!(serde_json::from_value::<AuthoringModel>(value).is_err());
        }
    }
}
