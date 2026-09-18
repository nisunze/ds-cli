//! The national administrative hierarchy, read exactly.
//!
//! One country's boundary authority answers on one path — `/api/v1/admin/
//! rwanda` — and the country is IN that path, which is why [`Country`] has a
//! single variant: a second authority is a second route and a reviewed
//! descriptor entry, never a string a caller passes.
//!
//! Two reads are declared, and both are bounded by construction. `children`
//! walks one leg of the hierarchy: province is the root, and every lower level
//! names its exact immediate parent, so a caller can never ask for "every
//! village" by omitting a parameter. `geometry` reads one unit's polygon by its
//! exact code.
//!
//! The polygon does not leave this module. A boundary is hundreds of kilobytes
//! of coordinates and a terminal is not where they belong, so what comes back
//! is *evidence* — identity, geometry type, bounds, coordinate count and a
//! digest over the authority's own canonical geometry. A caller who needs the
//! bytes asks for them explicitly, and gets a file.
//!
//! Until 2026-09-18 `ds data admin-bounds list|read` reached this same route
//! through the paired desktop, which held the same signed-in user and sent the
//! same query. The window was never part of the contract, only of the route:
//! national reference data has no project, no selection and nothing to render.

use crate::{
    authorized::Authority, AdminBoundsCall, AuthorizedTransport, ClientError, ClientProfile,
    ErrorKind, ServiceRefusal,
};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

/// The declared boundary authorities.
///
/// One variant, because the authority is a path segment: Rwanda's hierarchy is
/// the only one this deployment publishes. A second country arrives as a second
/// route, so it cannot arrive by accident here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Country {
    Rwanda,
}

impl Country {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Rwanda => "rwanda",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ClientError> {
        if value.trim().eq_ignore_ascii_case("rwanda") {
            Ok(Self::Rwanda)
        } else {
            Err(invalid(
                "`country` must be rwanda; no other boundary authority is declared",
            ))
        }
    }
}

/// One level of the hierarchy, and the code arithmetic that defines it.
///
/// A Rwandan administrative code is a prefix code: a village's eight digits
/// begin with its cell's six, which begin with its sector's four. So the level
/// fixes both the length of its own codes and the length of the parent code it
/// requires, and those two numbers are the whole of the hierarchy's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Province,
    District,
    Sector,
    Cell,
    Village,
}

impl Level {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Province => "province",
            Self::District => "district",
            Self::Sector => "sector",
            Self::Cell => "cell",
            Self::Village => "village",
        }
    }

    /// Exactly how many digits a code at this level carries.
    pub const fn code_digits(self) -> usize {
        match self {
            Self::Province => 1,
            Self::District => 2,
            Self::Sector => 4,
            Self::Cell => 6,
            Self::Village => 8,
        }
    }

    /// How many digits this level's immediate parent carries, or `None` at the
    /// root. Province is the root, and a root with a parent is a contradiction
    /// the authority would answer with the wrong rows.
    pub const fn parent_digits(self) -> Option<usize> {
        match self {
            Self::Province => None,
            Self::District => Some(1),
            Self::Sector => Some(2),
            Self::Cell => Some(4),
            Self::Village => Some(6),
        }
    }

    /// The level a code's own length declares it to be.
    pub const fn of_code(code: &str) -> Option<Self> {
        match code.len() {
            1 => Some(Self::Province),
            2 => Some(Self::District),
            4 => Some(Self::Sector),
            6 => Some(Self::Cell),
            8 => Some(Self::Village),
            _ => None,
        }
    }

    pub fn parse(value: &str) -> Result<Self, ClientError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "province" => Ok(Self::Province),
            "district" => Ok(Self::District),
            "sector" => Ok(Self::Sector),
            "cell" => Ok(Self::Cell),
            "village" => Ok(Self::Village),
            _ => Err(invalid(
                "`level` must be province, district, sector, cell, or village",
            )),
        }
    }
}

/// One exact read of the national hierarchy.
pub enum Command {
    /// The exact immediate children of one parent, or the roots.
    Children {
        country: Country,
        level: Level,
        parent_code: Option<String>,
    },
    /// One unit's polygon, by its exact code.
    Boundary { country: Country, code: String },
}

