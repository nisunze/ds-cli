//! Exact print-only binding to a staking workbook. This never edits a model.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.mv-structure-descriptions/v1";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Column {
    /// Exact spreadsheet column letter, within A..AN.
    pub column: String,
    /// Exact source header, not a guessed alias.
    pub header: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Columns {
    pub engineering_number: Column,
    pub engineering_name: Column,
    pub x_m: Column,
    pub y_m: Column,
    pub description: Column,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub schema: String,
    pub project_id: String,
    pub model_revision: String,
    /// Held source path; the native host reads its bytes once.
    pub workbook_path: String,
    pub workbook_sha256: String,
    pub sheet: String,
    /// One-based source header row.
    pub header_row: usize,
    /// Declared coordinate-rounding tolerance, bounded to 0..0.05 metres.
    pub xy_tolerance_m: f64,
    pub columns: Columns,
}

#[derive(Debug, Clone)]
pub struct Identity {
    pub entity_id: String,
    pub engineering_number: String,
    pub engineering_name: String,
    pub x_m: f64,
    pub y_m: f64,
}

fn column_index(column: &Column) -> Result<usize, String> {
    if column.header.is_empty()
        || column.header.len() > 200
        || column.column.is_empty()
        || column.column.len() > 2
        || !column.column.bytes().all(|b| b.is_ascii_uppercase())
    {
        return Err("description source needs exact column letters and nonempty headers".into());
    }
    let index = column
        .column
        .bytes()
        .fold(0usize, |a, b| a * 26 + usize::from(b - b'A' + 1))
        - 1;
    if index >= 40 {
        return Err("description source columns must be within A..AN".into());
    }
    Ok(index)
}

/// All placed identities must match number, native name and both coordinates.
/// Every source row is accounted for; duplicate/extra/missing rows refuse.
pub fn bind(
    binding: &Binding,
    workbook: &[u8],
    project: &str,
    revision: &str,
    identities: &[Identity],
) -> Result<BTreeMap<String, String>, String> {
    if binding.schema != SCHEMA
        || binding.project_id != project
        || binding.model_revision != revision
    {
        return Err(
            "description source project, model revision or schema differs from the print".into(),
        );
    }
    if binding.workbook_sha256 != crate::report_export::sha256_hex(workbook) {
        return Err("description workbook bytes differ from the pinned SHA-256".into());
    }
    if !(1..=100).contains(&binding.header_row)
        || binding.sheet.is_empty()
        || binding.sheet.len() > 100
        || !binding.xy_tolerance_m.is_finite()
        || !(0.0..=0.05).contains(&binding.xy_tolerance_m)
        || identities.is_empty()
        || identities.len() > 19_900
    {
        return Err("description source exceeds its header, tolerance or identity bounds".into());
    }
    let cols = [
        &binding.columns.engineering_number,
        &binding.columns.engineering_name,
        &binding.columns.x_m,
        &binding.columns.y_m,
        &binding.columns.description,
    ];
    let indices = cols
        .map(column_index)
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    if indices.iter().collect::<BTreeSet<_>>().len() != 5 {
        return Err("description source columns overlap".into());
    }
    let rows = crate::assets::preview::xlsx::complete_sheet(workbook, &binding.sheet, 20_000)?;
    let header = rows
        .get(binding.header_row - 1)
        .ok_or("description source header row is absent")?;
    for (col, index) in cols.iter().zip(&indices) {
        if header.get(*index) != Some(&col.header) {
            return Err(format!(
                "description source {} header differs from {}",
                col.column, col.header
            ));
        }
    }
    let mut targets = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for identity in identities {
        if identity.entity_id.is_empty()
            || identity.engineering_number.is_empty()
            || identity.engineering_name.is_empty()
            || !identity.x_m.is_finite()
            || !identity.y_m.is_finite()
            || !ids.insert(&identity.entity_id)
            || targets
                .insert(identity.engineering_number.clone(), identity)
                .is_some()
        {
            return Err(
                "description source requires distinct exact placed identities and numbers".into(),
            );
        }
    }
    let mut result = BTreeMap::new();
    for (offset, row) in rows.iter().enumerate().skip(binding.header_row) {
        let cell = |index: usize| row.get(index).map_or("", String::as_str);
        if indices.iter().all(|i| cell(*i).is_empty()) {
            continue;
        }
        let number = cell(indices[0]);
        let identity = targets.get(number).ok_or_else(|| {
            format!(
                "description source row {} has an unplaced number",
                offset + 1
            )
        })?;
        let coordinate = |index: usize| cell(index).parse::<f64>().ok().filter(|v| v.is_finite());
        if cell(indices[1]) != identity.engineering_name
            || coordinate(indices[2])
                .is_none_or(|x| (x - identity.x_m).abs() > binding.xy_tolerance_m)
            || coordinate(indices[3])
                .is_none_or(|y| (y - identity.y_m).abs() > binding.xy_tolerance_m)
        {
            return Err(format!(
                "description source row {} differs from placed name/coordinates",
                offset + 1
            ));
        }
        if result
            .insert(identity.entity_id.clone(), cell(indices[4]).into())
            .is_some()
        {
            return Err(format!("description source repeats structure {number}"));
        }
    }
    if result.len() != identities.len() {
        return Err("description source does not cover every placed structure".into());
    }
    Ok(result)
}

