//! Delimited text (§4.2 csv / tsv): one bounded grid, delimiter sniffed by
//! the kernel.
//!
//! The first record is the header row, the rest are data rows. Nothing is
//! typed, computed or reordered: a preview shows the cells as written. The
//! grid is cut at [`MAX_GRID_ROWS`] × [`MAX_GRID_COLUMNS`] and says so
//! through `rows_total` / `columns_total` and a note that names both numbers,
//! so a 5,000-row file is never presented as a 200-row file.

use std::borrow::Cow;

use super::super::{
    check_format_cap, thousands, Block, Format, PreviewDoc, PreviewRequest, MAX_GRID_COLUMNS,
    MAX_GRID_ROWS,
};

/// Lines the sniffer reads before deciding (§4.2 "delimiter sniffed by the
/// kernel").
const SNIFF_LINES: usize = 20;

/// Characters kept per shown cell (a display bound, counted into a note): a
/// single 6 MiB field is never carried whole into a 200-row grid.
const MAX_CELL_CHARS: usize = 500;

/// The delimiters the kernel knows, in preference order.
const DELIMITERS: [(u8, &str); 4] = [
    (b',', "comma"),
    (b';', "semicolon"),
    (b'\t', "tab"),
    (b'|', "pipe"),
];

pub fn delimited(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    check_format_cap(bytes.len() as u64, request.format)?;
    let mut doc = PreviewDoc::new(request.format);
    let text = String::from_utf8_lossy(bytes);
    if matches!(text, Cow::Owned(_)) {
        doc.notes.push(
            "the file is not valid UTF-8; invalid sequences are shown as the replacement character"
                .into(),
        );
    }
    if text.trim().is_empty() {
        return Err("delimited text is empty; there is no header row to read".into());
    }
    // A byte-order mark belongs to the encoding, not to the first column name.
    let source = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let (delimiter, name, sniffed) = sniff(source, request.format);
    doc.notes.push(if sniffed {
        format!("the delimiter was sniffed as a {name} over the first {SNIFF_LINES} lines")
    } else {
        format!("no delimiter was found in the first {SNIFF_LINES} lines; the file is read as one {name}-delimited column")
    });

    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .flexible(true)
        .from_reader(source.as_bytes());
    let mut records = reader.records();
    let header = match records.next() {
        Some(Ok(record)) => record,
        Some(Err(error)) => {
            return Err(format!("delimited text could not be read: {error}"));
        }
        None => return Err("delimited text has no rows; there is no header row to read".into()),
    };

    let mut columns_total = header.len();
    let mut cells_cut: u64 = 0;
    let mut cut = |text: &str| -> String {
        if text.chars().count() <= MAX_CELL_CHARS {
            return text.to_string();
        }
        cells_cut += 1;
        text.chars().take(MAX_CELL_CHARS).collect()
    };
    let columns: Vec<String> = header.iter().take(MAX_GRID_COLUMNS).map(&mut cut).collect();
    let shown = columns.len();
    let limit = request
        .rows
        .and_then(|rows| usize::try_from(rows).ok())
        .unwrap_or(MAX_GRID_ROWS)
        .min(MAX_GRID_ROWS);

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut rows_total: u64 = 0;
    for record in records {
        let record =
            record.map_err(|error| format!("delimited text could not be read: {error}"))?;
        rows_total += 1;
        columns_total = columns_total.max(record.len());
        if rows.len() < limit {
            let mut row: Vec<String> = record.iter().take(shown).map(&mut cut).collect();
            row.resize(shown, String::new());
            rows.push(row);
        }
    }

    let rows_shown = rows.len() as u64;
    if rows_total > rows_shown {
        doc.truncated = true;
        doc.notes.push(format!(
            "the file has {} rows; the first {} are shown (the grid row bound is {MAX_GRID_ROWS})",
            thousands(rows_total),
            thousands(rows_shown)
        ));
    }
    if columns_total > shown {
        doc.truncated = true;
        doc.notes.push(format!(
            "the file has {} columns; the first {} are shown (the grid column bound is {MAX_GRID_COLUMNS})",
            thousands(columns_total as u64),
            thousands(shown as u64)
        ));
    }
    if cells_cut > 0 {
        doc.truncated = true;
        doc.notes.push(format!(
            "{} cell values were cut at {} characters",
            thousands(cells_cut),
            thousands(MAX_CELL_CHARS as u64)
        ));
    }

    doc.blocks.push(Block::Grid {
        title: request.name.clone(),
        columns,
        rows,
        rows_total,
        rows_shown,
        columns_total: columns_total as u64,
    });
    doc.blocks.push(Block::Meta {
        entries: vec![
            (
                "bytes".into(),
                format!("{} bytes", thousands(bytes.len() as u64)),
            ),
            ("delimiter".into(), name.to_string()),
            ("rows".into(), thousands(rows_total)),
            ("columns".into(), thousands(columns_total as u64)),
        ],
    });
    Ok(doc)
}

