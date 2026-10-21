//! The reference grammar — the one typed thing the CLI, MCP and the desktop
//! all hand this module, parsed here and nowhere else.
//!
//! ```text
//! reference := "dsgrid:" source ":" object
//! source    := "local-" id | "package"
//! object    := "structure:" sel ("," sel)*
//!            | "alignment:" aln
//!            | "alignment:" aln ":" sel ".." sel
//! ```
//!
//! A selector is a structure's engineering number (what a consultant writes:
//! `74`) or its stable id; the resolver decides which. An alignment is named
//! by id or label. The grammar is deliberately closed: a head other than
//! `dsgrid`, a source that is neither shape, an empty or repeated selector is
//! refused by name with the part that failed, because a reference an agent
//! supplied is the ONLY input this module trusts and it has to be exactly
//! right or exactly refused.

use serde::Serialize;

use super::Refusal;

/// The longest reference this module reads. A selector list of a hundred
/// structure numbers fits in a few hundred bytes; past this the caller is
/// sending something other than a reference.
pub const MAX_REFERENCE_BYTES: usize = 4_096;

/// Where a reference's objects live.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// A working copy in this machine's DS Grid catalogue, by its opaque id
    /// exactly as `ds dsgrid model list` reports it (`local-<16 hex>`): the
    /// catalogue mints the `local-` prefix, so the token IS the id.
    Local { id: String },
    /// The `.dsgrid` the command named with `--package <path>`.
    Package,
}

impl Source {
    /// The token the index is keyed by: `local-<id>` or `package`.
    pub fn key(&self) -> String {
        match self {
            Self::Local { id } => id.clone(),
            Self::Package => "package".to_owned(),
        }
    }
}

/// What a reference selects inside its source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Object {
    /// One or more structures, by number or id.
    Structure { selectors: Vec<String> },
    /// One whole alignment, by id or label.
    Alignment { alignment: String },
    /// The stretch of one alignment between two of its structures.
    AlignmentRange {
        alignment: String,
        from: String,
        to: String,
    },
}

/// One parsed reference, carrying the text it came from so every refusal
/// downstream can name it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reference {
    pub reference: String,
    pub source: Source,
    pub object: Object,
}

fn invalid(reference: &str, part: &str, why: impl Into<String>) -> Refusal {
    Refusal::new(
        super::REFERENCE_INVALID,
        format!(
            "reference `{reference}` is not a typed DS object reference: {}",
            why.into()
        ),
        serde_json::json!({ "reference": reference, "part": part }),
    )
}

/// Parse one reference string.
pub fn parse(raw: &str) -> Result<Reference, Refusal> {
    let reference = raw.trim();
    if reference.is_empty() {
        return Err(invalid(raw, "reference", "it is empty"));
    }
    if reference.len() > MAX_REFERENCE_BYTES {
        return Err(invalid(
            reference,
            "reference",
            format!("it is longer than {MAX_REFERENCE_BYTES} bytes"),
        ));
    }
    if reference.chars().any(char::is_control) {
        return Err(invalid(
            reference,
            "reference",
            "it carries a control character",
        ));
    }
    let mut parts = reference.splitn(4, ':');
    let head = parts.next().unwrap_or_default();
    if head != "dsgrid" {
        return Err(invalid(
            reference,
            "head",
            format!("`{head}` is not a reference head; only `dsgrid` is (transformer and survey objects are linked, never shaped)"),
        ));
    }
    let source = match parts.next() {
        Some("package") => Source::Package,
        Some(local) if local.starts_with("local-") => {
            if local.len() == "local-".len() || !crate::local_models::id_is_safe(local) {
                return Err(invalid(
                    reference,
                    "source",
                    format!(
                        "`{local}` is not a working-copy id; use an id from `ds dsgrid model list`"
                    ),
                ));
            }
            Source::Local {
                id: local.to_owned(),
            }
        }
        Some(other) => {
            return Err(invalid(
                reference,
                "source",
                format!("`{other}` is neither `local-<id>` nor `package`"),
            ));
        }
        None => return Err(invalid(reference, "source", "no source follows `dsgrid:`")),
    };
    let object = match parts.next() {
        Some("structure") => {
            let list = parts.next().unwrap_or_default();
            let selectors = selectors(reference, "structure", list)?;
            Object::Structure { selectors }
        }
        Some("alignment") => {
            let rest = parts.next().unwrap_or_default();
            parse_alignment(reference, rest)?
        }
        Some(other) => {
            return Err(invalid(
                reference,
                "object",
                format!("`{other}` is not an object kind; use `structure` or `alignment`"),
            ));
        }
        None => return Err(invalid(reference, "object", "no object follows the source")),
    };
    Ok(Reference {
        reference: reference.to_owned(),
        source,
        object,
    })
}

/// `aln` or `aln:<a>..<b>`. The alignment name is everything up to the LAST
/// `:` that is followed by a range, so an alignment label may itself carry a
/// colon; a label with a range-looking tail is named by its id instead.
fn parse_alignment(reference: &str, rest: &str) -> Result<Object, Refusal> {
    let range = rest
        .rsplit_once(':')
        .and_then(|(alignment, range)| range.split_once("..").map(|(a, b)| (alignment, a, b)));
    if let Some((alignment, from, to)) = range {
        let alignment = one(reference, "alignment", alignment)?;
        let from = one(reference, "range", from)?;
        let to = one(reference, "range", to)?;
        if from == to {
            return Err(invalid(
                reference,
                "range",
                format!("`{from}..{to}` names one structure twice; a range needs two"),
            ));
        }
        return Ok(Object::AlignmentRange {
            alignment,
            from,
            to,
        });
    }
    let alignment = one(reference, "alignment", rest)?;
    Ok(Object::Alignment { alignment })
}

