//! Explicit-project portfolio authority; all calculation stays in the Solar owner.
use crate::{
    authorized::Authority,
    solar_project::{call, publish, Output},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
};
use serde_json::{json, Value};

pub enum Command {
    List,
    Get { portfolio: String },
    Mutate { intent: Value },
    Publish { result: Vec<u8> },
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
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        match self {
            Self::List => {}
            Self::Get { portfolio } => {
                // Use the shared identifier contract without inventing a second grammar.
                membership(portfolio, &json!(["validation_a", "validation_b"]))?;
            }
            Self::Mutate { intent } => {
                plan(intent)?;
            }
            Self::Publish { result: bytes } => {
                result(bytes, project)?;
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
        Command::Mutate { intent } => {
            let plan = plan(intent)?;
            let mut payload = plan["payload"].clone();
            payload["root"] = json!(root);
            payload["action"] = plan["action"].clone();
            let row = call(profile, transport, authority, false, payload)?;
            let id = if plan["operation"] == "delete" {
                if row["deleted"] != true
                    || row["portfolio_id"] != intent["portfolio_id"]
                    || row["membership_revision"] != intent["expected_membership_revision"]
                {
                    return Err(invalid());
                }
                row["portfolio_id"].clone()
            } else {
                let row = catalog(json!([row.clone()]))?["portfolios"][0].clone();
                if (plan["operation"] == "update" && row["id"] != intent["portfolio_id"])
                    || (intent.get("cities").is_some() && row["cities"] != intent["cities"])
                {
                    return Err(invalid());
                }
                row["id"].clone()
            };
            Ok(
                json!({"project_id":project,"operation":plan["operation"],"portfolio_id":id,"portfolio":row}),
            )
        }
        Command::Publish { result: bytes } => {
            let result = result(bytes, project)?;
            let current = run(
                profile,
                transport,
                authority,
                project,
                &Command::Get {
                    portfolio: result.portfolio_id.clone(),
                },
            )?;
            if current["membership_revision"] != result.membership_revision {
                return Err(invalid());
            }
            let output = Output {
                id: "portfolio-result".into(),
                format: "json".into(),
                content_type: "application/json".into(),
                bytes: bytes.clone(),
            };
            let base = result
                .membership_revision
                .strip_prefix("sha256:")
                .ok_or_else(invalid)?;
            let receipt = publish(
                profile,
                transport,
                authority,
                project,
                &result.run_id,
                &format!("{}@{}", result.engine.name, result.engine.version),
                &result.engine.build_manifest_sha256,
                &format!("portfolio-{}", result.portfolio_id),
                "aggregate-v2",
                base,
                None,
                &[&output],
            )?;
            Ok(
                json!({"project_id":project,"portfolio_id":result.portfolio_id,"membership_revision":result.membership_revision,"publication":receipt}),
            )
        }
    }
}
