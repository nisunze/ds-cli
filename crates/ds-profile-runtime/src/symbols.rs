//! Native semantic glyph placements. Hosts transform coordinates and paint.
use ds_command_kernel::{
    profile_display::{DisplayOption as O, DisplayState},
    profile_encoding::{DisplayRow, Encoding, LegendEntry},
};
use ds_grid_engine::{profile_atlas::ProfileAtlasPoint, ProfileAtlasScene};
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub entity_id: String,
    pub parent_id: Option<String>,
    pub point: ProfileAtlasPoint,
    pub glyph: String,
    pub encoding: Encoding,
}
pub fn project(
    scene: &ProfileAtlasScene,
    state: &DisplayState,
    rows: &[DisplayRow],
    legend: &[LegendEntry],
) -> Vec<Symbol> {
    let mut symbols = Vec::new();
    let pen = |option| {
        rows.iter()
            .find(|row| row.option == option)
            .unwrap()
            .encoding
            .clone()
    };
    if state.visibility[&O::Structures] {
        for structure in scene.bands.iter().flat_map(|b| &b.structures) {
            let encoding = pen(O::Structures);
            symbols.push(Symbol {
                entity_id: structure.entity_id.to_string(),
                parent_id: None,
                point: structure.top.unwrap_or(structure.ground),
                glyph: encoding.symbol.clone().unwrap_or("●".into()),
                encoding,
            });
            if structure.junction {
                let symbol = legend
                    .iter()
                    .find(|v| {
                        matches!(
                            v.kind,
                            ds_command_kernel::profile_encoding::LegendKind::Junction
                        )
                    })
                    .unwrap();
                symbols.push(Symbol {
                    entity_id: structure.entity_id.to_string(),
                    parent_id: None,
                    point: structure.top.unwrap_or(structure.ground),
                    glyph: symbol.glyph.clone(),
                    encoding: symbol.encoding.clone(),
                });
            }
            for attachment in &structure.attachment_points {
                use ds_command_kernel::profile_encoding::LegendKind as K;
                let kind = match attachment.dead_end {
                    Some(true) => K::Strain,
                    Some(false) => K::Suspension,
                    None => K::Unknown,
                };
                let symbol = legend.iter().find(|v| v.kind == kind).unwrap();
                symbols.push(Symbol {
                    entity_id: format!(
                        "{}\0{}",
                        structure.entity_id, attachment.attachment_point_id
                    ),
                    parent_id: Some(structure.entity_id.to_string()),
                    point: attachment.point,
                    glyph: symbol.glyph.clone(),
                    encoding: symbol.encoding.clone(),
                });
            }
        }
    }
    if state.visibility[&O::TerrainPoints] {
        let encoding = pen(O::TerrainPoints);
        for terrain in scene.bands.iter().flat_map(|b| &b.terrain_points) {
            symbols.push(Symbol {
                entity_id: terrain.entity_id.to_string(),
                parent_id: None,
                point: terrain.point,
                glyph: encoding.symbol.clone().unwrap_or("●".into()),
                encoding: encoding.clone(),
            });
        }
    }
    for symbol in &mut symbols {
        symbol.encoding.font = format!("{}px monospace", (symbol.encoding.width_px * 4.0).max(8.0));
    }
    symbols.sort_by(|a, b| a.point.scene_x.total_cmp(&b.point.scene_x));
    symbols
}