fn one(reference: &str, part: &str, raw: &str) -> Result<String, Refusal> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(invalid(reference, part, format!("the {part} is empty")));
    }
    Ok(value.to_owned())
}

fn selectors(reference: &str, part: &str, list: &str) -> Result<Vec<String>, Refusal> {
    let mut out: Vec<String> = Vec::new();
    for raw in list.split(',') {
        let selector = raw.trim();
        if selector.is_empty() {
            return Err(invalid(
                reference,
                part,
                "a selector is empty; write the structure numbers as `74,76,77`",
            ));
        }
        if out.iter().any(|seen| seen == selector) {
            return Err(invalid(
                reference,
                part,
                format!("selector `{selector}` is repeated"),
            ));
        }
        out.push(selector.to_owned());
    }
    if out.is_empty() {
        return Err(invalid(reference, part, "no selector follows `structure:`"));
    }
    Ok(out)
}

/// Parse every reference, in order. The first refusal is the answer: a caller
/// fixes references one at a time and each refusal names its part.
pub fn parse_all(raws: &[String]) -> Result<Vec<Reference>, Refusal> {
    if raws.is_empty() {
        return Err(Refusal::new(
            super::REFERENCE_INVALID,
            "no reference was given; name the objects as `dsgrid:<source>:structure:<numbers>`",
            serde_json::json!({ "part": "references" }),
        ));
    }
    raws.iter().map(|raw| parse(raw)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_owner_s_example_parses_to_three_selectors_on_a_local_copy() {
        let parsed = parse("dsgrid:local-abc-1:structure:74,76,77").expect("parses");
        // The token is the catalogue id itself, `local-` prefix included:
        // what `ds dsgrid model list` prints is what a reference names.
        assert_eq!(
            parsed.source,
            Source::Local {
                id: "local-abc-1".into()
            }
        );
        assert_eq!(parsed.source.key(), "local-abc-1");
        assert_eq!(
            parsed.object,
            Object::Structure {
                selectors: vec!["74".into(), "76".into(), "77".into()]
            }
        );
        // Whitespace around a selector is a typo, not a different structure.
        let padded = parse(" dsgrid:package:structure: 74 , s-9 ").expect("parses");
        assert_eq!(padded.source, Source::Package);
        assert_eq!(
            padded.object,
            Object::Structure {
                selectors: vec!["74".into(), "s-9".into()]
            }
        );
    }

    #[test]
    fn an_alignment_and_a_range_parse_and_a_label_may_carry_a_colon() {
        assert_eq!(
            parse("dsgrid:package:alignment:aln-1").unwrap().object,
            Object::Alignment {
                alignment: "aln-1".into()
            }
        );
        assert_eq!(
            parse("dsgrid:package:alignment:aln-1:74..77")
                .unwrap()
                .object,
            Object::AlignmentRange {
                alignment: "aln-1".into(),
                from: "74".into(),
                to: "77".into()
            }
        );
        assert_eq!(
            parse("dsgrid:package:alignment:Line A: north:77..74")
                .unwrap()
                .object,
            Object::AlignmentRange {
                alignment: "Line A: north".into(),
                from: "77".into(),
                to: "74".into()
            }
        );
        assert_eq!(
            parse("dsgrid:package:alignment:Line A: north")
                .unwrap()
                .object,
            Object::Alignment {
                alignment: "Line A: north".into()
            }
        );
    }

    #[test]
    fn every_malformed_reference_is_refused_by_name_with_its_part() {
        // (reference, the part named). A transformer or survey head is refused
        // with the reason the contract gives, so an agent that guessed the
        // grammar learns the boundary from the refusal.
        for (raw, part) in [
            ("", "reference"),
            ("transformer:TX-104", "head"),
            ("survey:form:entry-9", "head"),
            ("dsgrid", "source"),
            ("dsgrid:remote-x:structure:1", "source"),
            ("dsgrid:local-:structure:1", "source"),
            ("dsgrid:local-A/B:structure:1", "source"),
            ("dsgrid:package", "object"),
            ("dsgrid:package:span:1", "object"),
            ("dsgrid:package:structure:", "structure"),
            ("dsgrid:package:structure:74,,77", "structure"),
            ("dsgrid:package:structure:74,74", "structure"),
            ("dsgrid:package:alignment:", "alignment"),
            ("dsgrid:package:alignment:aln:74..74", "range"),
            ("dsgrid:package:alignment:aln:..77", "range"),
            ("dsgrid:package:structure:7\u{7}4", "reference"),
        ] {
            let refusal = parse(raw).expect_err(raw);
            assert_eq!(refusal.code, super::super::REFERENCE_INVALID, "{raw}");
            assert_eq!(refusal.detail["part"], part, "{raw}: {}", refusal.message);
        }
        let long = format!("dsgrid:package:structure:{}", "1,".repeat(3_000));
        assert_eq!(parse(&long).unwrap_err().detail["part"], "reference");
        assert_eq!(
            parse_all(&[]).unwrap_err().code,
            super::super::REFERENCE_INVALID
        );
    }
}
