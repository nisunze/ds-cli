//! The national reference catalogue as ds-brain publishes it, validated the way
//! the desktop validated it (`reference-data/resources.ts` `resourceValid`),
//! and the facts about this machine the kernel's `printing::context::catalog`
//! decides on.

use ds_command_kernel::printing::context::CatalogResource;
use ds_command_kernel::reference_cache::Identity;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;

use crate::bundle::BundleReceipt;
use crate::Failure;

/// One row of `list_datasets`. Unknown fields are ignored: the catalogue is
/// ds-brain's, and a new column must not make every held catalogue unreadable.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ReferenceResource {
    pub id: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub layer: String,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_modified_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_features: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_built_at: Option<u64>,
    /// The catalogue's own reason the dataset cannot be used, when it cannot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    /// `bundle` or `cloud` (`docs/contracts/foundation-datasets.md` R1). A
    /// catalogue published before residency existed reads as `bundle`.
    #[serde(default)]
    pub residency: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub residency_reason: Option<String>,
    /// Retired ids and layer names this row answers for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// The public bundle host every download is pinned to.
pub const BUNDLE_HOST: &str = "https://storage.googleapis.com/";

fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn positive(value: Option<u64>) -> bool {
    value.is_some_and(|value| value > 0)
}

impl ReferenceResource {
    /// Never held on this machine: BigQuery is its only holding.
    pub fn is_cloud_resident(&self) -> bool {
        self.residency == "cloud"
    }

    /// Does `needle` name this row — by id, by layer, or by a retired alias?
    pub fn answers_to(&self, needle: &str) -> bool {
        self.id == needle
            || self.layer == needle
            || self.aliases.iter().any(|alias| alias == needle)
    }

    /// A bundle-resident row the catalogue has not published a bundle for.
    pub fn is_unpublished(&self) -> bool {
        !self.is_cloud_resident() && !self.is_unavailable() && self.bundle().is_none()
    }

    pub fn is_unavailable(&self) -> bool {
        self.unavailable
            .as_deref()
            .is_some_and(|text| !text.is_empty())
    }

    /// Either no bundle field at all, or a complete verified bundle receipt on
    /// the pinned host — never a partial one.
    fn bundle_valid(&self) -> bool {
        let fields_absent = self.download_url.is_none()
            && self.bundle_sha256.is_none()
            && self.download_bytes.is_none()
            && self.expanded_bytes.is_none()
            && self.expanded_sha256.is_none()
            && self.bundle_features.is_none()
            && self.bundle_built_at.is_none();
        if fields_absent {
            return true;
        }
        let Some(url) = self.download_url.as_deref() else {
            return false;
        };
        url.starts_with(BUNDLE_HOST)
            && url.len() > BUNDLE_HOST.len()
            && !url.contains('?')
            && !url.contains('#')
            && self.bundle_sha256.as_deref().is_some_and(digest)
            && self.expanded_sha256.as_deref().is_some_and(digest)
            && positive(self.download_bytes)
            && positive(self.expanded_bytes)
            && positive(self.bundle_features)
            && positive(self.bundle_built_at)
    }

    /// The desktop's `resourceValid`, exactly, plus residency: a cloud row
    /// publishes no bundle, and residency is one of the two words.
    pub fn validate(&self) -> Result<(), Failure> {
        let residency_valid = match self.residency.as_str() {
            "" | "bundle" => true,
            "cloud" => self.download_url.is_none(),
            _ => false,
        };
        let valid = digest(&self.id)
            && (digest(&self.version) || self.is_unavailable())
            && self.active
            && self.kind == "bigquery"
            && self.source_modified_at.is_none_or(|value| value > 0)
            && self.bundle_valid()
            && residency_valid;
        if valid {
            Ok(())
        } else {
            Err(Failure::CatalogInvalid(format!(
                "catalogue row {} is not a valid published reference resource",
                bounded(&self.id)
            )))
        }
    }

    pub fn identity(&self) -> Identity {
        Identity {
            resource_id: self.id.clone(),
            version: self.version.clone(),
        }
    }

