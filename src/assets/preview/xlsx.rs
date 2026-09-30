//! Workbooks (§4.2 xlsx / ods): zip + quick-xml, cached values, one bounded grid per sheet.
//!
//! `workbook` reads four parts of an OOXML package and nothing else:
//! `xl/workbook.xml` (sheet names and order), `xl/_rels/workbook.xml.rels`
//! (`r:id` → worksheet part), `xl/sharedStrings.xml` (bounded) and the one
//! worksheet asked for. Every member is inflated through one bounded reader,
//! so a lying central directory or a zip bomb is refused with its own number
//! instead of being decompressed.
//!
//! A cell shows the **cached value** the spreadsheet wrote: `<v>`, an inline
//! `<is>` string, a shared string by index, or a boolean. A formula (`<f>`) is
//! never read and never evaluated — the preview shows what the workbook last
//! computed, which is the only honest answer without a formula engine.
//!
//! The grid's `columns` are spreadsheet column labels (`A` … `AN`), never a
//! guessed header row: a sheet's column identity is its letter, and row 1
//! stays a row. Sheet tabs are listed in a `meta` block so a host can switch
//! sheets by re-requesting the preview with `sheet` set to a sheet name
//! (`member` stays the container path the workbook was extracted from).
//!
//! `ods` is refused by name: an OpenDocument workbook is a different XML, and
//! this slice decodes OOXML.

use std::io::{Cursor, Read};

use quick_xml::events::{BytesEnd, BytesRef, BytesStart, BytesText, Event};
use quick_xml::Reader;
use zip::ZipArchive;

use super::super::{
    check_bytes, check_format_cap, thousands, Block, Format, PreviewDoc, PreviewRequest,
    MAX_GRID_COLUMNS, MAX_GRID_ROWS,
};

/// Largest single zip member this decoder inflates. A member that declares —
/// or inflates to — more is refused with its number rather than expanded.
const MAX_ZIP_MEMBER_BYTES: u64 = 64 * 1024 * 1024;
/// Shared strings read before the table is left where it is (a display bound,
/// reported in a note; later strings render as an empty cell).
const MAX_SHARED_STRINGS: usize = 200_000;
/// Sheets listed in the `meta` block (reported in a note when reached).
const MAX_SHEETS: usize = 1_000;
/// `Relationship` rows read from a `.rels` part before resolution falls back
/// to the positional `xl/worksheets/sheet<n>.xml` name.
const MAX_RELATIONSHIPS: usize = 10_000;
/// The spreadsheet grid ceilings; a row past them stops the bounded scan.
const MAX_SCAN_ROWS: usize = 1_048_576;
/// Highest column index a cell reference may name before it is ignored.
const MAX_SCAN_COLUMNS: usize = 16_384;
/// Characters kept per shown cell (a display bound, counted into a note).
const MAX_CELL_CHARS: usize = 500;

/// One `<sheet>` row of `xl/workbook.xml`, in workbook order.
struct Sheet {
    name: String,
    rel_id: String,
}

/// What one bounded worksheet scan found.
struct SheetGrid {
    rows: Vec<Vec<String>>,
    rows_total: u64,
    columns_total: u64,
    cells_cut: u64,
    scan_truncated: bool,
}

