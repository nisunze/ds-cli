//! GeoJSON features to one Arrow `RecordBatch`.
//!
//! Both output formats are written from the batch this module builds, so the
//! two cannot disagree about column order, types or null placement. Only the
//! geometry column's *name* differs between them (§2 of the contract), which
//! is why it is a parameter rather than two builders.

use crate::schema::{
    FEATURE_ID_COLUMN, PropertyKind, infer_property_kinds, ordered_columns, property, value_as_i64,
    value_to_text,
};
use crate::wkb::geometry_to_wkb;
use arrow_array::{
    ArrayRef, BinaryArray, BooleanArray, Float64Array, Int64Array, LargeBinaryArray,
    LargeStringArray, RecordBatch, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Which variable-width Arrow types a batch uses.
///
/// This is not a style preference — the two output formats genuinely differ:
///
/// - **Arrow IPC** must use 64-bit offsets (`LargeUtf8` / `LargeBinary`),
///   because that is what Polars wrote and what the browser streaming contract
///   already decodes. Emitting 32-bit offsets here would be a silent wire
///   change to a live consumer.
/// - **GeoParquet** uses 32-bit offsets (`Utf8` / `Binary`), the width every
///   Parquet reader handles. Both widths land in the same `BYTE_ARRAY` on
///   disk, so there is nothing to gain by being exotic.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Layout {
    /// `__ds_geometry_wkb`, 64-bit offsets — the browser working set.
    ArrowIpc,
    /// `geometry`, 32-bit offsets — the analytical artifact.
    GeoParquet,
}

impl Layout {
    pub fn geometry_column(self) -> &'static str {
        match self {
            Layout::ArrowIpc => crate::schema::IPC_GEOMETRY_COLUMN,
            Layout::GeoParquet => crate::schema::PARQUET_GEOMETRY_COLUMN,
        }
    }

    fn large_offsets(self) -> bool {
        matches!(self, Layout::ArrowIpc)
    }

    fn text_type(self) -> DataType {
        if self.large_offsets() {
            DataType::LargeUtf8
        } else {
            DataType::Utf8
        }
    }

    fn binary_type(self) -> DataType {
        if self.large_offsets() {
            DataType::LargeBinary
        } else {
            DataType::Binary
        }
    }
}

/// The batch plus the raw geometry bytes, which GeoParquet needs again for its
/// `bbox` metadata and would otherwise have to decode back out of the array.
pub struct FeatureBatch {
    pub batch: RecordBatch,
    pub geometries: Vec<Option<Vec<u8>>>,
    pub columns: Vec<String>,
}

pub fn build(
    features: &[Value],
    columns_hint: &[String],
    layout: Layout,
) -> Result<FeatureBatch, String> {
    let columns = ordered_columns(features, columns_hint);
    let kinds = infer_property_kinds(features, &columns);
    build_with_kinds(features, &columns, &kinds, layout)
}

/// Build against an ALREADY DECIDED column list and type ladder.
///
/// A streaming conversion writes many batches into one file, and a Parquet file
/// has exactly one schema. The types therefore cannot be re-inferred per batch:
/// they are pinned once and every later batch is built against them. Deciding
/// whether a batch actually fits those types is the caller's job — writing a
/// value that does not fit would be the silent loss this split exists to
/// prevent, so nothing here guesses.
pub fn build_with_kinds(
    features: &[Value],
    columns: &[String],
    kinds: &BTreeMap<String, PropertyKind>,
    layout: Layout,
) -> Result<FeatureBatch, String> {
    let columns = columns.to_vec();
    let ids: Vec<Option<String>> = features.iter().map(feature_id).collect();
    let geometries: Vec<Option<Vec<u8>>> = features
        .iter()
        .map(|feature| feature.get("geometry").and_then(geometry_to_wkb))
        .collect();

    let mut fields = Vec::with_capacity(columns.len() + 2);
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(columns.len() + 2);

    let text = |values: &[Option<String>]| -> ArrayRef {
        if layout.large_offsets() {
            Arc::new(
                values
                    .iter()
                    .map(Option::as_deref)
                    .collect::<LargeStringArray>(),
            )
        } else {
            Arc::new(values.iter().map(Option::as_deref).collect::<StringArray>())
        }
    };

    fields.push(Field::new(FEATURE_ID_COLUMN, layout.text_type(), true));
    arrays.push(text(&ids));

    fields.push(Field::new(
        layout.geometry_column(),
        layout.binary_type(),
        true,
    ));
    arrays.push(if layout.large_offsets() {
        Arc::new(
            geometries
                .iter()
                .map(|value| value.as_deref())
                .collect::<LargeBinaryArray>(),
        ) as ArrayRef
    } else {
        Arc::new(
            geometries
                .iter()
                .map(|value| value.as_deref())
                .collect::<BinaryArray>(),
        ) as ArrayRef
    });

    for column in &columns {
        // An all-null column is Utf8, never a null type: too many readers
        // reject or silently retype one. See contract §3.
        let kind = kinds.get(column).copied().unwrap_or(PropertyKind::Text);
        let (field, array): (Field, ArrayRef) = match kind {
            PropertyKind::Boolean => (
                Field::new(column, DataType::Boolean, true),
                Arc::new(
                    features
                        .iter()
                        .map(|feature| property(feature, column).and_then(Value::as_bool))
                        .collect::<BooleanArray>(),
                ),
            ),
            PropertyKind::Integer => (
                Field::new(column, DataType::Int64, true),
                Arc::new(
                    features
                        .iter()
                        .map(|feature| property(feature, column).and_then(value_as_i64))
                        .collect::<Int64Array>(),
                ),
            ),
            PropertyKind::Number => (
                Field::new(column, DataType::Float64, true),
                Arc::new(
                    features
                        .iter()
                        .map(|feature| property(feature, column).and_then(Value::as_f64))
                        .collect::<Float64Array>(),
                ),
            ),
            PropertyKind::Null | PropertyKind::Text => {
                let values: Vec<Option<String>> = features
                    .iter()
                    .map(|feature| property(feature, column).map(value_to_text))
                    .collect();
                (Field::new(column, layout.text_type(), true), text(&values))
            }
        };
        fields.push(field);
        arrays.push(array);
    }

    let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), arrays)
        .map_err(|error| format!("could not assemble the feature batch: {error}"))?;
    Ok(FeatureBatch {
        batch,
        geometries,
        columns,
    })
}

