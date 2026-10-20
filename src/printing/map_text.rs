//! The words one map sheet prints, decided once for every host.
//!
//! A layout names its sheet text as `{key}` bindings. Its authored
//! `title_block` holds the project's own values; the sheet's identity fills
//! what the project left out: the sealed project caption, the layout's name as
//! title, the paper and page as drawing, the capture date and whether every
//! context source was held. A key nobody can bind refuses before any source
//! is acquired, never at render time.

use std::collections::BTreeMap;

use super::{Layout, Orientation, Paper};

/// Keys the renderer binds itself from the page it draws.
const RENDERER_KEYS: [&str; 2] = ["scale", "admin_subtitle"];

/// The identity of one sheet, as the host captured it.
pub struct Sheet<'a> {
    /// Canonical caption from the sealed printing receipt, never the id;
    /// without one, a layout printing `{project}` must author it.
    pub caption: Option<&'a str>,
    /// Reporting country of the same receipt.
    pub country: &'a str,
    /// Capture date, `YYYY-MM-DD`, from the host clock.
    pub date: &'a str,
    /// One-based page and page count.
    pub page: (usize, usize),
    /// Some requested context source was not held.
    pub incomplete: bool,
}

/// Every `{key}` the layout's text elements name, in first-use order.
pub fn keys(layout: &Layout) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for element in &layout.elements {
        let mut rest = element.text.as_str();
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start + 1..].find('}') else {
                break;
            };
            let key = &rest[start + 1..start + 1 + end];
            if !key.is_empty() && !keys.iter().any(|seen| seen == key) {
                keys.push(key.to_owned());
            }
            rest = &rest[start + 2 + end..];
        }
    }
    keys
}

fn paper(layout: &Layout) -> String {
    let size = match layout.paper.size {
        Paper::A0 => "A0",
        Paper::A1 => "A1",
        Paper::A2 => "A2",
        Paper::A3 => "A3",
        Paper::A4 => "A4",
        Paper::A5 => "A5",
        Paper::Custom { .. } => "Custom",
    };
    let orientation = match layout.paper.orientation {
        Orientation::Landscape => "landscape",
        Orientation::Portrait => "portrait",
    };
    format!("{size} {orientation}")
}

/// The complete text bindings of one sheet, or the first key that has none.
pub fn bindings(layout: &Layout, sheet: &Sheet) -> Result<BTreeMap<String, String>, String> {
    let mut text = layout.title_block.clone();
    let source = if sheet.incomplete {
        "INCOMPLETE INPUTS — see the omitted context in the capture receipt".to_owned()
    } else {
        format!("Project designs and governed {} context", sheet.country)
    };
    if let Some(caption) = sheet.caption {
        text.entry("project".into())
            .or_insert_with(|| caption.to_owned());
    }
    for (key, value) in [
        ("title", layout.name.clone()),
        ("transformer", layout.name.clone()),
        (
            "drawing",
            format!("{} · {}/{}", paper(layout), sheet.page.0, sheet.page.1),
        ),
        ("date", sheet.date.to_owned()),
        ("source", source),
    ] {
        text.entry(key.into()).or_insert(value);
    }
    if let Some(key) = keys(layout)
        .into_iter()
        .find(|key| !text.contains_key(key) && !RENDERER_KEYS.contains(&key.as_str()))
    {
        return Err(format!(
            "print_text_missing: layout `{}` prints {{{key}}}, which neither its title_block nor the sheet binds; author it in the layout's title_block",
            layout.id
        ));
    }
    Ok(text)
}

#[cfg(test)]
#[path = "map_text_tests.rs"]
mod tests;
