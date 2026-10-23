//! Arrow IPC chunks for the browser working set.
//!
//! `docs/upload-idb-streaming-contract.md` is a hard boundary: this is a
//! re-implementation of an existing wire format, not a new one. The column
//! names, order, types and nullability match what `ds-cleaning::arrow_ipc`
//! produced through Polars — the point of the change is that a Tauri shell no
//! longer links a dataframe engine to serialize bytes.

use crate::batch;
use crate::batch::Layout;
use crate::schema::IPC_GEOMETRY_COLUMN;
use arrow_array::RecordBatch;
use arrow_array::{
    Array, BooleanArray, Float64Array, Int64Array, LargeBinaryArray, LargeStringArray, StringArray,
};
use arrow_ipc::reader::FileReader;
use arrow_ipc::writer::FileWriter;
use serde_json::{Map, Number, Value};
use std::io::Cursor;

/// Feature JSON to Arrow IPC file bytes, the format the upload workspace
/// stores and the pooled-WASM table reads.
pub fn features_to_ipc(features: &[Value], columns_hint: &[String]) -> Result<Vec<u8>, String> {
    let built = batch::build(features, columns_hint, Layout::ArrowIpc)?;
    batch_to_ipc(&built.batch)
}

pub fn batch_to_ipc(batch: &RecordBatch) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    let mut writer = FileWriter::try_new(&mut bytes, &batch.schema())
        .map_err(|error| format!("could not open the Arrow IPC writer: {error}"))?;
    writer
        .write(batch)
        .map_err(|error| format!("could not write the Arrow IPC batch: {error}"))?;
    writer
        .finish()
        .map_err(|error| format!("could not finish the Arrow IPC file: {error}"))?;
    drop(writer);
    Ok(bytes)
}

/// Read IPC back to record batches. Used by tests and by any caller that needs
/// to re-read a chunk it wrote; the browser reads these directly.
pub fn ipc_to_batches(bytes: &[u8]) -> Result<Vec<RecordBatch>, String> {
    let reader = FileReader::try_new(Cursor::new(bytes), None)
        .map_err(|error| format!("could not open the Arrow IPC file: {error}"))?;
    reader
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not read the Arrow IPC file: {error}"))
}

/// Property rows out of an IPC chunk, one JSON object per feature.
///
/// The geometry and identity columns are omitted: this is the tabular view a
/// caller wants when inspecting what a chunk carried. Reading geometry back is
/// a WKB decode nothing currently needs, so it is deliberately absent rather
/// than half-implemented.
pub fn ipc_to_rows(bytes: &[u8]) -> Result<Vec<Map<String, Value>>, String> {
    let mut rows = Vec::new();
    for batch in ipc_to_batches(bytes)? {
        let schema = batch.schema();
        let mut names = std::collections::BTreeSet::new();
        for field in schema.fields() {
            if !names.insert(field.name()) {
                return Err(format!("ambiguous duplicate Arrow column {}", field.name()));
            }
        }
        for index in 0..batch.num_rows() {
            let mut row = Map::new();
            for (column, field) in schema.fields().iter().enumerate() {
                let name = field.name();
                if name == crate::schema::FEATURE_ID_COLUMN || name == IPC_GEOMETRY_COLUMN {
                    continue;
                }
                row.insert(
                    name.clone(),
                    cell(batch.column(column), index)
                        .map_err(|error| format!("column {name}: {error}"))?,
                );
            }
            rows.push(row);
        }
    }
    Ok(rows)
}

/// The WKB geometry of every feature in an IPC chunk, in row order.
pub fn ipc_to_geometry_wkb(bytes: &[u8]) -> Result<Vec<Option<Vec<u8>>>, String> {
    let mut out = Vec::new();
    for batch in ipc_to_batches(bytes)? {
        let index = batch
            .schema()
            .index_of(IPC_GEOMETRY_COLUMN)
            .map_err(|error| format!("chunk has no geometry column: {error}"))?;
        let column = batch.column(index);
        let values = column
            .as_any()
            .downcast_ref::<LargeBinaryArray>()
            .ok_or("geometry column is not LargeBinary")?;
        for row in 0..values.len() {
            out.push((!values.is_null(row)).then(|| values.value(row).to_vec()));
        }
    }
    Ok(out)
}