/// The longest name this boundary authority may return. A row past it is a
/// column that has stopped being a name, not a long name.
pub const MAX_NAME_BYTES: usize = 200;
/// The most immediate children one level of the hierarchy holds. Rwanda's
/// largest single answer is its ~2,148 cells under one province; the bound is
/// a defect detector, not a page size.
pub const MAX_CHILDREN: usize = 5_000;

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "the administrative-boundary authority answered outside its exact hierarchy contract",
    )
}

/// An exact code is ASCII digits at one of the five declared lengths. Nothing
/// else reaches the wire, which is also why the query needs no escaping.
pub fn exact_code(value: &str) -> Result<&str, ClientError> {
    if Level::of_code(value).is_some() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        Ok(value)
    } else {
        Err(invalid(
            "an administrative code is exactly 1, 2, 4, 6, or 8 digits",
        ))
    }
}

impl Command {
    /// The action this command multiplexes the shared path with.
    ///
    /// `geometry` and `wkt` are aliases at the backend (`admin_handler.go`
    /// answers both with `svc.Geometry`); this client sends `geometry`, which
    /// is what it means and what ds-web has always sent.
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Children { .. } => "children",
            Self::Boundary { .. } => "geometry",
        }
    }

    /// Nothing here writes. The hierarchy is published reference data.
    pub const fn writes(&self) -> bool {
        false
    }

    pub const fn country(&self) -> Country {
        match self {
            Self::Children { country, .. } | Self::Boundary { country, .. } => *country,
        }
    }

    pub fn validate(&self) -> Result<(), ClientError> {
        match self {
            Self::Children {
                level, parent_code, ..
            } => match (level.parent_digits(), parent_code.as_deref()) {
                (None, Some(_)) => Err(invalid(
                    "province is the hierarchy root and takes no parent code",
                )),
                (Some(_), None) => Err(invalid(
                    "every level below province requires its exact immediate parent code",
                )),
                (Some(expected), Some(code)) => {
                    exact_code(code)?;
                    if code.len() == expected {
                        Ok(())
                    } else {
                        Err(invalid(
                            "the parent code is not the immediate parent of the requested level",
                        ))
                    }
                }
                (None, None) => Ok(()),
            },
            Self::Boundary { code, .. } => exact_code(code).map(|_| ()),
        }
    }

    /// The exact query string this command sends.
    ///
    /// Every value is either a closed token or ASCII digits, so no escaping is
    /// possible or needed — `validate` is what makes that true, and the test
    /// below is what keeps it true.
    pub(crate) fn query(&self) -> String {
        match self {
            Self::Children {
                level, parent_code, ..
            } => {
                let mut query = format!("action={}&level={}", self.action(), level.token());
                if let Some(code) = parent_code {
                    query.push_str("&parent_code=");
                    query.push_str(code);
                }
                query
            }
            Self::Boundary { code, .. } => format!("action={}&code={}", self.action(), code),
        }
    }
}

/// One bounded polygon, as evidence rather than as coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryGeometry {
    /// `Polygon` or `MultiPolygon`, as the authority typed it.
    pub geometry_type: String,
    /// `[west, south, east, north]`.
    pub bbox: [f64; 4],
    /// How many coordinate positions the rings hold in total.
    pub coordinate_positions: usize,
    /// SHA-256 over the canonical (key-sorted, compact) geometry JSON.
    pub sha256: String,
    /// The geometry itself, for a caller that asked to keep it.
    pub geojson: Value,
}

/// One boundary's exact identity and its geometry evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct Boundary {
    pub code: String,
    pub name: String,
    pub level: Level,
    pub geometry: BoundaryGeometry,
}

impl Boundary {
    /// The bounded receipt a caller reads. The coordinates stay out of it.
    pub fn evidence(&self) -> Value {
        json!({
            "boundary": {
                "code": self.code,
                "name": self.name,
                "level": self.level.token(),
            },
            "geometry": {
                "type": self.geometry.geometry_type,
                "bbox": self.geometry.bbox,
                "coordinate_positions": self.geometry.coordinate_positions,
                "sha256": self.geometry.sha256,
            },
        })
    }

    /// The boundary as one GeoJSON `FeatureCollection`, carrying the same three
    /// properties the desktop's own materialisation writes, so the file is a
    /// layer anything in the product can read.
    pub fn feature_collection(&self) -> Value {
        json!({
            "type": "FeatureCollection",
            "features": [{
                "type": "Feature",
                "id": format!("admin-{}", self.code),
                "geometry": self.geometry.geojson,
                "properties": {
                    "name": self.name,
                    "code": self.code,
                    "level": self.level.token(),
                },
            }],
        })
    }
}

