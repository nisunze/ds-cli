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

fn element(id: &str, kind: ElementKind, rect: [f64; 4], text: &str, pt: f64) -> Element {
    serde_json::from_value(json!({"id":id,"kind":kind,"rect":rect,"text":text,"font_pt":pt,"color":"#192b38","fill":"#ffffff"})).unwrap()
}

/// A paper-specific composition, never an A0 scaled down to A3.
pub fn layout(
    kind: Kind,
    role: PageRole,
    identity: &Identity,
    patch: Option<&Overrides>,
) -> Result<Layout, String> {
    identity.validate()?;
    if let Some(p) = patch {
        p.validate()?;
    }
    let paper = if matches!(role, PageRole::Network | PageRole::Schedule) {
        kind.paper()
    } else {
        Paper::A0
    };
    let mut out = named_layout(
        Some(kind.id().into()),
        Some(format!("LV standard · {role:?}")),
        paper,
        Orientation::Landscape,
    )?;
    let [w, h] = out.page_mm;
    let logo_w = if paper == Paper::A0 { 42. } else { 22. };
    out.ink_mode = patch.map_or(InkMode::Colour, |p| p.ink_mode);
    out.elements.clear();
    out.style_refs.clear();
    out.styles.clear();
    out.title_block = identity.text();
    if let Some(p) = patch {
        out.title_block.extend(p.title_block.clone());
    }
    out.elements.push(element(
        "border",
        ElementKind::Rectangle,
        [5., 5., w - 10., h - 10.],
        "",
        8.,
    ));
    out.elements.push(element(
        "heading",
        ElementKind::Text,
        [
            12.,
            10.,
            w - 24. - logo_w * identity.logos.len() as f64,
            16.,
        ],
        "{title}",
        if paper == Paper::A0 { 24. } else { 16. },
    ));
    out.elements.push(element("identity",ElementKind::Text,[12.,h-24.,w-24.,16.],"{project} · Contract {contract} · Model {model_version}\nIssue {issue} · {submission} · Rev {revision} · {date} · {drawing}",if paper==Paper::A0{10.}else{7.}));
    for (i, (key, logo)) in identity.logos.iter().enumerate() {
        out.assets.insert(key.clone(), logo.asset.clone());
        let mut logo_element = element(
            &format!("logo-{key}"),
            ElementKind::Logo,
            [w - 12. - logo_w * (i + 1) as f64, 10., logo_w - 2., 16.],
            "",
            8.,
        );
        logo_element.asset = key.clone();
        out.elements.push(logo_element);
    }
    match role {
        PageRole::Network => {
            let a0 = paper == Paper::A0;
            let sidebar = if a0 { 260. } else { 125. };
            let map_w = w - sidebar - 38.;
            let mut map = element("map", ElementKind::Map, [12., 34., map_w, h - 64.], "", 8.);
            map.viewport = Some(PrintViewport {
                name: "Focused LV network".into(),
                view_slot: "primary_map".into(),
                clip: true,
                fit_around: vec![],
                graticule: None,
            });
            out.elements.push(map);
            out.elements.push(element(
                "scale",
                ElementKind::ScaleBar,
                [20., h - 48., 80., 12.],
                "",
                8.,
            ));
            out.elements.push(element(
                "north",
                ElementKind::NorthArrow,
                [map_w - 12., 40., 16., 22.],
                "",
                8.,
            ));
            let x = w - sidebar - 12.;
            let font = if a0 { 9. } else { 7. };
            let mut info = element(
                "network-information",
                ElementKind::Table,
                [x, 34., sidebar, if a0 { 225. } else { 85. }],
                "NETWORK INFORMATION",
                font,
            );
            info.table = Some(TableBinding {
                layer: "workbook_info".into(),
                columns: vec!["Description".into(), "Unit".into(), "Quantity".into()],
                headings: vec!["Description".into(), "Unit".into(), "Quantity".into()],
                widths: vec![0.7, 0.1, 0.2],
                presentation: TablePresentation::WorkbookInfo,
                overflow: TableOverflow::Error,
                ..Default::default()
            });
            out.elements.push(info);
            let mut poles = element(
                "pole-schedule",
                ElementKind::Table,
                [
                    x,
                    if a0 { 275. } else { 125. },
                    sidebar,
                    if a0 { 190. } else { 62. },
                ],
                "POLE SCHEDULE",
                font,
            );
            poles.table = Some(TableBinding {
                layer: "lv_poles".into(),
                additional_layers: vec!["tapping_poles".into()],
                columns: vec![
                    "pole_number".into(),
                    "struct_type".into(),
                    "stay".into(),
                    "x".into(),
                    "y".into(),
                ],
                headings: vec![
                    "Pole".into(),
                    "Structure".into(),
                    "Stays".into(),
                    "X (m)".into(),
                    "Y (m)".into(),
                ],
                widths: vec![0.2; 5],
                sort_by: vec!["pole_number".into()],
                overflow: TableOverflow::Error,
                presentation: TablePresentation::Plain,
                ..Default::default()
            });
            out.elements.push(poles);
            let mut customers = element(
                "customer-schedule",
                ElementKind::Table,
                [
                    x,
                    if a0 { 480. } else { 193. },
                    sidebar,
                    if a0 { 310. } else { 72. },
                ],
                "HOUSE CONNECTION SCHEDULE",
                font,
            );
            customers.table = Some(TableBinding {
                layer: "customers".into(),
                columns: vec![
                    "house_number".into(),
                    "pole_number".into(),
                    "names".into(),
                    "cable_size".into(),
                    "service_length".into(),
                ],
                headings: vec![
                    "House".into(),
                    "Pole".into(),
                    "Names".into(),
                    "Cable size (mm²)".into(),
                    "Service (m)".into(),
                ],
                widths: vec![0.14, 0.14, 0.32, 0.25, 0.15],
                sort_by: vec!["house_number".into()],
                overflow: TableOverflow::Error,
                presentation: TablePresentation::Plain,
                ..Default::default()
            });
            out.elements.push(customers);
            // Legend belongs on its own governed page in a combined set;
            // individual sheets place it over a reserved strip of the map.
            let mut legend = element(
                "legend",
                ElementKind::Legend,
                [
                    20.,
                    42.,
                    if a0 { 245. } else { 132. },
                    if a0 { 270. } else { 115. },
                ],
                "LEGEND",
                font,
            );
            legend.legend = Some(
                serde_json::from_value(
                    json!({"layers":["placeholder"],"row_mm":if a0 {5.}else{4.}}),
                )
                .unwrap(),
            );
            out.elements.push(legend);
            out.elements
                .iter_mut()
                .find(|e| e.id == "map")
                .unwrap()
                .viewport
                .as_mut()
                .unwrap()
                .fit_around = vec!["legend".into()];
            out.scale_rounding = Some(100.);
        }
        PageRole::Schedule => {
            let mut table = element(
                "schedule",
                ElementKind::Table,
                [12., 36., w - 24., h - 68.],
                "CONTINUED ENGINEERING SCHEDULE",
                if paper == Paper::A0 { 10. } else { 7. },
            );
            table.table = Some(TableBinding {
                layer: "schedule".into(),
                columns: vec!["value".into()],
                headings: vec!["Value".into()],
                widths: vec![1.],
                presentation: TablePresentation::Plain,
                overflow: TableOverflow::Error,
                ..Default::default()
            });
            out.elements.push(table);
        }
        PageRole::KeyMap => {
            let mut map = element(
                "map",
                ElementKind::Map,
                [12., 36., w - 24., h - 66.],
                "",
                9.,
            );
            map.viewport = Some(PrintViewport {
                name: "Every transformer".into(),
                view_slot: "primary_map".into(),
                clip: true,
                fit_around: vec![],
                graticule: None,
            });
            out.elements.push(map);
            out.elements.push(element(
                "scale",
                ElementKind::ScaleBar,
                [20., h - 50., 100., 12.],
                "",
                10.,
            ));
        }
        PageRole::Legend => {
            out.elements.push(element(
                "legend",
                ElementKind::Legend,
                [24., 45., w - 48., h - 85.],
                "GOVERNED PRINT PENS",
                12.,
            ));
        }
        PageRole::SheetIndex => {
            let mut e = element(
                "sheet-index",
                ElementKind::Table,
                [24., 45., w - 48., h - 85.],
                "SHEET INDEX",
                12.,
            );
            e.table = Some(TableBinding {
                layer: "sheet_index".into(),
                panels: 4,
                columns: vec!["drawing".into(), "transformer".into(), "version".into()],
                headings: vec![
                    "Drawing reference".into(),
                    "Transformer".into(),
                    "Saved version".into(),
                ],
                widths: vec![0.2, 0.6, 0.2],
                overflow: TableOverflow::Error,
                presentation: TablePresentation::Plain,
                ..Default::default()
            });
            out.elements.push(e);
        }
        PageRole::Cover => {
            out.elements.push(element("parties",ElementKind::Text,[60.,130.,w-120.,500.],"{project}\nLV NETWORK DRAWINGS\n\nClient: {client}\nContractor: {contractor}\nConsultant: {consultant}\nContract: {contract}\n\nModel/version: {model_version}\nIssue: {issue}\nSubmission: {submission}\nRevision: {revision}\nIssued: {date}",24.));
        }
        PageRole::GeneralNotes => {
            out.elements.push(element("general-notes",ElementKind::Text,[30.,55.,w-60.,h-100.],"GENERAL NOTES\n\n1. Read these drawings with the project specifications and approved engineering schedules.\n2. Quantities and schedules refer only to the focused transformer. Adjacent circuits are context.\n3. Existing infrastructure and new design use distinct pens; consult the legend before construction.\n4. Coordinates retain the source engineering CRS. Ground distances and cable sizes use stated units.\n5. Optional contours/buildings/MV context show held coverage; absence is reported in the receipt.\n6. This issue identifies the supplied model/version; construction approval is not inferred by printing.\n7. Unclassified attributes are identified explicitly. Check source data before assigning quantities.\n8. Each drawing reference is stable in the full project roster and is shared by A0 and A3 outputs.",16.));
        }
        PageRole::Revisions => {
            let mut table = element(
                "revision-table",
                ElementKind::Table,
                [30., 55., w - 60., h - 100.],
                "REVISION TABLE",
                14.,
            );
            table.table = Some(TableBinding {
                layer: "revisions".into(),
                columns: vec![
                    "revision".into(),
                    "date".into(),
                    "issue".into(),
                    "submission".into(),
                ],
                headings: vec![
                    "Revision".into(),
                    "Issue date".into(),
                    "Issue".into(),
                    "Submission identity".into(),
                ],
                widths: vec![0.1, 0.15, 0.15, 0.6],
                presentation: TablePresentation::Plain,
                overflow: TableOverflow::Error,
                ..Default::default()
            });
            out.elements.push(table);
        }
    }
    validate(&out)?;
    Ok(out)
}

