//! Local printing proofs retain governed engineering inputs and record the
//! exact source receipt and changed recipes. They can never be published.
use super::*;
use crate::printing::Layout;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source_sheets_sha256: String,
    pub layouts: BTreeMap<String, String>,
}

pub fn with_layouts(source: &InputReceipt, layouts: &[Layout]) -> Result<InputReceipt, String> {
    source.validate()?;
    if source.local_print_recipe.is_some() || layouts.is_empty() || layouts.len() > 32 {
        return Err(
            "A print proof needs 1 through 32 layouts and one original governed receipt".into(),
        );
    }
    let mut sheets: Value = serde_json::from_str(&source.sheets_json).map_err(|e| e.to_string())?;
    let setups = sheets["printing_setups"]
        .as_array_mut()
        .ok_or("The project has no printing setups")?;
    let mut provenance = Provenance {
        source_sheets_sha256: source.sheets_sha256.clone(),
        layouts: BTreeMap::new(),
    };
    for layout in layouts {
        crate::printing::validate(layout)?;
        if provenance.layouts.contains_key(&layout.id) {
            return Err("Duplicate print proof layout".into());
        }
        let matches = setups.iter().filter(|s| s["id"] == layout.id).count();
        if matches != 1 {
            return Err(format!(
                "Proof layout {} must replace one existing project setup",
                layout.id
            ));
        }
        let setup = setups.iter_mut().find(|s| s["id"] == layout.id).unwrap();
        let original: Layout =
            serde_json::from_value(setup["layout"].clone()).map_err(|e| e.to_string())?;
        if original.paper != layout.paper || original.page_mm != layout.page_mm {
            return Err("A local proof must preserve the selected output's paper identity".into());
        }
        let bytes = serde_json::to_vec(layout).map_err(|e| e.to_string())?;
        let digest = sha256_hex(&bytes);
        setup["layout"] = serde_json::to_value(layout).map_err(|e| e.to_string())?;
        setup["revision"] = json!(digest);
        provenance.layouts.insert(layout.id.clone(), digest);
    }
    crate::report_formats::named_layouts(&sheets)?;
    let mut result = source.clone();
    result.sheets_json = serde_json::to_string(&sheets).map_err(|e| e.to_string())?;
    result.sheets_sha256 = sha256_hex(result.sheets_json.as_bytes());
    result.local_print_recipe = Some(provenance);
    result.validate()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_recipes_change_only_existing_print_setups_and_keep_source_provenance() {
        let mut layout = crate::printing::default_layout();
        let sheets = json!({"printing_setups":[{"id":layout.id,"revision":"a".repeat(64),"layout":layout}],"project_settings":[{"parameter":"design_export_format","value":["xlsx"]}],"pole_types":[{"height":9,"material":"steel"}]});
        let bytes = serde_json::to_string(&sheets).unwrap();
        let source = InputReceipt {
            local_print_recipe: None,
            schema: 1,
            country: "Rwanda".into(),
            sheets_sha256: sha256_hex(bytes.as_bytes()),
            sheets_json: bytes,
            reference_semantic_sha256: "a".repeat(64),
        };
        let original = source.clone();
        layout.elements[1].text = "Updated print title".into();
        let proof = with_layouts(&source, &[layout.clone()]).unwrap();
        assert_eq!(source, original);
        assert_eq!(
            proof
                .local_print_recipe
                .as_ref()
                .unwrap()
                .source_sheets_sha256,
            source.sheets_sha256
        );
        assert_ne!(proof.sheets_sha256, source.sheets_sha256);
        let changed: Value = serde_json::from_str(&proof.sheets_json).unwrap();
        assert_eq!(
            crate::report_formats::named_layouts(&changed).unwrap()[&layout.id],
            layout
        );
        assert_eq!(changed["pole_types"], sheets["pole_types"]);
        assert_eq!(changed["project_settings"], sheets["project_settings"]);
        assert_eq!(
            changed["printing_setups"][0]["layout"]["elements"][1]["text"],
            "Updated print title"
        );
        assert!(with_layouts(&proof, &[layout.clone()]).is_err());
        layout.id = "unknown-layout".into();
        assert!(with_layouts(&source, &[layout]).is_err());
        let mut corrupt = source;
        corrupt.sheets_sha256 = "b".repeat(64);
        assert!(with_layouts(&corrupt, &[crate::printing::default_layout()]).is_err());
    }
}
