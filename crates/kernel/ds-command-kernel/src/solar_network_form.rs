//! One portable form operation for CLI and browser hosts.
use serde::Deserialize;
use serde_json::Value;

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
        } => network_form::snapshot(&project_id, &document, information.as_ref())
            .map_err(|e| e.to_string())?,
        Request::CreateCity { identity, snapshot } => serde_json::to_value(
            ds_solar_contracts::city_draft::create(identity, snapshot)
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?,
        Request::Adopt {
            project_id,
            document,
            information,
        } => network_form::adopt_legacy(&project_id, &document, &information)
            .map_err(|e| e.to_string())?,
        Request::Information {
            project_id,
            information,
        } => {
            information
                .reference
                .validate(&project_id)
                .map_err(|e| e.to_string())?;
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
                .map(network_form::information_values)
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
