//! Documents (§4.2 docx / odt): document.xml paragraphs, headings, lists and bounded inline images.
//!
//! `document` reads three parts of an OOXML package: `word/document.xml`,
//! `word/_rels/document.xml.rels` (`r:embed` → a media part) and, when it is
//! there, `word/numbering.xml` (so a numbered list is not called a bulleted
//! one). Every member is inflated through one bounded reader, so a lying
//! central directory or a zip bomb is refused with its number.
//!
//! The block stream is the document's own order: `<w:p>` becomes a paragraph,
//! a `Heading1` … `Heading6` style becomes a heading of that level, a run of
//! `<w:numPr>` paragraphs becomes one list, and `<w:tbl>` becomes a bounded
//! grid whose first row is its columns — a Word table's column identity is
//! its header row, which is also how the document itself reads.
//!
//! Inline images carry their bytes only when the media part is at or under
//! [`MAX_INLINE_IMAGE_BYTES`] and fewer than [`MAX_INLINE_IMAGES`] have been
//! carried already; anything else is an image block without bytes and a note
//! that names the number. An image linked outside the package (`r:link`) is
//! never fetched and its address never reaches the preview, and an `svg` is
//! never inlined because it can reference remote content itself.
//!
//! `odt` is refused by name: an OpenDocument text document is a different
//! XML, and this slice decodes OOXML.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use base64::{engine::general_purpose, Engine as _};
use quick_xml::events::{BytesEnd, BytesRef, BytesStart, BytesText, Event};
use quick_xml::Reader;
use zip::ZipArchive;

use super::super::{
    check_bytes, check_format_cap, describe_bytes, thousands, Block, Format, PreviewDoc,
    PreviewRequest, MAX_DOC_BLOCKS, MAX_GRID_COLUMNS, MAX_GRID_ROWS, MAX_INLINE_IMAGES,
    MAX_INLINE_IMAGE_BYTES,
};

/// Largest single zip member this decoder inflates. A member that declares —
/// or inflates to — more is refused with its number rather than expanded.
const MAX_ZIP_MEMBER_BYTES: u64 = 64 * 1024 * 1024;
/// `Relationship` rows read from `word/_rels/document.xml.rels`.
const MAX_RELATIONSHIPS: usize = 10_000;
/// Numbering rows read from `word/numbering.xml`.
const MAX_NUMBERING_ROWS: usize = 10_000;
/// Paragraphs, tables and images kept from one document. Everything past this
/// is counted and reported, never quietly dropped.
const MAX_SCAN_ITEMS: usize = 100_000;
/// Characters kept across all items of one document.
const MAX_SCAN_CHARS: usize = 2_000_000;
/// Characters kept per paragraph or heading (a display bound, counted into a
/// note).
const MAX_BLOCK_CHARS: usize = 20_000;
/// Characters kept per table cell.
const MAX_CELL_CHARS: usize = 2_000;
/// Items kept in one list block.
const MAX_LIST_ITEMS: usize = 200;
/// Notes a document preview carries before the rest are counted in one line.
const MAX_NOTES: usize = 40;

/// One `<Relationship>` of the document part.
struct Relationship {
    id: String,
    target: String,
    external: bool,
}

/// `word/numbering.xml` reduced to the one question a preview asks: is this
/// `numId` an ordered list?
#[derive(Default)]
struct Numbering {
    /// `abstractNumId` → ordered (its level-0 `numFmt` is not a bullet).
    formats: BTreeMap<String, bool>,
    /// `numId` → `abstractNumId`.
    abstracts: BTreeMap<String, String>,
}

impl Numbering {
    fn ordered(&self, num_id: Option<&str>) -> bool {
        let Some(num_id) = num_id else {
            return false;
        };
        let Some(abstract_id) = self.abstracts.get(num_id) else {
            return false;
        };
        self.formats.get(abstract_id).copied().unwrap_or(false)
    }
}

/// The neutral fragment stream `word/document.xml` becomes before any zip
/// member is read for an image.
enum Fragment {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        text: String,
    },
    ListItem {
        num_id: Option<String>,
        text: String,
    },
    Image {
        rel_id: Option<String>,
        external: bool,
    },
    Table(TableItem),
}

impl Fragment {
    /// Bytes of text this fragment keeps, for the scan budget.
    fn text_bytes(&self) -> usize {
        match self {
            Fragment::Heading { text, .. }
            | Fragment::Paragraph { text }
            | Fragment::ListItem { text, .. } => text.len(),
            Fragment::Image { .. } => 0,
            Fragment::Table(table) => table.rows.iter().flatten().map(String::len).sum(),
        }
    }
}

/// A table as the parser finished it: bounded rows, honest totals.
struct TableItem {
    rows: Vec<Vec<String>>,
    rows_total: u64,
    columns_total: u64,
}

#[derive(Default)]
struct TableBuild {
    rows: Vec<Vec<String>>,
    rows_total: u64,
    columns_total: u64,
}

impl TableBuild {
    fn push_row(&mut self, row: RowBuild) {
        self.rows_total += 1;
        self.columns_total = self.columns_total.max(row.total);
        if self.rows.len() <= MAX_GRID_ROWS {
            self.rows.push(row.cells);
        }
    }

    fn finish(self) -> TableItem {
        TableItem {
            rows: self.rows,
            rows_total: self.rows_total,
            columns_total: self.columns_total,
        }
    }
}

#[derive(Default)]
struct RowBuild {
    cells: Vec<String>,
    total: u64,
}

impl RowBuild {
    fn push(&mut self, text: String) {
        self.total += 1;
        if self.cells.len() < MAX_GRID_COLUMNS {
            self.cells.push(text);
        }
    }
}

/// What one bounded pass over `word/document.xml` found.
#[derive(Default)]
struct Parsed {
    items: Vec<Fragment>,
    items_total: u64,
    kept_bytes: usize,
    text_cut: u64,
}

/// A run of consecutive `<w:numPr>` paragraphs sharing one `numId`.
struct PendingList {
    num_id: Option<String>,
    items: Vec<String>,
    cut: u64,
}

/// The document under construction, with the block bound applied once.
struct Assembly {
    doc: PreviewDoc,
    blocks_total: u64,
    notes_left_out: u64,
}

impl Assembly {
    fn push(&mut self, block: Block) {
        self.blocks_total += 1;
        if self.doc.blocks.len() < MAX_DOC_BLOCKS {
            self.doc.blocks.push(block);
        }
    }

    fn full(&self) -> bool {
        self.doc.blocks.len() >= MAX_DOC_BLOCKS
    }