pub fn workbook(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    let format = request.format;
    check_format_cap(bytes.len() as u64, format)?;
    if format == Format::Ods {
        return Err(
            "ods is an OpenDocument workbook and this kernel decodes xlsx (OOXML); convert it to xlsx or open it externally"
                .into(),
        );
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| format!("the workbook is not a readable zip container: {error}"))?;

    let Some(workbook_xml) = part(&mut archive, "xl/workbook.xml")? else {
        return Err(missing_workbook_message(&mut archive));
    };
    let (sheets, sheets_truncated) = parse_sheets(&workbook_xml, MAX_SHEETS)?;
    if sheets.is_empty() {
        return Err("the workbook declares no sheets".into());
    }
    let relationships = match part(&mut archive, "xl/_rels/workbook.xml.rels")? {
        Some(xml) => parse_relationships(&xml),
        None => Vec::new(),
    };
    let (shared, shared_truncated) = match part(&mut archive, "xl/sharedStrings.xml")? {
        Some(xml) => parse_shared_strings(&xml, MAX_SHARED_STRINGS)?,
        None => (Vec::new(), false),
    };

    let (index, member_named_the_container) = select_sheet(&sheets, request.sheet.as_deref())?;
    let Some(sheet) = sheets.get(index) else {
        return Err(format!("the workbook has no sheet {}", index + 1));
    };
    let part_name = worksheet_part(&relationships, sheet, index);
    let Some(sheet_xml) = part(&mut archive, &part_name)? else {
        return Err(format!(
            "the workbook has no worksheet part {part_name} for sheet '{}'",
            sheet.name
        ));
    };
    let row_limit = request
        .rows
        .map_or(MAX_GRID_ROWS, |rows| (rows as usize).min(MAX_GRID_ROWS));
    let grid = parse_worksheet(&sheet_xml, &shared, row_limit, &sheet.name)?;

    let shown_columns = grid.columns_total.min(MAX_GRID_COLUMNS as u64) as usize;
    let mut rows = grid.rows;
    for row in &mut rows {
        row.resize(shown_columns, String::new());
    }
    let rows_shown = rows.len() as u64;

    let mut doc = PreviewDoc::new(format);
    doc.blocks.push(Block::Grid {
        title: Some(sheet.name.clone()),
        columns: (0..shown_columns).map(column_label).collect(),
        rows,
        rows_total: grid.rows_total,
        rows_shown,
        columns_total: grid.columns_total,
    });
    doc.blocks.push(Block::Meta {
        entries: sheets
            .iter()
            .enumerate()
            .map(|(position, listed)| (format!("sheet {}", position + 1), listed.name.clone()))
            .collect(),
    });

    if grid.rows_total > rows_shown {
        doc.truncated = true;
        doc.notes.push(format!(
            "sheet '{}' has {} rows; the preview shows the first {}",
            sheet.name,
            thousands(grid.rows_total),
            thousands(rows_shown)
        ));
    }
    if grid.columns_total > shown_columns as u64 {
        doc.truncated = true;
        doc.notes.push(format!(
            "sheet '{}' has {} columns; the preview shows the first {}",
            sheet.name,
            thousands(grid.columns_total),
            thousands(shown_columns as u64)
        ));
    }
    if grid.cells_cut > 0 {
        doc.truncated = true;
        doc.notes.push(format!(
            "{} cell values were cut at {} characters",
            thousands(grid.cells_cut),
            thousands(MAX_CELL_CHARS as u64)
        ));
    }
    if grid.scan_truncated {
        doc.truncated = true;
        doc.notes.push(format!(
            "sheet '{}' names a row past {}; the row count stops there",
            sheet.name,
            thousands(MAX_SCAN_ROWS as u64)
        ));
    }
    if shared_truncated {
        doc.truncated = true;
        doc.notes.push(format!(
            "the shared string table has more than {} entries; a cell using a later string shows empty",
            thousands(MAX_SHARED_STRINGS as u64)
        ));
    }
    if sheets_truncated {
        doc.truncated = true;
        doc.notes.push(format!(
            "the workbook declares more than {} sheets; the rest are not listed",
            thousands(MAX_SHEETS as u64)
        ));
    }
    if member_named_the_container {
        doc.notes.push(format!(
            "sheet names the container entry, not a worksheet; this is sheet '{}'",
            sheet.name
        ));
    }
    if sheets.len() > 1 {
        doc.notes.push(format!(
            "the workbook has {} sheets; ask for another by name with sheet (this is '{}')",
            sheets.len(),
            sheet.name
        ));
    }
    Ok(doc)
}

/// The sheet to show: `sheet` naming a sheet wins, `sheet` naming the
/// container entry falls back to the first sheet and says so, and a name that
/// is neither is refused with the sheet names it could have been.
fn select_sheet(sheets: &[Sheet], member: Option<&str>) -> Result<(usize, bool), String> {
    let Some(member) = member.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok((0, false));
    };
    if let Some(index) = sheets.iter().position(|sheet| sheet.name == member) {
        return Ok((index, false));
    }
    if let Some(index) = sheets
        .iter()
        .position(|sheet| sheet.name.eq_ignore_ascii_case(member))
    {
        return Ok((index, false));
    }
    if names_a_container_entry(member) {
        return Ok((0, true));
    }
    let names = sheets
        .iter()
        .take(MAX_GRID_COLUMNS)
        .map(|sheet| format!("'{}'", sheet.name))
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "the workbook has no sheet named '{}'; its sheets are {names}",
        cut_text(member, 120).0
    ))
}

