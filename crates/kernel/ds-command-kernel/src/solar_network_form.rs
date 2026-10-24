//! One portable form operation for CLI and browser hosts.
use base64::Engine;
use ds_solar_contracts::{
    SolarError, SolarResult,
    network_form::{DecodedInformation, ResolvedInformation},
};
use serde::Deserialize;
use serde_json::Value;

const MAX_INFORMATION_ENCODED_BYTES: usize = 48 * 1024 * 1024;
const MAX_INFORMATION_BYTES: usize = 32 * 1024 * 1024;

/// Decode retained classified information at the host transport boundary.
/// Project/source admission remains with the operation consuming these cells.
pub fn decode_information(resolved: &ResolvedInformation) -> SolarResult<DecodedInformation> {
    if resolved.base64.len() > MAX_INFORMATION_ENCODED_BYTES {
        return Err(SolarError::invalid_input(
            "shared information exceeds 32 MiB",
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&resolved.base64)
        .map_err(|_| SolarError::invalid_input("invalid shared information encoding"))?;
    if bytes.len() > MAX_INFORMATION_BYTES {
        return Err(SolarError::invalid_input(
            "shared information exceeds 32 MiB",
        ));
    }
    if ds_solar_contracts::digest::sha256_hex(&bytes) != resolved.reference.digest {
        return Err(SolarError::invalid_input(
            "shared information digest mismatch",
        ));
    }
    Ok(DecodedInformation {
        reference: resolved.reference.clone(),
        table: ds_network_information::read(&bytes).map_err(SolarError::invalid_input)?,
    })
}

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Open {
        project_id: String,
        stored: Value,
    },
    Snapshot {
        project_id: String,
        document: Value,
        #[serde(default)]
        information: Option<ds_solar_contracts::network_form::ResolvedInformation>,
    },
    CreateCity {
        identity: ds_solar_contracts::CityIdentity,
        #[serde(default)]
        snapshot: Option<Value>,
    },
    Adopt {
        project_id: String,
        document: Value,
        information: ds_solar_contracts::network_form::ResolvedInformation,
    },
    Seed {
        project_id: String,
        document: Value,
        #[serde(default)]
        information: Option<ds_solar_contracts::network_form::ResolvedInformation>,
    },
    Save {
        project_id: String,
        document: Value,
        before: Value,
        edited: Value,
    },
    Empty,
    Information {
        project_id: String,
        information: ds_solar_contracts::network_form::ResolvedInformation,
    },
}
pub fn evaluate(bytes: &[u8]) -> Result<String, String> {
    use ds_solar_contracts::network_form;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("network form request exceeds 64 MiB".into());
    }
    let request: Request = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let result = match request {
        Request::Open { project_id, stored } => {
            network_form::open(&project_id, &stored).map_err(|e| e.to_string())?
        }
        Request::Snapshot {
            project_id,
            document,
            information,
        } => {
            if let Some(information) = &information {
                let form =
                    network_form::parse(&project_id, &document).map_err(|e| e.to_string())?;
                if form.sources.information.as_ref() != Some(&information.reference) {
                    return Err(SolarError::invalid_input(
                        "resolved information does not match the form's source",
                    )
                    .to_string());
                }
            }
            let information = information
                .as_ref()
                .map(decode_information)
                .transpose()
                .map_err(|e| e.to_string())?;
            network_form::snapshot(&project_id, &document, information.as_ref())
                .map_err(|e| e.to_string())?
        }
        Request::CreateCity { identity, snapshot } => serde_json::to_value(
            ds_solar_contracts::city_draft::create(identity, snapshot)
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?,
        Request::Adopt {
            project_id,
            document,
            information,
        } => {
            let form = network_form::parse(&project_id, &document).map_err(|e| e.to_string())?;
            if form.sources.information.as_ref() != Some(&information.reference) {
                return Err(SolarError::invalid_input(
                    "shared information does not match this form",
                )
                .to_string());
            }
            let information = decode_information(&information).map_err(|e| e.to_string())?;
            network_form::adopt_legacy(&project_id, &document, &information)
                .map_err(|e| e.to_string())?
        }
        Request::Information {
            project_id,
            information,
        } => {
            information
                .reference
                .validate(&project_id)
                .map_err(|e| e.to_string())?;
            let information = decode_information(&information).map_err(|e| e.to_string())?;
            network_form::information_values(&information).map_err(|e| e.to_string())?
        }
        Request::Empty => {
            serde_json::to_value(network_form::NetworkForm::default()).map_err(|e| e.to_string())?
        }
        Request::Seed {
            project_id,
            document,
            information,
        } => {
            let form = network_form::parse(&project_id, &document).map_err(|e| e.to_string())?;
            if information
                .as_ref()
                .is_some_and(|info| form.sources.information.as_ref() != Some(&info.reference))
            {
                return Err("resolved information does not match the form's source".into());
            }
            let values = information
                .as_ref()
                .map(|information| {
                    network_form::information_values(&decode_information(information)?)
                })
                .transpose()
                .map_err(|e| e.to_string())?;
            network_form::seed(&project_id, &document, values.as_ref())
                .map_err(|e| e.to_string())?
        }
        Request::Save {
            project_id,
            document,
            before,
            edited,
        } => network_form::save(&project_id, &document, &before, &edited)
            .map_err(|e| e.to_string())?,
    };
    serde_json::to_string(&result).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "solar_network_form_tests.rs"]
mod tests;