    fn note(&mut self, note: String) {
        if self.doc.notes.contains(&note) {
            return;
        }
        if self.doc.notes.len() >= MAX_NOTES {
            self.notes_left_out += 1;
            return;
        }
        self.doc.notes.push(note);
    }
}

pub fn document(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let format = request.format;
    check_format_cap(bytes.len() as u64, format)?;
    if format == Format::Odt {
        return Err(
            "odt is an OpenDocument text document and this kernel decodes docx (OOXML); convert it to docx or open it externally"
                .into(),
        );
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("the document is not a readable zip container: {error}"))?;

    let Some(document_xml) = part(&mut archive, "word/document.xml")? else {
        return Err(missing_document_message(&mut archive));
    };
    let relationships = match part(&mut archive, "word/_rels/document.xml.rels")? {
        Some(xml) => parse_relationships(&xml),
        None => Vec::new(),
    };
    let numbering = match part(&mut archive, "word/numbering.xml")? {
        Some(xml) => parse_numbering(&xml),
        None => Numbering::default(),
    };

    let parsed = parse_document(&document_xml)?;
    let items_total = parsed.items_total;
    let items_kept = parsed.items.len() as u64;
    let text_cut = parsed.text_cut;

    let mut assembly = Assembly {
        doc: PreviewDoc::new(format),
        blocks_total: 0,
        notes_left_out: 0,
    };
    assemble(
        &mut assembly,
        parsed,
        &mut archive,
        &relationships,
        &numbering,
    )?;

    if text_cut > 0 {
        assembly.doc.truncated = true;
        assembly.note(format!(
            "{} blocks were cut at {} characters",
            thousands(text_cut),
            thousands(MAX_BLOCK_CHARS as u64)
        ));
    }
    if items_total > items_kept {
        assembly.doc.truncated = true;
        assembly.note(format!(
            "the document has {} paragraphs, tables and images; the preview reads the first {}",
            thousands(items_total),
            thousands(items_kept)
        ));
    }
    if assembly.blocks_total > MAX_DOC_BLOCKS as u64 {
        assembly.doc.truncated = true;
        assembly.note(format!(
            "the document has {} blocks; the preview shows the first {}",
            thousands(assembly.blocks_total),
            thousands(MAX_DOC_BLOCKS as u64)
        ));
    }
    if assembly.notes_left_out > 0 {
        let left_out = assembly.notes_left_out;
        assembly.doc.notes.push(format!(
            "{} further notes were left out",
            thousands(left_out)
        ));
    }
    Ok(assembly.doc)
}

/// Turns the item stream into blocks, reading a media part only when an image
/// can still be carried.
fn assemble(
    assembly: &mut Assembly,
    parsed: Parsed,
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    relationships: &[Relationship],
    numbering: &Numbering,
) -> Result<(), String> {
    let mut pending: Option<PendingList> = None;
    let mut image_index: u32 = 0;
    let mut images_inlined: usize = 0;
    let mut table_number: usize = 0;

    for item in parsed.items {
        if !matches!(item, Fragment::ListItem { .. }) {
            if let Some(list) = pending.take() {
                push_list(assembly, list, numbering);
            }
        }
        match item {
            Fragment::Heading { level, text } => assembly.push(Block::Heading { level, text }),
            Fragment::Paragraph { text } => assembly.push(Block::Paragraph { text }),
            Fragment::ListItem { num_id, text } => match pending.as_mut() {
                Some(list) if list.num_id == num_id => {
                    if list.items.len() < MAX_LIST_ITEMS {
                        list.items.push(text);
                    } else {
                        list.cut += 1;
                    }
                }
                Some(_) => {
                    if let Some(list) = pending.take() {
                        push_list(assembly, list, numbering);
                    }
                    pending = Some(PendingList {
                        num_id,
                        items: vec![text],
                        cut: 0,
                    });
                }
                None => {
                    pending = Some(PendingList {
                        num_id,
                        items: vec![text],
                        cut: 0,
                    })
                }
            },
            Fragment::Image { rel_id, external } => {
                if assembly.full() {
                    assembly.blocks_total += 1;
                    continue;
                }
                let (block, note) = image_block(
                    archive,
                    relationships,
                    rel_id.as_deref(),
                    external,
                    image_index,
                    images_inlined,
                )?;
                if matches!(
                    block,
                    Block::Image {
                        bytes_b64: Some(_),
                        ..
                    }
                ) {
                    images_inlined += 1;
                } else {
                    assembly.doc.truncated = true;
                }
                image_index += 1;
                assembly.push(block);
                if let Some(note) = note {
                    assembly.note(note);
                }
            }
            Fragment::Table(table) => {
                table_number += 1;
                push_table(assembly, table, table_number);
            }
        }
    }
    if let Some(list) = pending.take() {
        push_list(assembly, list, numbering);
    }
    Ok(())
}

fn push_list(assembly: &mut Assembly, list: PendingList, numbering: &Numbering) {
    if list.cut > 0 {
        assembly.doc.truncated = true;
        assembly.note(format!(
            "a list has {} items; the preview shows the first {}",
            thousands(list.items.len() as u64 + list.cut),
            thousands(MAX_LIST_ITEMS as u64)
        ));
    }
    assembly.push(Block::List {
        ordered: numbering.ordered(list.num_id.as_deref()),
        items: list.items,
    });
}

fn push_table(assembly: &mut Assembly, table: TableItem, number: usize) {
    let mut rows = table.rows;
    if rows.is_empty() {
        return;
    }
    let mut columns = rows.remove(0);
    let shown_columns = table.columns_total.min(MAX_GRID_COLUMNS as u64) as usize;
    columns.resize(shown_columns, String::new());
    for row in &mut rows {
        row.resize(shown_columns, String::new());
    }
    let rows_shown = rows.len() as u64;
    let rows_total = table.rows_total.saturating_sub(1);
    if rows_total > rows_shown {
        assembly.doc.truncated = true;
        assembly.note(format!(
            "table {number} has {} rows; the preview shows the first {}",
            thousands(rows_total),
            thousands(rows_shown)
        ));
    }
    if table.columns_total > shown_columns as u64 {
        assembly.doc.truncated = true;
        assembly.note(format!(
            "table {number} has {} columns; the preview shows the first {}",
            thousands(table.columns_total),
            thousands(shown_columns as u64)
        ));
    }
    assembly.push(Block::Grid {
        title: Some(format!("table {number}")),
        columns,
        rows,
        rows_total,
        rows_shown,
        columns_total: table.columns_total,
    });
}