fn cell(array: &dyn Array, row: usize) -> Result<Value, String> {
    if array.is_null(row) {
        return Ok(Value::Null);
    }
    let any = array.as_any();
    if let Some(values) = any.downcast_ref::<StringArray>() {
        return Ok(Value::String(values.value(row).to_string()));
    }
    if let Some(values) = any.downcast_ref::<LargeStringArray>() {
        return Ok(Value::String(values.value(row).to_string()));
    }
    if let Some(values) = any.downcast_ref::<Int64Array>() {
        return Ok(Value::Number(Number::from(values.value(row))));
    }
    if let Some(values) = any.downcast_ref::<Float64Array>() {
        return Number::from_f64(values.value(row))
            .map(Value::Number)
            .ok_or_else(|| "non-finite numeric property".to_string());
    }
    if let Some(values) = any.downcast_ref::<BooleanArray>() {
        return Ok(Value::Bool(values.value(row)));
    }
    if let Some(values) = any.downcast_ref::<LargeBinaryArray>() {
        return Ok(Value::String(format!("{} bytes", values.value(row).len())));
    }
    Err(format!(
        "unsupported Arrow property type {}",
        array.data_type()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::FEATURE_ID_COLUMN;
    use serde_json::json;

    #[test]
    fn the_chunk_carries_the_contract_columns_in_the_contract_order() {
        let features = vec![json!({
            "id": "f1",
            "geometry": {"type": "Point", "coordinates": [30.06, -1.94]},
            "properties": {"name": "Kigali", "pop": 1200000}
        })];
        let bytes = features_to_ipc(&features, &[]).unwrap();
        let batches = ipc_to_batches(&bytes).unwrap();
        assert_eq!(batches.len(), 1);
        let schema = batches[0].schema();
        let names: Vec<&str> = schema
            .fields()
            .iter()
            .map(|field| field.name().as_str())
            .collect();
        assert_eq!(
            names,
            vec![FEATURE_ID_COLUMN, IPC_GEOMETRY_COLUMN, "name", "pop"],
            "the browser streaming contract fixes these names and this order"
        );
        assert_eq!(batches[0].num_rows(), 1);
    }

    #[test]
    fn an_empty_batch_round_trips_rather_than_erroring() {
        let bytes = features_to_ipc(&[], &[]).unwrap();
        let batches = ipc_to_batches(&bytes).unwrap();
        assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), 0);
    }

    #[test]
    fn rows_read_back_the_properties_a_chunk_carried() {
        let features = vec![
            json!({"geometry": {"type":"Point","coordinates":[30.0,-2.0]},
                   "properties": {"name": "Kigali", "pop": 12, "area": 1.5, "capital": true}}),
            json!({"properties": {"name": null, "pop": 7, "area": null, "capital": false}}),
        ];
        let rows = ipc_to_rows(&features_to_ipc(&features, &[]).unwrap()).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], json!("Kigali"));
        assert_eq!(rows[0]["pop"], json!(12));
        assert_eq!(rows[0]["area"], json!(1.5));
        assert_eq!(rows[0]["capital"], json!(true));
        assert_eq!(
            rows[1]["name"],
            Value::Null,
            "a null stays a null, not an empty string"
        );
        assert!(
            !rows[0].contains_key(IPC_GEOMETRY_COLUMN),
            "geometry is not a property"
        );
        assert!(
            !rows[0].contains_key(FEATURE_ID_COLUMN),
            "identity is not a property"
        );
    }

    #[test]
    fn the_column_hint_survives_the_round_trip() {
        let features = vec![json!({"properties": {"b": 1, "a": 2}})];
        let hint = vec!["a".to_string(), "b".to_string()];
        let bytes = features_to_ipc(&features, &hint).unwrap();
        let batches = ipc_to_batches(&bytes).unwrap();
        let schema = batches[0].schema();
        let names: Vec<&str> = schema
            .fields()
            .iter()
            .map(|field| field.name().as_str())
            .collect();
        assert_eq!(
            names[2..],
            ["a", "b"],
            "hint order wins over discovery order"
        );
    }
}
