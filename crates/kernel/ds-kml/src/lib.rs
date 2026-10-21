//! Portable KML style admission and layer-document projection. No network IO.
use quick_xml::{events::Event, Reader, XmlVersion};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct Node {
    pub name: String,
    pub id: Option<String>,
    /// The unqualified `name` attribute before entity expansion.
    pub name_attribute: Option<String>,
    pub is_empty_element: bool,
    pub text: String,
    pub children: Vec<Node>,
    pub start: usize,
    pub end: usize,
    pub content_start: usize,
    pub content_end: usize,
}
impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|n| n.name == name)
    }
    pub fn value(&self, name: &str) -> Option<&str> {
        self.child(name).map(|n| n.text.trim())
    }
    pub fn descendants<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for n in &self.children {
            if n.name == name {
                out.push(n);
            }
            n.descendants(name, out);
        }
    }
}

pub fn parse(text: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(text);
    let mut version = XmlVersion::Implicit1_0;
    let mut stack = vec![Node::default()];
    loop {
        let start = reader.buffer_position() as usize;
        match reader
            .read_event()
            .map_err(|e| format!("invalid KML XML: {e}"))?
        {
            Event::Start(e) | Event::Empty(e) => {
                let empty = text[start..reader.buffer_position() as usize].ends_with("/>");
                let mut n = Node {
                    name: String::from_utf8_lossy(e.local_name().as_ref()).into_owned(),
                    is_empty_element: empty,
                    start,
                    end: reader.buffer_position() as usize,
                    content_start: reader.buffer_position() as usize,
                    content_end: reader.buffer_position() as usize,
                    ..Node::default()
                };
                for a in e.attributes() {
                    let a = a.map_err(|e| e.to_string())?;
                    if a.key.as_ref() == b"id" {
                        n.id = Some(
                            a.decoded_and_normalized_value(version, reader.decoder())
                                .map_err(|e| e.to_string())?
                                .into_owned(),
                        );
                    } else if a.key.as_ref().eq_ignore_ascii_case(b"name") {
                        n.name_attribute = Some(
                            reader
                                .decoder()
                                .decode(a.value.as_ref())
                                .map_err(|e| e.to_string())?
                                .into_owned(),
                        );
                    }
                }
                if empty {
                    stack.last_mut().unwrap().children.push(n);
                } else {
                    stack.push(n);
                }
                if stack.len() > 128 {
                    return Err("KML exceeds 128 XML levels".into());
                }
            }
            Event::End(_) => {
                if stack.len() <= 1 {
                    return Err("unbalanced KML XML".into());
                }
                let mut n = stack.pop().unwrap();
                n.content_end = start;
                n.end = reader.buffer_position() as usize;
                stack.last_mut().unwrap().children.push(n);
            }
            Event::Text(e) => {
                let decoded = e.decode().map_err(|e| e.to_string())?;
                stack
                    .last_mut()
                    .unwrap()
                    .text
                    .push_str(&quick_xml::escape::unescape(&decoded).map_err(|e| e.to_string())?);
            }
            Event::CData(e) => stack
                .last_mut()
                .unwrap()
                .text
                .push_str(&e.decode().map_err(|e| e.to_string())?),
            Event::GeneralRef(e) => {
                let entity = format!("&{};", e.decode().map_err(|e| e.to_string())?);
                stack
                    .last_mut()
                    .unwrap()
                    .text
                    .push_str(&quick_xml::escape::unescape(&entity).map_err(|e| e.to_string())?);
            }
            Event::Decl(e) => {
                version = match e.version().map_err(|e| e.to_string())?.as_ref() {
                    b"1.0" => XmlVersion::Explicit1_0,
                    b"1.1" => XmlVersion::Explicit1_1,
                    _ => return Err("unsupported KML XML version".into()),
                };
            }
            Event::DocType(_) => return Err("KML document types are not admitted".into()),
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err("unclosed KML XML".into());
    }
    Ok(stack.pop().unwrap())
}

/// Resolve only an archive-local reference. Parent traversal may stay inside
/// the archive; schemes, absolute paths and escapes never become fetches.
pub fn member_reference(base: &str, reference: &str) -> Result<String, String> {
    if reference.contains(':') || reference.starts_with('/') || reference.contains('\\') {
        return Err(format!("not an archive-local reference: {reference}"));
    }
    let mut parts: Vec<&str> = base
        .rsplit_once('/')
        .map(|(dir, _)| dir.split('/').collect())
        .unwrap_or_default();
    for part in reference.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("reference escapes KMZ: {reference}"));
                }
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Decode a KML URI before looking up its archive member. ZIP names themselves
/// are raw paths and continue to use `member_reference`.
pub fn uri_member_reference(base: &str, reference: &str) -> Result<String, String> {
    let path = reference.split(['?', '#']).next().unwrap_or_default();
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map_err(|_| format!("invalid UTF-8 archive reference: {reference}"))?;
    member_reference(base, &decoded)
}

