//! Pure presentation for a scene already admitted by the managed Network
//! process. This does not open a GridSession, run Runtime or calculate a scene.
use super::*;
use ds_grid_model::{GridPresentationPolicy, StructureLabelPolicy};

pub struct Prepared {
    pub state: DisplayState,
    pub options: ProfileAtlasOptions,
    pub labels: StructureLabelPolicy,
    pub grid_ink: Value,
    rows: Vec<DisplayRow>,
    style: Value,
}

pub fn prepare(
    state: &DisplayState,
    options: ProfileAtlasOptions,
    patch: &DisplayPatch,
    style: Value,
    presentation: &GridPresentationPolicy,
) -> Result<Prepared, String> {
    let state = state.patched(patch)?;
    let rows = resolve(&state, &style)?;
    // Resolve these at admission too, retaining the existing refusal behavior.
    review_status_colors(&style)?;
    review_box_ink(&style)?;
    let options = project_options(options, &state)?;
    let mut labels = presentation.effective_profile_structure_labels();
    labels.fields =
        serde_json::from_value(json!(label_fields(&state))).map_err(|e| e.to_string())?;
    let grid = &rows
        .iter()
        .find(|row| row.option == ds_command_kernel::profile_display::DisplayOption::Grid)
        .ok_or("invalid_profile_view: native display has no Grid encoding")?
        .encoding;
    let grid_ink = json!({"color":grid.color,"width_px":grid.width_px,"dash_px":grid.dash_px});
    Ok(Prepared {
        state,
        options,
        labels,
        grid_ink,
        rows,
        style,
    })
}

pub fn project(
    prepared: Prepared,
    model_id: &str,
    history: &Value,
    retained_asset_count: usize,
    has_structures: bool,
    scene_json: &str,
    calculated: bool,
) -> Result<(DisplayState, Value), String> {
    let scene: ds_grid_engine::ProfileAtlasScene =
        serde_json::from_str(scene_json).map_err(|e| format!("profile_scene_invalid: {e}"))?;
    if serde_json::to_value(&scene.model_revision).map_err(|e| e.to_string())?
        != history["model_revision"]
    {
        return Err(
            "profile_selection_stale: native scene differs from retained authored history".into(),
        );
    }
    let display = Projection::from_scene(
        &scene,
        &prepared.state,
        prepared.rows.clone(),
        &prepared.labels,
        prepared.options,
        &prepared.style,
    )?;
    let command_view = command_view::project(
        &prepared.state,
        Some(&prepared.rows),
        Some(&scene),
        None,
        has_structures,
    );
    let result = json!({"model_id":model_id,"model_revision":history["model_revision"],"history":history,
        "retained_asset_count":retained_asset_count,"scene":null,"scene_json":scene_json,
        "display":display,"analysis":null,"display_state":prepared.state,"command_view":command_view,
        "ran":calculated,"projection_unavailable":null});
    Ok((prepared.state, result))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_preparation_keeps_existing_display_defaults_and_patch_refusals() {
        let options: ProfileAtlasOptions = Default::default();
        let prepared = prepare(
            &DisplayState::default(),
            options.clone(),
            &DisplayPatch::default(),
            serde_json::from_str(include_str!(
                "../../../tests/fixtures/profile-display-style.json"
            ))
            .unwrap(),
            &GridPresentationPolicy::default(),
        )
        .unwrap();
        assert_eq!(prepared.state.vertical_exaggeration, 5.0);
        assert_eq!(prepared.options.vertical_exaggeration, Some(5.0));
        let bad: DisplayPatch =
            serde_json::from_value(json!({"vertical_exaggeration":-1})).unwrap();
        assert!(prepare(
            &DisplayState::default(),
            options,
            &bad,
            json!({}),
            &GridPresentationPolicy::default()
        )
        .is_err());
    }
}