/// One image block, with its bytes when both bounds allow it. The second
/// value is the note that says why bytes are missing.
fn image_block(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    relationships: &[Relationship],
    rel_id: Option<&str>,
    external: bool,
    index: u32,
    images_inlined: usize,
) -> Result<(Block, Option<String>), String> {
    let resolved = rel_id.and_then(|id| relationships.iter().find(|relation| relation.id == id));
    let external = external || resolved.is_some_and(|relation| relation.external);
    let target = resolved
        .filter(|relation| !relation.external)
        .map(|relation| media_part(&relation.target));
    let mime = target
        .as_ref()
        .and_then(Option::as_deref)
        .map_or("application/octet-stream", image_mime)
        .to_string();

    if external {
        return Ok((
            image_without_bytes(index, mime),
            Some(
                "an image is linked to a file outside the document; a preview never fetches remote content, so it is shown without its bytes"
                    .to_string(),
            ),
        ));
    }
    let target = match target {
        Some(Some(target)) => target,
        Some(None) => {
            return Ok((
                image_without_bytes(index, mime),
                Some(format!(
                    "image reference {} points outside the package, so it is shown without its bytes",
                    cut_text(rel_id.unwrap_or("(none)"), 40).0
                )),
            ));
        }
        None => {
            return Ok((
                image_without_bytes(index, mime),
                Some(format!(
                    "image reference {} has no target in word/_rels/document.xml.rels, so it is shown without its bytes",
                    cut_text(rel_id.unwrap_or("(none)"), 40).0
                )),
            ));
        }
    };
    let Some((declared, bytes)) = member_prefix(archive, &target, MAX_INLINE_IMAGE_BYTES)? else {
        return Ok((
            image_without_bytes(index, mime),
            Some(format!(
                "{} is named by the document but is not in the package, so it is shown without its bytes",
                cut_text(&target, 120).0
            )),
        ));
    };
    let (width, height) = dimensions(&bytes);
    let size = declared.max(bytes.len() as u64);
    if size > MAX_INLINE_IMAGE_BYTES {
        return Ok((
            Block::Image {
                index,
                mime,
                width,
                height,
                bytes_b64: None,
            },
            Some(format!(
                "{} is {} bytes; a preview inlines images up to {}, so it is shown without its bytes",
                cut_text(&target, 120).0,
                thousands(size),
                describe_bytes(MAX_INLINE_IMAGE_BYTES)
            )),
        ));
    }
    if mime == "image/svg+xml" {
        return Ok((
            Block::Image {
                index,
                mime,
                width,
                height,
                bytes_b64: None,
            },
            Some(
                "an svg can reference remote content of its own, so it is shown without its bytes"
                    .to_string(),
            ),
        ));
    }
    if images_inlined >= MAX_INLINE_IMAGES {
        return Ok((
            Block::Image {
                index,
                mime,
                width,
                height,
                bytes_b64: None,
            },
            Some(format!(
                "a preview inlines {} images; the rest are shown without their bytes",
                thousands(MAX_INLINE_IMAGES as u64)
            )),
        ));
    }
    Ok((
        Block::Image {
            index,
            mime,
            width,
            height,
            bytes_b64: Some(general_purpose::STANDARD.encode(&bytes)),
        },
        None,
    ))
}

fn image_without_bytes(index: u32, mime: String) -> Block {
    Block::Image {
        index,
        mime,
        width: 0,
        height: 0,
        bytes_b64: None,
    }
}

/// Dimensions from the image header alone — never a decode.
fn dimensions(bytes: &[u8]) -> (u32, u32) {
    match imagesize::blob_size(bytes) {
        Ok(size) => (
            u32::try_from(size.width).unwrap_or(0),
            u32::try_from(size.height).unwrap_or(0),
        ),
        Err(_) => (0, 0),
    }
}