/// Canonical JSON: object keys sorted, no whitespace.
///
/// The digest has to mean the same thing whoever computed it, so the bytes it
/// is taken over cannot depend on the order a decoder happened to see.
fn canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_unstable();
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String(key.clone()).to_string());
                out.push(':');
                canonical(&map[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Walk one polygon's rings, checking every position and accumulating the box.
fn read_rings(rings: &Value, bounds: &mut Bounds) -> Result<usize, ClientError> {
    let rings = rings.as_array().ok_or_else(unreadable)?;
    if rings.is_empty() {
        return Err(unreadable());
    }
    let mut positions = 0;
    for ring in rings {
        let ring = ring.as_array().ok_or_else(unreadable)?;
        // A ring is closed and has an interior: three distinct corners plus the
        // repeated first position.
        if ring.len() < 4 || ring.first() != ring.last() {
            return Err(unreadable());
        }
        for position in ring {
            let position = position.as_array().ok_or_else(unreadable)?;
            let [longitude, latitude] = match position.as_slice() {
                [longitude, latitude, ..] => [longitude, latitude],
                _ => return Err(unreadable()),
            };
            let longitude = longitude.as_f64().ok_or_else(unreadable)?;
            let latitude = latitude.as_f64().ok_or_else(unreadable)?;
            if !(-180.0..=180.0).contains(&longitude) || !(-90.0..=90.0).contains(&latitude) {
                return Err(unreadable());
            }
            bounds.see(longitude, latitude);
            positions += 1;
        }
    }
    Ok(positions)
}

#[derive(Default)]
struct Bounds {
    west: f64,
    south: f64,
    east: f64,
    north: f64,
    seen: bool,
}

impl Bounds {
    fn new() -> Self {
        Self {
            west: f64::INFINITY,
            south: f64::INFINITY,
            east: f64::NEG_INFINITY,
            north: f64::NEG_INFINITY,
            seen: false,
        }
    }
    fn see(&mut self, longitude: f64, latitude: f64) {
        self.seen = true;
        self.west = self.west.min(longitude);
        self.east = self.east.max(longitude);
        self.south = self.south.min(latitude);
        self.north = self.north.max(latitude);
    }
    fn finish(self) -> Result<[f64; 4], ClientError> {
        if self.seen {
            Ok([self.west, self.south, self.east, self.north])
        } else {
            Err(unreadable())
        }
    }
}

/// Decode one authority geometry into bounded evidence.
pub fn read_boundary(code: &str, value: &Value) -> Result<Boundary, ClientError> {
    let level = Level::of_code(code).ok_or_else(unreadable)?;
    if value["code"].as_str() != Some(code) {
        return Err(unreadable());
    }
    // The level the authority names must be the level the code's own length
    // declares. A disagreement here means the row came from another table.
    if value["level"].as_str() != Some(level.token()) {
        return Err(unreadable());
    }
    let name = value["name"].as_str().ok_or_else(unreadable)?.trim();
    if name.is_empty() || name.len() > MAX_NAME_BYTES {
        return Err(unreadable());
    }
    let geometry = &value["geometry"];
    let geometry_type = geometry["type"].as_str().ok_or_else(unreadable)?;
    let mut bounds = Bounds::new();
    let coordinate_positions = match geometry_type {
        "Polygon" => read_rings(&geometry["coordinates"], &mut bounds)?,
        "MultiPolygon" => {
            let polygons = geometry["coordinates"].as_array().ok_or_else(unreadable)?;
            if polygons.is_empty() {
                return Err(unreadable());
            }
            let mut positions = 0;
            for polygon in polygons {
                positions += read_rings(polygon, &mut bounds)?;
            }
            positions
        }
        // A boundary is an area. A point or a line here is a broken row, and
        // approximating one into an area is exactly what this read exists to
        // prevent.
        _ => return Err(unreadable()),
    };
    let mut canonical_text = String::new();
    canonical(geometry, &mut canonical_text);
    Ok(Boundary {
        code: code.to_owned(),
        name: name.to_owned(),
        level,
        geometry: BoundaryGeometry {
            geometry_type: geometry_type.to_owned(),
            bbox: bounds.finish()?,
            coordinate_positions,
            sha256: format!("{:x}", Sha256::digest(canonical_text.as_bytes())),
            geojson: geometry.clone(),
        },
    })
}

/// Decode the children of one parent, refusing any row outside the exact leg
/// that was asked for.
pub fn read_children(
    level: Level,
    parent_code: Option<&str>,
    value: &Value,
) -> Result<Vec<(String, String)>, ClientError> {
    // An empty leg answers `null`, and that is a fact about the hierarchy, not
    // a broken authority: a cell with no villages in the table is exactly what
    // an operator needs to be told.
    let rows = match &value["children"] {
        Value::Null => return Ok(Vec::new()),
        Value::Array(rows) => rows,
        _ => return Err(unreadable()),
    };
    if rows.len() > MAX_CHILDREN {
        return Err(unreadable());
    }
    let mut children: Vec<(String, String)> = Vec::with_capacity(rows.len());
    for row in rows {
        let code = row["code"].as_str().ok_or_else(unreadable)?;
        if code.len() != level.code_digits()
            || !code.bytes().all(|byte| byte.is_ascii_digit())
            || parent_code.is_some_and(|parent| !code.starts_with(parent))
        {
            return Err(unreadable());
        }
        let name = row["name"].as_str().ok_or_else(unreadable)?.trim();
        if name.is_empty() || name.len() > MAX_NAME_BYTES {
            return Err(unreadable());
        }
        if children.iter().any(|(seen, _)| seen == code) {
            return Err(unreadable());
        }
        children.push((code.to_owned(), name.to_owned()));
    }
    Ok(children)
}

/// Classify one refused administrative read.
///
/// The authority answers a missing code with `500` and its own sentence
/// (`admin_handler.go` writes every service error that way), so the sentence is
/// the actionable half and crosses as a bounded [`ServiceRefusal`] rather than
/// being matched against here.
fn refusal(status: u16, body: &[u8]) -> ClientError {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let envelope = ServiceRefusal::new(status, None, value["error"].as_str());
    let (kind, message) = match status {
        400 | 422 => (
            ErrorKind::InvalidInput,
            "the administrative-boundary authority refused this scope",
        ),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "the administrative-boundary authority rejected this verified identity",
        ),
        _ => (
            ErrorKind::Transient,
            "the administrative-boundary authority could not answer this exact read",
        ),
    };
    if envelope.message().is_none() {
        return ClientError::new(kind, message);
    }
    ClientError::with_service_refusal(kind, message, envelope)
}

/// The result of one read, before a host shapes its receipt.
pub enum Answer {
    Children {
        level: Level,
        parent_code: Option<String>,
        boundaries: Vec<(String, String)>,
    },
    Boundary(Box<Boundary>),
}

impl Answer {
    /// The bounded receipt, identical in shape for every host that asks.
    pub fn receipt(&self, country: Country) -> Value {
        let mut receipt = Map::new();
        receipt.insert("country".into(), json!(country.token()));
        // National reference data belongs to no project, and saying so is the
        // point: a caller must never file one of these under a project.
        receipt.insert(
            "scope".into(),
            json!({ "kind": "national_reference", "country": country.token(), "project": null }),
        );
        receipt.insert("exact".into(), json!(true));
        match self {
            Self::Children {
                level,
                parent_code,
                boundaries,
            } => {
                receipt.insert("level".into(), json!(level.token()));
                receipt.insert("parent_code".into(), json!(parent_code));
                receipt.insert("count".into(), json!(boundaries.len()));
                receipt.insert(
                    "boundaries".into(),
                    Value::Array(
                        boundaries
                            .iter()
                            .map(|(code, name)| json!({ "code": code, "name": name }))
                            .collect(),
                    ),
                );
                receipt.insert("geometry_included".into(), json!(false));
            }
            Self::Boundary(boundary) => {
                let evidence = boundary.evidence();
                receipt.insert("boundary".into(), evidence["boundary"].clone());
                receipt.insert("geometry".into(), evidence["geometry"].clone());
            }
        }
        Value::Object(receipt)
    }
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    command: &Command,
) -> Result<Answer, ClientError> {
    command.validate()?;
    let response = transport.admin_bounds(AdminBoundsCall {
        profile,
        authority: authority.call(),
        query: command.query(),
    })?;
    if response.status != 200 {
        return Err(refusal(response.status, &response.body));
    }
    if response.body.len() > crate::transport::ADMIN_BOUNDS_RESPONSE_LIMIT {
        return Err(unreadable());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    match command {
        Command::Children {
            level, parent_code, ..
        } => Ok(Answer::Children {
            level: *level,
            parent_code: parent_code.clone(),
            boundaries: read_children(*level, parent_code.as_deref(), &body)?,
        }),
        Command::Boundary { code, .. } => {
            Ok(Answer::Boundary(Box::new(read_boundary(code, &body)?)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn children(level: Level, parent: Option<&str>) -> Command {
        Command::Children {
            country: Country::Rwanda,
            level,
            parent_code: parent.map(str::to_owned),
        }
    }

    #[test]
    fn every_action_sends_exactly_the_keys_the_backend_reads() {
        // `admin_handler.go` reads `action`, `level` and `parent_code` for a
        // children query and `action` and `code` for a geometry read, and
        // nothing else. A fourth key would be silently ignored, which is worse
        // than refused.
        assert_eq!(
            children(Level::Province, None).query(),
            "action=children&level=province"
        );
        assert_eq!(
            children(Level::Village, Some("110101")).query(),
            "action=children&level=village&parent_code=110101"
        );
        assert_eq!(
            Command::Boundary {
                country: Country::Rwanda,
                code: "11010102".into(),
            }
            .query(),
            "action=geometry&code=11010102"
        );
    }

    #[test]
    fn a_query_can_never_need_escaping() {
        // The query is concatenated, not encoded, and that is only safe
        // because every value is a closed token or ASCII digits.
        for command in [
            children(Level::Province, None),
            children(Level::Cell, Some("1101")),
            Command::Boundary {
                country: Country::Rwanda,
                code: "5".into(),
            },
        ] {
            command.validate().expect("a declared read validates");
            assert!(
                command
                    .query()
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'=' | b'&' | b'_')),
                "{} carries a byte a URL would have to escape",
                command.query()
            );
        }
    }

    #[test]
    fn the_hierarchy_refuses_a_parent_that_is_not_the_immediate_one() {
        assert!(children(Level::Province, None).validate().is_ok());
        assert!(children(Level::Village, Some("110101")).validate().is_ok());
        // A root with a parent, a leg without one, and a grandparent standing
        // in for a parent: each would answer with rows from another leg.
        assert!(children(Level::Province, Some("1")).validate().is_err());
        assert!(children(Level::Village, None).validate().is_err());
        assert!(children(Level::Village, Some("11")).validate().is_err());
        assert!(children(Level::Sector, Some("1x")).validate().is_err());
        assert!(Command::Boundary {
            country: Country::Rwanda,
            code: "110".into(),
        }
        .validate()
        .is_err());
    }

    #[test]
    fn a_code_is_its_parents_code_plus_its_own_digits() {
        // The prefix property is the whole hierarchy: a village's eight digits
        // begin with its cell's six. So each level's required parent length is
        // exactly the code length of the level above it, and a code's own
        // length is enough to say which level it belongs to.
        let ladder = [
            Level::Province,
            Level::District,
            Level::Sector,
            Level::Cell,
            Level::Village,
        ];
        assert_eq!(ladder[0].parent_digits(), None);
        for (index, level) in ladder.iter().enumerate() {
            let code = "1".repeat(level.code_digits());
            assert_eq!(Level::of_code(&code), Some(*level));
            assert_eq!(
                level.parent_digits(),
                index
                    .checked_sub(1)
                    .map(|above| ladder[above].code_digits()),
                "{} asks for a parent that is not the level above it",
                level.token()
            );
        }
    }

    #[test]
    fn children_outside_the_requested_leg_are_refused() {
        let ok = json!({"children": [
            {"code": "1101", "name": "Gitega"},
            {"code": "1102", "name": "Kanyinya"},
        ]});
        assert_eq!(
            read_children(Level::Sector, Some("11"), &ok).unwrap(),
            vec![
                ("1101".to_owned(), "Gitega".to_owned()),
                ("1102".to_owned(), "Kanyinya".to_owned()),
            ]
        );
        // An empty leg is a fact, not a failure.
        assert!(
            read_children(Level::Village, Some("110205"), &json!({"children": null}))
                .unwrap()
                .is_empty()
        );
        for broken in [
            json!({"children": [{"code": "1201", "name": "Elsewhere"}]}),
            json!({"children": [{"code": "110101", "name": "Too deep"}]}),
            json!({"children": [{"code": "1101", "name": "  "}]}),
            json!({"children": [{"code": "1101", "name": "A"}, {"code": "1101", "name": "A"}]}),
            json!({"children": "rows"}),
        ] {
            assert!(
                read_children(Level::Sector, Some("11"), &broken).is_err(),
                "{broken} was accepted"
            );
        }
    }

    fn square() -> Value {
        json!({
            "type": "Polygon",
            "coordinates": [[[30.0, -2.0], [30.5, -2.0], [30.5, -1.5], [30.0, -1.5], [30.0, -2.0]]],
        })
    }

    #[test]
    fn a_boundary_is_evidence_and_never_coordinates() {
        let answer = json!({
            "code": "11",
            "level": "district",
            "name": " Nyarugenge ",
            "geometry": square(),
        });
        let boundary = read_boundary("11", &answer).expect("an exact district reads");
        assert_eq!(boundary.name, "Nyarugenge");
        assert_eq!(boundary.level, Level::District);
        assert_eq!(boundary.geometry.coordinate_positions, 5);
        assert_eq!(boundary.geometry.bbox, [30.0, -2.0, 30.5, -1.5]);
        let receipt = Answer::Boundary(Box::new(boundary.clone())).receipt(Country::Rwanda);
        assert_eq!(receipt["boundary"]["name"], "Nyarugenge");
        assert_eq!(receipt["geometry"]["coordinate_positions"], 5);
        assert!(
            receipt["geometry"]["coordinates"].is_null(),
            "the polygon travelled into the receipt"
        );
        // The file a caller asks for carries the three properties the desktop's
        // own admin layer writes, so it is the same layer either way.
        let collection = boundary.feature_collection();
        assert_eq!(collection["features"][0]["properties"]["code"], "11");
        assert_eq!(collection["features"][0]["properties"]["level"], "district");
        assert_eq!(collection["features"][0]["id"], "admin-11");
    }

    #[test]
    fn the_digest_does_not_depend_on_key_order() {
        let one = read_boundary(
            "11",
            &json!({"code": "11", "level": "district", "name": "N", "geometry": square()}),
        )
        .unwrap();
        let other = read_boundary(
            "11",
            &json!({"name": "N", "geometry": {
                "coordinates": square()["coordinates"].clone(),
                "type": "Polygon",
            }, "level": "district", "code": "11"}),
        )
        .unwrap();
        assert_eq!(one.geometry.sha256, other.geometry.sha256);
        assert_eq!(one.geometry.sha256.len(), 64);
    }

    #[test]
    fn a_boundary_that_is_not_an_area_is_refused() {
        for broken in [
            json!({"code": "11", "level": "district", "name": "N", "geometry": {"type": "Point", "coordinates": [30.0, -2.0]}}),
            json!({"code": "11", "level": "district", "name": "N", "geometry": Value::Null}),
            json!({"code": "11", "level": "province", "name": "N", "geometry": square()}),
            json!({"code": "12", "level": "district", "name": "N", "geometry": square()}),
            json!({"code": "11", "level": "district", "name": "", "geometry": square()}),
            // An unclosed ring is not a polygon, however plausible it looks.
            json!({"code": "11", "level": "district", "name": "N", "geometry": {
                "type": "Polygon",
                "coordinates": [[[30.0, -2.0], [30.5, -2.0], [30.5, -1.5]]],
            }}),
        ] {
            assert!(read_boundary("11", &broken).is_err(), "{broken} was read");
        }
    }

    #[test]
    fn nothing_here_writes() {
        for command in [
            children(Level::Province, None),
            Command::Boundary {
                country: Country::Rwanda,
                code: "1".into(),
            },
        ] {
            assert!(!command.writes());
            assert_eq!(command.country(), Country::Rwanda);
        }
    }

    #[test]
    fn a_refused_read_carries_the_authoritys_own_sentence() {
        let error = refusal(500, br#"{"error":"code not found: 99999999"}"#);
        assert_eq!(error.kind(), ErrorKind::Transient);
        let carried = error.service_refusal().expect("the sentence crosses");
        assert_eq!(carried.status(), 500);
        assert_eq!(carried.message(), Some("code not found: 99999999"));
        assert_eq!(
            refusal(400, b"not json").kind(),
            ErrorKind::InvalidInput,
            "a refusal with no readable body still keeps its status class"
        );
    }
}
