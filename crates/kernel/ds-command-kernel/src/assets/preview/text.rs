//! Markdown and plain text (§4.2 md / txt): rendered blocks, no script, no
//! remote images.
//!
//! Both decoders read the file as UTF-8 under the per-format cap
//! ([`MAX_TEXT_BYTES`] through [`check_format_cap`]), split it into at most
//! [`MAX_DOC_BLOCKS`] blocks and say through `truncated` and a note when a
//! bound cut the document.
//!
//! Markdown is split here rather than by `pulldown-cmark`: that crate is in
//! the kernel lock only as a transitive dependency of `ds-solar-contracts`
//! and this slice may not declare new dependencies. The subset is the block
//! grammar a preview needs — ATX headings, unordered and ordered lists,
//! fenced code, block quotes and paragraphs — with two rules that are not
//! negotiable in either decoder: an image reference becomes the paragraph
//! text `[image: alt]` and is never fetched, and every remote reference
//! (`http://`, `https://`, `ftp://`, `cid:`) is replaced by
//! `[link removed]` and counted in a note. No block text a host renders is
//! ever a fetchable URL (§4.2, §12).

use std::borrow::Cow;

use super::super::{
    check_format_cap, thousands, Block, PreviewDoc, PreviewRequest, MAX_DOC_BLOCKS,
};

/// What a stripped remote reference leaves behind: readable, never fetchable.
const REMOVED: &str = "[link removed]";

/// Schemes a preview refuses to carry through to a renderer.
const REMOTE_SCHEMES: [&str; 4] = ["http://", "https://", "ftp://", "cid:"];

/// Markdown (§4.2 md): bounded blocks, links reduced to their text, images
/// named but never fetched.
pub fn markdown(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let mut doc = PreviewDoc::new(request.format);
    let source = read_text(bytes, request, &mut doc)?;
    let mut inline = Inline::default();
    let mut blocks = Blocks::default();
    let mut pending = Pending::default();
    let mut fence: Option<String> = None;
    let mut code: Vec<String> = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(marker) = &fence {
            if trimmed.starts_with(marker.as_str()) {
                push_paragraph(&mut blocks, &mut inline, &code.join("\n"));
                code.clear();
                fence = None;
            } else {
                code.push(line.to_string());
            }
            continue;
        }
        if let Some(marker) = fence_marker(trimmed) {
            pending.flush(&mut blocks, &mut inline);
            fence = Some(marker);
            continue;
        }
        if trimmed.is_empty() {
            pending.flush(&mut blocks, &mut inline);
            continue;
        }
        if let Some((level, text)) = atx_heading(trimmed) {
            pending.flush(&mut blocks, &mut inline);
            let text = inline.scan(text);
            blocks.push(Block::Heading { level, text });
            continue;
        }
        if thematic_break(trimmed) {
            pending.flush(&mut blocks, &mut inline);
            continue;
        }
        if let Some((ordered, item)) = list_item(trimmed) {
            pending.flush_paragraph(&mut blocks, &mut inline);
            if !pending.items.is_empty() && pending.ordered != ordered {
                pending.flush_list(&mut blocks, &mut inline);
            }
            pending.ordered = ordered;
            pending.items.push(item.to_string());
            continue;
        }
        pending.flush_list(&mut blocks, &mut inline);
        pending
            .paragraph
            .push(trimmed.trim_start_matches('>').trim().to_string());
    }
    push_paragraph(&mut blocks, &mut inline, &code.join("\n"));
    pending.flush(&mut blocks, &mut inline);
    inline.note(&mut doc);
    Ok(finish(doc, blocks, "markdown"))
}