/// One bounded pass over `word/document.xml`.
fn parse_document(xml: &[u8]) -> Result<Parsed, String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut parsed = Parsed::default();
    let mut in_paragraph = false;
    let mut in_run = false;
    let mut capture = false;
    let mut preserve = false;
    let mut run = String::new();
    let mut paragraph = String::new();
    let mut style: Option<String> = None;
    let mut numbered = false;
    let mut num_id: Option<String> = None;
    let mut paragraph_images: Vec<(Option<String>, bool)> = Vec::new();
    let mut table_images: Vec<(Option<String>, bool)> = Vec::new();
    let mut table_depth = 0usize;
    let mut table: Option<TableBuild> = None;
    let mut row: Option<RowBuild> = None;
    let mut cell: Option<String> = None;

    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|error| format!("word/document.xml is not readable xml: {error}"))?;
        match event {
            Event::Start(element) if is(&element, b"p") => {
                in_paragraph = true;
                paragraph.clear();
                style = None;
                numbered = false;
                num_id = None;
                paragraph_images.clear();
            }
            Event::Start(element) | Event::Empty(element)
                if in_paragraph && is(&element, b"pStyle") =>
            {
                style = attribute_local(&element, b"val");
            }
            Event::Start(element) | Event::Empty(element)
                if in_paragraph && is(&element, b"numPr") =>
            {
                numbered = true;
            }
            Event::Start(element) | Event::Empty(element) if numbered && is(&element, b"numId") => {
                num_id = attribute_local(&element, b"val");
            }
            Event::Start(element) if is(&element, b"r") => in_run = true,
            Event::End(element) if is_end(&element, b"r") => in_run = false,
            Event::Start(element) if in_run && is(&element, b"t") => {
                capture = true;
                preserve = attribute_local(&element, b"space").as_deref() == Some("preserve");
                run.clear();
            }
            Event::End(element) if capture && is_end(&element, b"t") => {
                capture = false;
                let text = if preserve {
                    run.clone()
                } else {
                    run.trim().to_string()
                };
                push_text(&mut cell, &mut paragraph, in_paragraph, &text);
                run.clear();
            }
            Event::Text(text) if capture => run.push_str(&text_of(&text)),
            Event::GeneralRef(reference) if capture => run.push_str(&reference_of(&reference)),
            Event::Start(element) | Event::Empty(element) if in_run && is(&element, b"tab") => {
                push_text(&mut cell, &mut paragraph, in_paragraph, "\t");
            }
            Event::Start(element) | Event::Empty(element)
                if in_run && (is(&element, b"br") || is(&element, b"cr")) =>
            {
                push_text(&mut cell, &mut paragraph, in_paragraph, "\n");
            }
            Event::Start(element) | Event::Empty(element) if is(&element, b"blip") => {
                let embedded = attribute_local(&element, b"embed");
                let entry = match embedded {
                    Some(id) => (Some(id), false),
                    None => (attribute_local(&element, b"link"), true),
                };
                if table_depth > 0 {
                    table_images.push(entry);
                } else {
                    paragraph_images.push(entry);
                }
            }
            Event::Start(element) if is(&element, b"tbl") => {
                table_depth += 1;
                if table_depth == 1 {
                    table = Some(TableBuild::default());
                }
            }
            Event::End(element) if is_end(&element, b"tbl") => {
                if table_depth == 1 {
                    if let Some(built) = table.take() {
                        push_item(&mut parsed, Fragment::Table(built.finish()));
                    }
                    for (rel_id, external) in table_images.drain(..) {
                        push_item(&mut parsed, Fragment::Image { rel_id, external });
                    }
                }
                table_depth = table_depth.saturating_sub(1);
            }
            Event::Start(element) if table_depth == 1 && is(&element, b"tr") => {
                row = Some(RowBuild::default());
            }
            Event::End(element) if table_depth == 1 && is_end(&element, b"tr") => {
                if let (Some(built), Some(cells)) = (table.as_mut(), row.take()) {
                    built.push_row(cells);
                }
            }
            Event::Start(element) if table_depth == 1 && is(&element, b"tc") => {
                cell = Some(String::new());
            }
            Event::End(element) if table_depth == 1 && is_end(&element, b"tc") => {
                if let Some(text) = cell.take() {
                    if let Some(cells) = row.as_mut() {
                        cells.push(cut_text(text.trim(), MAX_CELL_CHARS).0);
                    }
                }
            }
            Event::End(element) if is_end(&element, b"p") => {
                if table_depth == 0 {
                    emit_paragraph(
                        &mut parsed,
                        &paragraph,
                        style.as_deref(),
                        numbered,
                        num_id.clone(),
                        &mut paragraph_images,
                    );
                } else if let Some(text) = cell.as_mut() {
                    text.push('\n');
                }
                in_paragraph = false;
                paragraph.clear();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(parsed)
}

/// Text goes to the open table cell, else to the open paragraph. Both sinks
/// stop growing at their display bound (four bytes per character is the widest
/// UTF-8 encoding, so the character bound is always reached first and the cut
/// is reported).
fn push_text(cell: &mut Option<String>, paragraph: &mut String, in_paragraph: bool, text: &str) {
    if let Some(cell) = cell.as_mut() {
        if cell.len() < MAX_CELL_CHARS * 4 {
            cell.push_str(text);
        }
    } else if in_paragraph && paragraph.len() < MAX_BLOCK_CHARS * 4 {
        paragraph.push_str(text);
    }
}

/// One finished `<w:p>`: a heading, a list item or a paragraph, followed by
/// the images it carried.
fn emit_paragraph(
    parsed: &mut Parsed,
    paragraph: &str,
    style: Option<&str>,
    numbered: bool,
    num_id: Option<String>,
    images: &mut Vec<(Option<String>, bool)>,
) {
    let (text, cut) = cut_text(paragraph.trim_end(), MAX_BLOCK_CHARS);
    if cut {
        parsed.text_cut += 1;
    }
    if !text.is_empty() {
        if let Some(level) = heading_level(style) {
            push_item(parsed, Fragment::Heading { level, text });
        } else if numbered {
            push_item(parsed, Fragment::ListItem { num_id, text });
        } else {
            push_item(parsed, Fragment::Paragraph { text });
        }
    }
    for (rel_id, external) in images.drain(..) {
        push_item(parsed, Fragment::Image { rel_id, external });
    }
}

/// Keeps a fragment while both scan budgets allow it; every one is counted.
fn push_item(parsed: &mut Parsed, item: Fragment) {
    parsed.items_total += 1;
    let bytes = item.text_bytes();
    if parsed.items.len() < MAX_SCAN_ITEMS
        && parsed.kept_bytes.saturating_add(bytes) <= MAX_SCAN_CHARS
    {
        parsed.kept_bytes += bytes;
        parsed.items.push(item);
    }
}

/// `Heading1` … `Heading6` (however it is cased or spaced) → the level.
fn heading_level(style: Option<&str>) -> Option<u8> {
    let style = style?.trim().to_ascii_lowercase();
    let digits = style
        .strip_prefix("heading")?
        .trim_start_matches([' ', '-', '_']);
    match digits.parse::<u8>() {
        Ok(level) if (1..=6).contains(&level) => Some(level),
        _ => None,
    }
}

/// `<Relationship Id= Target= TargetMode=>` rows of the document part.
fn parse_relationships(xml: &[u8]) -> Vec<Relationship> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut relationships = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(element)) | Ok(Event::Empty(element))
                if is(&element, b"Relationship") =>
            {
                if let (Some(id), Some(target)) =
                    (attribute(&element, b"Id"), attribute(&element, b"Target"))
                {
                    relationships.push(Relationship {
                        id,
                        target,
                        external: attribute(&element, b"TargetMode").as_deref() == Some("External"),
                    });
                }
                if relationships.len() >= MAX_RELATIONSHIPS {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    relationships
}

/// `word/numbering.xml` reduced to `numId` → ordered. A malformed or absent
/// part simply leaves every list bulleted.
fn parse_numbering(xml: &[u8]) -> Numbering {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut numbering = Numbering::default();
    let mut abstract_id: Option<String> = None;
    let mut level: Option<String> = None;
    let mut num_id: Option<String> = None;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(element)) | Ok(Event::Empty(element)) => {
                if is(&element, b"abstractNum") {
                    abstract_id = attribute_local(&element, b"abstractNumId");
                    level = None;
                } else if is(&element, b"lvl") {
                    level = attribute_local(&element, b"ilvl");
                } else if is(&element, b"numFmt") {
                    if let (Some(id), Some(format)) =
                        (abstract_id.clone(), attribute_local(&element, b"val"))
                    {
                        if level.as_deref().unwrap_or("0") == "0" {
                            numbering
                                .formats
                                .entry(id)
                                .or_insert(format != "bullet" && format != "none");
                        }
                    }
                } else if is(&element, b"num") {
                    num_id = attribute_local(&element, b"numId");
                } else if is(&element, b"abstractNumId") {
                    if let (Some(number), Some(target)) =
                        (num_id.clone(), attribute_local(&element, b"val"))
                    {
                        numbering.abstracts.insert(number, target);
                    }
                }
                if numbering.formats.len() + numbering.abstracts.len() >= MAX_NUMBERING_ROWS {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    numbering
}

/// A document relationship target as a package part name. A relative target
/// is resolved against `word/` segment by segment; an absolute one (`/word/…`)
/// is a part name from the package root. A target that climbs past the root
/// names nothing in the package and answers `None`: it is never echoed and
/// never looked up, let alone used as a path.
fn media_part(target: &str) -> Option<String> {
    let target = target.trim();
    let (mut parts, rest): (Vec<&str>, &str) = match target.strip_prefix('/') {
        Some(absolute) => (Vec::new(), absolute),
        None => (vec!["word"], target),
    };
    for segment in rest.split(['/', '\\']) {
        match segment {
            "" | "." => continue,
            ".." => {
                parts.pop()?;
            }
            named => parts.push(named),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The media type of a package part, from its extension alone.
fn image_mime(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    let extension = lower.rsplit('.').next().unwrap_or_default();
    match extension {
        "png" => "image/png",
        "jpg" | "jpeg" | "jpe" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        _ => "application/octet-stream",
    }
}

/// Cuts display text at `limit` characters, saying whether it was cut.
fn cut_text(text: &str, limit: usize) -> (String, bool) {
    if text.chars().count() <= limit {
        return (text.to_string(), false);
    }
    (text.chars().take(limit).collect(), true)
}

/// Why `word/document.xml` is missing, in the words the reader needs.
fn missing_document_message(archive: &mut ZipArchive<Cursor<&[u8]>>) -> String {
    if is_opendocument(archive) {
        return "this file is an OpenDocument package, not an OOXML document; convert it to docx or open it externally".to_string();
    }
    "the document has no word/document.xml part, so it is not a readable docx".to_string()
}

/// Whether the package declares an OpenDocument `mimetype` member.
fn is_opendocument(archive: &mut ZipArchive<Cursor<&[u8]>>) -> bool {
    member_prefix(archive, "mimetype", 256)
        .ok()
        .flatten()
        .is_some_and(|(_, bytes)| {
            String::from_utf8_lossy(&bytes).starts_with("application/vnd.oasis.opendocument")
        })
}

/// One package part, inflated through the member bound.
fn part(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<Vec<u8>>, String> {
    let Some((_, bytes)) = member_bytes(archive, name, MAX_ZIP_MEMBER_BYTES)? else {
        return Ok(None);
    };
    Ok(Some(bytes))
}

/// Inflates one member into memory, refusing above `cap` by name — the
/// declared size and the inflated size both count, so a lying central
/// directory is refused rather than believed.
fn member_bytes(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    cap: u64,
) -> Result<Option<(u64, Vec<u8>)>, String> {
    let Some((declared, bytes)) = member_prefix(archive, name, cap)? else {
        return Ok(None);
    };
    let what = format!("zip member {name}");
    check_bytes(&what, declared, "zip member", cap)?;
    check_bytes(&what, bytes.len() as u64, "zip member", cap)?;
    Ok(Some((declared, bytes)))
}

/// Reads at most `cap + 1` bytes of a member, with the size its central
/// directory declares. Nothing larger is ever held in memory, so an oversized
/// image costs one bound rather than its own length.
fn member_prefix(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    cap: u64,
) -> Result<Option<(u64, Vec<u8>)>, String> {
    let Some(index) = archive.index_for_name(name) else {
        return Ok(None);
    };
    let file = archive
        .by_index(index)
        .map_err(|error| format!("zip member {name} cannot be opened: {error}"))?;
    let declared = file.size();
    let mut bytes = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("zip member {name} cannot be inflated: {error}"))?;
    Ok(Some((declared, bytes)))
}

/// The element's name without its namespace prefix.
fn is(element: &BytesStart<'_>, name: &[u8]) -> bool {
    element.local_name().as_ref() == name
}

/// The same for a closing tag.
fn is_end(element: &BytesEnd<'_>, name: &[u8]) -> bool {
    element.local_name().as_ref() == name
}

/// An attribute by its exact key.
fn attribute(element: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == key)
        .map(|attribute| decode_attribute(&attribute.value))
}

/// An attribute by its local name, whatever namespace prefix it carries
/// (`w:val`, `r:embed`, `xml:space`).
fn attribute_local(element: &BytesStart<'_>, local: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attribute| {
            let key = attribute.key.as_ref();
            key == local
                || key
                    .strip_suffix(local)
                    .is_some_and(|prefix| prefix.ends_with(b":"))
        })
        .map(|attribute| decode_attribute(&attribute.value))
}

fn decode_attribute(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    quick_xml::escape::unescape(text.as_ref())
        .map(|unescaped| unescaped.into_owned())
        .unwrap_or_else(|_| text.into_owned())
}

/// A `&…;` reference inside captured text: numeric character references and
/// the five predefined entities. Anything else resolves to nothing rather
/// than to a guess.
fn reference_of(reference: &BytesRef<'_>) -> String {
    if let Ok(Some(character)) = reference.resolve_char_ref() {
        return character.to_string();
    }
    match reference.decode() {
        Ok(name) => quick_xml::escape::resolve_predefined_entity(name.as_ref())
            .unwrap_or_default()
            .to_string(),
        Err(_) => String::new(),
    }
}

fn text_of(text: &BytesText<'_>) -> String {
    match text.decode() {
        Ok(decoded) => quick_xml::escape::unescape(decoded.as_ref())
            .map(|unescaped| unescaped.into_owned())
            .unwrap_or_else(|_| decoded.into_owned()),
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::write::{SimpleFileOptions, ZipWriter};

    use super::*;
    use crate::assets::{Kind, REQUEST_SCHEMA};

    const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body>"#;
    const TAIL: &str = "</w:body></w:document>";

    const NUMBERING: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
  <w:abstractNum w:abstractNumId="1"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl></w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
  <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;

    const IMAGE_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/small.png"/>
  <Relationship Id="rId6" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/big.png"/>
  <Relationship Id="rId7" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/gone.png"/>
  <Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://example.com/pixel.png" TargetMode="External"/>
</Relationships>"#;

    fn zip_of(parts: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in parts {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn ask(format: Format) -> PreviewRequest {
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

    fn body(inner: &str) -> Vec<u8> {
        format!("{HEAD}{inner}{TAIL}").into_bytes()
    }

    /// A PNG that declares 64 × 32 and is padded to `total` bytes.
    fn png_of(total: usize) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&64u32.to_be_bytes());
        bytes.extend_from_slice(&32u32.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        bytes.resize(total.max(bytes.len()), 0x5a);
        bytes
    }

    fn drawing(attribute: &str) -> String {
        format!(
            "<w:p><w:r><w:drawing><a:graphic><a:graphicData><a:blip {attribute}/></a:graphicData></a:graphic></w:drawing></w:r></w:p>"
        )
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_docx_blocks_headings_lists_tables() {
        let inner = concat!(
            r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Scope of works</w:t></w:r></w:p>"#,
            r#"<w:p><w:r><w:t xml:space="preserve">The contractor shall </w:t></w:r><w:r><w:t>  supply poles &amp; stays.  </w:t></w:r></w:p>"#,
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Survey</w:t></w:r></w:p>"#,
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Staking</w:t></w:r></w:p>"#,
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>Optional</w:t></w:r></w:p>"#,
            r#"<w:p><w:pPr><w:pStyle w:val="heading 2"/></w:pPr><w:r><w:t>Bill of quantities</w:t></w:r></w:p>"#,
            "<w:tbl>",
            "<w:tr><w:tc><w:p><w:r><w:t>Fragment</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Qty</w:t></w:r></w:p></w:tc></w:tr>",
            "<w:tr><w:tc><w:p><w:r><w:t>Pole 9m</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>120</w:t></w:r></w:p></w:tc></w:tr>",
            "<w:tr><w:tc><w:p><w:r><w:t>Stay</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>40</w:t></w:r></w:p></w:tc></w:tr>",
            "</w:tbl>",
            "<w:p/><w:p><w:r><w:t>   </w:t></w:r></w:p>",
        );
        let bytes = zip_of(&[
            ("word/document.xml", body(inner)),
            ("word/numbering.xml", NUMBERING.as_bytes().to_vec()),
        ]);
        let doc = document(&bytes, &ask(Format::Docx)).unwrap();
        assert_eq!(doc.format, Format::Docx);
        assert_eq!(doc.kind, Kind::Doc);
        assert!(doc.bounded);
        assert!(!doc.truncated);
        assert!(doc.notes.is_empty(), "{:?}", doc.notes);

        assert_eq!(
            doc.blocks,
            vec![
                Block::Heading {
                    level: 1,
                    text: "Scope of works".to_string(),
                },
                Block::Paragraph {
                    text: "The contractor shall supply poles & stays.".to_string(),
                },
                Block::List {
                    items: vec!["Survey".to_string(), "Staking".to_string()],
                    ordered: true,
                },
                Block::List {
                    items: vec!["Optional".to_string()],
                    ordered: false,
                },
                Block::Heading {
                    level: 2,
                    text: "Bill of quantities".to_string(),
                },
                Block::Grid {
                    title: Some("table 1".to_string()),
                    columns: vec!["Fragment".to_string(), "Qty".to_string()],
                    rows: vec![
                        vec!["Pole 9m".to_string(), "120".to_string()],
                        vec!["Stay".to_string(), "40".to_string()],
                    ],
                    rows_total: 2,
                    rows_shown: 2,
                    columns_total: 2,
                },
            ]
        );

        // Without numbering.xml every list is honestly bulleted rather than
        // guessed as numbered.
        let bytes = zip_of(&[("word/document.xml", body(inner))]);
        let doc = document(&bytes, &ask(Format::Docx)).unwrap();
        assert_eq!(
            doc.blocks.get(2),
            Some(&Block::List {
                items: vec!["Survey".to_string(), "Staking".to_string()],
                ordered: false,
            })
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_docx_inline_image_bounds() {
        let inner = format!(
            "{}{}{}{}{}",
            drawing(r#"r:embed="rId5""#),
            drawing(r#"r:embed="rId6""#),
            drawing(r#"r:link="rId9""#),
            drawing(r#"r:embed="rId7""#),
            drawing(r#"r:embed="rId42""#),
        );
        let bytes = zip_of(&[
            ("word/document.xml", body(&inner)),
            (
                "word/_rels/document.xml.rels",
                IMAGE_RELS.as_bytes().to_vec(),
            ),
            ("word/media/small.png", png_of(64)),
            (
                "word/media/big.png",
                png_of(MAX_INLINE_IMAGE_BYTES as usize + 1),
            ),
        ]);
        let doc = document(&bytes, &ask(Format::Docx)).unwrap();
        assert!(doc.truncated);

        // The image under the bound carries its bytes; the one over it keeps
        // its dimensions and loses only the bytes.
        match doc.blocks.first() {
            Some(Block::Image {
                index,
                mime,
                width,
                height,
                bytes_b64: Some(encoded),
            }) => {
                assert_eq!(*index, 0);
                assert_eq!(mime, "image/png");
                assert_eq!((*width, *height), (64, 32));
                assert!(!encoded.is_empty());
            }
            other => panic!("expected an inlined image, found {other:?}"),
        }
        assert_eq!(
            doc.blocks.get(1),
            Some(&Block::Image {
                index: 1,
                mime: "image/png".to_string(),
                width: 64,
                height: 32,
                bytes_b64: None,
            })
        );
        assert!(
            doc.notes.iter().any(|note| note
                == "word/media/big.png is 1,048,577 bytes; a preview inlines images up to 1 MiB, so it is shown without its bytes"),
            "{:?}",
            doc.notes
        );

        // A linked image is never fetched and its address never reaches the
        // preview.
        assert_eq!(
            doc.blocks.get(2),
            Some(&Block::Image {
                index: 2,
                mime: "application/octet-stream".to_string(),
                width: 0,
                height: 0,
                bytes_b64: None,
            })
        );
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("never fetches remote content")));
        let encoded = serde_json::to_string(&doc).unwrap();
        assert!(!encoded.contains("example.com"), "a remote address leaked");
        assert!(!encoded.contains("https://"), "a remote address leaked");

        // A media part named but absent, and a reference with no relationship.
        assert!(doc.notes.iter().any(|note| note
            == "word/media/gone.png is named by the document but is not in the package, so it is shown without its bytes"));
        assert!(doc.notes.iter().any(|note| note
            == "image reference rId42 has no target in word/_rels/document.xml.rels, so it is shown without its bytes"));

        // Twenty inlined images, then bytes stop and the note says so.
        let inner = drawing(r#"r:embed="rId5""#).repeat(MAX_INLINE_IMAGES + 1);
        let bytes = zip_of(&[
            ("word/document.xml", body(&inner)),
            (
                "word/_rels/document.xml.rels",
                IMAGE_RELS.as_bytes().to_vec(),
            ),
            ("word/media/small.png", png_of(64)),
        ]);
        let doc = document(&bytes, &ask(Format::Docx)).unwrap();
        let inlined = doc
            .blocks
            .iter()
            .filter(|block| {
                matches!(
                    block,
                    Block::Image {
                        bytes_b64: Some(_),
                        ..
                    }
                )
            })
            .count();
        assert_eq!(inlined, MAX_INLINE_IMAGES);
        assert_eq!(doc.blocks.len(), MAX_INLINE_IMAGES + 1);
        assert!(doc
            .notes
            .iter()
            .any(|note| note
                == "a preview inlines 20 images; the rest are shown without their bytes"));
        assert!(doc.truncated);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_docx_block_and_list_bounds_report_their_numbers() {
        let inner = "<w:p><w:r><w:t>Clause</w:t></w:r></w:p>".repeat(MAX_DOC_BLOCKS + 50);
        let bytes = zip_of(&[("word/document.xml", body(&inner))]);
        let doc = document(&bytes, &ask(Format::Docx)).unwrap();
        assert_eq!(doc.blocks.len(), MAX_DOC_BLOCKS);
        assert!(doc.truncated);
        assert!(
            doc.notes
                .iter()
                .any(|note| note == "the document has 250 blocks; the preview shows the first 200"),
            "{:?}",
            doc.notes
        );

        let item = r#"<w:p><w:pPr><w:numPr><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Fragment</w:t></w:r></w:p>"#;
        let bytes = zip_of(&[("word/document.xml", body(&item.repeat(205)))]);
        let doc = document(&bytes, &ask(Format::Docx)).unwrap();
        match doc.blocks.first() {
            Some(Block::List { items, ordered }) => {
                assert_eq!(items.len(), MAX_LIST_ITEMS);
                assert!(!ordered);
            }
            other => panic!("expected one list block, found {other:?}"),
        }
        assert_eq!(doc.blocks.len(), 1);
        assert!(doc
            .notes
            .iter()
            .any(|note| note == "a list has 205 items; the preview shows the first 200"));

        // A wide, long table keeps the sheet bounds and tells its own totals.
        let mut table = String::from("<w:tbl>");
        for row in 0..205 {
            table.push_str("<w:tr>");
            for column in 0..45 {
                table.push_str(&format!(
                    "<w:tc><w:p><w:r><w:t>r{row}c{column}</w:t></w:r></w:p></w:tc>"
                ));
            }
            table.push_str("</w:tr>");
        }
        table.push_str("</w:tbl>");
        let bytes = zip_of(&[("word/document.xml", body(&table))]);
        let doc = document(&bytes, &ask(Format::Docx)).unwrap();
        match doc.blocks.first() {
            Some(Block::Grid {
                columns,
                rows,
                rows_total,
                rows_shown,
                columns_total,
                ..
            }) => {
                assert_eq!(columns.len(), MAX_GRID_COLUMNS);
                assert_eq!(rows.len(), MAX_GRID_ROWS);
                assert_eq!(*rows_shown, 200);
                assert_eq!(*rows_total, 204);
                assert_eq!(*columns_total, 45);
                assert_eq!(columns.first().map(String::as_str), Some("r0c0"));
                assert_eq!(rows[0].first().map(String::as_str), Some("r1c0"));
            }
            other => panic!("expected a grid block, found {other:?}"),
        }
        assert!(doc
            .notes
            .iter()
            .any(|note| note == "table 1 has 204 rows; the preview shows the first 200"));
        assert!(doc
            .notes
            .iter()
            .any(|note| note == "table 1 has 45 columns; the preview shows the first 40"));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_docx_malformed_zip_is_a_sentence() {
        for bytes in [
            b"not a zip at all".to_vec(),
            b"PK\x03\x04 truncated".to_vec(),
            Vec::new(),
        ] {
            let error = document(&bytes, &ask(Format::Docx)).unwrap_err();
            assert!(
                error.starts_with("the document is not a readable zip container"),
                "{error}"
            );
        }
        let mut cut = zip_of(&[("word/document.xml", body("<w:p/>"))]);
        cut.truncate(48);
        assert!(document(&cut, &ask(Format::Docx)).is_err());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_docx_malformed_xml_is_a_sentence() {
        let bytes = zip_of(&[(
            "word/document.xml",
            b"<w:document><w:body><w:p></w:wrong></w:body></w:document>".to_vec(),
        )]);
        let error = document(&bytes, &ask(Format::Docx)).unwrap_err();
        assert!(
            error.starts_with("word/document.xml is not readable xml"),
            "{error}"
        );

        // A malformed rels or numbering part is not fatal: the document still
        // reads, without its relationships.
        let bytes = zip_of(&[
            (
                "word/document.xml",
                body("<w:p><w:r><w:t>Ok</w:t></w:r></w:p>"),
            ),
            (
                "word/_rels/document.xml.rels",
                b"<Relationships></wrong>".to_vec(),
            ),
            ("word/numbering.xml", b"<w:numbering></wrong>".to_vec()),
        ]);
        let doc = document(&bytes, &ask(Format::Docx)).unwrap();
        assert_eq!(
            doc.blocks,
            vec![Block::Paragraph {
                text: "Ok".to_string()
            }]
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_docx_missing_parts_are_named() {
        let bytes = zip_of(&[("docProps/app.xml", b"<Properties/>".to_vec())]);
        assert_eq!(
            document(&bytes, &ask(Format::Docx)).unwrap_err(),
            "the document has no word/document.xml part, so it is not a readable docx"
        );

        let bytes = zip_of(&[
            (
                "mimetype",
                b"application/vnd.oasis.opendocument.text".to_vec(),
            ),
            ("content.xml", b"<office:document-content/>".to_vec()),
        ]);
        assert_eq!(
            document(&bytes, &ask(Format::Docx)).unwrap_err(),
            "this file is an OpenDocument package, not an OOXML document; convert it to docx or open it externally"
        );

        assert_eq!(
            document(&bytes, &ask(Format::Odt)).unwrap_err(),
            "odt is an OpenDocument text document and this kernel decodes docx (OOXML); convert it to docx or open it externally"
        );

        let oversized = vec![0u8; 8 * 1024 * 1024 + 1];
        assert_eq!(
            document(&oversized, &ask(Format::Docx)).unwrap_err(),
            "preview input is 8,388,609 bytes; the docx bound is 8 MiB"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_docx_helpers_are_total() {
        assert_eq!(heading_level(Some("Heading1")), Some(1));
        assert_eq!(heading_level(Some("heading 6")), Some(6));
        assert_eq!(heading_level(Some("Heading-3")), Some(3));
        assert_eq!(heading_level(Some("Heading7")), None);
        assert_eq!(heading_level(Some("BodyText")), None);
        assert_eq!(heading_level(None), None);
        assert_eq!(
            media_part("media/image1.png").as_deref(),
            Some("word/media/image1.png")
        );
        assert_eq!(
            media_part("/word/media/image2.png").as_deref(),
            Some("word/media/image2.png")
        );
        assert_eq!(
            media_part("../customXml/item1.xml").as_deref(),
            Some("customXml/item1.xml")
        );
        assert_eq!(image_mime("word/media/image1.JPEG"), "image/jpeg");
        assert_eq!(image_mime("word/media/image1.svg"), "image/svg+xml");
        assert_eq!(image_mime("word/media/image1"), "application/octet-stream");
        assert_eq!(cut_text("abcdef", 3), ("abc".to_string(), true));
        assert_eq!(dimensions(b"not an image"), (0, 0));

        let numbering = parse_numbering(NUMBERING.as_bytes());
        assert!(numbering.ordered(Some("1")));
        assert!(!numbering.ordered(Some("2")));
        assert!(!numbering.ordered(Some("9")));
        assert!(!numbering.ordered(None));

        let relationships = parse_relationships(IMAGE_RELS.as_bytes());
        assert_eq!(relationships.len(), 4);
        assert!(relationships.iter().any(|relation| relation.external));

        // An svg is never inlined: it can reference remote content itself.
        let bytes = zip_of(&[
            ("word/document.xml", body(&drawing(r#"r:embed="rId5""#))),
            (
                "word/_rels/document.xml.rels",
                br#"<Relationships><Relationship Id="rId5" Target="media/logo.svg"/></Relationships>"#.to_vec(),
            ),
            ("word/media/logo.svg", b"<svg width=\"10\" height=\"10\"/>".to_vec()),
        ]);
        let doc = document(&bytes, &ask(Format::Docx)).unwrap();
        assert_eq!(
            doc.blocks,
            vec![Block::Image {
                index: 0,
                mime: "image/svg+xml".to_string(),
                width: 0,
                height: 0,
                bytes_b64: None,
            }]
        );
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("an svg can reference remote content")));

        let bytes = zip_of(&[("a.txt", b"0123456789".to_vec())]);
        let mut archive = ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert_eq!(
            member_bytes(&mut archive, "a.txt", 4).unwrap_err(),
            "zip member a.txt is 10 bytes; the zip member bound is 4 bytes"
        );
        assert!(member_bytes(&mut archive, "missing.txt", 4)
            .unwrap()
            .is_none());
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_docx_rels_outside_word_media_are_never_read_or_fetched() {
        const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="r1" Type="image" Target="../../../../etc/secret.png"/>
  <Relationship Id="r2" Type="image" Target="http://evil.example/pixel.png" TargetMode="External"/>
  <Relationship Id="r3" Type="image" Target="/absolute/outside.png"/>
  <Relationship Id="r4" Type="image" Target="../docProps/thumb.png"/>
  <Relationship Id="r5" Type="image" Target="file:///C:/Users/x/pixel.png" TargetMode="External"/>
</Relationships>"#;
        let document = body(&format!(
            "{}{}{}{}{}",
            drawing("r:embed=\"r1\""),
            drawing("r:embed=\"r2\""),
            drawing("r:embed=\"r3\""),
            drawing("r:embed=\"r4\""),
            drawing("r:link=\"r5\"")
        ));
        let package = zip_of(&[
            ("word/document.xml", document),
            ("word/_rels/document.xml.rels", RELS.as_bytes().to_vec()),
            ("docProps/thumb.png", png_of(200)),
            ("etc/secret.png", png_of(200)),
        ]);
        let doc = super::document(&package, &ask(Format::Docx)).expect("decodes");
        let carried: Vec<bool> = doc
            .blocks
            .iter()
            .map(|block| match block {
                Block::Image { bytes_b64, .. } => bytes_b64.is_some(),
                other => panic!("expected an image block, found {other:?}"),
            })
            .collect();
        assert_eq!(
            carried,
            vec![false, false, false, true, false],
            "only the part inside the package is carried; `..` is not walked, an absolute or external target is never read"
        );
        let text = serde_json::to_string(&doc).expect("json");
        for forbidden in ["evil.example", "http:", "file:", "C:/Users", "etc/secret"] {
            assert!(
                !text.contains(forbidden),
                "a preview never carries a remote or outside address: {forbidden} in {text}"
            );
        }
        assert!(doc.notes.iter().any(|note| note
            == "image reference r1 points outside the package, so it is shown without its bytes"));
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("outside the document")));
        assert!(doc
            .notes
            .iter()
            .any(|note| note == "absolute/outside.png is named by the document but is not in the package, so it is shown without its bytes"));
        assert_eq!(
            media_part("media/image1.png").as_deref(),
            Some("word/media/image1.png")
        );
        assert_eq!(
            media_part("/word/media/image1.png").as_deref(),
            Some("word/media/image1.png")
        );
        assert_eq!(
            media_part("./media/../media/./x.png").as_deref(),
            Some("word/media/x.png")
        );
        assert_eq!(media_part("..\\..\\x.png"), None);
        assert_eq!(media_part("/"), None);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_docx_entity_expansion_and_deep_nesting_are_harmless() {
        // A billion-laughs DOCTYPE: the reference resolves to nothing, never
        // to its expansion.
        let mut laughs =
            String::from(r#"<?xml version="1.0"?><!DOCTYPE w:document [<!ENTITY lol "lol">"#);
        for level in 1..=9 {
            let previous = format!(
                "&lol{};",
                if level == 1 {
                    String::new()
                } else {
                    (level - 1).to_string()
                }
            );
            laughs.push_str(&format!("<!ENTITY lol{level} \"{}\">", previous.repeat(10)));
        }
        laughs.push_str("]>");
        laughs.push_str(&HEAD[HEAD.find("<w:document").unwrap()..]);
        laughs.push_str("<w:p><w:r><w:t>before &lol9; after &#65; &amp;</w:t></w:r></w:p>");
        laughs.push_str(TAIL);
        let package = zip_of(&[("word/document.xml", laughs.into_bytes())]);
        let doc = super::document(&package, &ask(Format::Docx)).expect("decodes");
        assert_eq!(
            doc.blocks[0],
            Block::Paragraph {
                text: "before  after A &".into()
            }
        );
        // 100,000 unclosed paragraphs, tables and runs: a flat state machine,
        // no stack, no panic.
        for open in ["<w:p>", "<w:tbl><w:tr><w:tc>", "<w:r><w:t>"] {
            let mut xml = HEAD.to_string();
            xml.push_str(&open.repeat(100_000));
            xml.push('x');
            let package = zip_of(&[("word/document.xml", xml.into_bytes())]);
            let _ = super::document(&package, &ask(Format::Docx));
        }
        // A document.xml whose header claims 0 bytes still decodes: the claim
        // is not believed and the read is bounded by the member cap.
        let mut package = zip_of(&[(
            "word/document.xml",
            body("<w:p><w:r><w:t>honest</w:t></w:r></w:p>"),
        )]);
        let local = package.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
        let central = package.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        package[local + 22..local + 26].copy_from_slice(&0u32.to_le_bytes());
        package[central + 24..central + 28].copy_from_slice(&0u32.to_le_bytes());
        let doc = super::document(&package, &ask(Format::Docx)).expect("decodes");
        assert_eq!(
            doc.blocks[0],
            Block::Paragraph {
                text: "honest".into()
            }
        );
    }
}
