//! Test-only template equivariance and source guards. Never a runtime default.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn fixture_config() -> Value {
    serde_json::from_str(include_str!("../fixtures/network-template.json")).unwrap()
}
pub fn fixture_job() -> Value {
    let mut job: Value = serde_json::from_str(include_str!("../fixtures/lv-job.json")).unwrap();
    job["config_dfs"] = fixture_config();
    job
}

/// Explicit migration of existing unit fixture identities to numeric geometry.
/// The authored areas deliberately differ from digits in those fixture names.
/// No runtime catalogue or designation parser supplies this test data.
pub fn fixture_unit_wire_catalogue() -> Value {
    serde_json::from_str(include_str!("../fixtures/unit-wire-catalogue.json")).unwrap()
}

/// A simultaneous bijection. Reversing applies the inverse, never lowercases or
/// normalizes output. Keys (rule ids, cumulative-load categories) also rename.
#[derive(Debug, Clone)]
pub struct Alien {
    pub forward: BTreeMap<String, String>,
}
impl Alien {
    pub fn from_template(template: &Value) -> Self {
        let mut tokens = BTreeSet::new();
        fn strings(value: &Value, tokens: &mut BTreeSet<String>) {
            match value {
                Value::String(s) if !s.is_empty() => {
                    tokens.insert(s.clone());
                }
                Value::Array(items) => {
                    for v in items {
                        strings(v, tokens);
                    }
                }
                _ => {}
            }
        }
        fn walk(value: &Value, tokens: &mut BTreeSet<String>) {
            match value {
                Value::Object(map) => {
                    for (key, v) in map {
                        if matches!(key.as_str(), "aliases" | "misspelled_names" | "accepts") {
                            if let Some(text) =
                                v.as_str().map(str::trim).filter(|s| s.starts_with('['))
                                && let Ok(decoded @ Value::Array(_)) =
                                    serde_json::from_str::<Value>(text)
                            {
                                strings(&decoded, tokens);
                            } else if let Some(text) = v.as_str().filter(|s| s.contains("##")) {
                                tokens.extend(
                                    text.split("##")
                                        .map(str::trim)
                                        .filter(|s| !s.is_empty())
                                        .map(str::to_owned),
                                );
                            } else {
                                strings(v, tokens);
                            }
                        }
                        if matches!(
                            key.as_str(),
                            "clean_name"
                                | "simple_match"
                                | "base_structure"
                                | "result_structure"
                                | "main_cable"
                                | "cable_to_db"
                                | "min_cable_size"
                                | "material_type"
                                | "name"
                                | "struct_type"
                                | "assembly_type"
                                | "default_material"
                                | "material"
                                | "meter_type"
                                | "service_cable_size"
                                | "cable_size"
                                | "lv_cable_sizes_bundles"
                                | "standard_cables_size_transfo_to_db"
                                | "pole_class"
                                | "feature_code"
                                | "standard"
                                | "utility"
                                | "clause"
                                | "criterion"
                                | "diversity"
                                | "currency"
                                | "vector_group"
                                | "pole_prefix"
                                | "line_prefix"
                                | "customer_prefix"
                        ) {
                            strings(v, tokens);
                        }
                        if matches!(key.as_str(), "lv_poles_rules" | "category_loads") {
                            let decoded = v
                                .as_str()
                                .and_then(|text| serde_json::from_str::<Value>(text).ok());
                            if let Some(rows) = v
                                .as_object()
                                .or_else(|| decoded.as_ref().and_then(Value::as_object))
                            {
                                tokens.extend(rows.keys().cloned());
                            }
                        }
                        if key == "project_settings"
                            && let Some(rows) = v.as_array()
                        {
                            for row in rows {
                                if row["parameter"].as_str().is_some_and(|parameter| {
                                    matches!(
                                        parameter,
                                        "lv_poles_rule_set"
                                            | "default_category"
                                            | "minimum_cable_size"
                                            | "max_cable_size"
                                            | "material_type"
                                            | "label_set"
                                            | "load_profile"
                                    )
                                }) {
                                    strings(&row["value"], tokens);
                                }
                            }
                        }
                        if key == "domains"
                            && let Some(domains) = v.as_object()
                        {
                            for rows in domains.values().filter_map(Value::as_object) {
                                tokens.extend(rows.keys().filter(|k| !k.starts_with('_')).cloned());
                            }
                        }
                        walk(v, tokens);
                    }
                }
                Value::Array(items) => {
                    for v in items {
                        walk(v, tokens);
                    }
                }
                Value::String(text)
                    if text.trim_start().starts_with('{') || text.trim_start().starts_with('[') =>
                {
                    if let Ok(value) = serde_json::from_str::<Value>(text) {
                        walk(&value, tokens);
                    }
                }
                _ => {}
            }
        }
        walk(template, &mut tokens);
        // Keep Unicode case-fold equivalence as well as exact-token identity.
        // Compute publishes normalized lookup keys in its method evidence; a
        // bijection that destroys the alias case relation would manufacture a
        // failure in the harness itself. Case variants receive distinct ASCII
        // case bits in one opaque base id; their lowercase is the same base id.
        let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for token in tokens {
            groups.entry(token.to_lowercase()).or_default().push(token);
        }
        let mut forward = BTreeMap::new();
        for (index, (normalized, mut members)) in groups.into_iter().enumerate() {
            if !members.contains(&normalized) {
                members.push(normalized.clone());
            }
            members.sort();
            let hash = format!("{:x}", Sha256::digest(normalized.as_bytes()));
            let width = (members.len() as u64).ilog2() + 1;
            let mut variant = 1u64;
            for token in members {
                let code = if token == normalized {
                    0
                } else {
                    let n = variant;
                    variant += 1;
                    n
                };
                let bits: String = (0..width)
                    .map(|bit| if code & (1u64 << bit) == 0 { 'q' } else { 'Q' })
                    .collect();
                // Hash first deliberately scrambles lexical order. An ordinal
                // prefix would preserve minimum-name defaults under renaming.
                let target = format!("étranger,住宅·{}·{index:08x}·{bits}", &hash[..12]);
                forward.insert(token, target);
            }
        }
        assert_eq!(
            forward.len(),
            forward.values().collect::<BTreeSet<_>>().len()
        );
        assert!(forward.values().all(|v| !forward.contains_key(v)));
        Self { forward }
    }
    pub fn apply(&self, value: &Value) -> Value {
        transform(value, &self.forward)
    }
    pub fn restore(&self, value: &Value) -> Value {
        let inverse = self
            .forward
            .iter()
            .map(|(a, b)| (b.clone(), a.clone()))
            .collect();
        transform(value, &inverse)
    }
    pub fn rename(&self, value: &str) -> String {
        replace(value, &self.forward)
    }
}
fn replace(text: &str, map: &BTreeMap<String, String>) -> String {
    Replacements::new(map).replace(text)
}