/// Plain text (§4.2 txt): paragraphs split on blank lines, line structure
/// kept inside a paragraph, the same bounds and the same remote-reference
/// rule as markdown.
pub fn plain(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let mut doc = PreviewDoc::new(request.format);
    let source = read_text(bytes, request, &mut doc)?;
    let mut inline = Inline::default();
    let mut blocks = Blocks::default();
    let mut paragraph: Vec<&str> = Vec::new();
    for line in source.lines() {
        if line.trim().is_empty() {
            push_paragraph(&mut blocks, &mut inline, &paragraph.join("\n"));
            paragraph.clear();
        } else {
            paragraph.push(line);
        }
    }
    push_paragraph(&mut blocks, &mut inline, &paragraph.join("\n"));
    inline.note(&mut doc);
    Ok(finish(doc, blocks, "text file"))
}

/// The per-format cap first, then UTF-8. Invalid sequences are replaced and
/// noted, never hidden and never a panic.
fn read_text(
    bytes: &[u8],
    request: &PreviewRequest,
    doc: &mut PreviewDoc,
) -> Result<String, String> {
    check_format_cap(bytes.len() as u64, request.format)?;
    let text = String::from_utf8_lossy(bytes);
    if matches!(text, Cow::Owned(_)) {
        doc.notes.push(
            "the file is not valid UTF-8; invalid sequences are shown as the replacement character"
                .into(),
        );
    }
    // A byte-order mark is an encoding artefact, not a heading character.
    Ok(text.strip_prefix('\u{feff}').unwrap_or(&text).to_string())
}

/// Blocks kept for display, and how many the document actually has.
#[derive(Default)]
struct Blocks {
    shown: Vec<Block>,
    total: usize,
}

impl Blocks {
    fn push(&mut self, block: Block) {
        self.total += 1;
        if self.shown.len() < MAX_DOC_BLOCKS {
            self.shown.push(block);
        }
    }
}

/// Lines waiting to become a paragraph or a list.
#[derive(Default)]
struct Pending {
    paragraph: Vec<String>,
    items: Vec<String>,
    ordered: bool,
}

impl Pending {
    fn flush(&mut self, blocks: &mut Blocks, inline: &mut Inline) {
        self.flush_paragraph(blocks, inline);
        self.flush_list(blocks, inline);
    }

    fn flush_paragraph(&mut self, blocks: &mut Blocks, inline: &mut Inline) {
        if self.paragraph.is_empty() {
            return;
        }
        let joined = self.paragraph.join(" ");
        self.paragraph.clear();
        push_paragraph(blocks, inline, &joined);
    }

    fn flush_list(&mut self, blocks: &mut Blocks, inline: &mut Inline) {
        if self.items.is_empty() {
            return;
        }
        let items = self
            .items
            .drain(..)
            .map(|item| inline.scan(&item))
            .collect::<Vec<_>>();
        blocks.push(Block::List {
            items,
            ordered: self.ordered,
        });
    }
}

fn push_paragraph(blocks: &mut Blocks, inline: &mut Inline, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let text = inline.scan(text);
    blocks.push(Block::Paragraph { text });
}

fn finish(mut doc: PreviewDoc, blocks: Blocks, what: &str) -> PreviewDoc {
    if blocks.total > blocks.shown.len() {
        doc.truncated = true;
        doc.notes.push(format!(
            "the {what} has {} blocks; the first {} are shown (the block bound is {MAX_DOC_BLOCKS})",
            thousands(blocks.total as u64),
            thousands(blocks.shown.len() as u64)
        ));
    }
    doc.blocks = blocks.shown;
    doc
}

/// Counts what inline scanning removed so the notes can say it out loud.
#[derive(Default)]
struct Inline {
    images: usize,
    removed: usize,
}

impl Inline {
    /// Markdown links and images reduced to their text, then every remote
    /// reference removed.
    fn scan(&mut self, source: &str) -> String {
        let named = name_links(source, &mut self.images);
        let (text, removed) = strip_remote_references(&named);
        self.removed += removed;
        text
    }

    fn note(&self, doc: &mut PreviewDoc) {
        if self.images > 0 {
            doc.notes.push(format!(
                "remote content not fetched: {} image reference(s) are shown by name only",
                thousands(self.images as u64)
            ));
        }
        if self.removed > 0 {
            doc.notes.push(format!(
                "{} remote reference(s) were replaced by {REMOVED}; a preview never fetches remote content",
                thousands(self.removed as u64)
            ));
        }
    }
}