pub struct Styles {
    entries: BTreeMap<String, Node>,
}
impl Styles {
    pub fn new(documents: &BTreeMap<String, Node>) -> Self {
        let mut entries = BTreeMap::new();
        for (path, root) in documents {
            let mut nodes = Vec::new();
            root.descendants("Style", &mut nodes);
            root.descendants("StyleMap", &mut nodes);
            for n in nodes {
                if let Some(id) = &n.id {
                    entries.insert(format!("{path}#{id}"), n.clone());
                }
            }
        }
        Self { entries }
    }
    fn resolve(
        &self,
        base: &str,
        url: &str,
        seen: &mut Vec<String>,
    ) -> Result<(String, Value), String> {
        let (path, id) = url
            .split_once('#')
            .ok_or_else(|| format!("styleUrl has no fragment: {url}"))?;
        let path = if path.is_empty() {
            base.to_string()
        } else {
            uri_member_reference(base, path)?
        };
        let id = percent_encoding::percent_decode_str(id)
            .decode_utf8()
            .map_err(|_| format!("invalid UTF-8 style fragment: {url}"))?;
        let key = format!("{path}#{id}");
        if seen.contains(&key) || seen.len() >= 32 {
            return Err(format!("cyclic style {key}"));
        }
        seen.push(key.clone());
        let n = self
            .entries
            .get(&key)
            .ok_or_else(|| format!("missing style {key}"))?;
        if n.name == "Style" {
            return Ok((key, style_at(&path, n)?));
        }
        let pair = n
            .children
            .iter()
            .find(|n| n.name == "Pair" && n.value("key") == Some("normal"))
            .ok_or_else(|| format!("StyleMap {key} has no normal pair"))?;
        let style = if let Some(inline) = pair.child("Style") {
            style_at(&path, inline)?
        } else {
            self.resolve(
                &path,
                pair.value("styleUrl")
                    .ok_or_else(|| format!("StyleMap {key} has no normal style"))?,
                seen,
            )?
            .1
        };
        Ok((key, style))
    }
    pub fn placemark(&self, path: &str, n: &Node) -> Result<Option<(String, Value)>, String> {
        let shared = n
            .value("styleUrl")
            .map(|url| self.resolve(path, url, &mut Vec::new()));
        let mut resolved = match shared {
            Some(result) => Some(result?),
            None => None,
        };
        if let Some(inline) = n.child("Style") {
            let inline = style_at(path, inline)?;
            if let Some((id, style)) = &mut resolved {
                merge(style, inline);
                *id = format!("{id}@inline-{}", n.start);
            } else {
                resolved = Some((format!("{path}#@inline-{}", n.start), inline));
            }
        }
        Ok(resolved)
    }
}
fn style_at(path: &str, node: &Node) -> Result<Value, String> {
    let mut style = decode_style(node)?;
    if let Some(icon) = style.get_mut("IconStyle").and_then(Value::as_object_mut) {
        if let Some(href) = icon.get("href").and_then(Value::as_str) {
            if let Ok(member) = uri_member_reference(path, href) {
                icon.insert("member".into(), json!(member));
            }
        }
    }
    Ok(style)
}
fn merge(base: &mut Value, overlay: Value) {
    for (key, value) in overlay.as_object().unwrap() {
        if let Some(existing) = base.get_mut(key).and_then(Value::as_object_mut) {
            if key == "IconStyle" && value.get("href").is_some() {
                existing.remove("member");
            }
            existing.extend(value.as_object().unwrap().clone());
        } else {
            base[key] = value.clone();
        }
    }
}