struct Replacements<'a> {
    map: &'a BTreeMap<String, String>,
    prefixes: BTreeMap<char, Vec<(&'a str, &'a str)>>,
}
impl<'a> Replacements<'a> {
    fn new(map: &'a BTreeMap<String, String>) -> Self {
        let mut prefixes: BTreeMap<char, Vec<(&str, &str)>> = BTreeMap::new();
        for (source, target) in map {
            prefixes
                .entry(source.chars().next().expect("nonempty vocabulary token"))
                .or_default()
                .push((source, target));
        }
        for entries in prefixes.values_mut() {
            entries.sort_by_key(|(source, _)| std::cmp::Reverse(source.len()));
        }
        Self { map, prefixes }
    }
    fn replace(&self, text: &str) -> String {
        self.replace_in(text, KeyContext::Schema)
    }
    fn replace_in(&self, text: &str, context: KeyContext) -> String {
        let map = self.map;
        if let Some(value) = map.get(text) {
            return value.clone();
        }
        // Encoded JSON sheet cells remain JSON cells. Their inner keys/references
        // participate in exactly the same transform as object-shaped cells.
        if (text.trim_start().starts_with('{') || text.trim_start().starts_with('['))
            && let Ok(value) = serde_json::from_str::<Value>(text)
        {
            let _ = value; // preserve authored whitespace and numeric spellings.
            struct Frame {
                context: KeyContext,
                key: Option<String>,
                object: bool,
                preserve: bool,
            }
            let mut stack: Vec<Frame> = Vec::new();
            let mut out = String::new();
            let mut at = 0;
            while at < text.len() {
                if text.as_bytes()[at] != b'"' {
                    let ch = text[at..].chars().next().unwrap();
                    match ch {
                        '{' | '[' => {
                            let (child, preserve) =
                                stack.last().map_or((context, false), |parent| {
                                    let child =
                                        parent.key.as_deref().map_or(parent.context, |key| {
                                            child_context(parent.context, key)
                                        });
                                    let preserve = parent.preserve
                                        || parent.key.as_deref().is_some_and(|key| {
                                            !vocabulary_key(parent.context, key) && wire_value(key)
                                        });
                                    (child, preserve)
                                });
                            stack.push(Frame {
                                context: child,
                                key: None,
                                object: ch == '{',
                                preserve,
                            });
                        }
                        '}' | ']' => {
                            stack.pop().expect("validated JSON container");
                        }
                        ',' => {
                            if let Some(frame) = stack.last_mut() {
                                frame.key = None;
                            }
                        }
                        _ => {}
                    }
                    out.push(ch);
                    at += ch.len_utf8();
                    continue;
                }
                let start = at;
                at += 1;
                let mut escaped = false;
                while at < text.len() {
                    let byte = text.as_bytes()[at];
                    at += 1;
                    if escaped {
                        escaped = false;
                    } else if byte == b'\\' {
                        escaped = true;
                    } else if byte == b'"' {
                        break;
                    }
                }
                let literal = &text[start..at];
                let decoded: String = serde_json::from_str(literal).unwrap();
                let frame = stack.last().expect("validated JSON string container");
                let is_key = frame.object && text[at..].trim_start().starts_with(':');
                let preserve = frame.preserve
                    || (!is_key
                        && frame.key.as_deref().is_some_and(|key| {
                            !vocabulary_key(frame.context, key) && wire_value(key)
                        }));
                let translated = if preserve || (is_key && !vocabulary_key(frame.context, &decoded))
                {
                    decoded.clone()
                } else {
                    let child = frame
                        .key
                        .as_deref()
                        .map_or(frame.context, |key| child_context(frame.context, key));
                    self.replace_in(&decoded, child)
                };
                if is_key {
                    stack.last_mut().unwrap().key = Some(decoded.clone());
                }
                if translated == decoded {
                    out.push_str(literal);
                } else {
                    out.push_str(&serde_json::to_string(&translated).unwrap());
                }
            }
            return out;
        }
        let mut out = String::new();
        let mut offset = 0;
        while offset < text.len() {
            let rest = &text[offset..];
            let found = self
                .prefixes
                .get(&rest.chars().next().unwrap())
                .and_then(|tokens| {
                    tokens.iter().find(|(key, _)| {
                        rest.starts_with(key)
                            && (offset == 0
                                || !identifier_continues(
                                    text[..offset].chars().next_back().unwrap(),
                                ))
                            && (key.len() == rest.len()
                                || !identifier_continues(rest[key.len()..].chars().next().unwrap()))
                    })
                });
            if let Some((key, value)) = found {
                out.push_str(value);
                offset += key.len();
            } else {
                let ch = rest.chars().next().unwrap();
                out.push(ch);
                offset += ch.len_utf8();
            }
        }
        out
    }
}
fn transform(value: &Value, map: &BTreeMap<String, String>) -> Value {
    transform_in(value, &Replacements::new(map), KeyContext::Schema)
}

