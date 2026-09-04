use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const SOLAR_PORTFOLIO_CATALOG_SCHEMA: &str = "ds-solar.portfolio-catalog/v1";
pub const SOLAR_PORTFOLIO_MUTATION_PLAN_SCHEMA: &str = "ds-solar.portfolio-mutation-plan/v1";
pub const SOLAR_PORTFOLIO_MEMBERSHIP_SCHEMA: &str = "ds-solar.portfolio-membership/v1";
pub const MAX_SOLAR_PORTFOLIOS: usize = 500;
pub const MAX_SOLAR_PORTFOLIO_CITIES: usize = 200;
pub const MAX_SOLAR_PORTFOLIO_CATALOG_BYTES: usize = 512 * 1024;
pub const MAX_SOLAR_PORTFOLIO_MUTATION_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolarPortfolioError(String);

impl fmt::Display for SolarPortfolioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SolarPortfolioError {}

fn invalid(message: impl Into<String>) -> SolarPortfolioError {
    SolarPortfolioError(message.into())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolarPortfolio {
    pub id: String,
    pub name: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub cities: Vec<String>,
    pub membership_revision: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created_by: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
struct SolarPortfolioWire {
    id: String,
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    description: String,
    cities: Vec<String>,
    membership_revision: String,
    #[serde(default)]
    created_by: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct SolarPortfolioCatalog {
    schema: &'static str,
    portfolios: Vec<SolarPortfolio>,
}

/// Validate and normalize the project portfolio catalog returned by its host.
/// Transport and caching remain host concerns; membership identity is shared.
pub fn validate_solar_portfolio_catalog(bytes: &[u8]) -> Result<String, SolarPortfolioError> {
    if bytes.is_empty() || bytes.len() > MAX_SOLAR_PORTFOLIO_CATALOG_BYTES {
        return Err(invalid(
            "the Solar portfolio catalog is outside the 512 KiB shared bound",
        ));
    }
    let rows: Vec<SolarPortfolioWire> = serde_json::from_slice(bytes)
        .map_err(|_| invalid("the Solar portfolio catalog is not a JSON array"))?;
    if rows.len() > MAX_SOLAR_PORTFOLIOS {
        return Err(invalid("the Solar portfolio catalog exceeds 500 rows"));
    }
    let mut ids = BTreeSet::new();
    let portfolios = rows
        .into_iter()
        .map(|row| {
            validate_identifier(&row.id, "portfolio id")?;
            if !ids.insert(row.id.clone()) {
                return Err(invalid(format!(
                    "the Solar portfolio catalog repeats {}",
                    row.id
                )));
            }
            validate_text(&row.name, "portfolio name", 120, false)?;
            let display_name = if row.display_name.is_empty() {
                row.name.clone()
            } else {
                validate_text(&row.display_name, "portfolio display name", 120, false)?;
                row.display_name
            };
            validate_text(&row.description, "portfolio description", 2_000, true)?;
            validate_membership(&row.cities)?;
            validate_revision(&row.membership_revision)?;
            Ok(SolarPortfolio {
                id: row.id,
                name: row.name,
                display_name,
                description: row.description,
                cities: row.cities,
                membership_revision: row.membership_revision,
                created_by: row.created_by,
                created_at: row.created_at,
                updated_at: row.updated_at,
            })
        })
        .collect::<Result<Vec<_>, SolarPortfolioError>>()?;
    serde_json::to_string(&SolarPortfolioCatalog {
        schema: SOLAR_PORTFOLIO_CATALOG_SCHEMA,
        portfolios,
    })
    .map_err(|_| invalid("the Solar portfolio catalog projection could not be encoded"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SolarPortfolioMembershipInput {
    portfolio_id: String,
    cities: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SolarPortfolioMembershipIdentity {
    schema: &'static str,
    canonical_json: String,
    fingerprint: String,
    revision: String,
}

#[derive(Debug, Serialize)]
struct CanonicalSolarPortfolioMembership<'a> {
    schema: u8,
    portfolio_id: &'a str,
    city_ids: &'a [String],
}

/// Derive the one ordered-membership identity used by native computation,
/// artifact publication, the CLI, and the browser. The canonical bytes retain
/// their historical schema/key order so existing revisions remain stable.
pub fn solar_portfolio_membership_identity(bytes: &[u8]) -> Result<String, SolarPortfolioError> {
    if bytes.is_empty() || bytes.len() > MAX_SOLAR_PORTFOLIO_MUTATION_BYTES {
        return Err(invalid(
            "the Solar portfolio membership is outside the 64 KiB shared bound",
        ));
    }
    let input: SolarPortfolioMembershipInput = serde_json::from_slice(bytes)
        .map_err(|_| invalid("the Solar portfolio membership is not valid contract JSON"))?;
    validate_identifier(&input.portfolio_id, "portfolio id")?;
    validate_membership(&input.cities)?;
    let canonical_json = serde_json::to_string(&CanonicalSolarPortfolioMembership {
        schema: 1,
        portfolio_id: &input.portfolio_id,
        city_ids: &input.cities,
    })
    .map_err(|_| invalid("the Solar portfolio membership could not be encoded"))?;
    let digest = Sha256::digest(canonical_json.as_bytes());
    let fingerprint: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    serde_json::to_string(&SolarPortfolioMembershipIdentity {
        schema: SOLAR_PORTFOLIO_MEMBERSHIP_SCHEMA,
        canonical_json,
        revision: format!("sha256:{fingerprint}"),
        fingerprint,
    })
    .map_err(|_| invalid("the Solar portfolio membership identity could not be encoded"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SolarPortfolioMutationKind {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SolarPortfolioMutationInput {
    operation: SolarPortfolioMutationKind,
    #[serde(default)]
    portfolio_id: Option<String>,
    #[serde(default)]
    expected_membership_revision: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    cities: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct SolarPortfolioMutationPlan {
    schema: &'static str,
    operation: &'static str,
    action: &'static str,
    payload: Map<String, Value>,
}

/// Produce the exact backend action payload for one create/update/delete.
/// Update and delete are always fenced by the membership revision observed by
/// the caller; a stale page or CLI invocation can never silently overwrite.
pub fn plan_solar_portfolio_mutation(bytes: &[u8]) -> Result<String, SolarPortfolioError> {
    if bytes.is_empty() || bytes.len() > MAX_SOLAR_PORTFOLIO_MUTATION_BYTES {
        return Err(invalid(
            "the Solar portfolio mutation is outside the 64 KiB shared bound",
        ));
    }
    let input: SolarPortfolioMutationInput = serde_json::from_slice(bytes)
        .map_err(|_| invalid("the Solar portfolio mutation is not valid contract JSON"))?;
    let mut payload = Map::new();
    let (operation, action) = match input.operation {
        SolarPortfolioMutationKind::Create => {
            reject_present(&input.portfolio_id, "portfolio_id", "create")?;
            reject_present(
                &input.expected_membership_revision,
                "expected_membership_revision",
                "create",
            )?;
            reject_present(&input.display_name, "display_name", "create")?;
            let name = input
                .name
                .as_deref()
                .ok_or_else(|| invalid("portfolio create requires name"))?;
            validate_text(name, "portfolio name", 120, false)?;
            let cities = input
                .cities
                .as_ref()
                .ok_or_else(|| invalid("portfolio create requires cities"))?;
            validate_membership(cities)?;
            let description = input.description.as_deref().unwrap_or_default();
            validate_text(description, "portfolio description", 2_000, true)?;
            payload.insert("name".into(), json!(name));
            payload.insert("portfolio_description".into(), json!(description));
            payload.insert("cities".into(), json!(cities));
            ("create", "create_portfolio")
        }
        SolarPortfolioMutationKind::Update => {
            reject_present(&input.name, "name", "update")?;
            let id = required_identifier(input.portfolio_id.as_deref())?;
            let revision = required_revision(input.expected_membership_revision.as_deref())?;
            if input.display_name.is_none() && input.description.is_none() && input.cities.is_none()
            {
                return Err(invalid(
                    "portfolio update requires display_name, description, or cities",
                ));
            }
            payload.insert("portfolio_id".into(), json!(id));
            payload.insert("expected_membership_revision".into(), json!(revision));
            if let Some(display_name) = input.display_name.as_deref() {
                validate_text(display_name, "portfolio display name", 120, false)?;
                payload.insert("portfolio_display_name".into(), json!(display_name));
            }
            if let Some(description) = input.description.as_deref() {
                validate_text(description, "portfolio description", 2_000, true)?;
                payload.insert("portfolio_description".into(), json!(description));
            }
            if let Some(cities) = input.cities.as_ref() {
                validate_membership(cities)?;
                payload.insert("cities".into(), json!(cities));
            }
            ("update", "update_portfolio")
        }
        SolarPortfolioMutationKind::Delete => {
            reject_present(&input.name, "name", "delete")?;
            reject_present(&input.display_name, "display_name", "delete")?;
            reject_present(&input.description, "description", "delete")?;
            if input.cities.is_some() {
                return Err(invalid("cities is not accepted for portfolio delete"));
            }
            let id = required_identifier(input.portfolio_id.as_deref())?;
            let revision = required_revision(input.expected_membership_revision.as_deref())?;
            payload.insert("portfolio_id".into(), json!(id));
            payload.insert("expected_membership_revision".into(), json!(revision));
            ("delete", "delete_portfolio")
        }
    };
    serde_json::to_string(&SolarPortfolioMutationPlan {
        schema: SOLAR_PORTFOLIO_MUTATION_PLAN_SCHEMA,
        operation,
        action,
        payload,
    })
    .map_err(|_| invalid("the Solar portfolio mutation plan could not be encoded"))
}

fn required_identifier(value: Option<&str>) -> Result<&str, SolarPortfolioError> {
    let value = value.ok_or_else(|| invalid("portfolio mutation requires portfolio_id"))?;
    validate_identifier(value, "portfolio id")?;
    Ok(value)
}

fn required_revision(value: Option<&str>) -> Result<&str, SolarPortfolioError> {
    let value =
        value.ok_or_else(|| invalid("portfolio mutation requires expected_membership_revision"))?;
    validate_revision(value)?;
    Ok(value)
}

fn reject_present<T>(
    value: &Option<T>,
    field: &str,
    operation: &str,
) -> Result<(), SolarPortfolioError> {
    if value.is_some() {
        return Err(invalid(format!(
            "{field} is not accepted for portfolio {operation}"
        )));
    }
    Ok(())
}

fn validate_membership(cities: &[String]) -> Result<(), SolarPortfolioError> {
    if !(2..=MAX_SOLAR_PORTFOLIO_CITIES).contains(&cities.len()) {
        return Err(invalid("portfolio membership must contain 2..200 cities"));
    }
    let mut seen = BTreeSet::new();
    for city in cities {
        validate_identifier(city, "portfolio city id")?;
        if !seen.insert(city) {
            return Err(invalid(format!("portfolio membership repeats city {city}")));
        }
    }
    Ok(())
}

fn validate_identifier(value: &str, label: &str) -> Result<(), SolarPortfolioError> {
    let mut chars = value.chars();
    let valid = value.len() <= 128
        && chars
            .next()
            .is_some_and(|first| first.is_ascii_alphanumeric())
        && chars
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
    if !valid {
        return Err(invalid(format!("{label} is outside the closed id grammar")));
    }
    Ok(())
}

fn validate_revision(value: &str) -> Result<(), SolarPortfolioError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid(
            "expected_membership_revision must be a lowercase sha256 digest",
        ));
    }
    Ok(())
}

fn validate_text(
    value: &str,
    label: &str,
    max_chars: usize,
    allow_empty: bool,
) -> Result<(), SolarPortfolioError> {
    if value.trim() != value
        || (!allow_empty && value.is_empty())
        || value.chars().count() > max_chars
        || value.chars().any(char::is_control)
    {
        return Err(invalid(format!("{label} is outside its shared bound")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn revision(character: char) -> String {
        format!("sha256:{}", character.to_string().repeat(64))
    }

    #[test]
    fn catalog_normalizes_display_names_and_preserves_ordered_membership() {
        let value = json!([{
            "id": "portfolio_1",
            "name": "Northern portfolio",
            "display_name": "",
            "cities": ["city_b", "city_a"],
            "membership_revision": revision('a')
        }]);
        let result: Value = serde_json::from_str(
            &validate_solar_portfolio_catalog(&serde_json::to_vec(&value).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(result["schema"], SOLAR_PORTFOLIO_CATALOG_SCHEMA);
        assert_eq!(
            result["portfolios"][0]["display_name"],
            "Northern portfolio"
        );
        assert_eq!(
            result["portfolios"][0]["cities"],
            json!(["city_b", "city_a"])
        );
    }

    #[test]
    fn update_plan_is_revision_pinned_and_can_clear_description() {
        let value = json!({
            "operation": "update",
            "portfolio_id": "portfolio_1",
            "expected_membership_revision": revision('b'),
            "description": "",
            "cities": ["city_a", "city_c"]
        });
        let result: Value = serde_json::from_str(
            &plan_solar_portfolio_mutation(&serde_json::to_vec(&value).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(result["action"], "update_portfolio");
        assert_eq!(result["payload"]["portfolio_description"], "");
        assert_eq!(
            result["payload"]["expected_membership_revision"],
            revision('b')
        );
    }

    #[test]
    fn mutations_refuse_duplicate_membership_and_unfenced_writes() {
        let duplicate = json!({
            "operation": "create",
            "name": "Portfolio",
            "cities": ["city_a", "city_a"]
        });
        assert!(plan_solar_portfolio_mutation(&serde_json::to_vec(&duplicate).unwrap()).is_err());

        let stale = json!({
            "operation": "delete",
            "portfolio_id": "portfolio_1"
        });
        assert!(plan_solar_portfolio_mutation(&serde_json::to_vec(&stale).unwrap()).is_err());
    }

    #[test]
    fn membership_identity_preserves_the_deployed_canonical_bytes() {
        let value = json!({
            "portfolio_id": "portfolio_1",
            "cities": ["city_b", "city_a"]
        });
        let result: Value = serde_json::from_str(
            &solar_portfolio_membership_identity(&serde_json::to_vec(&value).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(result["schema"], SOLAR_PORTFOLIO_MEMBERSHIP_SCHEMA);
        assert_eq!(
            result["canonical_json"],
            r#"{"schema":1,"portfolio_id":"portfolio_1","city_ids":["city_b","city_a"]}"#
        );
        assert_eq!(result["fingerprint"].as_str().unwrap().len(), 64);
        assert_eq!(
            result["revision"],
            format!("sha256:{}", result["fingerprint"].as_str().unwrap())
        );

        let parity = json!({
            "portfolio_id": "pf_north",
            "cities": ["rw_kigali", "rw_musanze"]
        });
        let parity: Value = serde_json::from_str(
            &solar_portfolio_membership_identity(&serde_json::to_vec(&parity).unwrap()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            parity["fingerprint"],
            "a28725fa6d460cf2ed3fdda79a2fd9a712df29f8421b2609bdf74a716e59d92c"
        );
    }
}