/// `[text](url)` becomes `text` and `![alt](url)` becomes `[image: alt]`;
/// the destination never reaches a block.
fn name_links(source: &str, images: &mut usize) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(open) = rest.find('[') {
        let image = open > 0 && rest.as_bytes()[open - 1] == b'!';
        let head = if image { open - 1 } else { open };
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            break;
        };
        out.push_str(&rest[..head]);
        let label = &after[..close];
        let tail = &after[close + 1..];
        let tail = match tail.strip_prefix('(') {
            Some(destination) => match destination.find(')') {
                Some(end) => &destination[end + 1..],
                None => tail,
            },
            None => tail,
        };
        if image {
            *images += 1;
            out.push_str("[image: ");
            out.push_str(if label.trim().is_empty() {
                "untitled"
            } else {
                label
            });
            out.push(']');
        } else {
            out.push_str(label);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Replaces every `http://`, `https://`, `ftp://` and `cid:` reference with
/// [`REMOVED`], returning the text and how many were removed. Shared with
/// the mail decoder: one rule, one implementation.
pub(crate) fn strip_remote_references(text: &str) -> (String, usize) {
    let lowered = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    let mut removed = 0usize;
    // Each scheme's next occurrence is looked up once and kept until the
    // cursor passes it, so a text of a hundred thousand links costs one scan
    // per scheme rather than one scan per link (a 1 MiB note must not take
    // minutes).
    let mut next: [Option<usize>; REMOTE_SCHEMES.len()] = [None; REMOTE_SCHEMES.len()];
    for (slot, scheme) in next.iter_mut().zip(REMOTE_SCHEMES) {
        *slot = lowered.find(scheme);
    }
    loop {
        for (slot, scheme) in next.iter_mut().zip(REMOTE_SCHEMES) {
            if slot.is_some_and(|found| found < at) {
                *slot = lowered
                    .get(at..)
                    .and_then(|rest| rest.find(scheme))
                    .map(|found| at + found);
            }
        }
        let Some(start) = next.iter().flatten().copied().min() else {
            break;
        };
        out.push_str(&text[at..start]);
        out.push_str(REMOVED);
        removed += 1;
        let end = text[start..]
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(character, '>' | '<' | ')' | '"' | '\'' | ']' | ',' | ';')
            })
            .map_or(text.len(), |offset| start + offset);
        at = end;
    }
    out.push_str(&text[at..]);
    (out, removed)
}

/// ` ``` ` or `~~~` opens and closes a fenced code block.
fn fence_marker(trimmed: &str) -> Option<String> {
    for marker in ["```", "~~~"] {
        if trimmed.starts_with(marker) {
            return Some(marker.to_string());
        }
    }
    None
}

fn atx_heading(trimmed: &str) -> Option<(u8, &str)> {
    let hashes = trimmed.len() - trimmed.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return None;
    }
    Some((hashes as u8, rest.trim().trim_end_matches('#').trim_end()))
}

/// `-`, `*`, `+` (unordered) or `1.` / `1)` (ordered). Nested indentation is
/// flattened into one list: a preview shows the items, not their depth.
fn list_item(trimmed: &str) -> Option<(bool, &str)> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(item) = trimmed.strip_prefix(marker) {
            return Some((false, item.trim()));
        }
    }
    let digits = trimmed.len()
        - trimmed
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .len();
    if (1..=9).contains(&digits) {
        let rest = &trimmed[digits..];
        for marker in [". ", ") "] {
            if let Some(item) = rest.strip_prefix(marker) {
                return Some((true, item.trim()));
            }
        }
    }
    None
}

fn thematic_break(trimmed: &str) -> bool {
    let bare: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    bare.len() >= 3
        && (bare.chars().all(|c| c == '-')
            || bare.chars().all(|c| c == '*')
            || bare.chars().all(|c| c == '_'))
}

