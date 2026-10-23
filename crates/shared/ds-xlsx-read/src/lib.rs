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
//!
//! Shared by the Kernel's asset previews (`ds-command-kernel`
//! `assets::preview::xlsx`) and the Reporter's MV structure-description
//! binding, so both read a workbook cell by the same rule.

use std::io::{Cursor, Read};

use quick_xml::events::{BytesEnd, BytesRef, BytesStart, BytesText, Event};
use quick_xml::Reader;
use zip::ZipArchive;

const MIB: u64 = 1024 * 1024;
/// §4.2 xlsx / ods (xls shares it: recognised, never decoded).
pub const MAX_WORKBOOK_BYTES: u64 = 8 * MIB;
/// §4.2: rows shown per grid (sheet, csv, tsv).
pub const MAX_GRID_ROWS: usize = 200;
/// §4.2: columns shown per grid.
pub const MAX_GRID_COLUMNS: usize = 40;

/// Refuses `actual` over `bound`, naming both: "preview input is 34,603,008
/// bytes; the pdf bound is 32 MiB".
pub fn check_bytes(what: &str, actual: u64, bound_label: &str, bound: u64) -> Result<(), String> {
    if actual > bound {
        return Err(format!(
            "{what} is {} bytes; the {bound_label} bound is {}",
            thousands(actual),
            describe_bytes(bound)
        ));
    }
    Ok(())
}

/// "32 MiB", "64 KiB" or "200,000 bytes" — whichever is exact.
pub fn describe_bytes(bytes: u64) -> String {
    if bytes > 0 && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else if bytes > 0 && bytes.is_multiple_of(1024) {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{} bytes", thousands(bytes))
    }
}

/// Decimal digits grouped by thousands: 34603008 → "34,603,008".
pub fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Largest single zip member this decoder inflates. A member that declares —
/// or inflates to — more is refused with its number rather than expanded.
pub const MAX_ZIP_MEMBER_BYTES: u64 = 64 * 1024 * 1024;
/// Shared strings read before the table is left where it is (a display bound,
/// reported in a note; later strings render as an empty cell).
pub const MAX_SHARED_STRINGS: usize = 200_000;
/// Sheets listed in the `meta` block (reported in a note when reached).
pub const MAX_SHEETS: usize = 1_000;
/// `Relationship` rows read from a `.rels` part before resolution falls back
/// to the positional `xl/worksheets/sheet<n>.xml` name.
pub const MAX_RELATIONSHIPS: usize = 10_000;
/// The spreadsheet grid ceilings; a row past them stops the bounded scan.
pub const MAX_SCAN_ROWS: usize = 1_048_576;
/// Highest column index a cell reference may name before it is ignored.
pub const MAX_SCAN_COLUMNS: usize = 16_384;
/// Characters kept per shown cell (a display bound, counted into a note).
pub const MAX_CELL_CHARS: usize = 500;

/// One `<sheet>` row of `xl/workbook.xml`, in workbook order.
pub struct Sheet {
    pub name: String,
    pub rel_id: String,
}

/// What one bounded worksheet scan found.
pub struct SheetGrid {
    pub rows: Vec<Vec<String>>,
    pub rows_total: u64,
    pub columns_total: u64,
    pub cells_cut: u64,
    pub cells_unresolved: u64,
    pub scan_truncated: bool,
}

