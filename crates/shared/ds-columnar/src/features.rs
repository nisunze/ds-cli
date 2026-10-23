//! Bounded native feature readback for masks and generation, without IPC.
use crate::schema::{FEATURE_ID_COLUMN, IPC_GEOMETRY_COLUMN, PARQUET_GEOMETRY_COLUMN};
use arrow_array::{
    Array, BinaryArray, BooleanArray, Float64Array, Int64Array, LargeBinaryArray, LargeStringArray,
    RecordBatch, StringArray,
};
use serde_json::{Map, Number, Value, json};
use std::collections::BTreeSet;

/// Preserve the columnar contract's scalar values, nulls, stable identity and
/// complete geometry. Unsupported types and corrupt geometry are errors,
/// never a silently omitted row or a synthesized null. JSON stored as text
/// stays text; this reader does not guess a property's meaning.
pub fn batch_to_features(
    batch: &RecordBatch,
    max_geometry_nodes: usize,
) -> Result<Vec<Value>, String> {
    let schema = batch.schema();
    let mut names = BTreeSet::new();
    for field in schema.fields() {
        if !names.insert(field.name()) {
            return Err("duplicate columnar field name".into());
        }
    }
    let id = schema
        .index_of(FEATURE_ID_COLUMN)
        .map_err(|_| "missing columnar identity column")?;
    let geometry = match (
        schema.index_of(PARQUET_GEOMETRY_COLUMN).ok(),
        schema.index_of(IPC_GEOMETRY_COLUMN).ok(),
    ) {
        (Some(index), None) | (None, Some(index)) => index,
        _ => return Err("missing or ambiguous columnar geometry column".into()),
    };
    use arrow_schema::DataType;
    for (index, field) in schema.fields().iter().enumerate() {
        let admitted = if index == geometry {
            matches!(field.data_type(), DataType::Binary | DataType::LargeBinary)
        } else if index == id {
            matches!(field.data_type(), DataType::Utf8 | DataType::LargeUtf8)
        } else {
            matches!(
                field.data_type(),
                DataType::Utf8
                    | DataType::LargeUtf8
                    | DataType::Int64
                    | DataType::Float64
                    | DataType::Boolean
            )
        };
        if !admitted {
            return Err(format!(
                "unsupported columnar field {}: {}",
                field.name(),
                field.data_type()
            ));
        }
    }
    let mut features = Vec::with_capacity(batch.num_rows());
    for row in 0..batch.num_rows() {
        let identity = text(batch.column(id).as_ref(), row)?;
        let array = batch.column(geometry).as_ref();
        // Validate the type even for nulls, so an empty corrupt geometry cell
        // cannot turn an unsupported schema into an admitted one.
        let bytes = if let Some(values) = array.as_any().downcast_ref::<BinaryArray>() {
            (!values.is_null(row)).then(|| values.value(row))
        } else if let Some(values) = array.as_any().downcast_ref::<LargeBinaryArray>() {
            (!values.is_null(row)).then(|| values.value(row))
        } else {
            return Err("columnar geometry must be binary WKB".into());
        };
        let geometry_value = bytes
            .map(|bytes| ds_geo_lite::wkb::geometry_from_wkb(bytes, max_geometry_nodes))
            .transpose()?
            .unwrap_or(Value::Null);
        let mut properties = Map::new();
        for (column, field) in schema.fields().iter().enumerate() {
            if column != id && column != geometry {
                properties.insert(
                    field.name().clone(),
                    scalar(batch.column(column).as_ref(), row)?,
                );
            }
        }
        features.push(json!({"type":"Feature", "id":identity, "geometry":geometry_value, "properties":properties}));
    }
    Ok(features)
}

