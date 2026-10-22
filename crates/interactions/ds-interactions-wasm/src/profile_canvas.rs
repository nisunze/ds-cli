//! Profile surface on the permanent Interactions WASM; the host handles typed intents.
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct ProfileCanvas {
    mounted: ds_profile_view::browser::Mounted,
}

#[wasm_bindgen(js_name = mountProfileCanvas)]
pub fn mount(
    element: web_sys::HtmlElement,
    scene_json: &str,
    state_json: &str,
    on_intent: js_sys::Function,
) -> Result<ProfileCanvas, JsValue> {
    if scene_json.len() > 256 * 1024 * 1024 || state_json.len() > 4 * 1024 * 1024 {
        return Err(JsValue::from_str("profile_view_input_too_large"));
    }
    Ok(ProfileCanvas {
        mounted: ds_profile_view::browser::mount(element, scene_json, state_json, on_intent)?,
    })
}

#[wasm_bindgen]
impl ProfileCanvas {
    pub fn update(&mut self, scene_json: Option<String>, state_json: &str) -> Result<(), JsValue> {
        if state_json.len() > 4 * 1024 * 1024
            || scene_json
                .as_ref()
                .is_some_and(|s| s.len() > 256 * 1024 * 1024)
        {
            return Err(JsValue::from_str(
                "{\"code\":\"profile_view_input_too_large\",\"message\":\"Profile view input exceeds its bound\"}",
            ));
        }
        self.mounted.update(scene_json.as_deref(), state_json)
    }
    pub fn payload(&self, json: &str) -> Result<String, JsValue> {
        ds_profile_view::payload::evaluate(json).map_err(|e| {
            let (code, message) = e.split_once(':').unwrap_or(("profile_payload_invalid", &e));
            JsValue::from_str(
                &serde_json::json!({"code":code,"message":message.trim()}).to_string(),
            )
        })
    }
    pub fn command(&mut self, json: &str) -> Result<(), JsValue> {
        self.mounted.command(json)
    }
    pub fn reply(&mut self, id: u32, json: &str) -> Result<(), JsValue> {
        self.mounted.reply(u64::from(id), json)
    }
    pub fn invalidate(&mut self) {
        self.mounted.invalidate();
    }
    pub fn read(&self) -> Result<String, JsValue> {
        self.mounted.read()
    }
    pub fn unmount(&mut self) {
        self.mounted.unmount();
    }
}
