//! Complete governed paper interpretation, held beside authored page documents.
//! The API chooses its revision; renderers only admit and consume that reply.
use crate::style_resolution::{Binding, Key, Snapshot};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub italic: bool,
    pub bold: bool,
    pub color: String,
    pub halo: String,
    pub halo_mm: f64,
    pub padding_mm: f64,
    pub radial_mm: f64,
    pub offset_mm: [f64; 2],
    pub anchors: Vec<String>,
    pub sort_key: f64,
    pub max_width_em: f64,
    pub line_height: f64,
    pub spacing_mm: f64,
    pub along_line: bool,
    pub allow_overlap: bool,
    pub keep_upright: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Table {
    pub border_width_mm: f64,
    pub plain_header_fill: String,
    pub omission_fill: String,
    pub omission_text: String,
    pub info_title_fill: String,
    pub info_header_fill: String,
    pub info_section_fill: String,
    pub schedule_title_fill: String,
    pub schedule_band_fill: String,
    pub styled_header_font_factor: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Furniture {
    pub fill_outline_mm: f64,
    pub map_frame_mm: f64,
    pub rectangle_mm: f64,
    pub north_arrow_mm: f64,
    pub page_border_mm: f64,
    pub legend_frame_mm: f64,
    pub scale_bar_stroke_mm: f64,
    pub scale_bar_halo_mm: f64,
    pub text_line_height: f64,
    pub paper_fill: String,
}

/// Exact compatibility typography for documents preceding Layout.attribution.
/// Its gray operator remains gray, preserving the original PDF representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegacyAttribution {
    pub pdf_font_family: String,
    pub font_pt: f64,
    pub gray: f64,
    pub right_inset_pt: f64,
    pub baseline_pt: f64,
    pub min_x_pt: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    pub base_style: super::PrintStyle,
    pub label: Label,
    pub table: Table,
    pub furniture: Furniture,
    pub svg_font_family: String,
    pub converter_font_family: String,
    pub legacy_attribution: LegacyAttribution,
}

pub fn key() -> Key {
    super::context::style_key("print_renderer_defaults", "dataset_geojson", "project")
}

pub fn resolve(snapshot: &Snapshot) -> Result<Binding, String> {
    let binding = crate::style_resolution::resolve(snapshot, &key()).map_err(|e| e.to_string())?;
    admit(&binding)?;
    Ok(binding)
}

pub fn admit(binding: &Binding) -> Result<Defaults, String> {
    crate::style_resolution::admit_binding(binding, &key()).map_err(|e| e.to_string())?;
    from_document(&binding.document)
}

pub fn from_document(document: &serde_json::Value) -> Result<Defaults, String> {
    if document["metadata"].get("physical_print_style").is_some()
        || document["paint"].as_object().is_some_and(|v| !v.is_empty())
        || document["layout"]
            .as_object()
            .is_some_and(|v| !v.is_empty())
        || ["label", "boundary", "d3"]
            .iter()
            .any(|f| document.get(f).is_some())
    {
        return Err(
            "print_renderer_defaults_invalid: metadata is the sole renderer policy authority"
                .into(),
        );
    }
    let value = document["metadata"].get("print_renderer_defaults").ok_or(
        "print_renderer_defaults_missing: held policy document requires complete metadata",
    )?;
    let defaults: Defaults = serde_json::from_value(value.clone())
        .map_err(|e| format!("print_renderer_defaults_invalid: {e}"))?;
    // The legacy PrintStyle representation has defaults; governed policy must
    // still spell every physical field explicitly.
    super::style_overrides::physical_style(&serde_json::json!({
        "type":"line", "metadata":{"physical_print_style":value["base_style"]}
    }))?;
    defaults.validate()?;
    Ok(defaults)
}