fn text(array: &dyn Array, row: usize) -> Result<Value, String> {
    let value = if let Some(values) = array.as_any().downcast_ref::<StringArray>() {
        (!values.is_null(row)).then(|| values.value(row))
    } else if let Some(values) = array.as_any().downcast_ref::<LargeStringArray>() {
        (!values.is_null(row)).then(|| values.value(row))
    } else {
        return Err("columnar text or identity must be Utf8 or LargeUtf8".into());
    };
    Ok(value.map_or(Value::Null, |value| Value::String(value.to_owned())))
}

fn scalar(array: &dyn Array, row: usize) -> Result<Value, String> {
    let any = array.as_any();
    if any.is::<StringArray>() || any.is::<LargeStringArray>() {
        return text(array, row);
    }
    if let Some(values) = any.downcast_ref::<BooleanArray>() {
        return Ok(if values.is_null(row) {
            Value::Null
        } else {
            Value::Bool(values.value(row))
        });
    }
    if let Some(values) = any.downcast_ref::<Int64Array>() {
        return Ok(if values.is_null(row) {
            Value::Null
        } else {
            Value::Number(values.value(row).into())
        });
    }
    if let Some(values) = any.downcast_ref::<Float64Array>() {
        return if values.is_null(row) {
            Ok(Value::Null)
        } else {
            Number::from_f64(values.value(row))
                .map(Value::Number)
                .ok_or("columnar number must be finite".into())
        };
    }
    Err(format!(
        "unsupported columnar property type {}",
        array.data_type()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Layout, batch};
    use arrow_schema::{DataType, Field, Schema};
    use std::sync::Arc;

    #[test]
    fn readback_preserves_native_and_ipc_layouts_and_nulls() {
        let features = vec![
            json!({"type":"Feature","id":"pole-1","geometry":{"type":"Polygon","coordinates":[[[0.0,0.0],[3.0,0.0],[3.0,3.0],[0.0,0.0]],[[1.0,1.0],[2.0,1.0],[2.0,2.0],[1.0,1.0]]]},"properties":{"text":"[1,2]","n":5,"f":1.25,"b":true,"empty":null}}),
        ];
        for layout in [Layout::GeoParquet, Layout::ArrowIpc] {
            let built = batch::build(&features, &[], layout).unwrap();
            assert_eq!(batch_to_features(&built.batch, 100).unwrap(), features);
            assert!(batch_to_features(&built.batch, 2).is_err());
        }
    }

    #[test]
    fn corrupt_geometry_and_nonfinite_properties_refuse_the_batch() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(FEATURE_ID_COLUMN, DataType::Utf8, true),
            Field::new(PARQUET_GEOMETRY_COLUMN, DataType::Binary, true),
            Field::new("n", DataType::Float64, true),
        ]));
        let ids = Arc::new(StringArray::from(vec!["id"]));
        let geometry =
            ds_geo_lite::wkb::geometry_to_wkb(&json!({"type":"Point","coordinates":[1.0,2.0]}))
                .unwrap();
        let valid = Arc::new(BinaryArray::from(vec![geometry.as_slice()]));
        let make = |geometry, n| {
            RecordBatch::try_new(
                schema.clone(),
                vec![ids.clone(), geometry, Arc::new(Float64Array::from(vec![n]))],
            )
            .unwrap()
        };
        assert!(batch_to_features(&make(valid.clone(), f64::INFINITY), 10).is_err());
        assert!(
            batch_to_features(
                &make(Arc::new(BinaryArray::from(vec![&b"invalid"[..]])), 1.0),
                10
            )
            .is_err()
        );
        assert!(batch_to_features(&make(valid, 1.0), 10).is_ok());
    }

    #[test]
    fn an_empty_batch_cannot_hide_an_unsupported_schema() {
        let schema = Arc::new(Schema::new(vec![
            Field::new(FEATURE_ID_COLUMN, DataType::Int64, true),
            Field::new(PARQUET_GEOMETRY_COLUMN, DataType::Binary, true),
        ]));
        let batch = RecordBatch::new_empty(schema);
        assert!(
            batch_to_features(&batch, 10)
                .unwrap_err()
                .contains(FEATURE_ID_COLUMN)
        );
    }
}
