//! Pure authored-layout admission shared by native validation and Reporter.
use super::Layout;

pub fn validate_heading_encoding(layout: &Layout) -> Result<(), String> {
    for (element_index, element) in layout.elements.iter().enumerate() {
        let Some(table) = &element.table else {
            continue;
        };
        for (heading_index, heading) in table.headings.iter().enumerate() {
            // A degree sign encoded as UTF-8 and then read as Latin-1 appears
            // as Â°. Repeating that mistake produces Ã‚Â°. Both are authored
            // text errors: silently changing a saved layout would conceal the
            // correction from the project owner.
            if heading.contains("Â°") {
                let corrected = heading.replace("Ã‚Â°", "(deg)").replace("Â°", "(deg)");
                return Err(format!(
                    "print layout element `{}` source `elements[{element_index}].table.headings[{heading_index}]` contains likely UTF-8 mojibake; edit this heading to `{corrected}`",
                    element.id
                ));
            }
        }
    }
    Ok(())
}

pub fn validate_fit_around_references(layout: &Layout) -> Result<(), String> {
    for element in &layout.elements {
        let Some(viewport) = &element.viewport else {
            continue;
        };
        for id in &viewport.fit_around {
            if !layout.elements.iter().any(|candidate| candidate.id == *id) {
                return Err(format!(
                    "print layout map `{}` fit_around references missing element `{id}`",
                    element.id
                ));
            }
        }
    }
    Ok(())
}