#[derive(Clone, Copy)]
enum KeyContext {
    Schema,
    Domains,
    Vocabulary,
}
fn identifier_continues(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
fn wire_value(field: &str) -> bool {
    matches!(
        field,
        "know_columns"
            | "existing_status"
            | "drafting_status"
            | "connection_phase"
            | "layer"
            | "layer_name"
            | "source_layer"
            | "schema"
            | "method"
            | "method_id"
            | "severity"
            | "code"
            | "status"
            | "dtype"
            | "parameter"
            | "position"
            | "role"
            | "transformer_nature"
            | "phase_source"
    )
}
fn vocabulary_key(context: KeyContext, key: &str) -> bool {
    matches!(context, KeyContext::Vocabulary)
        || key
            .rsplit_once(',')
            .is_some_and(|(_, watts)| watts.trim().parse::<f64>().is_ok())
}
fn child_context(context: KeyContext, key: &str) -> KeyContext {
    if matches!(context, KeyContext::Domains) {
        KeyContext::Vocabulary
    } else if key == "domains" {
        KeyContext::Domains
    } else if matches!(
        key,
        "lv_poles_rules"
            | "category_loads"
            | "initial_load_w"
            | "saturation_load_w"
            | "cum_load"
            | "load_detail"
    ) {
        KeyContext::Vocabulary
    } else {
        KeyContext::Schema
    }
}
fn transform_in(value: &Value, replacements: &Replacements<'_>, context: KeyContext) -> Value {
    match value {
        Value::Object(rows) => Value::Object(
            rows.iter()
                .map(|(k, v)| {
                    let vocabulary_key = vocabulary_key(context, k);
                    let renamed = if vocabulary_key {
                        replacements.replace(k)
                    } else {
                        k.clone()
                    };
                    let child = child_context(context, k);
                    let translated = if !vocabulary_key && wire_value(k) {
                        v.clone()
                    } else {
                        transform_in(v, replacements, child)
                    };
                    (renamed, translated)
                })
                .collect(),
        ),
        Value::Array(rows) => Value::Array(
            rows.iter()
                .map(|v| transform_in(v, replacements, context))
                .collect(),
        ),
        Value::String(text) => Value::String(replacements.replace_in(text, context)),
        _ => value.clone(),
    }
}

/// Seed vocabulary is a scan input only, never the fixture's engineering data.
pub fn vocabulary_tokens() -> Vec<String> {
    let doc: Value =
        serde_json::from_str(include_str!("../fixtures/master-vocabulary.json")).unwrap();
    // Lifecycle cleaning and numeric/single-character aliases overlap stable
    // Boolean, physical and XML/workbook encodings. Parameter perturbation and
    // the behavioral alias transform cover them; a text scan cannot distinguish
    // their label use from transport/layout numbers. This is provenance, never
    // a production-file or existing-violation baseline.
    doc["tokens"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(token, paths)| {
            token.chars().count() > 1
                && token.parse::<f64>().is_err()
                && paths.as_array().unwrap().iter().any(|p| {
                    !p["path"]
                        .as_str()
                        .unwrap()
                        .starts_with("/domains/existing/")
                })
        })
        .map(|(token, _)| token.clone())
        .collect()
}

fn compound_vocabulary_tokens() -> BTreeSet<String> {
    let doc: Value =
        serde_json::from_str(include_str!("../fixtures/master-vocabulary.json")).unwrap();
    doc["tokens"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, paths)| {
            paths.as_array().unwrap().iter().any(|p| {
                matches!(
                    p["kind"].as_str(),
                    Some("canonical" | "catalogue" | "default-vocabulary")
                ) && !p["path"]
                    .as_str()
                    .unwrap()
                    .starts_with("/domains/existing/")
            })
        })
        .map(|(token, _)| token.clone())
        .collect()
}

fn literal_contains_vocabulary(value: &str, token: &str, compound: &BTreeSet<String>) -> bool {
    // Short match ids and aliases ("service", "three", "exists") are also
    // ordinary words in engineering/error prose. Guard their entire literal
    // use; only authored canonical/catalogue labels match inside compounds.
    value == token || (compound.contains(token) && contains_token(value, token))
}

fn test_only(attrs: &[syn::Attribute]) -> bool {
    fn requires_test(meta: &syn::Meta) -> bool {
        match meta {
            syn::Meta::Path(path) => path.is_ident("test"),
            syn::Meta::List(list) => {
                use syn::parse::Parser;
                let parser =
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated;
                let Ok(children) = parser.parse2(list.tokens.clone()) else {
                    return false;
                };
                if list.path.is_ident("all") {
                    children.iter().any(requires_test)
                } else if list.path.is_ident("any") {
                    !children.is_empty() && children.iter().all(requires_test)
                } else {
                    false
                }
            }
            _ => false,
        }
    }
    attrs.iter().any(|a| {
        a.path().is_ident("test")
            || (a.path().is_ident("cfg")
                && a.meta.require_list().is_ok_and(|m| {
                    syn::parse2::<syn::Meta>(m.tokens.clone()).is_ok_and(|m| requires_test(&m))
                }))
    })
}