/// Picks the delimiter whose field count is most consistent over the first
/// [`SNIFF_LINES`] non-empty lines, counting only separators outside quotes.
/// With no evidence at all the format's own delimiter is used and the caller
/// is told the file reads as one column.
fn sniff(text: &str, format: Format) -> (u8, &'static str, bool) {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(SNIFF_LINES)
        .collect();
    let mut best: Option<(usize, usize, u8, &'static str)> = None;
    for (delimiter, name) in candidates(format) {
        let counts: Vec<usize> = lines
            .iter()
            .map(|line| count_outside_quotes(line, delimiter))
            .collect();
        let Some(fields) = counts.iter().copied().filter(|count| *count > 0).max() else {
            continue;
        };
        let agreeing = counts.iter().filter(|count| **count == fields).count();
        let score = (agreeing, fields);
        if best
            .is_none_or(|(best_agreeing, best_fields, _, _)| score > (best_agreeing, best_fields))
        {
            best = Some((agreeing, fields, delimiter, name));
        }
    }
    match best {
        Some((_, _, delimiter, name)) => (delimiter, name, true),
        None => {
            let (delimiter, name) = natural(format);
            (delimiter, name, false)
        }
    }
}

/// [`DELIMITERS`] with the format's own delimiter first, so a `.tsv` whose
/// tabs and commas are equally consistent is read as tab-separated.
fn candidates(format: Format) -> [(u8, &'static str); 4] {
    let (natural, _) = natural(format);
    let mut ordered = DELIMITERS;
    if let Some(at) = ordered.iter().position(|(byte, _)| *byte == natural) {
        ordered.swap(0, at);
    }
    ordered
}

/// The delimiter the declared format implies, used first when sniffing and
/// alone when the file holds no evidence at all.
fn natural(format: Format) -> (u8, &'static str) {
    if format == Format::Tsv {
        (b'\t', "tab")
    } else {
        (b',', "comma")
    }
}

/// Counts `delimiter` outside double quotes; a quoted `a,b` is one field.
fn count_outside_quotes(line: &str, delimiter: u8) -> usize {
    let mut quoted = false;
    let mut count = 0usize;
    for byte in line.as_bytes() {
        match *byte {
            b'"' => quoted = !quoted,
            byte if byte == delimiter && !quoted => count += 1,
            _ => {}
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::REQUEST_SCHEMA;

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

    fn grid(doc: &PreviewDoc) -> (&Vec<String>, &Vec<Vec<String>>, u64, u64) {
        match &doc.blocks[0] {
            Block::Grid {
                columns,
                rows,
                rows_total,
                columns_total,
                ..
            } => (columns, rows, *rows_total, *columns_total),
            other => panic!("the first block is the grid, not {other:?}"),
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_csv_delimiter_sniffing() {
        let cases: [(&str, Format, &str); 4] = [
            ("name,kind\npole,mv\nspan,lv\n", Format::Csv, "comma"),
            ("name;kind\npole;mv\nspan;lv\n", Format::Csv, "semicolon"),
            ("name\tkind\npole\tmv\nspan\tlv\n", Format::Tsv, "tab"),
            ("name|kind\npole|mv\nspan|lv\n", Format::Csv, "pipe"),
        ];
        for (source, format, delimiter) in cases {
            let doc = delimited(source.as_bytes(), &request(format)).expect("delimited decodes");
            let (columns, rows, rows_total, columns_total) = grid(&doc);
            assert_eq!(columns, &vec!["name".to_string(), "kind".to_string()]);
            assert_eq!(
                rows,
                &vec![
                    vec!["pole".to_string(), "mv".to_string()],
                    vec!["span".to_string(), "lv".to_string()],
                ]
            );
            assert_eq!((rows_total, columns_total), (2, 2));
            assert!(
                doc.notes.iter().any(|note| note.contains(delimiter)),
                "{delimiter}: {:?}",
                doc.notes
            );
            assert!(!doc.truncated);
        }

        // A quoted comma is a cell, not a separator: the semicolon wins.
        let doc = delimited(
            "name;note\npole;\"a, b\"\nspan;\"c, d\"\n".as_bytes(),
            &request(Format::Csv),
        )
        .expect("delimited decodes");
        let (columns, rows, _, _) = grid(&doc);
        assert_eq!(columns.len(), 2);
        assert_eq!(rows[0], vec!["pole".to_string(), "a, b".to_string()]);

        // A byte-order mark is not part of the first column name.
        let doc = delimited(
            "\u{feff}name,kind\npole,mv\n".as_bytes(),
            &request(Format::Csv),
        )
        .expect("delimited decodes");
        assert_eq!(grid(&doc).0, &vec!["name".to_string(), "kind".to_string()]);

        // No delimiter anywhere: one column, said out loud.
        let doc = delimited("alpha\nbeta\n".as_bytes(), &request(Format::Csv))
            .expect("delimited decodes");
        let (columns, rows, rows_total, _) = grid(&doc);
        assert_eq!(columns, &vec!["alpha".to_string()]);
        assert_eq!(rows, &vec![vec!["beta".to_string()]]);
        assert_eq!(rows_total, 1);
        assert!(doc.notes.iter().any(|note| note.contains("no delimiter")));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_csv_ragged_rows_padded() {
        let doc = delimited("a,b,c\n1,2\n3,4,5,6\n".as_bytes(), &request(Format::Csv))
            .expect("delimited decodes");
        let (columns, rows, rows_total, columns_total) = grid(&doc);
        assert_eq!(columns.len(), 3);
        assert_eq!(
            rows[0],
            vec!["1".to_string(), "2".to_string(), String::new()]
        );
        assert_eq!(
            rows[1],
            vec!["3".to_string(), "4".to_string(), "5".to_string()]
        );
        assert_eq!((rows_total, columns_total), (2, 4));
        assert!(doc.truncated);
        assert!(
            doc.notes.iter().any(|note| note.contains("4 columns")),
            "{:?}",
            doc.notes
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_tabular_malformed_input() {
        assert!(delimited(&[], &request(Format::Csv))
            .expect_err("empty is refused")
            .contains("no header row"));
        assert!(delimited(b"   \n\n", &request(Format::Csv))
            .expect_err("blank is refused")
            .contains("no header row"));

        let broken = [b'a', b',', b'b', b'\n', 0xff, b',', b'c', b'\n'];
        let doc = delimited(&broken, &request(Format::Csv)).expect("invalid UTF-8 never panics");
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("not valid UTF-8")));

        let over = vec![b','; 8 * 1024 * 1024 + 1];
        let refusal = delimited(&over, &request(Format::Csv)).expect_err("over the csv bound");
        assert!(
            refusal.contains("8,388,609") && refusal.contains("8 MiB"),
            "{refusal}"
        );
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_csv_single_huge_field_is_refused_or_cut_by_number() {
        // Over the cap: refused by number before the csv reader sees a byte.
        let huge = vec![b'a'; 10 * 1024 * 1024];
        let error = delimited(&huge, &request(Format::Csv)).unwrap_err();
        assert_eq!(
            error,
            "preview input is 10,485,760 bytes; the csv bound is 8 MiB"
        );
        // Under the cap: a 600,000-character field is a display cut that the
        // grid says out loud, never a 600 KB cell in a 200-row grid.
        let mut source = b"id,note\n1,".to_vec();
        source.extend(std::iter::repeat_n(b'n', 600_000));
        source.extend_from_slice(b"\n2,short\n");
        let doc = delimited(&source, &request(Format::Csv)).expect("csv decodes");
        let (_, rows, rows_total, _) = grid(&doc);
        assert_eq!(rows_total, 2);
        assert_eq!(rows[0][1].chars().count(), MAX_CELL_CHARS);
        assert_eq!(rows[1][1], "short");
        assert!(doc.truncated);
        assert!(doc
            .notes
            .iter()
            .any(|note| note == "1 cell values were cut at 500 characters"));
        // A one-byte file and a header-only file are honest, not panics.
        let doc = delimited(b"a", &request(Format::Csv)).expect("one byte");
        let (columns, rows, rows_total, _) = grid(&doc);
        assert_eq!(columns, &vec!["a".to_string()]);
        assert!(rows.is_empty());
        assert_eq!(rows_total, 0);
    }
}