#[cfg(test)]
mod tests {
    use super::super::{eml, tabular};
    use super::*;
    use crate::assets::{
        Format, MAX_DOC_BLOCKS, MAX_GRID_ROWS, MAX_MAIL_BODY_BYTES, MAX_TEXT_BYTES, REQUEST_SCHEMA,
    };

    fn request(format: Format) -> PreviewRequest {
        PreviewRequest {
            schema: REQUEST_SCHEMA.to_string(),
            format,
            member: None,
            sheet: None,
            pages: None,
            rows: None,
            name: None,
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_markdown_blocks() {
        let source = "# Handover\n\nFirst line\nsecond line\n\n- one\n- two\n\n1. step\n2. step two\n\n```\ncargo test\n```\n\n---\n\n> quoted note\n";
        let doc = markdown(source.as_bytes(), &request(Format::Md)).expect("markdown decodes");
        assert!(!doc.truncated);
        assert!(doc.bounded);
        assert_eq!(
            doc.blocks[0],
            Block::Heading {
                level: 1,
                text: "Handover".into()
            }
        );
        assert_eq!(
            doc.blocks[1],
            Block::Paragraph {
                text: "First line second line".into()
            }
        );
        assert_eq!(
            doc.blocks[2],
            Block::List {
                items: vec!["one".into(), "two".into()],
                ordered: false
            }
        );
        assert_eq!(
            doc.blocks[3],
            Block::List {
                items: vec!["step".into(), "step two".into()],
                ordered: true
            }
        );
        assert_eq!(
            doc.blocks[4],
            Block::Paragraph {
                text: "cargo test".into()
            }
        );
        assert_eq!(
            doc.blocks[5],
            Block::Paragraph {
                text: "quoted note".into()
            }
        );
        assert_eq!(doc.blocks.len(), 6);

        // A byte-order mark is not a heading character.
        let doc = markdown("\u{feff}# Title\n".as_bytes(), &request(Format::Md))
            .expect("markdown decodes");
        assert_eq!(
            doc.blocks[0],
            Block::Heading {
                level: 1,
                text: "Title".into()
            }
        );

        let plain_doc = plain("one\ntwo\n\n\nthree\n".as_bytes(), &request(Format::Txt))
            .expect("plain text decodes");
        assert_eq!(
            plain_doc.blocks,
            vec![
                Block::Paragraph {
                    text: "one\ntwo".into()
                },
                Block::Paragraph {
                    text: "three".into()
                }
            ]
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_text_malformed_input() {
        let broken = [b'a', 0xff, 0xfe, b'b'];
        let doc = markdown(&broken, &request(Format::Md)).expect("invalid UTF-8 never panics");
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("not valid UTF-8")));
        assert_eq!(doc.blocks.len(), 1);
        assert!(plain(&broken, &request(Format::Txt)).is_ok());
        assert!(plain(&[], &request(Format::Txt))
            .expect("empty text is a document with no blocks")
            .blocks
            .is_empty());

        let over = vec![b'#'; 1024 * 1024 + 1];
        let refusal = markdown(&over, &request(Format::Md)).expect_err("over the md bound");
        assert!(
            refusal.contains("1,048,577") && refusal.contains("1 MiB"),
            "{refusal}"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_preview_doc_bounds() {
        // Blocks (md), rows (csv) and body bytes (eml) are the three display
        // bounds of this slice; each reports truncation with its number.
        let long = (0..MAX_DOC_BLOCKS + 50)
            .map(|index| format!("# heading {index}\n\n"))
            .collect::<String>();
        let doc = markdown(long.as_bytes(), &request(Format::Md)).expect("markdown decodes");
        assert!(doc.truncated);
        assert_eq!(doc.blocks.len(), MAX_DOC_BLOCKS);
        assert!(
            doc.notes
                .iter()
                .any(|note| note.contains("250 blocks") && note.contains("200")),
            "{:?}",
            doc.notes
        );

        let mut csv = String::from("name,value\n");
        for row in 0..MAX_GRID_ROWS + 60 {
            csv.push_str(&format!("row {row},{row}\n"));
        }
        let grid = tabular::delimited(csv.as_bytes(), &request(Format::Csv)).expect("csv decodes");
        assert!(grid.truncated);
        assert!(
            grid.notes
                .iter()
                .any(|note| note.contains("260 rows") && note.contains("200")),
            "{:?}",
            grid.notes
        );

        let body = "x".repeat(MAX_MAIL_BODY_BYTES + 40_000);
        let mail = format!("From: a@b.test\r\nSubject: long\r\n\r\n{body}");
        let doc = eml::mail(mail.as_bytes(), &request(Format::Eml)).expect("mail decodes");
        assert!(doc.truncated);
        assert!(
            doc.notes
                .iter()
                .any(|note| note.contains("240,000") && note.contains("200,000")),
            "{:?}",
            doc.notes
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_preview_refuses_remote_content() {
        let source = "# Site\n\n![tracker](https://tracker.example/pixel.png)\n\nRead [the site](http://example.com/deep?a=1) or ftp://files.example/x.zip\n";
        let doc = markdown(source.as_bytes(), &request(Format::Md)).expect("markdown decodes");
        let rendered = serde_json::to_string(&doc).expect("preview serialises");
        assert!(!rendered.contains("http"), "{rendered}");
        assert!(!rendered.contains("ftp://"), "{rendered}");
        assert!(!rendered.contains("tracker.example"), "{rendered}");
        assert!(rendered.contains("[image: tracker]"), "{rendered}");
        assert!(rendered.contains("the site"), "{rendered}");
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("remote content not fetched")));

        let mail = "From: a@b.test\r\nSubject: see http://tracker.example/s\r\nContent-Type: text/plain\r\n\r\nOpen https://example.com/x and cid:logo.png\r\n";
        let doc = eml::mail(mail.as_bytes(), &request(Format::Eml)).expect("mail decodes");
        let rendered = serde_json::to_string(&doc).expect("preview serialises");
        assert!(!rendered.contains("http"), "{rendered}");
        assert!(!rendered.contains("cid:"), "{rendered}");
        assert!(rendered.contains(REMOVED), "{rendered}");

        let plain_doc = plain(
            "see http://example.com/tracker for more\n".as_bytes(),
            &request(Format::Txt),
        )
        .expect("plain text decodes");
        let rendered = serde_json::to_string(&plain_doc).expect("preview serialises");
        assert!(!rendered.contains("http"), "{rendered}");

        // The same rule through the two-slice entry point the hosts call.
        let call = serde_json::json!({
            "schema": REQUEST_SCHEMA,
            "action": "preview",
            "format": "md",
        });
        let rendered = crate::assets::evaluate_with_bytes(
            source.as_bytes(),
            &serde_json::to_vec(&call).expect("request serialises"),
        )
        .expect("preview evaluates");
        assert!(!rendered.contains("http"), "{rendered}");
        assert!(rendered.contains("[image: tracker]"), "{rendered}");
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_remote_reference_stripping_is_linear_in_the_text() {
        // 45,000 links in a note just under the txt cap, with two of the four
        // schemes absent: the stripper used to rescan the whole remainder for
        // every absent scheme on every link (minutes); it is one scan now.
        let link = "see http://x.example/a ";
        let repeats = 45_000usize;
        let source = link.repeat(repeats);
        assert!(source.len() < MAX_TEXT_BYTES as usize);
        let (stripped, removed) = strip_remote_references(&source);
        assert_eq!(removed, repeats);
        assert!(!stripped.to_ascii_lowercase().contains("http"));
        assert_eq!(stripped, "see [link removed] ".repeat(repeats));

        let doc = plain(source.as_bytes(), &request(Format::Txt)).expect("plain text decodes");
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("45,000 remote reference(s)")));
        // Case and every scheme, interleaved, with the last one at the end.
        let mixed = "HTTP://A x Cid:img ftp://f y https://z end";
        let (stripped, removed) = strip_remote_references(mixed);
        assert_eq!(removed, 4);
        assert_eq!(
            stripped,
            "[link removed] x [link removed] [link removed] y [link removed] end"
        );
    }
}
