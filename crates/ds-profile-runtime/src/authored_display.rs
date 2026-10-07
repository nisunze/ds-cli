//! Presentation is partitioned natively so structure edits never transfer terrain.
use super::*;
use ds_command_kernel::profile_display::DisplayOption as O;
use ds_grid_engine::ProfileAtlasScene;

pub(super) type Groups = BTreeMap<String, Value>;

pub(super) fn partition(
    symbols: Vec<symbols::Symbol>,
    primitives: Vec<decorations::Primitive>,
    scene: &ProfileAtlasScene,
) -> (Groups, Vec<symbols::Symbol>, Vec<decorations::Primitive>) {
    let mut groups = scene
        .bands
        .iter()
        .flat_map(|band| &band.structures)
        .map(|row| {
            (
                row.entity_id.to_string(),
                json!({"symbols":[], "primitives":[]}),
            )
        })
        .collect::<Groups>();
    let mut static_symbols = vec![];
    let mut static_primitives = vec![];
    for symbol in symbols {
        let id = symbol.parent_id.as_ref().unwrap_or(&symbol.entity_id);
        if let Some(group) = groups.get_mut(id) {
            group["symbols"].as_array_mut().unwrap().push(json!(symbol));
        } else {
            static_symbols.push(symbol);
        }
    }
    for primitive in primitives {
        if let Some(group) = primitive
            .structure_id
            .as_ref()
            .and_then(|id| groups.get_mut(id))
        {
            group["primitives"]
                .as_array_mut()
                .unwrap()
                .push(json!(primitive));
        } else {
            static_primitives.push(primitive);
        }
    }
    (groups, static_symbols, static_primitives)
}

pub(super) fn project(
    scene: &ProfileAtlasScene,
    state: &DisplayState,
    rows: &[DisplayRow],
    labels: &ds_grid_model::StructureLabelPolicy,
) -> Groups {
    let mut state = state.clone();
    for option in [
        O::TerrainPoints,
        O::TerrainOrdinates,
        O::SpanDistances,
        O::SectionLabels,
    ] {
        state.visibility.insert(option, false);
    }
    let legend = ds_command_kernel::profile_encoding::styled_legend(&state, rows, &[]);
    partition(
        symbols::project(scene, &state, rows, &legend),
        decorations::project(scene, &state, rows, labels),
        scene,
    )
    .0
}

pub(super) fn delta(before: &Groups, after: &Groups) -> Groups {
    before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(*key) != after.get(*key))
        .map(|key| (key.clone(), after.get(key).cloned().unwrap_or(Value::Null)))
        .collect()
}
