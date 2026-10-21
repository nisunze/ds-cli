//! Shared typed vector admission packets; no host or data serialization boundary.
use serde::{Deserialize, Serialize};

/// The geometry classes vector operations branch on. Multi-part geometries
/// are the same class as their single-part form: a MultiLineString is lines,
/// and an operation that works on lines works on each part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeometryKind {
    Point,
    Line,
    Polygon,
}

impl GeometryKind {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Line => "line",
            Self::Polygon => "polygon",
        }
    }

    /// The engine's geometry selector. `ds-geo-ops` takes the kind as a `u32`
    /// on its flat-array boundary; naming the mapping once here is what stops
    /// each call site from inventing its own.
    pub const fn engine_code(self) -> u32 {
        match self {
            Self::Point => 0,
            Self::Line => 1,
            Self::Polygon => 2,
        }
    }

    pub fn from_geojson_type(value: &str) -> Option<Self> {
        match value {
            "Point" | "MultiPoint" => Some(Self::Point),
            "LineString" | "MultiLineString" => Some(Self::Line),
            "Polygon" | "MultiPolygon" => Some(Self::Polygon),
            _ => None,
        }
    }
}

/// The operations this family plans. Each one names what it needs, so the
/// eligibility rule is declared next to the operation rather than repeated at
/// every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VectorOperation {
    /// A zone of a fixed distance around each feature.
    Buffer,
    /// The points where the lines of one document cross another's.
    Intersect,
    /// Points placed along each line at a fixed interval.
    Sample,
    /// Length, area and vertex counts per feature.
    Measure,
}

impl VectorOperation {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Buffer => "buffer",
            Self::Intersect => "intersect",
            Self::Sample => "sample",
            Self::Measure => "measure",
        }
    }

    /// Which geometry classes this operation acts on. Anything else is
    /// skipped with a reason, never silently dropped.
    pub const fn accepts(self) -> &'static [GeometryKind] {
        match self {
            Self::Buffer => &[
                GeometryKind::Point,
                GeometryKind::Line,
                GeometryKind::Polygon,
            ],
            Self::Intersect | Self::Sample => &[GeometryKind::Line],
            Self::Measure => &[
                GeometryKind::Point,
                GeometryKind::Line,
                GeometryKind::Polygon,
            ],
        }
    }

    pub fn accepts_kind(self, kind: GeometryKind) -> bool {
        let mut index = 0;
        let accepted = self.accepts();
        while index < accepted.len() {
            if accepted[index] as u8 == kind as u8 {
                return true;
            }
            index += 1;
        }
        false
    }

    /// What a caller should do when a document holds nothing this operation
    /// can act on. Named per operation because "convert your points to lines"
    /// is useless advice for `measure`.
    pub fn empty_remedy(self) -> &'static str {
        match self {
            Self::Buffer => "Pass a document holding point, line or polygon features.",
            Self::Intersect | Self::Sample => {
                "This operation reads lines; pass a LineString or MultiLineString document."
            }
            Self::Measure => "Pass a document holding at least one feature with a geometry.",
        }
    }
}

/// One feature the operation will act on: where it is in the source document,
/// what it is called, and the flat `[lng, lat, …]` parts the engine consumes.
///
/// The parts are already split: a MultiLineString contributes one part per
/// line, so an engine call is always one simple geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlannedFeature {
    /// Zero-based position in the source document. A caller correlates the
    /// answer back to their own file with this, not with a name we invented.
    pub index: usize,
    /// The feature's own `id`, or its first string property, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub kind: GeometryKind,
    /// Flat `[lng, lat, lng, lat, …]` per simple part, WGS-84 degrees.
    pub parts: Vec<Vec<f64>>,
    /// The polygon rings this feature encloses but does not cover: its holes,
    /// flat in the same `[lng, lat, …]` form.
    ///
    /// An operation that *grows* a geometry is right to ignore them — a
    /// buffer of a lake with an island is a zone around the lake. An
    /// operation that *reports* one is not: a parcel with a courtyard cut out
    /// of it is smaller than its outer ring, and answering the outer ring's
    /// area is a wrong number, not a bounded one. Carrying the holes here is
    /// what lets each operation make that choice deliberately instead of
    /// never having been offered it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interior: Vec<Vec<f64>>,
}

impl PlannedFeature {
    /// Total coordinate count across every part. Used to price a run before
    /// running it, so it counts what the engine will actually be handed.
    pub fn vertex_count(&self) -> usize {
        self.parts.iter().map(|part| part.len() / 2).sum()
    }

    /// Every coordinate the feature is written with, holes included. What a
    /// caller asking "how big is this feature" means, as against what an
    /// engine call will cost.
    pub fn written_vertex_count(&self) -> usize {
        self.vertex_count()
            + self
                .interior
                .iter()
                .map(|ring| ring.len() / 2)
                .sum::<usize>()
    }
}