pub fn schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Binding)).expect("description binding schema")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    fn workbook(description: &str, extra: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name,body) in [
            ("xl/workbook.xml",r#"<workbook xmlns:r="r"><sheets><sheet name="Staking table" r:id="r1"/></sheets></workbook>"#.to_string()),
            ("xl/_rels/workbook.xml.rels",r#"<Relationships><Relationship Id="r1" Target="worksheets/sheet1.xml"/></Relationships>"#.to_string()),
            ("xl/worksheets/sheet1.xml",format!(r#"<worksheet><sheetData><row r="2"><c r="A2" t="inlineStr"><is><t>Number</t></is></c><c r="B2" t="inlineStr"><is><t>Name</t></is></c><c r="C2" t="inlineStr"><is><t>X</t></is></c><c r="D2" t="inlineStr"><is><t>Y</t></is></c><c r="E2" t="inlineStr"><is><t>Structure Description</t></is></c></row><row r="3"><c r="A3"><v>355</v></c><c r="B3" t="inlineStr"><is><t>j-w-toff-S325.014</t></is></c><c r="C3"><v>500000.00</v></c><c r="D3"><v>4700000.01</v></c><c r="E3" t="inlineStr"><is><t>{description}</t></is></c></row>{extra}</sheetData></worksheet>"#)),
        ] { zip.start_file(name,zip::write::SimpleFileOptions::default()).unwrap();zip.write_all(body.as_bytes()).unwrap(); }
        zip.finish().unwrap().into_inner()
    }
    fn binding(bytes: &[u8]) -> Binding {
        let col = |column: &str, header: &str| Column {
            column: column.into(),
            header: header.into(),
        };
        Binding {
            schema: SCHEMA.into(),
            project_id: "p".into(),
            model_revision: "rev-1".into(),
            workbook_path: "staking.xlsx".into(),
            workbook_sha256: crate::report_export::sha256_hex(bytes),
            sheet: "Staking table".into(),
            header_row: 2,
            xy_tolerance_m: 0.011,
            columns: Columns {
                engineering_number: col("A", "Number"),
                engineering_name: col("B", "Name"),
                x_m: col("C", "X"),
                y_m: col("D", "Y"),
                description: col("E", "Structure Description"),
            },
        }
    }
    fn identities() -> Vec<Identity> {
        vec![Identity {
            entity_id: "str-355".into(),
            engineering_number: "355".into(),
            engineering_name: "j-w-toff-S325.014".into(),
            x_m: 500000.,
            y_m: 4700000.,
        }]
    }
    #[test]
    fn cleaned_source_column_and_intentionally_blank_source_override_library_fallback() {
        let cleaned = "STRUCTURE TYPE J: H-POLE STRUCTURE - DEVIATION LESS THAN 60°";
        for description in [cleaned, ""] {
            let bytes = workbook(description, "");
            let map = bind(&binding(&bytes), &bytes, "p", "rev-1", &identities()).unwrap();
            assert_eq!(map["str-355"], description);
        }
    }
    #[test]
    fn source_scope_bytes_name_coordinate_and_header_are_all_required() {
        let bytes = workbook("Exact source wording", "");
        let b = binding(&bytes);
        assert!(bind(&b, &bytes, "other", "rev-1", &identities())
            .unwrap_err()
            .contains("project"));
        assert!(bind(&b, &bytes, "p", "rev-2", &identities())
            .unwrap_err()
            .contains("revision"));
        assert!(bind(
            &b,
            &workbook("Changed source", ""),
            "p",
            "rev-1",
            &identities()
        )
        .unwrap_err()
        .contains("SHA"));
        let mut ids = identities();
        ids[0].engineering_name = "another.012".into();
        assert!(bind(&b, &bytes, "p", "rev-1", &ids)
            .unwrap_err()
            .contains("name/coordinates"));
        ids = identities();
        ids[0].x_m += 0.1;
        assert!(bind(&b, &bytes, "p", "rev-1", &ids)
            .unwrap_err()
            .contains("name/coordinates"));
        let mut wrong = b;
        wrong.columns.description.header = "Description".into();
        assert!(bind(&wrong, &bytes, "p", "rev-1", &identities())
            .unwrap_err()
            .contains("header"));
    }
    #[test]
    fn ambiguous_incomplete_or_truncated_workbooks_never_supply_descriptions() {
        for (desc, extra) in [
            (
                "Exact",
                r#"<row r="4"><c r="A4"><v>355</v></c><c r="B4" t="inlineStr"><is><t>j-w-toff-S325.014</t></is></c><c r="C4"><v>500000</v></c><c r="D4"><v>4700000</v></c></row>"#,
            ),
            (
                "Exact",
                r#"<row r="4"><c r="E4" t="s"><v>999</v></c></row>"#,
            ),
        ] {
            let bytes = workbook(desc, extra);
            assert!(bind(&binding(&bytes), &bytes, "p", "rev-1", &identities()).is_err());
        }
        let bytes = workbook(&"W".repeat(501), "");
        assert!(bind(&binding(&bytes), &bytes, "p", "rev-1", &identities())
            .unwrap_err()
            .contains("truncated"));
        let bytes = workbook("Exact", "");
        let mut ids = identities();
        ids.push(Identity {
            entity_id: "str-356".into(),
            engineering_number: "356".into(),
            ..ids[0].clone()
        });
        assert!(bind(&binding(&bytes), &bytes, "p", "rev-1", &ids)
            .unwrap_err()
            .contains("every placed"));
    }
}
