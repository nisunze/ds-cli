//! Explicit-project portfolio authority; all calculation stays in the Solar owner.
use crate::{
    authorized::Authority, solar_project::call, AuthorizedTransport, ClientError, ClientProfile,
    ErrorKind,
};
use serde_json::{json, Value};

pub enum Command {
    List,
    Get { portfolio: String },
    Analysis { portfolio: String },
    PublishedRead { portfolio: String, run_id: String },
    Mutate { intent: Value },
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "Solar portfolio identity, membership or artifact is invalid or has moved",
    )
}
fn plan(intent: &Value) -> Result<Value, ClientError> {
    serde_json::from_str(
        &ds_command_kernel::plan_solar_portfolio_mutation(
            &serde_json::to_vec(intent).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())
}
fn membership(id: &str, cities: &Value) -> Result<Value, ClientError> {
    serde_json::from_str(
        &ds_command_kernel::solar_portfolio_membership_identity(
            &serde_json::to_vec(&json!({"portfolio_id":id,"cities":cities}))
                .map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())
}
fn catalog(rows: Value) -> Result<Value, ClientError> {
    let value: Value = serde_json::from_str(
        &ds_command_kernel::validate_solar_portfolio_catalog(
            &serde_json::to_vec(&rows).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())?;
    for row in value["portfolios"].as_array().ok_or_else(invalid)? {
        if membership(row["id"].as_str().ok_or_else(invalid)?, &row["cities"])?["revision"]
            != row["membership_revision"]
        {
            return Err(invalid());
        }
    }
    Ok(value)
}
fn result(
    bytes: &[u8],
    project: &str,
) -> Result<ds_solar_contracts::SolarPortfolioResult, ClientError> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid());
    }
    let result: ds_solar_contracts::SolarPortfolioResult =
        serde_json::from_slice(bytes).map_err(|_| invalid())?;
    result.validate().map_err(|_| invalid())?;
    if result.project_id != project
        || result.root != format!("eds_project/{project}/eds_solar")
        || result.engine.build_manifest_sha256.len() != 64
    {
        return Err(invalid());
    }
    let cities = json!(result.cities.iter().map(|c| &c.city_id).collect::<Vec<_>>());
    if membership(&result.portfolio_id, &cities)?["revision"] != result.membership_revision {
        return Err(invalid());
    }
    Ok(result)
}
impl Command {
    pub fn validate(&self, _project: &str) -> Result<(), ClientError> {
        match self {
            Self::List => {}
            Self::Get { portfolio } | Self::Analysis { portfolio } => {
                // Use the shared identifier contract without inventing a second grammar.
                membership(portfolio, &json!(["validation_a", "validation_b"]))?;
            }
            Self::PublishedRead { portfolio, run_id } => {
                membership(portfolio, &json!(["validation_a", "validation_b"]))?;
                if !ds_command_kernel::execution_context::valid_project(run_id) {
                    return Err(invalid());
                }
            }
            Self::Mutate { intent } => {
                plan(intent)?;
            }
        }
        Ok(())
    }
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate(project)?;
    let root = format!("eds_project/{project}/eds_solar");
    match command {
        Command::List => {
            let mut value = catalog(call(
                profile,
                transport,
                authority,
                false,
                json!({"action":"list_portfolios","root":root}),
            )?)?;
            value["project_id"] = json!(project);
            Ok(value)
        }
        Command::Get { portfolio } => {
            let row = call(
                profile,
                transport,
                authority,
                false,
                json!({"action":"get_portfolio","root":root,"portfolio_id":portfolio}),
            )?;
            if row["id"] != *portfolio {
                return Err(invalid());
            }
            Ok(catalog(json!([row]))?["portfolios"][0].clone())
        }
        Command::Analysis { portfolio } | Command::PublishedRead { portfolio, .. } => {
            let get_command = Command::Get {
                portfolio: portfolio.clone(),
            };
            let before = run(profile, transport, authority, project, &get_command)?;
            let published = crate::solar_artifact::portfolio(
                profile,
                transport,
                authority,
                project,
                portfolio,
                &before["membership_revision"],
            )?;
            let (data, publication) = if let Some((bytes, publication)) = published {
                let validated = result(&bytes, project)?;
                if validated.portfolio_id != *portfolio
                    || validated.membership_revision != before["membership_revision"]
                    || publication["engine_version"]
                        != format!("{}@{}", validated.engine.name, validated.engine.version)
                    || publication["engine_build_manifest_sha256"]
                        != validated.engine.build_manifest_sha256
                {
                    return Err(invalid());
                }
                (
                    serde_json::from_slice(&bytes).map_err(|_| invalid())?,
                    Some(publication),
                )
            } else {
                // Absence of the current contract is an explicit not-yet state.
                // Historical Firestore projections cannot substitute for its head.
                (Value::Null, None)
            };
            let after = run(profile, transport, authority, project, &get_command)?;
            if before["membership_revision"] != after["membership_revision"]
                || before["cities"] != after["cities"]
            {
                return Err(invalid());
            }
            if let Command::PublishedRead { run_id, .. } = command {
                if data.is_null() {
                    return Err(ClientError::new(
                        ErrorKind::InvalidInput,
                        "No current published Solar portfolio result exists for this membership",
                    ));
                }
                let validated = ds_solar_contracts::validate_portfolio_result_read(data, run_id)
                    .map_err(|_| invalid())?;
                return Ok(json!({
                    "project_id":project,"document":validated.document,
                    "trace":validated.trace,"publication":publication
                }));
            }
            let request = json!({
                "binding":{"portfolioId":portfolio,"portfolioName":before["display_name"],
                    "membershipRevision":before["membership_revision"],"cityIds":before["cities"]},
                "outcome":{"data":data},"require_binding":true
            });
            let mut receipt: Value = serde_json::from_str(
                &ds_command_kernel::solar_saved_analysis::evaluate(
                    &serde_json::to_vec(&request).map_err(|_| invalid())?,
                )
                .map_err(|_| invalid())?,
            )
            .map_err(|_| invalid())?;
            receipt["project_id"] = json!(project);
            if let Some(publication) = publication {
                receipt["publication"] = publication;
            }
            Ok(receipt)
        }
        Command::Mutate { intent } => {
            let plan = plan(intent)?;
            let mut payload = plan["payload"].clone();
            payload["root"] = json!(root);
            payload["action"] = plan["action"].clone();
            let row = call(profile, transport, authority, false, payload)?;
            let verify = |catalog: Option<Value>| -> Result<Value, ClientError> {
                let mut request = json!({"intent":intent,"response":row});
                if let Some(catalog) = catalog {
                    request["catalog"] = catalog;
                }
                serde_json::from_str(
                    &ds_command_kernel::validate_solar_portfolio_mutation_receipt(
                        &serde_json::to_vec(&request).map_err(|_| invalid())?,
                    )
                    .map_err(|_| invalid())?,
                )
                .map_err(|_| invalid())
            };
            verify(None)?;
            let rows = call(
                profile,
                transport,
                authority,
                false,
                json!({"action":"list_portfolios","root":root}),
            )?;
            let mut receipt = verify(Some(rows))?;
            receipt["project_id"] = json!(project);
            Ok(receipt)
        }
    }
}