/// Feature identity, taken from the top-level `id` or from a property of that
/// name. Absent identity is null, never a synthesized value — a fabricated id
/// would look stable across re-imports without being so.
fn feature_id(feature: &Value) -> Option<String> {
    let direct = feature.get("id").filter(|value| !value.is_null());
    let indirect = || property(feature, "id");
    direct.or_else(indirect).map(|value| match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use arrow_array::Array;
    use serde_json::json;

    fn sample() -> Vec<Value> {
        vec![
            json!({"id":"a","geometry":{"type":"Point","coordinates":[30.0,-2.0]},
                   "properties":{"n":1,"t":"x","f":1.5,"b":true}}),
            json!({"geometry":null,"properties":{"n":2,"t":null,"f":2.0,"b":false}}),
        ]
    }

    #[test]
    fn identity_and_geometry_lead_then_properties_in_order() {
        let built = build(&sample(), &[], Layout::GeoParquet).unwrap();
        let schema = built.batch.schema();
        let names: Vec<&str> = schema
            .fields()
            .iter()
            .map(|field| field.name().as_str())
            .collect();
        // Properties follow in discovery order: the JSON object's own order,
        // alphabetical without serde_json's `preserve_order` and source order
        // with it (Core links it). Source order reaches the file either way
        // through the column hint — see the ordering test below.
        let features = sample();
        let mut expected = vec![FEATURE_ID_COLUMN, "geometry"];
        expected.extend(
            features[0]["properties"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str),
        );
        assert_eq!(names, expected);
    }

    #[test]
    fn every_column_is_nullable_and_absence_is_null() {
        let built = build(&sample(), &[], Layout::GeoParquet).unwrap();
        assert!(
            built
                .batch
                .schema()
                .fields()
                .iter()
                .all(|f| f.is_nullable())
        );
        // row 1 has no id and no geometry; row 1's `t` is an explicit null
        let schema = built.batch.schema();
        let t = schema.index_of("t").unwrap();
        assert!(built.batch.column(0).is_null(1), "absent id is null");
        assert!(built.batch.column(1).is_null(1), "absent geometry is null");
        assert!(
            built.batch.column(t).is_null(1),
            "explicit JSON null is null"
        );
    }

    #[test]
    fn types_follow_the_shared_ladder() {
        let built = build(&sample(), &[], Layout::GeoParquet).unwrap();
        let schema = built.batch.schema();
        let kind = |name: &str| {
            schema
                .field(schema.index_of(name).unwrap())
                .data_type()
                .clone()
        };
        assert_eq!(kind("n"), DataType::Int64);
        assert_eq!(kind("t"), DataType::Utf8);
        assert_eq!(kind("f"), DataType::Float64);
        assert_eq!(kind("b"), DataType::Boolean);
    }

    #[test]
    fn an_empty_batch_still_produces_the_two_fixed_columns() {
        let built = build(&[], &[], Layout::GeoParquet).unwrap();
        assert_eq!(built.batch.num_rows(), 0);
        assert_eq!(built.batch.num_columns(), 2);
    }
}