impl Defaults {
    pub fn validate(&self) -> Result<(), String> {
        let bounded = super::bounded;
        if !super::color(&self.label.color)
            || !super::color(&self.label.halo)
            || !bounded(self.label.halo_mm, 0., 10.)
            || !bounded(self.label.padding_mm, 0., 20.)
            || !bounded(self.label.radial_mm, 0., 20.)
            || self.label.offset_mm.iter().any(|v| !bounded(*v, -20., 20.))
            || !bounded(self.label.sort_key, -1_000_000., 1_000_000.)
            || !bounded(self.label.max_width_em, 1., 100.)
            || !bounded(self.label.line_height, 0.5, 5.)
            || !bounded(self.label.spacing_mm, 0., 1000.)
            || self.label.anchors.is_empty()
            || self.label.anchors.len() > 9
            || self.label.anchors.iter().any(|a| {
                ![
                    "center",
                    "left",
                    "right",
                    "top",
                    "bottom",
                    "top-left",
                    "top-right",
                    "bottom-left",
                    "bottom-right",
                ]
                .contains(&a.as_str())
            })
            || !bounded(self.table.border_width_mm, 0., 10.)
            || !bounded(self.table.styled_header_font_factor, 0.5, 2.)
            || [
                &self.table.plain_header_fill,
                &self.table.omission_fill,
                &self.table.omission_text,
                &self.table.info_title_fill,
                &self.table.info_header_fill,
                &self.table.info_section_fill,
                &self.table.schedule_title_fill,
                &self.table.schedule_band_fill,
                &self.furniture.paper_fill,
            ]
            .iter()
            .any(|v| !super::color(v))
            || [
                self.furniture.fill_outline_mm,
                self.furniture.map_frame_mm,
                self.furniture.rectangle_mm,
                self.furniture.north_arrow_mm,
                self.furniture.page_border_mm,
                self.furniture.legend_frame_mm,
                self.furniture.scale_bar_stroke_mm,
                self.furniture.scale_bar_halo_mm,
            ]
            .iter()
            .any(|v| !bounded(*v, 0., 10.))
            || !bounded(self.furniture.text_line_height, 0.5, 5.)
            || [&self.svg_font_family, &self.converter_font_family]
                .iter()
                .any(|f| !["DejaVu Sans"].contains(&f.as_str()))
            || !bounded(self.legacy_attribution.font_pt, 7., 48.)
            || !bounded(self.legacy_attribution.gray, 0., 0.6)
            || !bounded(self.legacy_attribution.right_inset_pt, 40., 1000.)
            || !bounded(self.legacy_attribution.baseline_pt, 7., 1000.)
            || !bounded(self.legacy_attribution.min_x_pt, 0., 100.)
            || ![
                "Helvetica",
                "Helvetica-Bold",
                "Helvetica-Oblique",
                "Helvetica-BoldOblique",
                "Times-Roman",
                "Times-Bold",
                "Times-Italic",
                "Times-BoldItalic",
                "Courier",
                "Courier-Bold",
                "Courier-Oblique",
                "Courier-BoldOblique",
            ]
            .contains(&self.legacy_attribution.pdf_font_family.as_str())
        {
            return Err(
                "print_renderer_defaults_invalid: paper policy is outside its closed bounds".into(),
            );
        }
        super::validate_physical_style("renderer_defaults", &self.base_style)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_complete_closed_and_refuse_a_wrong_source_or_digest() {
        let value: serde_json::Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/governed-renderer-defaults.json"
        ))
        .unwrap();
        let document = serde_json::json!({"type":"line","paint":{},"layout":{},"metadata":{"print_renderer_defaults":value}});
        let mut binding = Binding {
            key: key(),
            style_ref: "test/renderer_defaults_print".into(),
            revision_id: "a".repeat(64),
            content_sha256: crate::style_plan::style_digest(&document),
            scope: "catalogue".into(),
            document,
            presentation_tier: None,
        };
        assert!(admit(&binding).is_ok());
        binding.key.source_kind = "cold_existing".into();
        assert!(admit(&binding).unwrap_err().contains("key_mismatch"));
        binding.key = key();
        binding.document["metadata"]["print_renderer_defaults"]["table"]["unknown"] =
            serde_json::json!(true);
        binding.content_sha256 = crate::style_plan::style_digest(&binding.document);
        assert!(admit(&binding).unwrap_err().contains("unknown field"));
        binding.document["metadata"]["print_renderer_defaults"]["table"]
            .as_object_mut()
            .unwrap()
            .remove("unknown");
        binding.document["metadata"]["print_renderer_defaults"]["label"]
            .as_object_mut()
            .unwrap()
            .remove("halo_mm");
        binding.content_sha256 = crate::style_plan::style_digest(&binding.document);
        assert!(admit(&binding).unwrap_err().contains("missing field"));
    }
}