/// Retains every requested KML component without supplying implicit defaults.
pub fn decode_style(n: &Node) -> Result<Value, String> {
    let mut out = Map::new();
    for component in [
        "IconStyle",
        "LineStyle",
        "PolyStyle",
        "LabelStyle",
        "BalloonStyle",
    ] {
        let Some(c) = n.child(component) else {
            continue;
        };
        let mut values = Map::new();
        for field in [
            "color",
            "bgColor",
            "textColor",
            "width",
            "scale",
            "heading",
            "fill",
            "outline",
            "text",
            "displayMode",
        ] {
            let Some(text) = c.value(field) else { continue };
            let value = match field {
                "color" | "bgColor" | "textColor" => {
                    if text.len() != 8 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
                        return Err(format!(
                            "{component}.{field}: invalid aabbggrr colour {text}"
                        ));
                    }
                    Value::String(text.to_ascii_lowercase())
                }
                "width" | "scale" | "heading" => {
                    let number = text
                        .parse::<f64>()
                        .map_err(|_| format!("{component}.{field}: invalid number {text}"))?;
                    if !number.is_finite() || (field != "heading" && number < 0.0) {
                        return Err(format!("{component}.{field}: invalid number {text}"));
                    }
                    json!(number)
                }
                "fill" | "outline" => match text {
                    "0" | "false" => json!(false),
                    "1" | "true" => json!(true),
                    _ => return Err(format!("{component}.{field}: expected 0, 1, false or true")),
                },
                _ => json!(text),
            };
            values.insert(field.into(), value);
        }
        if let Some(href) = c.child("Icon").and_then(|i| i.value("href")) {
            values.insert("href".into(), json!(href));
        }
        out.insert(component.into(), Value::Object(values));
    }
    Ok(Value::Object(out))
}

fn css(color: &str) -> Value {
    let alpha = u8::from_str_radix(&color[..2], 16).unwrap() as f64 / 255.0;
    let b = u8::from_str_radix(&color[2..4], 16).unwrap();
    let g = u8::from_str_radix(&color[4..6], 16).unwrap();
    let r = u8::from_str_radix(&color[6..], 16).unwrap();
    json!(format!("rgba({r},{g},{b},{alpha})"))
}

pub fn feature_colours(style: &Value) -> Map<String, Value> {
    [
        ("LineStyle", "kml_line_color"),
        ("PolyStyle", "kml_fill_color"),
        ("LabelStyle", "kml_label_color"),
    ]
    .into_iter()
    .filter_map(|(component, field)| {
        Some((
            field.to_string(),
            css(style.get(component)?.get("color")?.as_str()?),
        ))
    })
    .collect()
}