/// Apply to a resolved pen before drawing *and* to its legend swatch.
/// Cable categories retain physical dash patterns when photocopied.
pub fn monochrome_pen(style: &mut PrintStyle, properties: &Value, category: Option<&str>) {
    style.stroke = "#222222".into();
    for colour in style.colors.values_mut() {
        *colour = "#222222".into();
    }
    if let Some(hatch) = &mut style.hatch {
        hatch.color = "#222222".into();
    }
    style.casing = "#ffffff".into();
    let existing = properties["existing_status"]
        .as_str()
        .is_some_and(|v| v.eq_ignore_ascii_case("existing"));
    if existing {
        style.dash_mm = vec![2., 1.2];
        style.fill = "#ffffff".into();
    } else {
        style.fill = if style.fill == "#ffffff" || style.fill == "none" {
            style.fill.clone()
        } else {
            "#222222".into()
        };
    }
    if let Some(category) = category.filter(|v| !v.is_empty()) {
        // Stable across page subsets and render order. Category text in the
        // legend disambiguates even equal-size structures with the same pen.
        let digest = Sha256::digest(category.as_bytes());
        if !existing {
            style.dash_mm = match digest[0] % 4 {
                0 => vec![],
                1 => vec![6., 1.],
                2 => vec![2., 1.],
                _ => vec![6., 1., 1., 1.],
            };
        }
    }
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
    fn each_standard_is_independent_and_front_matter_is_always_a0() {
        let id = identity();
        for kind in [
            Kind::LvTransformerA0,
            Kind::LvTransformerA3,
            Kind::LvCombinedA0,
            Kind::LvCombinedA3,
        ] {
            let network = layout(kind, PageRole::Network, &id, None).unwrap();
            assert_eq!(network.paper.size, kind.paper());
            for role in FRONT_ROLES {
                assert_eq!(layout(kind, role, &id, None).unwrap().paper.size, Paper::A0);
            }
            assert!(
                network
                    .elements
                    .iter()
                    .filter_map(|e| e.table.as_ref())
                    .all(|t| t.overflow == TableOverflow::Error)
            );
        }
    }
    #[test]
    fn unknown_override_and_forged_logo_are_refused() {
        assert!(
            serde_json::from_value::<Overrides>(
                json!({"schema":OVERRIDE_SCHEMA,"template_version":1,"attribution":false})
            )
            .is_err()
        );
        let mut id = identity();
        id.logos.values_mut().next().unwrap().sha256 = "0".repeat(64);
        assert!(
            id.validate()
                .unwrap_err()
                .starts_with("print_logo_invalid:")
        );
    }
}
