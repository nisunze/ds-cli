//! Explicit-project portfolio authority; all calculation stays in the Solar owner.
use crate::{
    authorized::Authority,
    solar_project::{call, publish, Output},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
};
use serde_json::{json, Value};

pub enum Command {
    List,
    Get {
        portfolio: String,
    },
    Analysis {
        portfolio: String,
    },
    Mutate {
        intent: Value,
    },
    Publish {
        result: Vec<u8>,
        outputs: Vec<Output>,
    },
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
            Self::Get { portfolio } | Self::Analysis { portfolio } => {
                // Use the shared identifier contract without inventing a second grammar.
                membership(portfolio, &json!(["validation_a", "validation_b"]))?;
            }
            Self::Mutate { intent } => {
                plan(intent)?;
            }
            Self::Publish {
                result: bytes,
                outputs,
            } => {
                result(bytes, project)?;
                validate_outputs(bytes, outputs)?;
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
        Command::Analysis { portfolio } => {
            let command = Command::Get {
                portfolio: portfolio.clone(),
            };
            let before = run(profile, transport, authority, project, &command)?;
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
            let after = run(profile, transport, authority, project, &command)?;
            if before["membership_revision"] != after["membership_revision"]
                || before["cities"] != after["cities"]
            {
                return Err(invalid());
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
        Command::Publish {
            result: bytes,
            outputs,
        } => {
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
            let mut selected: Vec<_> = std::iter::once(&output).chain(outputs).collect();
            selected.sort_by(|a, b| a.id.cmp(&b.id));
            let publication_run_id = publication_run_id(&result.run_id, &selected)?;
            let receipt = publish(
                profile,
                transport,
                authority,
                project,
                &publication_run_id,
                &format!("{}@{}", result.engine.name, result.engine.version),
                &result.engine.build_manifest_sha256,
                &format!("portfolio-{}", result.portfolio_id),
                "aggregate-v3",
                base,
                None,
                &selected,
            )?;
            let head = call(
                profile,
                transport,
                authority,
                true,
                json!({"action":"read","project_id":project,"engine":"solar","operation":format!("portfolio-{}", result.portfolio_id),"variant":"aggregate-v3"}),
            )?;
            if head["engine_version"] != format!("{}@{}", result.engine.name, result.engine.version)
                || head["engine_build_manifest_sha256"] != result.engine.build_manifest_sha256
            {
                return Err(invalid());
            }
            verify_published_head(&head, &receipt, &result.portfolio_id, base, &selected)?;
            let after = run(
                profile,
                transport,
                authority,
                project,
                &Command::Get {
                    portfolio: result.portfolio_id.clone(),
                },
            )?;
            if after["membership_revision"] != current["membership_revision"]
                || after["cities"] != current["cities"]
            {
                return Err(invalid());
            }
            Ok(
                json!({"project_id":project,"portfolio_id":result.portfolio_id,"membership_revision":result.membership_revision,"source_run_id":result.run_id,"publication_run_id":publication_run_id,"output_count":selected.len(),"head_revision":head["head_revision"],"publication":receipt}),
            )
        }
    }
}

fn validate_outputs(result: &[u8], outputs: &[Output]) -> Result<(), ClientError> {
    let mut ids = std::collections::BTreeSet::from(["portfolio-result"]);
    let mut total = result.len();
    if outputs.len() > 99 {
        return Err(invalid());
    }
    for output in outputs {
        total = total.checked_add(output.bytes.len()).ok_or_else(invalid)?;
        if !ids.insert(output.id.as_str())
            || output.bytes.is_empty()
            || output.bytes.len() > 16 * 1024 * 1024
            || total > 128 * 1024 * 1024
            || !ds_solar_contracts::portfolio_publication::output_metadata(
                &output.id,
                &output.format,
                &output.content_type,
            )
        {
            return Err(invalid());
        }
        if output.format == "md" && std::str::from_utf8(&output.bytes).is_err() {
            return Err(invalid());
        }
        if output.format == "png" && !output.bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(invalid());
        }
    }
    Ok(())
}

fn publication_run_id(source: &str, outputs: &[&Output]) -> Result<String, ClientError> {
    use sha2::{Digest, Sha256};
    let declarations: Vec<_> = outputs.iter().map(|output| json!({"id":output.id,"format":output.format,
        "content_type":output.content_type,"sha256":format!("{:x}", Sha256::digest(&output.bytes)),"size_bytes":output.bytes.len()})).collect();
    let identity = serde_json::to_vec(&json!({"source_run_id":source,"outputs":declarations}))
        .map_err(|_| invalid())?;
    Ok(format!(
        "portfolio-publication-{:x}",
        Sha256::digest(identity)
    ))
}

fn verify_published_head(
    head: &Value,
    receipt: &Value,
    portfolio: &str,
    base: &str,
    outputs: &[&Output],
) -> Result<(), ClientError> {
    use sha2::{Digest, Sha256};
    if head["work_id"] != receipt["work_id"]
        || head["engine"] != "solar"
        || head["operation"] != format!("portfolio-{portfolio}")
        || head["variant"] != "aggregate-v3"
        || head["input_base_fingerprint"] != base
        || !head["head_revision"].as_u64().is_some_and(|n| n > 0)
    {
        return Err(invalid());
    }
    let declarations = head["outputs"]
        .as_array()
        .filter(|v| v.len() == outputs.len())
        .ok_or_else(invalid)?;
    let mut seen = std::collections::BTreeSet::new();
    for declared in declarations {
        let id = declared["output_id"].as_str().ok_or_else(invalid)?;
        if !seen.insert(id) {
            return Err(invalid());
        }
        let output = outputs
            .iter()
            .find(|output| output.id == id)
            .ok_or_else(invalid)?;
        if declared["format"] != output.format
            || declared["content_type"] != output.content_type
            || declared["size_bytes"].as_u64() != Some(output.bytes.len() as u64)
            || declared["sha256"] != format!("{:x}", Sha256::digest(&output.bytes))
        {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_identity_changes_with_the_bundle_and_repeats_exactly() {
        let result = Output {
            id: "portfolio-result".into(),
            format: "json".into(),
            content_type: "application/json".into(),
            bytes: b"sealed-result".to_vec(),
        };
        let mut chart = Output {
            id: "plot-p7_cashflow".into(),
            format: "png".into(),
            content_type: "image/png".into(),
            bytes: b"\x89PNG\r\n\x1a\nfixture".to_vec(),
        };
        let calculation = publication_run_id("run", &[&result]).unwrap();
        let complete = publication_run_id("run", &[&chart, &result]).unwrap();
        assert_ne!(calculation, complete);
        assert_eq!(
            complete,
            publication_run_id("run", &[&chart, &result]).unwrap()
        );
        assert!(validate_outputs(b"result", &[chart]).is_ok());
        chart = Output {
            id: "plot-../map".into(),
            format: "png".into(),
            content_type: "image/png".into(),
            bytes: b"\x89PNG\r\n\x1a\nfixture".to_vec(),
        };
        assert!(validate_outputs(b"result", &[chart]).is_err());
        assert_ne!(
            complete,
            publication_run_id("another-run", &[&result]).unwrap()
        );
    }
    #[test]
    fn a_result_only_or_moved_head_cannot_claim_complete_bundle_delivery() {
        use sha2::{Digest, Sha256};
        let result = Output {
            id: "portfolio-result".into(),
            format: "json".into(),
            content_type: "application/json".into(),
            bytes: b"sealed-result".to_vec(),
        };
        let chart = Output {
            id: "plot-p7_cashflow".into(),
            format: "png".into(),
            content_type: "image/png".into(),
            bytes: b"\x89PNG\r\n\x1a\nfixture".to_vec(),
        };
        let outputs = [&chart, &result];
        let declarations: Vec<_> = outputs.iter().map(|o| json!({"output_id":o.id,"format":o.format,"content_type":o.content_type,"sha256":format!("{:x}",Sha256::digest(&o.bytes)),"size_bytes":o.bytes.len()})).collect();
        let receipt = json!({"work_id":"work-1"});
        let head = json!({"work_id":"work-1","engine":"solar","operation":"portfolio-pf_1","variant":"aggregate-v3","input_base_fingerprint":"base","head_revision":2,"outputs":declarations});
        assert!(verify_published_head(&head, &receipt, "pf_1", "base", &outputs).is_ok());
        let mut old = head.clone();
        old["outputs"] = json!([head["outputs"][1]]);
        assert!(verify_published_head(&old, &receipt, "pf_1", "base", &outputs).is_err());
        let mut moved = head.clone();
        moved["work_id"] = json!("another-work");
        assert!(verify_published_head(&moved, &receipt, "pf_1", "base", &outputs).is_err());
        let mut corrupt = head;
        corrupt["outputs"][0]["sha256"] = json!("a".repeat(64));
        assert!(verify_published_head(&corrupt, &receipt, "pf_1", "base", &outputs).is_err());
    }
}