fn contains_token(value: &str, token: &str) -> bool {
    fn continues_identifier(ch: char) -> bool {
        ch.is_alphanumeric() || ch == '_'
    }
    value.match_indices(token).any(|(at, _)| {
        (at == 0 || !continues_identifier(value[..at].chars().next_back().unwrap()))
            && (at + token.len() == value.len()
                || !continues_identifier(value[at + token.len()..].chars().next().unwrap()))
    })
}

// Git objects and checked-in inventories use slash paths on every host.
fn repository_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_str().unwrap())
        .collect::<Vec<_>>()
        .join("/")
}
/// A single matching literal/token pair. Hashes distinguish separate literals
/// in the same file; line numbers are diagnostics, not occurrence identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct LiteralOccurrence {
    pub path: String,
    pub line: usize,
    pub token: String,
    pub literal_sha256: String,
}
impl LiteralOccurrence {
    fn new(path: &Path, line: usize, token: &str, literal: &str) -> Self {
        Self {
            path: repository_path(path),
            line,
            token: token.into(),
            literal_sha256: format!("{:x}", Sha256::digest(literal.as_bytes())),
        }
    }
    fn identity(&self) -> (&str, &str, &str) {
        (&self.path, &self.token, &self.literal_sha256)
    }
    fn diagnostic(&self) -> String {
        format!(
            "{}:{}: template token {:?}",
            self.path, self.line, self.token
        )
    }
}
#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LiteralBaseline {
    pub count: usize,
    pub occurrences: Vec<LiteralOccurrence>,
}
pub fn literal_baseline(repo: &Path) -> LiteralBaseline {
    let occurrences = source_occurrences(repo);
    LiteralBaseline {
        count: occurrences.len(),
        occurrences,
    }
}
/// Retained for callers that need a plain finding list rather than a ratchet.
pub fn source_violations(repo: &Path) -> Vec<String> {
    source_occurrences(repo)
        .iter()
        .map(LiteralOccurrence::diagnostic)
        .collect()
}
fn ratchet_diagnostics(baseline: &LiteralBaseline, current: &[LiteralOccurrence]) -> Vec<String> {
    let mut errors = Vec::new();
    if baseline.count != baseline.occurrences.len() {
        errors.push(format!(
            "baseline count {} disagrees with its {} occurrences",
            baseline.count,
            baseline.occurrences.len()
        ));
    }
    if current.len() != baseline.count {
        errors.push(format!("literal count: baseline {}, current {}; removals require lowering the baseline in the same change; increases are forbidden", baseline.count, current.len()));
    }
    let mut remaining = BTreeMap::new();
    for occurrence in &baseline.occurrences {
        *remaining.entry(occurrence.identity()).or_insert(0usize) += 1;
    }
    for occurrence in current {
        let count = remaining.entry(occurrence.identity()).or_default();
        if *count > 0 {
            *count -= 1;
        } else {
            errors.push(format!("new occurrence: {}", occurrence.diagnostic()));
        }
    }
    let removed: usize = remaining.values().sum();
    if removed > 0 {
        errors.push(format!("{removed} baseline occurrences removed; update the checked-in inventory and lower its count"));
    }
    errors
}
/// Enforce exact debt accounting, new-occurrence diagnostics and a historical
/// count ceiling. The first checked-in snapshot establishes that ceiling.
pub fn assert_source_ratchet(repo: &Path, baseline_path: &Path) {
    let history_depth = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--is-shallow-repository"])
        .output()
        .expect("git history depth");
    assert!(
        history_depth.status.success(),
        "cannot determine baseline history depth"
    );
    assert!(
        String::from_utf8(history_depth.stdout).unwrap().trim() == "false",
        "template literal ratchet requires full Git history; fetch with --unshallow or set CI checkout fetch-depth: 0"
    );
    let bytes = std::fs::read(repo.join(baseline_path)).unwrap();
    let baseline: LiteralBaseline = serde_json::from_slice(&bytes).unwrap();
    let mut errors = ratchet_diagnostics(&baseline, &source_occurrences(repo));
    // Look at commits changing this baseline, rather than HEAD's parent: merge
    // commits and commits touching unrelated files must not reset the ceiling.
    let history = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["log", "--format=%H", "--"])
        .arg(baseline_path)
        .output()
        .expect("git baseline history");
    assert!(history.status.success(), "cannot read baseline git history");
    for commit in String::from_utf8(history.stdout).unwrap().lines() {
        let previous = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .arg("show")
            .arg(format!("{commit}:./{}", repository_path(baseline_path)))
            .output()
            .expect("git previous baseline");
        assert!(
            previous.status.success(),
            "cannot read historical literal baseline"
        );
        // Compare parsed content: CRLF checkout conversion or JSON formatting
        // must not hide the predecessor's ceiling by changing byte equality.
        let previous: LiteralBaseline = serde_json::from_slice(&previous.stdout).unwrap();
        if previous == baseline {
            continue;
        }
        errors.extend(
            ratchet_diagnostics(&previous, &baseline.occurrences)
                .into_iter()
                .filter(|message| message.starts_with("new occurrence:"))
                .map(|message| format!("historical baseline admits {message}")),
        );
        if baseline.count > previous.count {
            errors.push(format!(
                "baseline increase forbidden: {} -> {}",
                previous.count, baseline.count
            ));
        }
    }
    assert!(
        errors.is_empty(),
        "template literal ratchet failed:\n{}",
        errors.join("\n")
    );
}

