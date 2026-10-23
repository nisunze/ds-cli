//! GeoPackage geometry binary interpretation shared by every DS host: the
//! browser Kernel, the native reader in `ds-io` and the CLI inspectors.
//!
//! SQLite/file access remains a host concern. This module owns the GeoPackage
//! header and WKB semantics once a host has supplied one geometry cell.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(hashmap_as_object, missing_as_null)
)]
pub struct GpkgGeometryHeader {
    pub version: u8,
    pub flags: u8,
    pub empty: bool,
    pub extended: bool,
    pub little_endian: bool,
    pub srs_id: i32,
    pub envelope_code: u8,
    pub wkb_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(hashmap_as_object, missing_as_null)
)]
pub struct GpkgGeometry {
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "GeoJSON.Geometry | null")
    )]
    pub geometry: Option<Value>,
    pub header: Option<GpkgGeometryHeader>,
    pub bare_wkb: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(hashmap_as_object, missing_as_null)
)]
pub struct GpkgLayerPolicyInput {
    pub data_type: String,
    #[serde(default)]
    pub geometry_column: Option<String>,
    #[serde(default)]
    pub geometry_type: Option<String>,
    #[serde(default)]
    pub srs_id: Option<i32>,
    #[serde(default)]
    pub srs_organization: Option<String>,
    #[serde(default)]
    pub srs_organization_coord_sys_id: Option<i32>,
    #[serde(default)]
    pub row_count: Option<usize>,
    pub max_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    derive(tsify::Tsify)
)]
#[cfg_attr(
    all(feature = "bindings", target_arch = "wasm32"),
    tsify(hashmap_as_object, missing_as_null)
)]
pub struct GpkgLayerPolicy {
    /// One of the four GeoPackage content kinds this policy recognizes.
    #[cfg_attr(
        all(feature = "bindings", target_arch = "wasm32"),
        tsify(type = "\"features\" | \"tiles\" | \"attributes\" | \"unknown\"")
    )]
    pub data_type: String,
    pub epsg: Option<i32>,
    pub readable: bool,
    pub reason: Option<String>,
    pub fallback_reason: Option<String>,
}

/// Interpret one host-extracted GeoPackage catalog row. SQLite access is host
/// I/O; feature-table, CRS, geometry and row-bound policy is shared Rust.
pub fn gpkg_layer_policy(input: &GpkgLayerPolicyInput) -> GpkgLayerPolicy {
    let data_type = match input.data_type.trim().to_ascii_lowercase().as_str() {
        "features" => "features",
        "tiles" => "tiles",
        "attributes" => "attributes",
        _ => "unknown",
    }
    .to_string();
    let organization = input
        .srs_organization
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_uppercase);
    let epsg = if organization.as_deref() == Some("EPSG") {
        input.srs_organization_coord_sys_id
    } else if input.srs_id == Some(4326) {
        Some(4326)
    } else {
        None
    };
    let refusal = if data_type != "features" {
        Some(("Only feature tables can be imported locally.", None))
    } else if input
        .geometry_column
        .as_deref()
        .is_none_or(|column| column.trim().is_empty())
    {
        Some((
            "Feature table has no registered geometry column.",
            Some("invalid_database"),
        ))
    } else if input.row_count.is_some_and(|count| count > input.max_rows) {
        Some((
            "Feature table exceeds the browser row limit.",
            Some("row_count_too_large"),
        ))
    } else if epsg != Some(4326) {
        Some((
            "Feature table CRS requires backend reprojection.",
            Some("unsupported_crs"),
        ))
    } else if !input.geometry_type.as_deref().is_some_and(|value| {
        matches!(
            value
                .trim()
                .to_ascii_uppercase()
                .replace("MULTI_", "MULTI")
                .as_str(),
            "GEOMETRY"
                | "POINT"
                | "LINESTRING"
                | "POLYGON"
                | "MULTIPOINT"
                | "MULTILINESTRING"
                | "MULTIPOLYGON"
                | "GEOMETRYCOLLECTION"
        )
    }) {
        Some((
            "Feature table geometry type is not supported locally.",
            Some("unsupported_geometry"),
        ))
    } else {
        None
    };
    GpkgLayerPolicy {
        data_type,
        epsg,
        readable: refusal.is_none(),
        reason: refusal.map(|(reason, _)| reason.to_string()),
        fallback_reason: refusal.and_then(|(_, fallback)| fallback.map(str::to_string)),
    }
}