    /// The verified bundle this row publishes, when it publishes one.
    pub fn bundle(&self) -> Option<BundleReceipt> {
        if !self.bundle_valid() {
            return None;
        }
        Some(BundleReceipt {
            identity: self.identity(),
            url: self.download_url.clone()?,
            bundle_sha256: self.bundle_sha256.clone()?,
            compressed_bytes: self.download_bytes?,
            expanded_sha256: self.expanded_sha256.clone()?,
            expanded_bytes: self.expanded_bytes?,
            feature_count: self.bundle_features?,
            label: self.label.clone(),
        })
    }
}

fn bounded(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).take(64).collect()
}

/// Decode and validate every row of a `list_datasets` answer.
pub fn validate_resources(rows: &Value) -> Result<Vec<ReferenceResource>, Failure> {
    let rows = rows
        .as_array()
        .ok_or_else(|| Failure::CatalogInvalid("the catalogue is not an array".into()))?;
    if rows.len() > 512 {
        return Err(Failure::CatalogInvalid(
            "the catalogue exceeds 512 resources".into(),
        ));
    }
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let resource: ReferenceResource = serde_json::from_value(row.clone()).map_err(|error| {
            Failure::CatalogInvalid(format!("unreadable catalogue row: {error}"))
        })?;
        resource.validate()?;
        out.push(resource);
    }
    Ok(out)
}

/// Is this exact resource version installed and indexed on this machine?
pub fn installed(root: &Path, resource: &ReferenceResource) -> Result<bool, Failure> {
    if !digest(&resource.version) {
        return Ok(false);
    }
    let status = ds_layer_store::reference_cache::execute_at(
        root,
        json!({"action": "status", "identity": resource.identity()}),
    )
    .map_err(crate::store)?;
    Ok(status["ready"] == Value::Bool(true))
}

/// What the kernel's catalogue decision needs to know about each row on this
/// machine. `ready` is a property of THIS machine; `downloadable` is what makes
/// a row one any project can hold.
pub fn facts(
    root: &Path,
    resources: &[ReferenceResource],
) -> Result<Vec<CatalogResource>, Failure> {
    resources
        .iter()
        .map(|resource| {
            // A cloud row is neither installed nor downloadable anywhere:
            // printing's `catalog()` never selects it.
            Ok(CatalogResource {
                layer: resource.layer.clone(),
                label: resource.label.clone(),
                country: resource.country.clone(),
                ready: !resource.is_unavailable()
                    && !resource.is_cloud_resident()
                    && installed(root, resource)?,
                downloadable: resource.bundle().is_some(),
                unavailable: resource.is_unavailable(),
            })
        })
        .collect()
}

/// The catalogue row a `Catalog { country, layer }` context source names.
pub fn find<'a>(
    resources: &'a [ReferenceResource],
    country: &str,
    layer: &str,
) -> Option<&'a ReferenceResource> {
    resources
        .iter()
        .find(|row| row.layer == layer && row.country.eq_ignore_ascii_case(country))
}

/// How one `--dataset` name resolved against the catalogue: the row, and the
/// alias it was asked by when that differs from the row's own id.
#[derive(Debug, Clone, PartialEq)]
pub struct Named<'a> {
    pub resource: &'a ReferenceResource,
    /// `Some(requested)` when the row answered for a retired alias or a layer
    /// name rather than its exact id.
    pub answered_as: Option<String>,
}

/// Resolve a dataset id, a layer name, or a retired alias to its authority
/// row (`docs/contracts/foundation-datasets.md` R4, R6). An exact id wins;
/// a layer name or alias answers as the row and says so.
pub fn named<'a>(resources: &'a [ReferenceResource], needle: &str) -> Option<Named<'a>> {
    if let Some(resource) = resources.iter().find(|row| row.id == needle) {
        return Some(Named {
            resource,
            answered_as: None,
        });
    }
    resources
        .iter()
        .find(|row| row.answers_to(needle))
        .map(|resource| Named {
            resource,
            answered_as: Some(needle.to_owned()),
        })
}
