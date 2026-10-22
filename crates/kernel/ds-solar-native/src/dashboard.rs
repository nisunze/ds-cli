//! Resolve approved, immutable native runs before shared dashboard composition.
use crate::{
    NativeHost,
    native::{portfolio_run_directory, run_directory},
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Site,
    Plant,
    Finance,
    Boq,
    PortfolioFinance,
    PortfolioSite,
    PortfolioPlant,
    PortfolioBoq,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub workspace_handle: String,
    pub root: String,
    pub run_id: String,
    pub expected_batch_id: String,
    pub expected_batch_digest: String,
    pub section: Section,
    pub city_id: Option<String>,
    pub portfolio_id: Option<String>,
    pub membership_revision: Option<String>,
    pub system: Option<String>,
}

pub fn compose(host: &NativeHost, project: &str, command: Command) -> Result<Value, String> {
    let portfolio = matches!(
        command.section,
        Section::PortfolioFinance
            | Section::PortfolioSite
            | Section::PortfolioPlant
            | Section::PortfolioBoq
    );
    if portfolio && (command.city_id.is_some() || command.system.is_some())
        || !portfolio && (command.portfolio_id.is_some() || command.membership_revision.is_some())
    {
        return Err("Native dashboard mixes city and portfolio selectors".into());
    }
    let identity = if portfolio {
        command.portfolio_id.as_deref()
    } else {
        command.city_id.as_deref()
    }
    .ok_or("Native dashboard needs its exact city or portfolio id")?;
    if command.root != format!("eds_project/{project}/eds_solar") {
        return Err("Native dashboard belongs to another project".into());
    }
    let section = match command.section {
        Section::Site => ds_command_kernel::solar_dashboard::Section::Site,
        Section::Plant => ds_command_kernel::solar_dashboard::Section::Plant,
        Section::Finance => ds_command_kernel::solar_dashboard::Section::Finance,
        Section::Boq => ds_command_kernel::solar_dashboard::Section::Boq,
        Section::PortfolioFinance => ds_command_kernel::solar_dashboard::Section::PortfolioFinance,
        Section::PortfolioSite => ds_command_kernel::solar_dashboard::Section::PortfolioSite,
        Section::PortfolioPlant => ds_command_kernel::solar_dashboard::Section::PortfolioPlant,
        Section::PortfolioBoq => ds_command_kernel::solar_dashboard::Section::PortfolioBoq,
    };
    let system = command
        .system
        .as_ref()
        .map(|s| serde_json::from_value(json!(s)))
        .transpose()
        .map_err(|_| "Unknown native dashboard scenario")?;
    let dir = if portfolio {
        portfolio_run_directory(
            host,
            &command.workspace_handle,
            &command.root,
            &command.run_id,
        )
    } else {
        run_directory(
            host,
            &command.workspace_handle,
            &command.root,
            &command.run_id,
        )
    }?;
    let mut source = if portfolio {
        ds_solar_project::dashboard::portfolio_source(
            &dir,
            project,
            &command.run_id,
            identity,
            command
                .membership_revision
                .as_deref()
                .ok_or("Native portfolio dashboard needs its exact membership revision")?,
        )
    } else {
        ds_solar_project::dashboard::source(&dir, project, &command.run_id, identity)
    }
    .map_err(|e| e.to_string())?;
    if source["batch_id"] != command.expected_batch_id
        || source["batch_digest"] != command.expected_batch_digest
    {
        return Err("Native dashboard source differs from its pinned batch".into());
    }
    let report = source
        .as_object_mut()
        .ok_or("Invalid native dashboard source")?
        .remove("report")
        .ok_or("Native dashboard report is unavailable")?;
    let dashboard =
        ds_command_kernel::solar_dashboard::compose(ds_command_kernel::solar_dashboard::Request {
            section,
            system,
            report,
        })?;
    let html =
        ds_command_kernel::solar_dashboard::html(&dashboard, &format!("{project} · {identity}"));
    let response = json!({"source":source,"dashboard":dashboard,"html":html,
        "publication":"not_requested","plots":"not_included"});
    if serde_json::to_vec(&response)
        .map_err(|_| "Cannot encode native dashboard")?
        .len()
        > 16 * 1024 * 1024
    {
        return Err("Native Solar dashboard exceeds its 16 MiB response bound; export bounded sections instead".into());
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixed_selectors_and_foreign_projects_refuse_before_workspace_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let host = NativeHost::headless(dir.path().join("native")).unwrap();
        let input = json!({"workspace_handle":"unknown","root":"eds_project/p/eds_solar","run_id":"r",
            "expected_batch_id":"none","expected_batch_digest":"none","section":"portfolio_finance","city_id":"a","portfolio_id":"pf"});
        let mixed =
            compose(&host, "p", serde_json::from_value(input.clone()).unwrap()).unwrap_err();
        assert!(mixed.contains("mixes"));
        let mut foreign = input;
        foreign["city_id"] = Value::Null;
        let refused = compose(&host, "q", serde_json::from_value(foreign).unwrap()).unwrap_err();
        assert!(refused.contains("another project"));
    }
}
