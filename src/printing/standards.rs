//! Governed LV publication defaults. Both native and WASM hosts consume this
//! plan; only a host writes PNG/PDF bytes. Identity is an input, never inferred.
use super::*;
use base64::Engine;
use sha2::{Digest, Sha256};

pub const TEMPLATE_SCHEMA: &str = "ds.print-standard/v1";
pub const OVERRIDE_SCHEMA: &str = "ds.print-standard-overrides/v1";
pub const VERSION: u16 = 1;
pub const FRONT_ROLES: [PageRole; 6] = [
    PageRole::Cover,
    PageRole::KeyMap,
    PageRole::SheetIndex,
    PageRole::Legend,
    PageRole::GeneralNotes,
    PageRole::Revisions,
];

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InkMode {
    #[default]
    Colour,
    Monochrome,
}
impl InkMode {
    pub fn is_colour(&self) -> bool {
        *self == Self::Colour
    }
}

/// Generic page roles can also be used by MV booklets and custom atlases.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PageRole {
    Cover,
    KeyMap,
    SheetIndex,
    Legend,
    GeneralNotes,
    Revisions,
    Network,
    Schedule,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    LvTransformerA0,
    LvTransformerA3,
    LvCombinedA0,
    LvCombinedA3,
}
impl Kind {
    pub fn paper(self) -> Paper {
        match self {
            Self::LvTransformerA3 | Self::LvCombinedA3 => Paper::A3,
            _ => Paper::A0,
        }
    }
    pub fn combined(self) -> bool {
        matches!(self, Self::LvCombinedA0 | Self::LvCombinedA3)
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::LvTransformerA0 => "lv-transformer-a0-v1",
            Self::LvTransformerA3 => "lv-transformer-a3-v1",
            Self::LvCombinedA0 => "lv-combined-a0-v1",
            Self::LvCombinedA3 => "lv-combined-a3-v1",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Logo {
    pub asset: Asset,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub project_id: String,
    pub project_label: String,
    pub client: String,
    pub contractor: String,
    pub consultant: String,
    pub contract: String,
    pub model_version: String,
    pub issue: String,
    pub submission: String,
    pub revision: String,
    pub issue_date: String,
    /// Governed held PNGs with exact digests; no network lookup at print time.
    pub logos: BTreeMap<String, Logo>,
}
impl Identity {
    pub fn validate(&self) -> Result<(), String> {
        for (key, value) in self.text() {
            if value.trim().is_empty() || value.len() > 200 || value.contains(['\n', '\r']) {
                return Err(format!(
                    "print_identity_invalid: {key} requires 1..200 single-line bytes"
                ));
            }
        }
        if self.issue_date.len() != 10
            || time::Date::parse(
                &self.issue_date,
                &time::format_description::parse("[year]-[month]-[day]").unwrap(),
            )
            .is_err()
        {
            return Err("print_identity_invalid: issue_date requires YYYY-MM-DD".into());
        }
        if self.logos.is_empty() || self.logos.len() > 6 {
            return Err("print_logo_invalid: supply 1..6 governed logos".into());
        }
        for (key, logo) in &self.logos {
            let raw = logo
                .asset
                .png_data_url
                .strip_prefix("data:image/png;base64,")
                .ok_or("print_logo_invalid: logo must be a held PNG")?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(raw)
                .map_err(|_| "print_logo_invalid: invalid base64")?;
            if !identifier(key)
                || bytes.len() > 100_000
                || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                || format!("{:x}", Sha256::digest(&bytes)) != logo.sha256
            {
                return Err(format!(
                    "print_logo_invalid: {key} is invalid or its digest differs"
                ));
            }
        }
        Ok(())
    }
    pub fn text(&self) -> BTreeMap<String, String> {
        BTreeMap::from(
            [
                ("project_id", &self.project_id),
                ("project", &self.project_label),
                ("client", &self.client),
                ("contractor", &self.contractor),
                ("consultant", &self.consultant),
                ("contract", &self.contract),
                ("model_version", &self.model_version),
                ("issue", &self.issue),
                ("submission", &self.submission),
                ("revision", &self.revision),
                ("date", &self.issue_date),
            ]
            .map(|(k, v)| (k.into(), v.clone())),
        )
    }
}

/// A bounded versioned layer above the governed template. No attribution or
/// source ownership field exists here. Transformer patches never change peers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Overrides {
    pub schema: String,
    pub template_version: u16,
    #[serde(default)]
    pub ink_mode: InkMode,
    #[serde(default)]
    pub title_block: BTreeMap<String, String>,
    #[serde(default)]
    pub transformers: BTreeMap<String, overrides::PrintOverride>,
}
impl Overrides {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != OVERRIDE_SCHEMA || self.template_version != VERSION {
            return Err(
                "print_override_version: override schema/template version is unsupported".into(),
            );
        }
        if self.transformers.len() > 500
            || self.title_block.len() > 12
            || self.title_block.iter().any(|(k, v)| {
                !matches!(
                    k.as_str(),
                    "issue"
                        | "submission"
                        | "revision"
                        | "date"
                        | "drawn_by"
                        | "checked_by"
                        | "approved_by"
                ) || v.len() > 200
                    || v.trim().is_empty()
                    || v.contains(['\n', '\r'])
            })
        {
            return Err(
                "print_override_invalid: only bounded issue/title-block fields may be overridden"
                    .into(),
            );
        }
        if let Some(date) = self.title_block.get("date") {
            if time::Date::parse(
                date,
                &time::format_description::parse("[year]-[month]-[day]").unwrap(),
            )
            .is_err()
            {
                return Err("print_override_invalid: date requires YYYY-MM-DD".into());
            }
        }
        Ok(())
    }
}