/// Parse production Rust, including macros, and scan quoted literals in embedded
/// JSON/HTML/JS/TS and script adapters; skip comments and test-only source.
/// Every production occurrence counts; baselines record debt, never exempt files.
pub fn source_occurrences(repo: &Path) -> Vec<LiteralOccurrence> {
    use syn::visit::Visit;
    struct Scan<'a> {
        tokens: &'a [String],
        compound: &'a BTreeSet<String>,
        file: &'a Path,
        repo: &'a Path,
        violations: Vec<LiteralOccurrence>,
    }
    impl Scan<'_> {
        fn check(&mut self, lit: &syn::LitStr) {
            let value = lit.value();
            for token in self.tokens {
                let hit = literal_contains_vocabulary(&value, token, self.compound);
                if hit {
                    self.violations.push(LiteralOccurrence::new(
                        self.file.strip_prefix(self.repo).unwrap(),
                        lit.span().start().line,
                        token,
                        &value,
                    ));
                }
            }
        }
        fn macro_tokens(&mut self, stream: proc_macro2::TokenStream) {
            for token in stream {
                match token {
                    proc_macro2::TokenTree::Literal(lit) => {
                        if let Ok(mut s) = syn::parse_str::<syn::LitStr>(&lit.to_string()) {
                            s.set_span(lit.span());
                            self.check(&s);
                        }
                    }
                    proc_macro2::TokenTree::Group(group) => self.macro_tokens(group.stream()),
                    _ => {}
                }
            }
        }
    }
    impl<'ast> Visit<'ast> for Scan<'_> {
        fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
            if !attribute.path().is_ident("doc") {
                syn::visit::visit_attribute(self, attribute);
            }
        }
        fn visit_lit_str(&mut self, lit: &'ast syn::LitStr) {
            self.check(lit);
        }
        fn visit_macro(&mut self, m: &'ast syn::Macro) {
            self.macro_tokens(m.tokens.clone());
        }
        fn visit_item(&mut self, item: &'ast syn::Item) {
            let attrs = match item {
                syn::Item::Mod(i) => &i.attrs,
                syn::Item::Fn(i) => &i.attrs,
                syn::Item::Const(i) => &i.attrs,
                syn::Item::Static(i) => &i.attrs,
                syn::Item::Impl(i) => &i.attrs,
                syn::Item::Struct(i) => &i.attrs,
                syn::Item::Enum(i) => &i.attrs,
                _ => &[][..],
            };
            if !test_only(attrs) {
                syn::visit::visit_item(self, item);
            }
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            if !test_only(&item.attrs) {
                syn::visit::visit_impl_item_fn(self, item);
            }
        }
    }
    fn collect(repo: &Path, dir: &Path, files: &mut Vec<std::path::PathBuf>) {
        if !dir.exists() {
            return;
        }
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            let name = p.file_name().unwrap().to_str().unwrap();
            if p.is_dir() {
                // Job outputs may contain complete copied repositories. Only
                // root scratch is excluded; src/out remains production code.
                if p == repo.join("out") || p == repo.join("_discardable") {
                    continue;
                }
                if !matches!(
                    name,
                    "target"
                        | "node_modules"
                        | "vendor"
                        | "tests"
                        | "fixtures"
                        | "docs"
                        | "backlog"
                        | ".git"
                        | "ds-template-test-support"
                        | "__tests__"
                ) {
                    collect(repo, &p, files);
                }
            } else if p.extension().is_some_and(|ext| {
                matches!(
                    ext.to_str(),
                    Some("rs" | "json" | "html" | "svg" | "js" | "mjs" | "ts" | "py" | "sh")
                )
            }) && p
                .components()
                .any(|c| matches!(c.as_os_str().to_str(), Some("src" | "assets" | "scripts")))
                && name != "tests.rs"
                && !name.ends_with("_test.rs")
                && !name.ends_with("_tests.rs")
                && !name.starts_with("test_")
            {
                files.push(p);
            }
        }
    }
    let tokens = vocabulary_tokens();
    let compound = compound_vocabulary_tokens();
    let mut files = Vec::new();
    collect(repo, repo, &mut files);
    assert!(
        !files.is_empty(),
        "no production sources found in {}",
        repo.display()
    );
    let mut out = Vec::new();
    for file in files {
        let code = std::fs::read_to_string(&file).unwrap();
        if file.extension().is_none_or(|ext| ext != "rs") {
            // Embedded JSON/HTML/JS and executable adapters are production too.
            // Scan quoted literals without treating comments as catalogue ids.
            let chars: Vec<char> = code.chars().collect();
            let mut at = 0;
            let mut line = 1;
            while at < chars.len() {
                if chars[at] == '\n' {
                    line += 1;
                    at += 1;
                    continue;
                }
                if chars[at..].starts_with(&['<', '!', '-', '-']) {
                    at += 4;
                    while at < chars.len() {
                        if chars[at] == '\n' {
                            line += 1;
                        }
                        if chars[at..].starts_with(&['-', '-', '>']) {
                            at += 3;
                            break;
                        }
                        at += 1;
                    }
                    continue;
                }
                if chars[at] == '#' || (chars[at] == '/' && chars.get(at + 1) == Some(&'/')) {
                    while at < chars.len() && chars[at] != '\n' {
                        at += 1;
                    }
                    continue;
                }
                if chars[at] == '/' && chars.get(at + 1) == Some(&'*') {
                    at += 2;
                    while at < chars.len() {
                        if chars[at] == '\n' {
                            line += 1;
                        }
                        if chars[at] == '*' && chars.get(at + 1) == Some(&'/') {
                            at += 2;
                            break;
                        }
                        at += 1;
                    }
                    continue;
                }
                if matches!(chars[at], '\'' | '"' | '`') {
                    let quote = chars[at];
                    let literal_start = at;
                    let start_line = line;
                    at += 1;
                    let mut value = String::new();
                    while at < chars.len() {
                        let ch = chars[at];
                        at += 1;
                        if ch == '\n' {
                            line += 1;
                        }
                        if ch == quote {
                            break;
                        }
                        if ch == '\\' && at < chars.len() {
                            value.push(chars[at]);
                            at += 1;
                        } else {
                            value.push(ch);
                        }
                    }
                    if quote == '"' {
                        let raw: String = chars[literal_start..at].iter().collect();
                        if let Ok(decoded) = serde_json::from_str::<String>(&raw) {
                            value = decoded;
                        }
                    }
                    for token in tokens
                        .iter()
                        .filter(|token| literal_contains_vocabulary(&value, token, &compound))
                    {
                        out.push(LiteralOccurrence::new(
                            file.strip_prefix(repo).unwrap(),
                            start_line,
                            token,
                            &value,
                        ));
                    }
                } else {
                    at += 1;
                }
            }
            if file
                .extension()
                .is_some_and(|ext| ext == "html" || ext == "svg")
            {
                // Rendered text is production content too. Remove markup and
                // HTML comments while retaining positions for useful findings.
                let mut visible = String::new();
                let mut at = 0;
                while at < code.len() {
                    let rest = &code[at..];
                    let end = if rest.starts_with("<!--") {
                        rest.find("-->").map(|n| n + 3).unwrap_or(rest.len())
                    } else if rest.starts_with('<') {
                        rest.find('>').map(|n| n + 1).unwrap_or(rest.len())
                    } else {
                        let ch = rest.chars().next().unwrap();
                        visible.push(ch);
                        at += ch.len_utf8();
                        continue;
                    };
                    for ch in rest[..end].chars() {
                        visible.push(if ch == '\n' { '\n' } else { ' ' });
                    }
                    at += end;
                }
                for token in &tokens {
                    for (line, value) in visible.lines().enumerate() {
                        if literal_contains_vocabulary(value.trim(), token, &compound) {
                            out.push(LiteralOccurrence::new(
                                file.strip_prefix(repo).unwrap(),
                                line + 1,
                                token,
                                value.trim(),
                            ));
                        }
                    }
                }
            }
            continue;
        }
        let ast = syn::parse_file(&code)
            .unwrap_or_else(|e| panic!("cannot guard {}: {e}", file.display()));
        if test_only(&ast.attrs) {
            continue;
        }
        let mut scan = Scan {
            tokens: &tokens,
            compound: &compound,
            file: &file,
            repo,
            violations: Vec::new(),
        };
        scan.visit_file(&ast);
        out.extend(scan.violations);
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn literal_ratchet_requires_same_change_debt_reduction_and_reports_additions() {
        let first = LiteralOccurrence::new(Path::new("src/a.rs"), 3, "fixture-token", "first");
        let second = LiteralOccurrence::new(Path::new("src/a.rs"), 9, "fixture-token", "second");
        let baseline = LiteralBaseline {
            count: 1,
            occurrences: vec![first.clone()],
        };
        assert!(ratchet_diagnostics(&baseline, std::slice::from_ref(&first)).is_empty());
        let mut moved = first.clone();
        moved.line = 100;
        assert!(ratchet_diagnostics(&baseline, &[moved]).is_empty());
        let added = ratchet_diagnostics(&baseline, &[first.clone(), second.clone()]);
        assert!(
            added
                .iter()
                .any(|e| e.contains("new occurrence: src/a.rs:9"))
        );
        assert!(!ratchet_diagnostics(&baseline, &[]).is_empty());
        let lowered = LiteralBaseline {
            count: 0,
            occurrences: vec![],
        };
        assert!(ratchet_diagnostics(&lowered, &[]).is_empty());
        // A replacement at the same total is still a newly introduced literal.
        assert!(
            ratchet_diagnostics(&baseline, &[second])
                .iter()
                .any(|e| e.contains("new occurrence"))
        );
        // Identical literals on one line are separate occurrences, never deduped.
        assert!(
            ratchet_diagnostics(&baseline, &[first.clone(), first])
                .iter()
                .any(|e| e.contains("new occurrence"))
        );
        let forged = LiteralBaseline {
            count: 0,
            occurrences: baseline.occurrences,
        };
        assert!(
            ratchet_diagnostics(&forged, &[])
                .iter()
                .any(|e| e.contains("disagrees"))
        );
    }
    #[test]
    fn source_ratchet_excludes_root_job_copies_but_scans_nested_source_out() {
        let root = std::path::PathBuf::from(std::env::var_os("TMPDIR").unwrap()).join(format!(
            "literal-job-copies-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for file in [
            "src/lib.rs",
            "src/out/real.rs",
            "out/gate/source/src/lib.rs",
            "_discardable/gate/src/lib.rs",
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "pub const LABEL: &str = \"S140\";\n").unwrap();
        }
        let found = source_occurrences(&root);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found.iter().all(|row| row.path.starts_with("src/")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|row| row.path == "src/out/real.rs"),
            "{found:?}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn historical_ratchet_rejects_raised_working_and_committed_baselines() {
        let root = std::env::var_os("TMPDIR").expect("tests require explicit TMPDIR");
        let repo = std::path::PathBuf::from(root).join(format!(
            "literal-ratchet-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args([
                    "-c",
                    "user.name=Fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        let path = Path::new("baseline.json");
        let save = || {
            std::fs::write(
                repo.join(path),
                serde_json::to_vec_pretty(&literal_baseline(&repo)).unwrap(),
            )
            .unwrap()
        };
        git(&["init", "--quiet"]);
        std::fs::write(
            repo.join("src/lib.rs"),
            "pub const LABEL: &str = \"Residential\";\n",
        )
        .unwrap();
        save();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "initial baseline"]);
        assert_source_ratchet(&repo, path);
        // A depth-one checkout cannot establish a historical ceiling.
        git(&[
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "history predecessor",
        ]);
        let shallow = repo.with_file_name(format!(
            "{}-shallow",
            repo.file_name().unwrap().to_str().unwrap()
        ));
        let cloned = std::process::Command::new("git")
            .args(["clone", "--quiet", "--depth", "1", "--no-local"])
            .arg(&repo)
            .arg(&shallow)
            .output()
            .unwrap();
        assert!(
            cloned.status.success(),
            "{}",
            String::from_utf8_lossy(&cloned.stderr)
        );
        let rejection =
            std::panic::catch_unwind(|| assert_source_ratchet(&shallow, path)).unwrap_err();
        let message = rejection
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| rejection.downcast_ref::<&str>().copied())
            .expect("guard panic message");
        assert!(message.contains("requires full Git history"), "{message}");
        std::fs::remove_dir_all(shallow).unwrap();

        std::fs::write(
            repo.join("src/lib.rs"),
            "pub const LABEL: &str = \"S140\";\n",
        )
        .unwrap();
        save();
        assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "forged replacement"]);
        assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
        std::fs::write(
            repo.join("src/lib.rs"),
            "pub const LABEL: &str = \"fixture-token\";\n",
        )
        .unwrap();
        assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
        save();
        assert_source_ratchet(&repo, path);
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "lower baseline"]);
        assert_source_ratchet(&repo, path);
        std::fs::write(
            repo.join("src/lib.rs"),
            "pub const LABEL: &str = \"Residential\";\n",
        )
        .unwrap();
        save();
        assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "forged increase"]);
        assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
        // Formatting-only commits must not reset an older, lower ceiling.
        std::fs::write(
            repo.join(path),
            serde_json::to_vec(&literal_baseline(&repo)).unwrap(),
        )
        .unwrap();
        git(&["add", "."]);
        git(&["commit", "--quiet", "-m", "format forged baseline"]);
        assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
        std::fs::remove_dir_all(repo).unwrap();
    }
    #[test]
    fn simultaneous_transform_is_bijective_and_preserves_shape() {
        let config = fixture_config();
        let alien = Alien::from_template(&config);
        assert!(alien.forward.len() > 25);
        assert!(alien.forward.values().all(|v| v.len() <= 64));
        assert_eq!(alien.restore(&alien.apply(&config)), config);
        assert_eq!(alien.restore(&alien.apply(&fixture_job())), fixture_job());
        let loads = serde_json::json!({format!("{}, 173",config["cust_category"][0]["clean_name"].as_str().unwrap()):2});
        assert_eq!(alien.restore(&alien.apply(&loads)), loads);
        assert_ne!(alien.apply(&loads), loads);
        assert!(
            alien
                .forward
                .values()
                .all(|v| v.chars().any(|c| !c.is_ascii()))
        );
    }
    #[test]
    fn the_bijection_commutes_with_unicode_case_normalization() {
        let template = serde_json::json!({"cust_category":[{"clean_name":"Résidentiel","aliases":["RÉSIDENTIEL","résidentiel"]}]});
        let alien = Alien::from_template(&template);
        for original in alien.forward.keys() {
            assert_eq!(
                alien.rename(&original.to_lowercase()),
                alien.rename(original).to_lowercase()
            );
        }
        assert_eq!(alien.restore(&alien.apply(&template)), template);
        assert_eq!(
            alien.forward.len(),
            alien.forward.values().collect::<BTreeSet<_>>().len()
        );
    }

    #[test]
    fn alien_category_ids_scramble_the_fixture_lexical_order() {
        let config = fixture_config();
        let alien = Alien::from_template(&config);
        let mut original: Vec<_> = config["cust_category"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["clean_name"].as_str().unwrap().to_owned())
            .collect();
        original.sort();
        let mut reordered = original.clone();
        reordered.sort_by_key(|name| alien.rename(name));
        assert_ne!(
            original, reordered,
            "the control must expose name-order defaults"
        );
        for name in reordered {
            assert_eq!(alien.restore(&Value::String(alien.rename(&name))), name);
        }
    }

    #[test]
    fn encoded_sheet_cells_collect_and_rename_vocabulary_without_format_drift() {
        let template = serde_json::json!({"transfo_sizes":[{"electrical_params":"{ \"vector_group\": \"Groupe Ω\", \"vk_percent\": 4.70 }"}]});
        let alien = Alien::from_template(&template);
        assert!(alien.forward.contains_key("Groupe Ω"));
        assert_ne!(alien.apply(&template), template);
        assert_eq!(alien.restore(&alien.apply(&template)), template);
    }

    #[test]
    fn alias_cell_encodings_preserve_their_delimiters_and_references() {
        let template = serde_json::json!({"cust_category":[
            {"clean_name":"Catégorie Ω","aliases":"  [ \"résumé\", \"住宅\" ]  "},
            {"clean_name":"Catégorie β","misspelled_names":"ancien Ω##ancien β"}
        ]});
        let project = serde_json::json!({"category":"résumé", "another_category":"ancien β"});
        let alien = Alien::from_template(&template);
        let translated = alien.apply(&template);
        assert!(
            serde_json::from_str::<Value>(
                translated["cust_category"][0]["aliases"].as_str().unwrap()
            )
            .unwrap()
            .is_array()
        );
        assert_eq!(
            translated["cust_category"][1]["misspelled_names"]
                .as_str()
                .unwrap()
                .split("##")
                .count(),
            2
        );
        assert_ne!(alien.apply(&project), project);
        assert_eq!(alien.restore(&translated), template);
        assert_eq!(alien.restore(&alien.apply(&project)), project);
    }

    #[test]
    fn vocabulary_collisions_never_rename_schema_or_lifecycle_wire_values() {
        let template = serde_json::json!({
            "cust_category":[{"clean_name":"existing", "aliases":["category", "service", "1", "calculated"]}],
            "lv_poles_rules":{"table":{"table":[], "voltage_drop":{"standard":"existing"}}}
        });
        let project = serde_json::json!({"gdfs":{"service_cables":{"features":[{"properties":{
            "category":"category", "existing_status":"existing", "connection_phase":"1", "layer_name":"service_cables"
        }}]}}, "status":"calculated", "initial_load_w":{"existing":211}});
        let alien = Alien::from_template(&template);
        let translated = alien.apply(&project);
        let props = &translated["gdfs"]["service_cables"]["features"][0]["properties"];
        assert_eq!(props["category"], alien.rename("category"));
        assert_eq!(props["existing_status"], "existing");
        assert_eq!(props["connection_phase"], "1");
        assert_eq!(props["layer_name"], "service_cables");
        assert_eq!(translated["status"], "calculated");
        assert_eq!(translated["initial_load_w"][alien.rename("existing")], 211);
        let renamed = alien.apply(&template);
        assert!(renamed["lv_poles_rules"][alien.rename("table")]["table"].is_array());
        assert_eq!(alien.restore(&renamed), template);
        assert_eq!(alien.restore(&translated), project);
    }

    #[test]
    fn encoded_vocabulary_maps_keep_the_same_context_and_exact_cell_bytes() {
        let template = serde_json::json!({"cust_category":[{"clean_name":"table", "aliases":["status", "calculated"]}],
            "lv_poles_rules":{"rule Ω":{"voltage_drop":" { \"category_loads\": { \"table\": { \"initial_w\": 173.0, \"saturation_w\": 611.00 } }, \"status\": \"calculated\" } "}}});
        let mut template = template;
        template["category_loads"] =
            serde_json::json!(" { \"extérieur Ω\": { \"initial_w\": 211.0 } } ");
        let alien = Alien::from_template(&template);
        assert!(alien.forward.contains_key("extérieur Ω"));
        let translated = alien.apply(&template);
        let encoded: Value =
            serde_json::from_str(translated["category_loads"].as_str().unwrap()).unwrap();
        assert_eq!(encoded[alien.rename("extérieur Ω")]["initial_w"], 211.0);
        let cell = translated["lv_poles_rules"][alien.rename("rule Ω")]["voltage_drop"]
            .as_str()
            .unwrap();
        let decoded: Value = serde_json::from_str(cell).unwrap();
        assert_eq!(
            decoded["category_loads"][alien.rename("table")]["initial_w"],
            173.0
        );
        assert_eq!(decoded["status"], "calculated");
        assert!(cell.contains("173.0") && cell.contains("611.00"));
        assert_eq!(alien.restore(&translated), template);
        let project =
            serde_json::json!({"category_loads":" { \"table\": { \"initial_w\": 173.0 } } "});
        let renamed = alien.apply(&project);
        let decoded: Value =
            serde_json::from_str(renamed["category_loads"].as_str().unwrap()).unwrap();
        assert!(decoded.get(alien.rename("table")).is_some());
        assert_eq!(alien.restore(&renamed), project);
    }

    #[test]
    fn authored_column_schema_survives_alias_collisions_in_values_and_encoded_json() {
        let template = serde_json::json!({
            "cust_category":[{"clean_name":"fixture Ω", "aliases":["category", "status", "service"]}],
            "know_columns":[{"customers":"category", "common":"status", "service_cables":"service"}],
            "customer":{"category":"category"}
        });
        let alien = Alien::from_template(&template);
        for value in [
            template.clone(),
            Value::String(serde_json::to_string(&template).unwrap()),
        ] {
            let translated = alien.apply(&value);
            let decoded = match &translated {
                Value::String(text) => serde_json::from_str::<Value>(text).unwrap(),
                value => value.clone(),
            };
            assert_eq!(decoded["know_columns"], template["know_columns"]);
            assert_eq!(decoded["customer"]["category"], alien.rename("category"));
            assert_eq!(alien.restore(&translated), value);
        }
    }

    #[test]
    fn scanner_ignores_tests_and_comments_but_finds_production_macros() {
        let dir =
            std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("disk TMPDIR required"))
                .join(format!("template-guard-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(
            dir.join("src/lib.rs"),
            r#"// Residential
/// Residential documentation is allowed, including Rust's lowered doc attribute.
#[cfg(test)] mod tests { fn fixture(){ let _ = "Residential"; } }
#[cfg(all(test, feature="test-helper"))] mod deep_tests { const NAME: &str = "Residential"; }
fn real() { let _=serde_json::json!({"category":"Residential"}); let _="S140 / S190 / S255"; let _="nonResidentialSuffix"; }
"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("assets/labels.json"),
            r#"{"label":"\u0052esidential"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("assets/print.html"),
            "<!-- \"Residential\" -->\n<p>Residential / S140</p>",
        )
        .unwrap();
        let violations = source_violations(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(
            violations
                .iter()
                .filter(|v| v.contains("Residential"))
                .count(),
            3,
            "{violations:?}"
        );
        for token in ["S140", "S190", "S255"] {
            assert!(
                violations.iter().any(|v| v.contains(token)),
                "{violations:?}"
            );
        }
    }
}