/// A container member path (`Lot3/boq.xlsx`) rather than a sheet name.
fn names_a_container_entry(member: &str) -> bool {
    if member.contains('/') || member.contains('\\') {
        return true;
    }
    let lower = member.to_ascii_lowercase();
    [".xlsx", ".xlsm", ".xls", ".ods"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

/// The worksheet part for a sheet: its relationship target, or the positional
/// `xl/worksheets/sheet<n>.xml` when the rels part is absent or partial.
fn worksheet_part(relationships: &[(String, String)], sheet: &Sheet, index: usize) -> String {
    relationships
        .iter()
        .find(|(id, _)| !sheet.rel_id.is_empty() && *id == sheet.rel_id)
        .and_then(|(_, target)| normalise_part(target))
        .unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", index + 1))
}

/// A workbook relationship target as a package part name: relative targets
/// resolve against `xl/` segment by segment, absolute ones from the package
/// root, and a target that climbs past the root names nothing (`None`, so the
/// positional worksheet name is used instead). Never a path on any disk.
fn normalise_part(target: &str) -> Option<String> {
    let target = target.trim();
    let (mut parts, rest): (Vec<&str>, &str) = match target.strip_prefix('/') {
        Some(absolute) => (Vec::new(), absolute),
        None => (vec!["xl"], target),
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

/// `<sheet name= r:id=>` rows in document order, bounded by `limit`.
fn parse_sheets(xml: &[u8], limit: usize) -> Result<(Vec<Sheet>, bool), String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut sheets: Vec<Sheet> = Vec::new();
    let mut truncated = false;
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|error| format!("xl/workbook.xml is not readable xml: {error}"))?;
        match event {
            Event::Start(element) | Event::Empty(element) if is(&element, b"sheet") => {
                if sheets.len() >= limit {
                    truncated = true;
                    break;
                }
                let name = attribute(&element, b"name").unwrap_or_default();
                let position = sheets.len() + 1;
                sheets.push(Sheet {
                    name: if name.is_empty() {
                        format!("sheet {position}")
                    } else {
                        cut_text(&name, 200).0
                    },
                    rel_id: attribute_local(&element, b"id").unwrap_or_default(),
                });
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok((sheets, truncated))
}

/// `<Relationship Id= Target=>` pairs. A malformed rels part is not fatal:
/// resolution falls back to the positional worksheet name.
fn parse_relationships(xml: &[u8]) -> Vec<(String, String)> {
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
                    relationships.push((id, target));
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

/// `<si>` entries as display strings (rich-text runs concatenated), bounded
/// by `limit`; the flag says the table went past it.
fn parse_shared_strings(xml: &[u8], limit: usize) -> Result<(Vec<String>, bool), String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut strings: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_entry = false;
    let mut capture = false;
    let mut truncated = false;
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|error| format!("xl/sharedStrings.xml is not readable xml: {error}"))?;
        match event {
            Event::Start(element) if is(&element, b"si") => {
                if strings.len() >= limit {
                    truncated = true;
                    break;
                }
                in_entry = true;
                current.clear();
            }
            Event::End(element) if is_end(&element, b"si") => {
                in_entry = false;
                strings.push(current.clone());
            }
            Event::Start(element) if is(&element, b"t") => capture = in_entry,
            Event::End(element) if is_end(&element, b"t") => capture = false,
            Event::Text(text) if capture => current.push_str(&text_of(&text)),
            Event::GeneralRef(reference) if capture => {
                current.push_str(&reference_of(&reference));
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok((strings, truncated))
}

/// One bounded pass over a worksheet: the first `row_limit` rows and the
/// first [`MAX_GRID_COLUMNS`] columns are kept; every `<row>` and `<c>` is
/// counted so the totals are the sheet's own, not the preview's.
fn parse_worksheet(
    xml: &[u8],
    shared: &[String],
    row_limit: usize,
    sheet_name: &str,
) -> Result<SheetGrid, String> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();

    let mut grid = SheetGrid {
        rows: Vec::new(),
        rows_total: 0,
        columns_total: 0,
        cells_cut: 0,
        scan_truncated: false,
    };
    let mut row_index = 0usize;
    let mut rows_seen = 0usize;
    let mut row_cells: Vec<(usize, String)> = Vec::new();
    let mut cursor = 0usize;
    let mut cell_column = 0usize;
    let mut cell_type = String::new();
    let mut cell_text = String::new();
    let mut in_cell = false;
    let mut capture = false;

    loop {
        let event = reader.read_event_into(&mut buf).map_err(|error| {
            format!("the worksheet of sheet '{sheet_name}' is not readable xml: {error}")
        })?;
        match event {
            Event::Start(element) | Event::Empty(element) if is(&element, b"row") => {
                row_index = attribute(&element, b"r")
                    .and_then(|reference| reference.trim().parse::<usize>().ok())
                    .map_or(rows_seen, |number| number.saturating_sub(1));
                if row_index >= MAX_SCAN_ROWS {
                    grid.scan_truncated = true;
                    break;
                }
                rows_seen = rows_seen.max(row_index + 1);
                grid.rows_total = grid.rows_total.max(row_index as u64 + 1);
                row_cells.clear();
                cursor = 0;
            }
            Event::Start(element) if is(&element, b"c") => {
                cell_column = attribute(&element, b"r")
                    .and_then(|reference| column_index(&reference))
                    .unwrap_or(cursor);
                cursor = cell_column.saturating_add(1);
                grid.columns_total = grid.columns_total.max(cell_column as u64 + 1);
                cell_type = attribute(&element, b"t").unwrap_or_default();
                cell_text.clear();
                in_cell = true;
                capture = false;
            }
            // A self-closing cell is styled but empty: it counts toward the
            // sheet's columns and carries no value.
            Event::Empty(element) if is(&element, b"c") => {
                let column = attribute(&element, b"r")
                    .and_then(|reference| column_index(&reference))
                    .unwrap_or(cursor);
                cursor = column.saturating_add(1);
                grid.columns_total = grid.columns_total.max(column as u64 + 1);
                in_cell = false;
                capture = false;
            }
            Event::Start(element) if in_cell && (is(&element, b"v") || is(&element, b"t")) => {
                capture = true;
            }
            Event::End(element) if is_end(&element, b"v") || is_end(&element, b"t") => {
                capture = false
            }
            Event::Text(text) if in_cell && capture => cell_text.push_str(&text_of(&text)),
            Event::GeneralRef(reference) if in_cell && capture => {
                cell_text.push_str(&reference_of(&reference));
            }
            Event::End(element) if is_end(&element, b"c") => {
                in_cell = false;
                capture = false;
                let (text, cut) =
                    cut_text(&cell_value(&cell_type, &cell_text, shared), MAX_CELL_CHARS);
                if cut {
                    grid.cells_cut += 1;
                }
                if !text.is_empty() && cell_column < MAX_GRID_COLUMNS && row_index < row_limit {
                    row_cells.push((cell_column, text));
                }
                cell_text.clear();
            }
            Event::End(element) if is_end(&element, b"row") => {
                if row_index < row_limit {
                    let width = row_cells
                        .iter()
                        .map(|(column, _)| column + 1)
                        .max()
                        .unwrap_or(0);
                    let mut row = vec![String::new(); width];
                    for (column, text) in row_cells.drain(..) {
                        if let Some(slot) = row.get_mut(column) {
                            *slot = text;
                        }
                    }
                    while grid.rows.len() < row_index {
                        grid.rows.push(Vec::new());
                    }
                    if grid.rows.len() == row_index {
                        grid.rows.push(row);
                    } else if let Some(slot) = grid.rows.get_mut(row_index) {
                        *slot = row;
                    }
                }
                row_cells.clear();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(grid)
}

/// A cell's display text: shared strings by index, booleans as words, and
/// every other type exactly as the cached value was written. `<f>` is never
/// read, so a formula cell shows what the workbook last computed.
fn cell_value(cell_type: &str, raw: &str, shared: &[String]) -> String {
    match cell_type {
        "s" => raw
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|index| shared.get(index))
            .cloned()
            .unwrap_or_default(),
        "b" => match raw.trim() {
            "1" => "TRUE".to_string(),
            "0" => "FALSE".to_string(),
            other => other.to_string(),
        },
        _ => raw.to_string(),
    }
}

/// `"AB12"` → the 0-based column index `27`; `None` when the reference names
/// no column or a column past [`MAX_SCAN_COLUMNS`].
fn column_index(cell_reference: &str) -> Option<usize> {
    let mut index = 0usize;
    let mut seen = false;
    for character in cell_reference.chars() {
        if !character.is_ascii_alphabetic() {
            break;
        }
        seen = true;
        let value = character.to_ascii_uppercase() as usize - 'A' as usize + 1;
        index = index.checked_mul(26)?.checked_add(value)?;
        if index > MAX_SCAN_COLUMNS {
            return None;
        }
    }
    if seen {
        index.checked_sub(1)
    } else {
        None
    }
}

/// The 0-based column index `27` → `"AB"`.
fn column_label(index: usize) -> String {
    let mut letters = Vec::new();
    let mut value = index + 1;
    while value > 0 {
        let remainder = (value - 1) % 26;
        letters.push(b'A' + remainder as u8);
        value = (value - 1) / 26;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

/// Cuts display text at `limit` characters, saying whether it was cut.
fn cut_text(text: &str, limit: usize) -> (String, bool) {
    if text.chars().count() <= limit {
        return (text.to_string(), false);
    }
    (text.chars().take(limit).collect(), true)
}

/// Why `xl/workbook.xml` is missing, in the words the reader needs.
fn missing_workbook_message(archive: &mut ZipArchive<Cursor<&[u8]>>) -> String {
    if is_opendocument(archive) {
        return "this file is an OpenDocument package, not an OOXML workbook; convert it to xlsx or open it externally".to_string();
    }
    "the workbook has no xl/workbook.xml part, so it is not a readable xlsx".to_string()
}

/// Whether the package declares an OpenDocument `mimetype` member.
fn is_opendocument(archive: &mut ZipArchive<Cursor<&[u8]>>) -> bool {
    member_bytes(archive, "mimetype", 256)
        .ok()
        .flatten()
        .is_some_and(|bytes| {
            String::from_utf8_lossy(&bytes).starts_with("application/vnd.oasis.opendocument")
        })
}

/// One package part, inflated through the member bound.
fn part(archive: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Option<Vec<u8>>, String> {
    member_bytes(archive, name, MAX_ZIP_MEMBER_BYTES)
}

/// Inflates one member into memory, refusing above `cap` by name — the
/// declared size and the inflated size both count, so a lying central
/// directory is refused rather than believed.
fn member_bytes(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
    cap: u64,
) -> Result<Option<Vec<u8>>, String> {
    let Some(index) = archive.index_for_name(name) else {
        return Ok(None);
    };
    let file = archive
        .by_index(index)
        .map_err(|error| format!("zip member {name} cannot be opened: {error}"))?;
    let what = format!("zip member {name}");
    check_bytes(&what, file.size(), "zip member", cap)?;
    let mut bytes = Vec::new();
    file.take(cap.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| format!("zip member {name} cannot be inflated: {error}"))?;
    check_bytes(&what, bytes.len() as u64, "zip member", cap)?;
    Ok(Some(bytes))
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
/// (`r:id`, `xml:space`).
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

    const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets><sheet name="Summary" sheetId="1" r:id="rId1"/><sheet name="Data" sheetId="2" r:id="rId2"/></sheets>
</workbook>"#;

    const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/>
</Relationships>"#;

    const SHARED_STRINGS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3"><si><t>Pole</t></si><si><t>Type</t></si><si><t>Wood &amp; steel</t></si></sst>"#;

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

    fn ask(format: Format, sheet: Option<&str>, rows: Option<u32>) -> PreviewRequest {
        PreviewRequest {
            schema: REQUEST_SCHEMA.to_string(),
            format,
            member: None,
            sheet: sheet.map(str::to_string),
            pages: None,
            rows,
            name: None,
        }
    }

    /// 205 rows × 45 columns: a shared string, an inline string, a boolean and
    /// a formula whose cached value is 42.
    fn wide_sheet() -> Vec<u8> {
        let mut xml = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
        );
        for row in 1..=205usize {
            xml.push_str(&format!("<row r=\"{row}\" spans=\"1:45\">"));
            for column in 0..45usize {
                let reference = format!("{}{row}", column_label(column));
                match (row, column) {
                    (1, 0) => xml.push_str(&format!("<c r=\"{reference}\" t=\"s\"><v>0</v></c>")),
                    (1, 1) => xml.push_str(&format!("<c r=\"{reference}\" t=\"s\"><v>2</v></c>")),
                    (2, 0) => xml.push_str(&format!(
                        "<c r=\"{reference}\" t=\"inlineStr\"><is><t>P-002</t></is></c>"
                    )),
                    (2, 1) => xml.push_str(&format!("<c r=\"{reference}\" t=\"b\"><v>1</v></c>")),
                    (2, 2) => xml.push_str(&format!(
                        "<c r=\"{reference}\"><f>SUM(D2:E2)</f><v>42</v></c>"
                    )),
                    _ => xml.push_str(&format!(
                        "<c r=\"{reference}\"><v>{}</v></c>",
                        row * 100 + column
                    )),
                }
            }
            xml.push_str("</row>");
        }
        xml.push_str("</sheetData></worksheet>");
        xml.into_bytes()
    }

    fn data_sheet() -> Vec<u8> {
        String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Transformer</t></is></c><c r="B1" s="4"/></row><row r="2"><c r="A2"><v>7</v></c></row></sheetData></worksheet>"#,
        )
        .into_bytes()
    }

    fn workbook_zip() -> Vec<u8> {
        zip_of(&[
            ("[Content_Types].xml", b"<Types/>".to_vec()),
            ("xl/workbook.xml", WORKBOOK.as_bytes().to_vec()),
            (
                "xl/_rels/workbook.xml.rels",
                WORKBOOK_RELS.as_bytes().to_vec(),
            ),
            ("xl/sharedStrings.xml", SHARED_STRINGS.as_bytes().to_vec()),
            ("xl/worksheets/sheet1.xml", wide_sheet()),
            ("xl/worksheets/sheet2.xml", data_sheet()),
        ])
    }

    struct GridView<'a> {
        title: Option<&'a str>,
        columns: &'a [String],
        rows: &'a [Vec<String>],
        rows_total: u64,
        rows_shown: u64,
        columns_total: u64,
    }

    fn grid_of(doc: &PreviewDoc) -> GridView<'_> {
        match doc.blocks.first() {
            Some(Block::Grid {
                title,
                columns,
                rows,
                rows_total,
                rows_shown,
                columns_total,
            }) => GridView {
                title: title.as_deref(),
                columns,
                rows,
                rows_total: *rows_total,
                rows_shown: *rows_shown,
                columns_total: *columns_total,
            },
            other => panic!("expected a grid block, found {other:?}"),
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_xlsx_grid_cached_values_and_bounds() {
        let doc = workbook(&workbook_zip(), &ask(Format::Xlsx, None, None)).unwrap();
        assert_eq!(doc.format, Format::Xlsx);
        assert_eq!(doc.kind, Kind::Sheet);
        assert!(doc.bounded);
        assert!(doc.truncated);

        let grid = grid_of(&doc);
        let rows = grid.rows;
        assert_eq!(grid.title, Some("Summary"));
        assert_eq!(grid.columns.len(), MAX_GRID_COLUMNS);
        assert_eq!(grid.columns.first().map(String::as_str), Some("A"));
        assert_eq!(grid.columns.last().map(String::as_str), Some("AN"));
        assert_eq!(rows.len(), MAX_GRID_ROWS);
        assert_eq!(grid.rows_shown, 200);
        assert_eq!(grid.rows_total, 205);
        assert_eq!(grid.columns_total, 45);
        assert!(rows.iter().all(|row| row.len() == MAX_GRID_COLUMNS));

        // Shared strings by index, an inline string, a boolean as a word and a
        // formula shown as its cached value — never the formula itself.
        assert_eq!(rows[0][0], "Pole");
        assert_eq!(rows[0][1], "Wood & steel");
        assert_eq!(rows[1][0], "P-002");
        assert_eq!(rows[1][1], "TRUE");
        assert_eq!(rows[1][2], "42");
        assert_eq!(rows[2][0], "300");
        let encoded = serde_json::to_string(&doc).unwrap();
        assert!(!encoded.contains("SUM("), "a formula reached the preview");

        assert!(
            doc.notes
                .iter()
                .any(|note| note == "sheet 'Summary' has 205 rows; the preview shows the first 200"),
            "{:?}",
            doc.notes
        );
        assert!(
            doc.notes.iter().any(
                |note| note == "sheet 'Summary' has 45 columns; the preview shows the first 40"
            ),
            "{:?}",
            doc.notes
        );
        assert!(doc.notes.iter().any(|note| note.contains("2 sheets")));

        // Sheet tabs, so the host can re-request another sheet by name.
        assert_eq!(
            doc.blocks.get(1),
            Some(&Block::Meta {
                entries: vec![
                    ("sheet 1".to_string(), "Summary".to_string()),
                    ("sheet 2".to_string(), "Data".to_string()),
                ],
            })
        );

        // The row hint narrows the window without changing the honest totals.
        let doc = workbook(&workbook_zip(), &ask(Format::Xlsx, None, Some(3))).unwrap();
        let grid = grid_of(&doc);
        assert_eq!(grid.rows.len(), 3);
        assert_eq!(grid.rows_shown, 3);
        assert_eq!(grid.rows_total, 205);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_xlsx_sheet_selection_by_sheet() {
        let bytes = workbook_zip();
        let doc = workbook(&bytes, &ask(Format::Xlsx, Some("Data"), None)).unwrap();
        let grid = grid_of(&doc);
        assert_eq!(grid.title, Some("Data"));
        assert_eq!(grid.rows[0][0], "Transformer");
        assert_eq!(grid.rows[1][0], "7");
        assert_eq!(grid.rows_total, 2);
        assert_eq!(grid.columns_total, 2);
        assert!(!doc.truncated);

        let doc = workbook(&bytes, &ask(Format::Xlsx, Some("data"), None)).unwrap();
        assert_eq!(grid_of(&doc).title, Some("Data"));

        // A container member path is not a sheet name: the first sheet is
        // shown and the preview says why.
        let doc = workbook(&bytes, &ask(Format::Xlsx, Some("Lot3/boq.xlsx"), None)).unwrap();
        assert_eq!(grid_of(&doc).title, Some("Summary"));
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("names the container entry")));

        let error = workbook(&bytes, &ask(Format::Xlsx, Some("Ledger"), None)).unwrap_err();
        assert_eq!(
            error,
            "the workbook has no sheet named 'Ledger'; its sheets are 'Summary', 'Data'"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_xlsx_malformed_zip_is_a_sentence() {
        for bytes in [
            b"not a zip at all".to_vec(),
            b"PK\x03\x04 truncated".to_vec(),
            Vec::new(),
        ] {
            let error = workbook(&bytes, &ask(Format::Xlsx, None, None)).unwrap_err();
            assert!(
                error.starts_with("the workbook is not a readable zip container"),
                "{error}"
            );
        }
        let mut cut = workbook_zip();
        cut.truncate(64);
        assert!(workbook(&cut, &ask(Format::Xlsx, None, None)).is_err());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_xlsx_malformed_xml_is_a_sentence() {
        let broken = b"<workbook><sheets><sheet name=\"A\"/></wrong></workbook>".to_vec();
        let bytes = zip_of(&[("xl/workbook.xml", broken.clone())]);
        let error = workbook(&bytes, &ask(Format::Xlsx, None, None)).unwrap_err();
        assert!(
            error.starts_with("xl/workbook.xml is not readable xml"),
            "{error}"
        );

        let bytes = zip_of(&[
            ("xl/workbook.xml", WORKBOOK.as_bytes().to_vec()),
            (
                "xl/_rels/workbook.xml.rels",
                WORKBOOK_RELS.as_bytes().to_vec(),
            ),
            ("xl/sharedStrings.xml", broken.clone()),
            ("xl/worksheets/sheet1.xml", data_sheet()),
        ]);
        let error = workbook(&bytes, &ask(Format::Xlsx, None, None)).unwrap_err();
        assert!(
            error.starts_with("xl/sharedStrings.xml is not readable xml"),
            "{error}"
        );

        let bytes = zip_of(&[
            ("xl/workbook.xml", WORKBOOK.as_bytes().to_vec()),
            (
                "xl/_rels/workbook.xml.rels",
                WORKBOOK_RELS.as_bytes().to_vec(),
            ),
            (
                "xl/worksheets/sheet1.xml",
                b"<worksheet><sheetData><row></wrong></sheetData></worksheet>".to_vec(),
            ),
        ]);
        let error = workbook(&bytes, &ask(Format::Xlsx, None, None)).unwrap_err();
        assert!(
            error.starts_with("the worksheet of sheet 'Summary' is not readable xml"),
            "{error}"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_xlsx_missing_parts_are_named() {
        let bytes = zip_of(&[("docProps/app.xml", b"<Properties/>".to_vec())]);
        assert_eq!(
            workbook(&bytes, &ask(Format::Xlsx, None, None)).unwrap_err(),
            "the workbook has no xl/workbook.xml part, so it is not a readable xlsx"
        );

        let bytes = zip_of(&[(
            "xl/workbook.xml",
            b"<workbook><sheets/></workbook>".to_vec(),
        )]);
        assert_eq!(
            workbook(&bytes, &ask(Format::Xlsx, None, None)).unwrap_err(),
            "the workbook declares no sheets"
        );

        let bytes = zip_of(&[
            ("xl/workbook.xml", WORKBOOK.as_bytes().to_vec()),
            (
                "xl/_rels/workbook.xml.rels",
                WORKBOOK_RELS.as_bytes().to_vec(),
            ),
        ]);
        assert_eq!(
            workbook(&bytes, &ask(Format::Xlsx, None, None)).unwrap_err(),
            "the workbook has no worksheet part xl/worksheets/sheet1.xml for sheet 'Summary'"
        );

        let bytes = zip_of(&[
            (
                "mimetype",
                b"application/vnd.oasis.opendocument.spreadsheet".to_vec(),
            ),
            ("content.xml", b"<office:document-content/>".to_vec()),
        ]);
        assert_eq!(
            workbook(&bytes, &ask(Format::Xlsx, None, None)).unwrap_err(),
            "this file is an OpenDocument package, not an OOXML workbook; convert it to xlsx or open it externally"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_xlsx_member_and_input_bounds_are_named_refusals() {
        let bytes = zip_of(&[("a.txt", b"0123456789".to_vec())]);
        let mut archive = ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        assert_eq!(
            member_bytes(&mut archive, "a.txt", 4).unwrap_err(),
            "zip member a.txt is 10 bytes; the zip member bound is 4 bytes"
        );
        assert_eq!(member_bytes(&mut archive, "missing.txt", 4).unwrap(), None);
        assert_eq!(
            member_bytes(&mut archive, "a.txt", 10).unwrap().as_deref(),
            Some(&b"0123456789"[..])
        );

        let oversized = vec![0u8; 8 * 1024 * 1024 + 1];
        assert_eq!(
            workbook(&oversized, &ask(Format::Xlsx, None, None)).unwrap_err(),
            "preview input is 8,388,609 bytes; the xlsx bound is 8 MiB"
        );

        assert_eq!(
            workbook(&workbook_zip(), &ask(Format::Ods, None, None)).unwrap_err(),
            "ods is an OpenDocument workbook and this kernel decodes xlsx (OOXML); convert it to xlsx or open it externally"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_xlsx_helpers_are_total() {
        assert_eq!(column_index("A1"), Some(0));
        assert_eq!(column_index("AB12"), Some(27));
        assert_eq!(column_index("12"), None);
        assert_eq!(column_index(""), None);
        assert_eq!(column_index("XFDA1"), None);
        assert_eq!(column_label(0), "A");
        assert_eq!(column_label(25), "Z");
        assert_eq!(column_label(26), "AA");
        assert_eq!(column_label(39), "AN");
        for index in [0usize, 1, 25, 26, 27, 700, 16_000] {
            assert_eq!(
                column_index(&format!("{}1", column_label(index))),
                Some(index)
            );
        }
        assert_eq!(
            normalise_part("worksheets/sheet1.xml").as_deref(),
            Some("xl/worksheets/sheet1.xml")
        );
        assert_eq!(
            normalise_part("/xl/worksheets/sheet2.xml").as_deref(),
            Some("xl/worksheets/sheet2.xml")
        );
        assert_eq!(
            normalise_part("../customXml/item1.xml").as_deref(),
            Some("customXml/item1.xml")
        );
        assert_eq!(normalise_part("../../../etc/sheet.xml"), None);
        assert_eq!(normalise_part("/"), None);
        assert_eq!(cut_text("abcdef", 3), ("abc".to_string(), true));
        assert_eq!(cut_text("abc", 3), ("abc".to_string(), false));
        assert_eq!(cell_value("s", " 1 ", &["a".into(), "b".into()]), "b");
        assert_eq!(cell_value("s", "9", &["a".into()]), "");
        assert_eq!(cell_value("b", "0", &[]), "FALSE");
        assert_eq!(cell_value("n", "12.5", &[]), "12.5");

        let (sheets, truncated) = parse_sheets(WORKBOOK.as_bytes(), 1).unwrap();
        assert!(truncated);
        assert_eq!(sheets.len(), 1);
        let (strings, truncated) = parse_shared_strings(SHARED_STRINGS.as_bytes(), 2).unwrap();
        assert!(truncated);
        assert_eq!(strings, vec!["Pole".to_string(), "Type".to_string()]);
        let (strings, truncated) =
            parse_shared_strings(SHARED_STRINGS.as_bytes(), MAX_SHARED_STRINGS).unwrap();
        assert!(!truncated);
        assert_eq!(strings.len(), 3);
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_xlsx_entities_wild_references_and_lying_headers() {
        // Entities in the shared strings and the sheet: predefined and
        // numeric references resolve, a DOCTYPE entity resolves to nothing.
        let shared = r#"<?xml version="1.0"?><!DOCTYPE sst [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">]><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>a &lol2; b &#66; &lt;</t></si></sst>"#;
        // A cell past every column, a row past the scan bound, a cell with no
        // reference at all, and a shared-string index that does not exist.
        let sheet = r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c t="s"><v>99</v></c><c r="ZZZZZZ1"><v>far</v></c><c r="B1" t="s"><v>-1</v></c></row><row r="99999999"><c r="A99999999"><v>late</v></c></row></sheetData></worksheet>"#;
        let mut package = zip_of(&[
            ("xl/workbook.xml", WORKBOOK.as_bytes().to_vec()),
            (
                "xl/_rels/workbook.xml.rels",
                WORKBOOK_RELS.as_bytes().to_vec(),
            ),
            ("xl/sharedStrings.xml", shared.as_bytes().to_vec()),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes().to_vec()),
        ]);
        let doc = workbook(&package, &ask(Format::Xlsx, None, None)).expect("decodes");
        let grid = grid_of(&doc);
        assert_eq!(grid.rows[0][0], "a  b B <");
        assert_eq!(
            grid.rows[0][1], "",
            "a shared-string index past the table is empty"
        );
        assert_eq!(
            grid.rows_total, 1,
            "a row past the scan bound stops the count there"
        );
        assert!(doc.notes.iter().any(|note| note.contains("1,048,576")));
        assert!(doc.truncated);

        // Every header now claims the worksheet is 0 bytes: the claim is not
        // believed, the bounded read gets the real bytes, the grid is the same.
        let name = b"xl/worksheets/sheet1.xml";
        let mut at = 0usize;
        let mut patched = 0usize;
        while let Some(found) = package[at..]
            .windows(4)
            .position(|w| w == b"PK\x03\x04" || w == b"PK\x01\x02")
        {
            let start = at + found;
            let (name_at, size_at) = if &package[start..start + 4] == b"PK\x03\x04" {
                (start + 30, start + 22)
            } else {
                (start + 46, start + 24)
            };
            if package.get(name_at..name_at + name.len()) == Some(name.as_slice()) {
                package[size_at..size_at + 4].copy_from_slice(&0u32.to_le_bytes());
                patched += 1;
            }
            at = start + 4;
        }
        assert_eq!(patched, 2, "both headers of the worksheet were rewritten");
        let doc = workbook(&package, &ask(Format::Xlsx, None, None)).expect("decodes");
        assert_eq!(grid_of(&doc).rows[0][0], "a  b B <");

        // Truncated at every length: a refusal or a document, never a panic.
        for length in (0..package.len()).step_by(7) {
            let _ = workbook(&package[..length], &ask(Format::Xlsx, None, None));
        }
        // 100,000 unclosed rows and cells: a flat state machine.
        let mut deep = String::from(r#"<worksheet><sheetData>"#);
        deep.push_str(&"<row><c><v>".repeat(100_000));
        let package = zip_of(&[
            ("xl/workbook.xml", WORKBOOK.as_bytes().to_vec()),
            ("xl/worksheets/sheet1.xml", deep.into_bytes()),
        ]);
        let _ = workbook(&package, &ask(Format::Xlsx, None, None));
    }
}
