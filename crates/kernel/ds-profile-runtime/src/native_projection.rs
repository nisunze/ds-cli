//! Network owns the single Profile scene presentation implementation.
#[cfg(test)]
use super::*;
pub use ds_profile_presentation::native_projection::*;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_scene_projection_keeps_existing_display_and_does_not_claim_analysis() {
        let input = super::super::transactional_projection_tests::request();
        let seed: ds_grid_exchange::profile_session::ProfileSessionSeed =
            serde_json::from_str(&input.seed_json).unwrap();
        let history = serde_json::to_value(&seed.history).unwrap();
        let prepared = prepare(
            &DisplayState::default(),
            input.options.clone(),
            &input.patch,
            input.style_document.clone(),
            &seed.presentation,
        )
        .unwrap();
        let legacy = Runtime::default().execute(input).unwrap();
        let mut scene = legacy["scene"].clone();
        let display = scene.as_object_mut().unwrap().remove("display").unwrap();
        let scene_json = serde_json::to_string(&scene).unwrap();
        let (_, projected) = project(
            prepared,
            "profile-test",
            &history,
            seed.assets.len(),
            true,
            &scene_json,
        )
        .unwrap();
        assert_eq!(projected["display"], display);
        assert_eq!(projected["scene_json"], scene_json);
        assert_eq!(projected["history"], history);
        assert_eq!(projected["analysis"], Value::Null);
        assert_eq!(projected["ran"], false);
        let mut stale = history;
        stale["model_revision"] = json!("rev:foreign");
        let input = super::super::transactional_projection_tests::request();
        let prepared = prepare(
            &DisplayState::default(),
            input.options,
            &input.patch,
            input.style_document,
            &seed.presentation,
        )
        .unwrap();
        assert!(project(
            prepared,
            "profile-test",
            &stale,
            seed.assets.len(),
            true,
            &scene_json
        )
        .is_err());
    }
}