pub fn registry() -> Value {
    json!({"schema":TEMPLATE_SCHEMA,"version":VERSION,"templates":[
        {"id":Kind::LvTransformerA0.id(),"kind":"lv_transformer_a0","paper":"A0"},
        {"id":Kind::LvTransformerA3.id(),"kind":"lv_transformer_a3","paper":"A3"},
        {"id":Kind::LvCombinedA0.id(),"kind":"lv_combined_a0","paper":"A0","front_matter_paper":"A0","front_roles":FRONT_ROLES},
        {"id":Kind::LvCombinedA3.id(),"kind":"lv_combined_a3","paper":"A3","front_matter_paper":"A0","front_roles":FRONT_ROLES}],
        "defaults":{"ink_mode":"colour","preview":"PNG before PDF","attribution":attribution::CREDIT,
        "optional_context":"omit with diagnostics","adjacent_networks":"muted, no labels or schedule rows",
        "identity":"explicit issue date, project parties, contract, logos and model version",
        "pdf":"vector geometry; mixed page sizes retained","schedule_overflow":"automatic numbered continuation sheets; never silently omit rows"},
        "override_schema":schemars::schema_for!(Overrides),"identity_schema":schemars::schema_for!(Identity)})
}

/// Immutable printing documents captured through the authorized API. Layouts
/// own geometry, fonts, table typography and declared semantic layer keys.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateDocument {
    pub template_id: String,
    pub role: PageRole,
    pub revision_id: String,
    pub content_sha256: String,
    pub layout: Layout,
    /// Ordered asset slots authored by the document, never generated by a host.
    pub logo_slots: Vec<String>,
    /// API resolver requests authored by the printing document.
    pub layer_keys: BTreeMap<String, Value>,
}
impl TemplateDocument {
    pub fn content_digest(&self) -> Result<String, String> {
        super::resolved_styles::content_digest(
            &json!({"template_id":self.template_id,"role":self.role,"layout":self.layout,"logo_slots":self.logo_slots,"layer_keys":self.layer_keys}),
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Templates {
    pub schema: String,
    pub project_id: String,
    pub revision_id: String,
    pub documents: Vec<TemplateDocument>,
}
impl Templates {
    pub fn document(
        &self,
        kind: Kind,
        role: PageRole,
        project: &str,
    ) -> Result<&TemplateDocument, String> {
        let paper = if matches!(role, PageRole::Network | PageRole::Schedule) {
            kind.paper()
        } else {
            Paper::A0
        };
        self.document_by_id(kind.id(), role, paper, project)
    }
    /// Generic consumer used by MV/booklet families with their own registry.
    pub fn document_by_id(
        &self,
        template_id: &str,
        role: PageRole,
        paper: Paper,
        project: &str,
    ) -> Result<&TemplateDocument, String> {
        if self.schema != TEMPLATE_SCHEMA || self.project_id != project || self.documents.len() > 64
        {
            return Err("print_template_invalid: schema, project or inventory invalid".into());
        }
        let mut matches = self
            .documents
            .iter()
            .filter(|d| d.template_id == template_id && d.role == role);
        let document = matches
            .next()
            .ok_or("print_template_missing: API capture lacks the standard page role")?;
        if matches.next().is_some() {
            return Err("print_template_ambiguous: duplicate standard page role".into());
        }
        let digest = document.content_digest()?;
        if document.revision_id.len() != 64
            || self.revision_id.len() != 64
            || digest != document.content_sha256
        {
            return Err(
                "print_template_revision_mismatch: layout differs from its pinned API document"
                    .into(),
            );
        }
        if document.layout.paper.size != paper
            || !document.layout.styles.is_empty()
            || !document.layout.style_refs.is_empty()
        {
            return Err("print_template_invalid: paper mismatch or embedded style choice".into());
        }
        validate(&document.layout).map_err(|e| format!("print_template_invalid: {e}"))?;
        Ok(document)
    }
}
/// Instantiate a governed document; this function defines no paper typography,
/// colours, pens or furniture geometry. Native and WASM call the same consumer.
pub fn layout(
    templates: &Templates,
    kind: Kind,
    role: PageRole,
    identity: &Identity,
    patch: Option<&Overrides>,
) -> Result<Layout, String> {
    identity.validate()?;
    if let Some(p) = patch {
        p.validate()?;
    }
    let document = templates.document(kind, role, &identity.project_id)?;
    let mut out = document.layout.clone();
    if identity.logos.len() > document.logo_slots.len() {
        return Err("print_logo_invalid: governed template has too few logo slots".into());
    }
    out.ink_mode = patch.map_or(InkMode::Colour, |p| p.ink_mode);
    out.title_block = identity.text();
    if let Some(p) = patch {
        out.title_block.extend(p.title_block.clone());
    }
    for (index, slot) in document.logo_slots.iter().enumerate() {
        if let Some((_, logo)) = identity.logos.iter().nth(index) {
            out.assets.insert(slot.clone(), logo.asset.clone());
        } else {
            out.elements
                .retain(|e| e.kind != ElementKind::Logo || e.asset != *slot);
            out.assets.remove(slot);
        }
    }
    validate(&out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity() -> Identity {
        let bytes=base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aBfcAAAAASUVORK5CYII=").unwrap();
        Identity {
            project_id: "fixture".into(),
            project_label: "Fixture project".into(),
            client: "Fixture client".into(),
            contractor: "Fixture contractor".into(),
            consultant: "Fixture consultant".into(),
            contract: "FIXTURE-1".into(),
            model_version: "fixture-model-v1".into(),
            issue: "01".into(),
            submission: "Design submission".into(),
            revision: "A".into(),
            issue_date: "2026-10-02".into(),
            logos: BTreeMap::from([(
                "fixture".into(),
                Logo {
                    asset: Asset {
                        label: "Fixture logo".into(),
                        png_data_url: format!(
                            "data:image/png;base64,{}",
                            base64::engine::general_purpose::STANDARD.encode(&bytes)
                        ),
                    },
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                },
            )]),
        }
    }
    #[test]
    fn unknown_override_and_forged_logo_are_refused() {
        assert!(serde_json::from_value::<Overrides>(
            json!({"schema":OVERRIDE_SCHEMA,"template_version":1,"attribution":false})
        )
        .is_err());
        let mut id = identity();
        id.logos.values_mut().next().unwrap().sha256 = "0".repeat(64);
        assert!(id
            .validate()
            .unwrap_err()
            .starts_with("print_logo_invalid:"));
    }
}