/// Unstyled features use the explicit KML standard document. Style IDs and
/// original components travel in metadata.
pub fn layer_document(features: &[Value], geometry: &str) -> Option<Value> {
    let styles: BTreeMap<String, Value> = features
        .iter()
        .filter_map(|f| {
            let p = f.get("properties")?;
            let style = p.get("kml_style")?;
            if !style.is_object() {
                return None;
            }
            Some((p.get("kml_style_id")?.as_str()?.to_string(), style.clone()))
        })
        .collect();
    if styles.is_empty() {
        return None;
    }
    let defaults: Value =
        serde_json::from_str(include_str!("import-defaults.json")).expect("KML standard seed");
    let mut effective = styles.clone();
    let mut images = BTreeMap::new();
    for style in effective.values_mut() {
        let mut base = defaults["metadata"]["kml"].clone();
        merge(&mut base, style.clone());
        *style = base;
        if matches!(geometry, "LineString" | "Polygon") {
            continue;
        }
        let icon = &mut style["IconStyle"];
        let href = icon["href"].as_str().unwrap().to_string();
        let color = icon["color"].as_str().unwrap().to_string();
        let id = format!(
            "kml-{:x}",
            Sha256::digest(format!("{href}\0{color}").as_bytes())
        );
        images.insert(id.clone(), json!({"id": id, "href": href, "color": color}));
        icon["href"] = json!(id);
    }
    let expression =
        |component: &str, field: &str, transform: fn(&Value) -> Value, absent: Value| {
            let mut expr = vec![json!("match"), json!(["get", "kml_style_id"])];
            for (id, style) in &effective {
                if let Some(value) = style.get(component).and_then(|s| s.get(field)) {
                    expr.push(json!(id));
                    expr.push(transform(value));
                }
            }
            if expr.len() == 2 {
                absent
            } else {
                expr.push(absent);
                Value::Array(expr)
            }
        };
    let identity = |v: &Value| v.clone();
    let colour = |v: &Value| css(v.as_str().unwrap());
    let opacity = |v: &Value| json!(if v.as_bool().unwrap() { 1 } else { 0 });
    let mut doc = match geometry {
        "LineString" => json!({"type":"line", "paint": {
            "line-color": expression("LineStyle", "color", colour, css(defaults["metadata"]["kml"]["LineStyle"]["color"].as_str().unwrap())),
            "line-width": expression("LineStyle", "width", identity, defaults["metadata"]["kml"]["LineStyle"]["width"].clone())}}),
        "Polygon" => json!({"type":"fill", "paint": {
            "fill-color": expression("PolyStyle", "color", colour, css(defaults["metadata"]["kml"]["PolyStyle"]["color"].as_str().unwrap())),
            "fill-opacity": expression("PolyStyle", "fill", opacity, opacity(&defaults["metadata"]["kml"]["PolyStyle"]["fill"]))},
            "boundary":{"type":"line", "paint": {
                "line-color": expression("LineStyle", "color", colour, css(defaults["metadata"]["kml"]["LineStyle"]["color"].as_str().unwrap())),
                "line-width": expression("LineStyle", "width", identity, defaults["metadata"]["kml"]["LineStyle"]["width"].clone()),
                "line-opacity": expression("PolyStyle", "outline", opacity, opacity(&defaults["metadata"]["kml"]["PolyStyle"]["outline"]))}}}),
        _ => json!({"type":"symbol", "paint": {}, "layout": {
            "icon-image": expression("IconStyle", "href", identity, json!("")),
            "icon-size": expression("IconStyle", "scale", identity, defaults["metadata"]["kml"]["IconStyle"]["scale"].clone()),
            "icon-rotate": expression("IconStyle", "heading", identity, defaults["metadata"]["kml"]["IconStyle"]["heading"].clone()),
            "icon-rotation-alignment":defaults["layout"]["icon-rotation-alignment"]}}),
    };
    doc["label"] = json!({"type":"symbol", "layout":{"visibility":defaults["label"]["layout"]["visibility"], "text-field":["get",defaults["metadata"]["label_field"]],
        "text-size":expression("LabelStyle", "scale", identity, defaults["metadata"]["kml"]["LabelStyle"]["scale"].clone())},
        "paint":{"text-color":expression("LabelStyle", "color", colour, css(defaults["metadata"]["kml"]["LabelStyle"]["color"].as_str().unwrap()))}});
    let scale = doc["label"]["layout"]["text-size"].take();
    doc["label"]["layout"]["text-size"] =
        json!(["*", defaults["metadata"]["label_size_px"], scale]);
    doc["metadata"] =
        json!({"kml_styles": styles, "kml_images": images.into_values().collect::<Vec<_>>()});
    // Colours are feature data rather than one categorical branch per row.
    let default_color = css(defaults["metadata"]["kml"]["LineStyle"]["color"]
        .as_str()
        .unwrap());
    match geometry {
        "LineString" => {
            doc["paint"]["line-color"] =
                json!(["coalesce", ["get", "kml_line_color"], default_color])
        }
        "Polygon" => {
            doc["paint"]["fill-color"] = json!([
                "coalesce",
                ["get", "kml_fill_color"],
                css(defaults["metadata"]["kml"]["PolyStyle"]["color"]
                    .as_str()
                    .unwrap())
            ]);
            doc["boundary"]["paint"]["line-color"] =
                json!(["coalesce", ["get", "kml_line_color"], default_color]);
        }
        _ => {}
    }
    doc["label"]["paint"]["text-color"] = json!([
        "coalesce",
        ["get", "kml_label_color"],
        css(defaults["metadata"]["kml"]["LabelStyle"]["color"]
            .as_str()
            .unwrap())
    ]);
    Some(doc)
}

/// The sole pre-resolver document read. The integration resolver must supply
/// this exact tuple and a typed style document; no old-shape reader exists.
pub fn governed_document<'a>(
    catalogue: &'a Value,
    entity: &str,
    source: &str,
    target: &str,
    role: &str,
) -> Result<&'a Value, String> {
    catalogue
        .get("documents")
        .and_then(Value::as_array)
        .and_then(|entries| {
            entries.iter().find(|e| {
                e.get("entity_class").and_then(Value::as_str) == Some(entity)
                    && e.get("source_kind").and_then(Value::as_str) == Some(source)
                    && e.get("target").and_then(Value::as_str) == Some(target)
                    && e.get("role").and_then(Value::as_str) == Some(role)
            })
        })
        .and_then(|e| e.get("document"))
        .filter(|d| d.get("type").and_then(Value::as_str).is_some())
        .ok_or_else(|| format!("missing governed style ({entity}, {source}, {target}, {role})"))
}
