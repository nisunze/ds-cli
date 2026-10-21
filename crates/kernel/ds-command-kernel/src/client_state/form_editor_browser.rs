//! The Form Editor's bindings to the page's one ClientState.
use serde_json::Value;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

use super::{PageState, StateSignal};
use crate::client_state::Effects;
use crate::gateway_exchange::browser::{rejection, typed};
use crate::gateway_exchange::retained::browser::BrowserEdge;

impl PageState {
    fn editor_edge(&self) -> Rc<BrowserEdge> {
        Rc::new(BrowserEdge::new(
            self.1.gateway.clone().unchecked_into(),
            self.1.store.clone().unchecked_into(),
        ))
    }
}

#[wasm_bindgen(js_class = ClientState)]
impl PageState {
    #[wasm_bindgen(js_name = formEditorView, unchecked_return_type = "StateSignal<Record<string, unknown>>")]
    pub fn form_editor_view(&self) -> StateSignal {
        StateSignal::of(&self.0.form_editor.view)
    }

    #[wasm_bindgen(js_name = formEditorLoad)]
    pub fn form_editor_load(&self, slug: String, refresh: bool) -> js_sys::Promise {
        let loading = self.0.form_editor.load(self.editor_edge(), slug, refresh);
        wasm_bindgen_futures::future_to_promise(async move {
            loading.await.map_err(|error| rejection(&error))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    #[wasm_bindgen(js_name = formEditorEdit, unchecked_return_type = "string | null")]
    pub fn form_editor_edit(
        &self,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] intent: JsValue,
    ) -> Result<JsValue, JsValue> {
        let intent = tsify::serde_wasm_bindgen::from_value(intent)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let added: Value = self
            .0
            .form_editor
            .edit(intent, &self.0.effects.nonce())
            .map_err(|error| rejection(&error))?;
        typed(&added)
    }

    #[wasm_bindgen(js_name = formEditorSaveDraft)]
    pub fn form_editor_save_draft(&self) -> js_sys::Promise {
        let saving = self.0.form_editor.save_draft(self.editor_edge());
        wasm_bindgen_futures::future_to_promise(async move {
            saving.await.map_err(|error| rejection(&error))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    #[wasm_bindgen(js_name = formEditorReset)]
    pub fn form_editor_reset(&self) -> js_sys::Promise {
        let resetting = self.0.form_editor.reset(self.editor_edge());
        wasm_bindgen_futures::future_to_promise(async move {
            resetting.await.map_err(|error| rejection(&error))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    #[wasm_bindgen(js_name = formEditorSave)]
    pub fn form_editor_save(&self) -> js_sys::Promise {
        let saving = self.0.form_editor.save(self.editor_edge());
        wasm_bindgen_futures::future_to_promise(async move {
            saving.await.map_err(|error| rejection(&error))?;
            Ok(JsValue::UNDEFINED)
        })
    }

    #[wasm_bindgen(js_name = formEditorClose)]
    pub fn form_editor_close(&self) {
        self.0.form_editor.close();
    }
}