pub fn geojson_geometry_to_gpkg_blob(geometry: &Value, srs_id: i32) -> Option<Vec<u8>> {
    let wkb = crate::wkb::geometry_to_wkb_with_collections(geometry)?;
    let mut bytes = Vec::with_capacity(8 + wkb.len());
    bytes.extend_from_slice(&[0x47, 0x50, 0, 1]);
    bytes.extend_from_slice(&srs_id.to_le_bytes());
    bytes.extend_from_slice(&wkb);
    Some(bytes)
}

pub fn gpkg_geometry_to_geojson(bytes: &[u8]) -> Result<GpkgGeometry, String> {
    decode_gpkg_geometry(bytes, false)
}

/// Historical/native source extraction retains every original Z/M ordinate.
/// The established browser/render import remains explicitly XY-only.
pub fn gpkg_geometry_to_geojson_preserving_dimensions(
    bytes: &[u8],
) -> Result<GpkgGeometry, String> {
    decode_gpkg_geometry(bytes, true)
}

fn decode_gpkg_geometry(bytes: &[u8], preserve_dimensions: bool) -> Result<GpkgGeometry, String> {
    let (header, offset, bare_wkb) = if bytes.starts_with(b"GP") {
        let header = parse_header(bytes)?;
        if header.empty {
            return Ok(GpkgGeometry {
                geometry: None,
                header: Some(header),
                bare_wkb: false,
            });
        }
        let offset = header.wkb_offset;
        (Some(header), offset, false)
    } else if matches!(bytes.first(), Some(0 | 1)) {
        (None, 0, true)
    } else {
        return Err("geometry blob is neither GeoPackage binary geometry nor WKB".to_string());
    };
    if offset >= bytes.len() {
        return Err("geometry blob ended before its WKB payload".to_string());
    }
    // Node allocations are bounded by the supplied payload: each admitted
    // geometry/ring/coordinate consumes at least four input bytes. The shared
    // decoder also checks counts before allocation and complete consumption.
    let geometry =
        crate::wkb::geometry_from_extended_wkb(&bytes[offset..], bytes.len(), preserve_dimensions)?;
    Ok(GpkgGeometry {
        geometry: Some(geometry),
        header,
        bare_wkb,
    })
}

fn parse_header(bytes: &[u8]) -> Result<GpkgGeometryHeader, String> {
    if bytes.len() < 8 || !bytes.starts_with(b"GP") {
        return Err("invalid GeoPackage geometry header".to_string());
    }
    if bytes[2] != 0 {
        return Err(format!(
            "unsupported GeoPackage geometry version {}",
            bytes[2]
        ));
    }
    let flags = bytes[3];
    if flags & 0b1100_0000 != 0 {
        return Err("GeoPackage geometry header has non-zero reserved flags".to_string());
    }
    let little_endian = flags & 1 == 1;
    let envelope_code = (flags >> 1) & 0b111;
    let envelope_bytes = match envelope_code {
        0 => 0,
        1 => 32,
        2 | 3 => 48,
        4 => 64,
        other => return Err(format!("unsupported GeoPackage envelope code {other}")),
    };
    let wkb_offset = 8 + envelope_bytes;
    if wkb_offset > bytes.len() {
        return Err("GeoPackage geometry envelope is truncated".to_string());
    }
    let srs_bytes: [u8; 4] = bytes[4..8].try_into().expect("bounded header");
    let srs_id = if little_endian {
        i32::from_le_bytes(srs_bytes)
    } else {
        i32::from_be_bytes(srs_bytes)
    };
    Ok(GpkgGeometryHeader {
        version: bytes[2],
        flags,
        empty: flags & 0b1_0000 != 0,
        extended: flags & 0b10_0000 != 0,
        little_endian,
        srs_id,
        envelope_code,
        wkb_offset,
    })
}

#[cfg(test)]
#[path = "gpkg_tests.rs"]
mod tests;
