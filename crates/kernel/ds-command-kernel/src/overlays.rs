//! Offline raster-overlay registry transitions. Hosts supply IDs and persistence.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Overlay {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub url: String,
    #[serde(default = "tile_size")]
    pub tile_size: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    #[serde(default = "visible")]
    pub visible: bool,
}
fn tile_size() -> u16 {
    256
}
fn visible() -> bool {
    true
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum OverlayEdit {
    List,
    Add { layer: Overlay },
    Remove { id: String },
    Visibility { id: String, visible: bool },
    Import { layers: Vec<Overlay> },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayRegistry {
    pub schema: u32,
    pub revision: u64,
    pub legacy_imported: bool,
    pub layers: Vec<Overlay>,
}
impl Default for OverlayRegistry {
    fn default() -> Self {
        Self {
            schema: 1,
            revision: 0,
            legacy_imported: false,
            layers: vec![],
        }
    }
}
fn text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.chars().count() <= max
        && !value.chars().any(char::is_control)
}
impl Overlay {
    pub fn validate(&self) -> Result<(), String> {
        if !text(&self.id, 200)
            || !text(&self.name, 120)
            || !text(&self.url, 2048)
            || !matches!(self.tile_size, 256 | 512)
            || self.attribution.as_ref().is_some_and(|s| !text(s, 500))
        {
            return Err("invalid overlay name, id, URL, attribution or tile size".into());
        }
        if !matches!(self.kind.as_str(), "xyz" | "pmtiles") {
            return Err("kind must be xyz or pmtiles".into());
        }
        let raw = if self.kind == "pmtiles" {
            self.url.strip_prefix("pmtiles://").unwrap_or(&self.url)
        } else {
            &self.url
        };
        let url = url::Url::parse(raw).map_err(|_| "overlay URL must be absolute HTTP(S)")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err("overlay URL must be HTTP(S) without embedded credentials".into());
        }
        if self.kind == "xyz" && !["{z}", "{x}", "{y}"].iter().all(|t| raw.contains(t)) {
            return Err("XYZ URL requires {z}, {x}, and {y}".into());
        }
        Ok(())
    }
}
impl OverlayRegistry {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != 1 || self.layers.len() > 100 {
            return Err("unsupported or oversized overlay registry".into());
        }
        let mut ids = std::collections::HashSet::new();
        for row in &self.layers {
            row.validate()?;
            if !ids.insert(&row.id) {
                return Err("duplicate overlay id".into());
            }
        }
        Ok(())
    }
    pub fn apply(&mut self, edit: OverlayEdit) -> Result<Value, String> {
        self.validate()?;
        // A refusal leaves the caller's registry unchanged.
        let mut next = self.clone();
        let receipt = match edit {
            OverlayEdit::List => json!({}),
            OverlayEdit::Add { layer } => {
                layer.validate()?;
                if next.layers.iter().any(|r| r.id == layer.id) {
                    return Err("overlay id already exists".into());
                }
                let result = json!(layer);
                next.layers.push(layer);
                result
            }
            OverlayEdit::Remove { id } => {
                let n = next.layers.len();
                next.layers.retain(|r| r.id != id);
                if next.layers.len() == n {
                    return Err("overlay was not found".into());
                }
                json!({"layerId":id,"removed":true})
            }
            OverlayEdit::Visibility { id, visible } => {
                let row = next
                    .layers
                    .iter_mut()
                    .find(|r| r.id == id)
                    .ok_or("overlay was not found")?;
                row.visible = visible;
                json!({"layerId":id,"visible":visible})
            }
            OverlayEdit::Import { layers } => {
                if !next.legacy_imported {
                    for layer in layers {
                        layer.validate()?;
                        if !next.layers.iter().any(|r| r.id == layer.id) {
                            next.layers.push(layer)
                        }
                    }
                    next.legacy_imported = true
                }
                json!({"imported":true})
            }
        };
        next.validate()?;
        let changed = next.layers != self.layers || next.legacy_imported != self.legacy_imported;
        if changed {
            next.revision = next
                .revision
                .checked_add(1)
                .ok_or("overlay revision exhausted")?;
        }
        *self = next;
        Ok(
            json!({"layers":self.layers,"revision":self.revision,"receipt":receipt,"changed":changed}),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn row(id: &str) -> Overlay {
        Overlay {
            id: id.into(),
            name: "Base".into(),
            kind: "xyz".into(),
            url: "https://tiles.example/{z}/{x}/{y}.png".into(),
            tile_size: 256,
            attribution: None,
            visible: true,
        }
    }
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn transitions_preserve_other_hosts_and_import_only_once() {
        let mut state = OverlayRegistry::default();
        state.apply(OverlayEdit::Add { layer: row("cli") }).unwrap();
        state
            .apply(OverlayEdit::Import {
                layers: vec![row("desktop")],
            })
            .unwrap();
        assert_eq!(state.layers.len(), 2);
        state
            .apply(OverlayEdit::Remove {
                id: "desktop".into(),
            })
            .unwrap();
        state
            .apply(OverlayEdit::Import {
                layers: vec![row("desktop")],
            })
            .unwrap();
        assert_eq!(state.layers.len(), 1);
        let rev = state.revision;
        state
            .apply(OverlayEdit::Visibility {
                id: "cli".into(),
                visible: true,
            })
            .unwrap();
        assert_eq!(state.revision, rev);
    }
    #[test]
    fn refuses_credentials_and_keeps_state_after_failure() {
        let mut state = OverlayRegistry::default();
        let mut bad = row("bad");
        bad.url = "https://u:secret@host/{z}/{x}/{y}".into();
        assert!(state.apply(OverlayEdit::Add { layer: bad }).is_err());
        assert!(state.layers.is_empty());
    }
}