/// Reuse the bounded OOXML decoder for an exact source-column binding.
/// Unlike a preview, any cut cell, sheet, column or row refuses the source.
pub fn complete_sheet(
    bytes: &[u8],
    name: &str,
    max_rows: usize,
) -> Result<Vec<Vec<String>>, String> {
    check_bytes(
        "preview input",
        bytes.len() as u64,
        "xlsx",
        MAX_WORKBOOK_BYTES,
    )?;
    if !(1..=20_000).contains(&max_rows) {
        return Err("complete sheet row bound must be 1..20000".into());
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let xml = part(&mut archive, "xl/workbook.xml")?.ok_or("workbook has no sheet catalogue")?;
    let (sheets, truncated) = parse_sheets(&xml, MAX_SHEETS)?;
    let matches = sheets.iter().filter(|s| s.name == name).collect::<Vec<_>>();
    let [sheet] = matches.as_slice() else {
        return Err("source sheet must have one exact declared name".into());
    };
    if truncated {
        return Err("source workbook sheet catalogue is truncated".into());
    }
    let rels = part(&mut archive, "xl/_rels/workbook.xml.rels")?
        .ok_or("source workbook has no sheet relationships")?;
    let relationships = parse_relationships(&rels);
    let targets = relationships
        .iter()
        .filter(|(id, _)| id == &sheet.rel_id)
        .collect::<Vec<_>>();
    let [(_, target)] = targets.as_slice() else {
        return Err("source sheet must have one exact relationship".into());
    };
    let member = normalise_part(target).ok_or("source sheet relationship escapes its package")?;
    let (shared, shared_cut) = match part(&mut archive, "xl/sharedStrings.xml")? {
        Some(xml) => parse_shared_strings(&xml, MAX_SHARED_STRINGS)?,
        None => (Vec::new(), false),
    };
    let xml = part(&mut archive, &member)?.ok_or("source sheet part is missing")?;
    let grid = parse_worksheet(&xml, &shared, max_rows, name)?;
    if shared_cut
        || grid.cells_unresolved > 0
        || grid.cells_cut > 0
        || grid.scan_truncated
        || grid.rows_total > max_rows as u64
        || grid.columns_total > MAX_GRID_COLUMNS as u64
    {
        return Err(
            "source sheet exceeds a complete decoding bound; no descriptions were truncated".into(),
        );
    }
    Ok(grid.rows)
}

/// A workbook relationship target as a package part name: relative targets
/// resolve against `xl/` segment by segment, absolute ones from the package
/// root, and a target that climbs past the root names nothing (`None`, so the
/// positional worksheet name is used instead). Never a path on any disk.
pub fn normalise_part(target: &str) -> Option<String> {
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
pub fn parse_sheets(xml: &[u8], limit: usize) -> Result<(Vec<Sheet>, bool), String> {
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
pub fn parse_relationships(xml: &[u8]) -> Vec<(String, String)> {
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
pub fn parse_shared_strings(xml: &[u8], limit: usize) -> Result<(Vec<String>, bool), String> {
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
pub fn parse_worksheet(
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
        cells_unresolved: 0,
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
                if row_index < rows_seen {
                    grid.cells_unresolved += 1;
                }
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
                if cell_column < cursor {
                    grid.cells_unresolved += 1;
                }
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
                if cell_type == "s"
                    && cell_text
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .is_none_or(|index| index >= shared.len())
                {
                    grid.cells_unresolved += 1;
                }
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
pub fn cell_value(cell_type: &str, raw: &str, shared: &[String]) -> String {
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
pub fn column_index(cell_reference: &str) -> Option<usize> {
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
pub fn column_label(index: usize) -> String {
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
pub fn cut_text(text: &str, limit: usize) -> (String, bool) {
    if text.chars().count() <= limit {
        return (text.to_string(), false);
    }
    (text.chars().take(limit).collect(), true)
}

/// Why `xl/workbook.xml` is missing, in the words the reader needs.
pub fn missing_workbook_message(archive: &mut ZipArchive<Cursor<&[u8]>>) -> String {
    if is_opendocument(archive) {
        return "this file is an OpenDocument package, not an OOXML workbook; convert it to xlsx or open it externally".to_string();
    }
    "the workbook has no xl/workbook.xml part, so it is not a readable xlsx".to_string()
}

/// Whether the package declares an OpenDocument `mimetype` member.
pub fn is_opendocument(archive: &mut ZipArchive<Cursor<&[u8]>>) -> bool {
    member_bytes(archive, "mimetype", 256)
        .ok()
        .flatten()
        .is_some_and(|bytes| {
            String::from_utf8_lossy(&bytes).starts_with("application/vnd.oasis.opendocument")
        })
}

/// One package part, inflated through the member bound.
pub fn part(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Option<Vec<u8>>, String> {
    member_bytes(archive, name, MAX_ZIP_MEMBER_BYTES)
}

/// Inflates one member into memory, refusing above `cap` by name — the
/// declared size and the inflated size both count, so a lying central
/// directory is refused rather than believed.
pub fn member_bytes(
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
pub fn is(element: &BytesStart<'_>, name: &[u8]) -> bool {
    element.local_name().as_ref() == name
}

/// The same for a closing tag.
pub fn is_end(element: &BytesEnd<'_>, name: &[u8]) -> bool {
    element.local_name().as_ref() == name
}

/// An attribute by its exact key.
pub fn attribute(element: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|attribute| attribute.key.as_ref() == key)
        .map(|attribute| decode_attribute(&attribute.value))
}

/// An attribute by its local name, whatever namespace prefix it carries
/// (`r:id`, `xml:space`).
pub fn attribute_local(element: &BytesStart<'_>, local: &[u8]) -> Option<String> {
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

pub fn decode_attribute(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value);
    quick_xml::escape::unescape(text.as_ref())
        .map(|unescaped| unescaped.into_owned())
        .unwrap_or_else(|_| text.into_owned())
}

/// A `&…;` reference inside captured text: numeric character references and
/// the five predefined entities. Anything else resolves to nothing rather
/// than to a guess.
pub fn reference_of(reference: &BytesRef<'_>) -> String {
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

pub fn text_of(text: &BytesText<'_>) -> String {
    match text.decode() {
        Ok(decoded) => quick_xml::escape::unescape(decoded.as_ref())
            .map(|unescaped| unescaped.into_owned())
            .unwrap_or_else(|_| decoded.into_owned()),
        Err(_) => String::new(),
    }
}
