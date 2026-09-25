//! Format recognition (§2.3): magic bytes beat extensions, `.bak` is not a
//! format, and `unknown` is the answer rather than a guess.
//!
//! The order is fixed. A signature in the leading bytes decides what the file
//! is. The name may only pick between formats that share one container (a
//! `.docx`, a `.kmz` and a plain `.zip` are all ZIP archives) or, when no
//! bytes were read at all — a container listing reads member names, never
//! member bytes (§3.4) — stand in for them, and the reason says which
//! happened. A name that contradicts the bytes never wins: a `.xlsx` whose
//! bytes are not a ZIP archive is `unknown`, and the reason says so.
//!
//! `reason` is the honest sentence beside the answer: why a format is not
//! previewable, or which evidence decided a close call.

use serde::Serialize;

use super::{
    describe_bytes, thousands, Format, Kind, MAX_DELIMITED_BYTES, MAX_DOCUMENT_BYTES,
    MAX_GEO_BYTES, MAX_IMAGE_BYTES, MAX_MAIL_BYTES, MAX_PDF_BYTES, MAX_PREVIEW_INPUT_BYTES,
    MAX_TEXT_BYTES, MAX_WORKBOOK_BYTES, RECOGNITION_HEAD_BYTES,
};

/// What the kernel found in the leading bytes. `reason` says why something is
/// not previewable, or which evidence decided a close call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recognition {
    pub format: Format,
    pub kind: Kind,
    pub previewable: bool,
    pub reason: Option<String>,
}

/// Bytes of the head the text sniffers read. More of a text file is the same
/// text file; reading further only costs time.
const TEXT_SNIFF_BYTES: usize = 8 * 1024;
/// Lines the delimiter and mail-header sniffers consider.
const SNIFF_LINES: usize = 20;
/// Suffixes that name no format at all (§2.3: "a `.bak` is not a format").
/// They are peeled off so the suffix beneath can still be a name hint, and
/// the reason always says the wrapper decided nothing.
const WRAPPER_SUFFIXES: [&str; 6] = ["bak", "backup", "old", "tmp", "part", "download"];
/// Delimiters the csv / tsv sniffer tries, most conventional first.
const DELIMITERS: [(char, &str); 4] = [
    (',', "comma"),
    ('\t', "tab"),
    (';', "semicolon"),
    ('|', "pipe"),
];

/// Recognises `head` (the leading bytes of a `bytes_len`-byte file). `name`
/// is a hint that may break ties between formats sharing a container (a
/// `.kmz` and a `.zip` are both ZIP archives), never the evidence itself.
pub fn recognise(name: Option<&str>, head: &[u8], bytes_len: u64) -> Recognition {
    let hint = NameHint::of(name);
    let (format, mut notes) = decide(&hint, head, bytes_len);
    if let Some(note) = preview_note(format) {
        notes.push(note.to_string());
    }
    Recognition {
        format,
        kind: kind_of(format),
        previewable: previewable(format),
        reason: (!notes.is_empty()).then(|| notes.join("; ")),
    }
}

/// §2.3 kind table.
pub fn kind_of(format: Format) -> Kind {
    match format {
        Format::Pdf | Format::Docx | Format::Doc | Format::Odt | Format::Rtf => Kind::Doc,
        Format::Xlsx | Format::Xls | Format::Ods | Format::Csv | Format::Tsv => Kind::Sheet,
        Format::Eml | Format::Msg => Kind::Mail,
        Format::Md | Format::Html | Format::Txt => Kind::Note,
        Format::Shp | Format::Kmz | Format::Kml | Format::Gpkg | Format::Geojson => Kind::Geo,
        Format::Jpg | Format::Png | Format::Webp | Format::Tif => Kind::Image,
        Format::Zip | Format::SevenZ | Format::TarGz => Kind::Pack,
        Format::Dsgrid | Format::Unknown => Kind::Other,
    }
}

/// Whether the product previews this format at all: in the kernel, or (pdf)
/// through the host's `pdf.js`. Containers are walked, not previewed; msg
/// and gpkg are refused by name in slice 1 (§4.1), and so are odt and ods:
/// the slice decodes OOXML only, and `preview::decode` says the same.
pub fn previewable(format: Format) -> bool {
    match format {
        Format::Pdf
        | Format::Docx
        | Format::Xlsx
        | Format::Csv
        | Format::Tsv
        | Format::Eml
        | Format::Md
        | Format::Txt
        | Format::Shp
        | Format::Kmz
        | Format::Kml
        | Format::Geojson
        | Format::Jpg
        | Format::Png
        | Format::Webp
        | Format::Tif => true,
        Format::Html
        | Format::Doc
        | Format::Rtf
        | Format::Xls
        | Format::Odt
        | Format::Ods
        | Format::Msg
        | Format::Gpkg
        | Format::Zip
        | Format::SevenZ
        | Format::TarGz
        | Format::Dsgrid
        | Format::Unknown => false,
    }
}

/// §4.2 per-format input cap; formats without a row share the input ceiling.
pub fn format_cap_bytes(format: Format) -> u64 {
    match format {
        Format::Pdf => MAX_PDF_BYTES,
        Format::Docx | Format::Doc | Format::Odt | Format::Rtf => MAX_DOCUMENT_BYTES,
        Format::Xlsx | Format::Xls | Format::Ods => MAX_WORKBOOK_BYTES,
        Format::Csv | Format::Tsv => MAX_DELIMITED_BYTES,
        Format::Eml | Format::Msg => MAX_MAIL_BYTES,
        Format::Md | Format::Html | Format::Txt => MAX_TEXT_BYTES,
        Format::Jpg | Format::Png | Format::Webp | Format::Tif => MAX_IMAGE_BYTES,
        Format::Shp | Format::Kmz | Format::Kml | Format::Gpkg | Format::Geojson => MAX_GEO_BYTES,
        Format::Zip | Format::SevenZ | Format::TarGz | Format::Dsgrid | Format::Unknown => {
            MAX_PREVIEW_INPUT_BYTES
        }
    }
}

/// The honest sentence for a format the kernel will not decode, and for pdf,
/// which the host renders (§4.1, decisions D8 and D20). These are the same
/// refusals `preview::decode` returns, said before any bytes are read.
fn preview_note(format: Format) -> Option<&'static str> {
    Some(match format {
        Format::Pdf => {
            "pdf is rendered by pdf.js in the host (decision D8); the kernel recognises and bounds it"
        }
        Format::Html => {
            "HTML is rendered only in a sandboxed attachment viewer; the kernel does not execute or decode it"
        }
        Format::Msg => {
            "msg is recognised as mail by its OLE compound-file magic; its decoder is slice 2"
        }
        Format::Gpkg => {
            "gpkg needs a SQLite reader and there is none in WASM (decision D20); open it in the map instead"
        }
        Format::Doc | Format::Rtf => {
            "this format has no kernel decoder; open it externally or convert it to docx"
        }
        Format::Xls => "xls has no kernel decoder; open it externally or convert it to xlsx",
        Format::Odt => {
            "odt is an OpenDocument text document and the kernel decodes docx (OOXML); convert it to docx or open it externally"
        }
        Format::Ods => {
            "ods is an OpenDocument workbook and the kernel decodes xlsx (OOXML); convert it to xlsx or open it externally"
        }
        Format::Zip | Format::SevenZ | Format::TarGz => {
            "a container is not a document; walk it with container_walk and preview one member"
        }
        Format::Dsgrid => "dsgrid is a DS Grid model; open it in DS Grid rather than as a preview",
        Format::Unknown => {
            "unknown format has no preview; the metadata card is the honest alternative"
        }
        _ => return None,
    })
}

// ── the name ────────────────────────────────────────────────────────────────

/// What a file name suggests. Never evidence on its own.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct NameHint {
    /// The suffix the name ends with, lowercased and without its dot, once
    /// every wrapper suffix has been peeled off.
    suffix: Option<String>,
    /// The format that suffix claims, when the suffix names one at all.
    claim: Option<Format>,
    /// The wrapper suffix the name ended in (`bak`), which names no format.
    wrapper: Option<String>,
}

impl NameHint {
    fn of(name: Option<&str>) -> NameHint {
        let mut hint = NameHint::default();
        let Some(name) = name else {
            return hint;
        };
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
        let mut stem = base.to_lowercase();
        // Bounded: a name of dots can never spin this loop.
        for _ in 0..WRAPPER_SUFFIXES.len() + 1 {
            let Some((head, suffix)) = stem.rsplit_once('.') else {
                break;
            };
            if suffix.is_empty() {
                break;
            }
            if WRAPPER_SUFFIXES.contains(&suffix) {
                if hint.wrapper.is_none() {
                    hint.wrapper = Some(suffix.to_string());
                }
                stem = head.to_string();
                continue;
            }
            let suffix = if suffix == "gz" && head.ends_with(".tar") {
                "tar.gz".to_string()
            } else {
                suffix.to_string()
            };
            hint.claim = claimed_format(&suffix);
            hint.suffix = Some(suffix);
            break;
        }
        hint
    }
}

/// The format a suffix claims. Suffixes outside the §2.3 vocabulary claim
/// nothing, and neither do the companion suffixes of a shapefile.
fn claimed_format(suffix: &str) -> Option<Format> {
    Some(match suffix {
        "pdf" => Format::Pdf,
        "docx" => Format::Docx,
        "doc" => Format::Doc,
        "odt" => Format::Odt,
        "rtf" => Format::Rtf,
        "xlsx" => Format::Xlsx,
        "xls" => Format::Xls,
        "ods" => Format::Ods,
        "csv" => Format::Csv,
        "tsv" | "tab" => Format::Tsv,
        "eml" => Format::Eml,
        "msg" => Format::Msg,
        "md" | "markdown" => Format::Md,
        "html" | "htm" => Format::Html,
        "txt" | "text" | "log" => Format::Txt,
        "shp" => Format::Shp,
        "kmz" => Format::Kmz,
        "kml" => Format::Kml,
        "gpkg" => Format::Gpkg,
        "geojson" => Format::Geojson,
        "jpg" | "jpeg" => Format::Jpg,
        "png" => Format::Png,
        "webp" => Format::Webp,
        "tif" | "tiff" => Format::Tif,
        "zip" => Format::Zip,
        "7z" => Format::SevenZ,
        "tar.gz" | "tgz" => Format::TarGz,
        "dsgrid" => Format::Dsgrid,
        _ => return None,
    })
}

// ── the bytes ───────────────────────────────────────────────────────────────

/// A signature in the leading bytes. Every arm is a fixed byte string; none
/// of them is a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Magic {
    Pdf,
    Zip,
    Cfb,
    SevenZ,
    Gzip,
    Sqlite,
    Png,
    Jpeg,
    Webp,
    Tiff,
    Shp,
    Rtf,
}

fn magic_of(head: &[u8]) -> Option<Magic> {
    if head.starts_with(b"%PDF-") {
        return Some(Magic::Pdf);
    }
    if head.starts_with(b"PK\x03\x04")
        || head.starts_with(b"PK\x05\x06")
        || head.starts_with(b"PK\x07\x08")
    {
        return Some(Magic::Zip);
    }
    if head.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Some(Magic::Cfb);
    }
    if head.starts_with(&[b'7', b'z', 0xBC, 0xAF, 0x27, 0x1C]) {
        return Some(Magic::SevenZ);
    }
    if head.starts_with(&[0x1F, 0x8B]) {
        return Some(Magic::Gzip);
    }
    if head.starts_with(b"SQLite format 3\0") {
        return Some(Magic::Sqlite);
    }
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(Magic::Png);
    }
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Magic::Jpeg);
    }
    if head.starts_with(b"RIFF") && head.get(8..12) == Some(&b"WEBP"[..]) {
        return Some(Magic::Webp);
    }
    if head.starts_with(b"II*\0")
        || head.starts_with(b"MM\0*")
        || head.starts_with(b"II+\0")
        || head.starts_with(b"MM\0+")
    {
        return Some(Magic::Tiff);
    }
    if head.starts_with(&[0x00, 0x00, 0x27, 0x0A]) {
        return Some(Magic::Shp);
    }
    if head.starts_with(b"{\\rtf") {
        return Some(Magic::Rtf);
    }
    None
}

impl Magic {
    /// The format this signature means. Only the OLE compound file needs the
    /// name at all: `.doc`, `.xls` and `.msg` share one container.
    fn format(self, hint: &NameHint) -> Format {
        match self {
            Magic::Pdf => Format::Pdf,
            Magic::Zip => Format::Zip,
            Magic::Cfb => match hint.claim {
                Some(claim @ (Format::Doc | Format::Xls | Format::Msg)) => claim,
                _ => Format::Msg,
            },
            Magic::SevenZ => Format::SevenZ,
            Magic::Gzip => Format::TarGz,
            Magic::Sqlite => Format::Gpkg,
            Magic::Png => Format::Png,
            Magic::Jpeg => Format::Jpg,
            Magic::Webp => Format::Webp,
            Magic::Tiff => Format::Tif,
            Magic::Shp => Format::Shp,
            Magic::Rtf => Format::Rtf,
        }
    }
}

fn decide(hint: &NameHint, head: &[u8], bytes_len: u64) -> (Format, Vec<String>) {
    if head.is_empty() {
        return by_name_only(hint, bytes_len);
    }
    match magic_of(head) {
        Some(Magic::Zip) => {
            let (format, evidence) = zip_format(hint, head);
            let mut notes = name_notes(hint, format);
            notes.extend(evidence);
            (format, notes)
        }
        Some(Magic::Sqlite) => {
            let (format, evidence) = sqlite_format(hint, head);
            let mut notes = name_notes(hint, format);
            notes.extend(evidence);
            (format, notes)
        }
        Some(magic) => {
            let format = magic.format(hint);
            let mut notes = name_notes(hint, format);
            if magic == Magic::Cfb && hint.claim.is_none() {
                notes.push(
                    "an OLE compound file names its own type inside; with no name to go on the kernel reads it as msg"
                        .into(),
                );
            }
            if format == Format::Shp {
                notes.extend(shp_length_note(head, bytes_len));
            }
            (format, notes)
        }
        None => text_or_unknown(hint, head),
    }
}

/// A container listing reads member names, never member bytes (§3.4), so a
/// walk recognises its members with an empty head. The name then decides,
/// and the reason says that no byte was read.
fn by_name_only(hint: &NameHint, bytes_len: u64) -> (Format, Vec<String>) {
    let mut notes = wrapper_note(hint);
    if bytes_len == 0 {
        notes.push("the file is empty; there are no bytes to recognise".into());
        return (Format::Unknown, notes);
    }
    match (hint.claim, hint.suffix.as_deref()) {
        (Some(format), Some(suffix)) => {
            notes.push(format!(
                "no bytes were read; the name's .{suffix} suffix decided (a container listing reads names, not members)"
            ));
            (format, notes)
        }
        _ => {
            notes.push(format!(
                "no bytes were read and the name claims no format; recognition reads the first {}",
                describe_bytes(RECOGNITION_HEAD_BYTES as u64)
            ));
            (Format::Unknown, notes)
        }
    }
}

fn wrapper_note(hint: &NameHint) -> Vec<String> {
    match &hint.wrapper {
        Some(wrapper) => vec![format!(".{wrapper} is not a format")],
        None => Vec::new(),
    }
}

/// What the name claimed, when the bytes said otherwise — and the standing
/// note that a wrapper suffix decided nothing.
fn name_notes(hint: &NameHint, format: Format) -> Vec<String> {
    let mut notes = wrapper_note(hint);
    match (hint.suffix.as_deref(), hint.claim) {
        (Some(suffix), Some(claim)) if claim != format => notes.push(format!(
            "the name says .{suffix} but the bytes are {}; the bytes decide",
            describe_signature(format)
        )),
        _ if !notes.is_empty() => {
            notes.push(format!("the bytes are {}", describe_signature(format)));
        }
        _ => {}
    }
    notes
}

fn unknown_notes(hint: &NameHint) -> Vec<String> {
    let mut notes = wrapper_note(hint);
    match (hint.suffix.as_deref(), hint.claim) {
        (Some(suffix), Some(claim)) => notes.push(format!(
            "the name says .{suffix} but the bytes are not {}",
            expected_signature(claim)
        )),
        _ => notes.push("the bytes match no known format signature".into()),
    }
    notes
}

/// A ZIP archive is only ever `zip` on its own evidence. ODF declares its own
/// type in a stored first member, which is byte evidence and beats the name;
/// otherwise the name picks the subtype, and a name that picks none leaves a
/// plain `zip` for the container walk to classify member by member (§3.4).
fn zip_format(hint: &NameHint, head: &[u8]) -> (Format, Vec<String>) {
    if let Some(media) = odf_mimetype(head) {
        let format = if media.contains("opendocument.text") {
            Format::Odt
        } else if media.contains("opendocument.spreadsheet") {
            Format::Ods
        } else {
            Format::Zip
        };
        return (
            format,
            vec![format!(
                "the archive declares the OpenDocument type {media}"
            )],
        );
    }
    let format = match hint.claim {
        Some(
            claim @ (Format::Docx
            | Format::Xlsx
            | Format::Odt
            | Format::Ods
            | Format::Kmz
            | Format::Dsgrid
            | Format::Zip),
        ) => claim,
        _ => Format::Zip,
    };
    let notes = match (format, hint.suffix.as_deref()) {
        (Format::Zip, _) | (_, None) => Vec::new(),
        (_, Some(suffix)) => vec![format!(
            "the bytes are a ZIP archive; the name's .{suffix} suffix named the {format} subtype"
        )],
    };
    (format, notes)
}

/// An OpenDocument package stores an uncompressed `mimetype` member first.
/// Returns its declared media type, or `None` when the head is not one.
fn odf_mimetype(head: &[u8]) -> Option<String> {
    if !head.starts_with(b"PK\x03\x04") {
        return None;
    }
    let method = u16_le(head, 8)?;
    let compressed = u32_le(head, 18)? as usize;
    let name_len = usize::from(u16_le(head, 26)?);
    let extra_len = usize::from(u16_le(head, 28)?);
    if method != 0 || name_len != "mimetype".len() {
        return None;
    }
    if !head
        .get(30..30 + name_len)?
        .eq_ignore_ascii_case(b"mimetype")
    {
        return None;
    }
    let start = 30 + name_len + extra_len;
    let end = start.saturating_add(compressed.min(256)).min(head.len());
    let media = std::str::from_utf8(head.get(start..end)?).ok()?.trim();
    (!media.is_empty()).then(|| media.to_string())
}

/// A SQLite file is a GeoPackage when its header says so: the application id
/// at offset 68 is `GPKG`. Any other SQLite database is not an asset format.
fn sqlite_format(hint: &NameHint, head: &[u8]) -> (Format, Vec<String>) {
    if head.get(68..72) == Some(&b"GPKG"[..]) {
        return (Format::Gpkg, Vec::new());
    }
    if hint.claim == Some(Format::Gpkg) {
        return (
            Format::Gpkg,
            vec!["the SQLite header does not carry the GPKG application id".into()],
        );
    }
    (
        Format::Unknown,
        vec!["the bytes are a SQLite database without the GPKG application id".into()],
    )
}

/// The shapefile header declares the file length in 16-bit words. When it
/// disagrees with the real length the file is truncated or padded, and the
/// preview should say so rather than decode half a layer.
fn shp_length_note(head: &[u8], bytes_len: u64) -> Option<String> {
    let declared = u64::from(u32_be(head, 24)?) * 2;
    if bytes_len == 0 || declared == bytes_len {
        return None;
    }
    Some(format!(
        "the shapefile header declares {} bytes but the file is {} bytes",
        thousands(declared),
        thousands(bytes_len)
    ))
}

fn u16_le(bytes: &[u8], at: usize) -> Option<u16> {
    bytes
        .get(at..at + 2)
        .and_then(|slice| slice.try_into().ok())
        .map(u16::from_le_bytes)
}

fn u32_le(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_le_bytes)
}

fn u32_be(bytes: &[u8], at: usize) -> Option<u32> {
    bytes
        .get(at..at + 4)
        .and_then(|slice| slice.try_into().ok())
        .map(u32::from_be_bytes)
}

// ── text ────────────────────────────────────────────────────────────────────

/// The head decoded as text, and whether decoding had to be lossy.
struct HeadText {
    text: String,
    lossy: bool,
}

fn text_or_unknown(hint: &NameHint, head: &[u8]) -> (Format, Vec<String>) {
    let Some(head_text) = printable_head(head) else {
        return (Format::Unknown, unknown_notes(hint));
    };
    let (format, evidence) = sniff_text(hint, &head_text.text);
    let mut notes = name_notes(hint, format);
    notes.extend(evidence);
    if head_text.lossy {
        notes.push("the head is not valid UTF-8; recognition read its printable bytes".into());
    }
    (format, notes)
}

/// The leading bytes as text, or `None` when they are not text at all: a NUL
/// byte, or control bytes no text file carries.
fn printable_head(head: &[u8]) -> Option<HeadText> {
    let slice = head.get(..head.len().min(TEXT_SNIFF_BYTES)).unwrap_or(head);
    let slice = slice.strip_prefix(&[0xEFu8, 0xBB, 0xBF]).unwrap_or(slice);
    if slice.is_empty() {
        return None;
    }
    let mut control = 0usize;
    for byte in slice {
        if *byte == 0 {
            return None;
        }
        if *byte < 0x20 && !matches!(*byte, b'\t' | b'\n' | b'\r' | 0x0C) {
            control += 1;
        }
    }
    if control * 100 > slice.len() {
        return None;
    }
    match std::str::from_utf8(slice) {
        Ok(text) => Some(HeadText {
            text: text.to_string(),
            lossy: false,
        }),
        // The head may cut a multi-byte character in half; that is not a
        // reason to call a text file binary.
        Err(error) if error.error_len().is_none() && error.valid_up_to() > 0 => {
            let text = std::str::from_utf8(slice.get(..error.valid_up_to())?).ok()?;
            Some(HeadText {
                text: text.to_string(),
                lossy: false,
            })
        }
        Err(_) => {
            let ascii = slice.iter().filter(|byte| **byte < 0x80).count();
            if ascii * 100 < slice.len() * 60 {
                return None;
            }
            Some(HeadText {
                text: String::from_utf8_lossy(slice).into_owned(),
                lossy: true,
            })
        }
    }
}

fn sniff_text(hint: &NameHint, text: &str) -> (Format, Vec<String>) {
    let trimmed = text.trim_start();
    if is_geojson(trimmed) {
        return (Format::Geojson, Vec::new());
    }
    if trimmed.starts_with('<') {
        if is_html(trimmed) {
            return (Format::Html, Vec::new());
        }
        if is_kml(trimmed) {
            return (Format::Kml, Vec::new());
        }
        return (
            Format::Txt,
            vec!["the head is XML but declares no <kml> document".into()],
        );
    }
    if is_eml(trimmed) {
        return (Format::Eml, Vec::new());
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return (
            Format::Txt,
            vec!["the head is JSON but not a GeoJSON feature collection".into()],
        );
    }
    let delimited = sniff_delimiter(text);
    match (hint.claim, delimited) {
        (Some(claim @ (Format::Csv | Format::Tsv)), Some(found)) if found.format == claim => {
            (claim, Vec::new())
        }
        (Some(Format::Csv | Format::Tsv), Some(found)) => (
            found.format,
            vec![format!("the rows are {}-separated", found.delimiter)],
        ),
        (Some(claim @ (Format::Csv | Format::Tsv)), None) => (
            claim,
            vec!["no delimiter was found in the head; the name decided".into()],
        ),
        (Some(Format::Md), _) => (Format::Md, Vec::new()),
        (Some(Format::Txt), _) => (Format::Txt, Vec::new()),
        (_, Some(found)) => (
            found.format,
            vec![format!("the rows are {}-separated", found.delimiter)],
        ),
        (_, None) => (Format::Txt, Vec::new()),
    }
}

fn is_geojson(text: &str) -> bool {
    if !text.starts_with('{') {
        return false;
    }
    text.contains("FeatureCollection")
        || (text.contains("\"type\"")
            && (text.contains("\"Feature\"")
                || text.contains("\"coordinates\"")
                || text.contains("\"geometry\"")))
}

fn is_html(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.starts_with("<!doctype html") || lower.starts_with("<html")
}

fn is_kml(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("<kml") || lower.contains("opengis.net/kml")
}

/// RFC 5322: a run of `Name: value` header lines, folded continuations
/// allowed, ended by a blank line. At least one header a message must carry.
fn is_eml(text: &str) -> bool {
    let mut headers = 0usize;
    let mut named = false;
    for (index, line) in text.lines().take(SNIFF_LINES * 2).enumerate() {
        if line.trim().is_empty() {
            break;
        }
        if index == 0 && line.starts_with("From ") {
            continue; // an mbox separator, not a header
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            continue; // a folded continuation of the header above
        }
        let Some((name, _)) = line.split_once(':') else {
            return false;
        };
        if name.is_empty()
            || !name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
        {
            return false;
        }
        headers += 1;
        if matches!(
            name.to_ascii_lowercase().as_str(),
            "from" | "received" | "subject" | "to" | "date" | "message-id" | "mime-version"
        ) {
            named = true;
        }
    }
    headers >= 2 && named
}

/// A delimiter the rows agree on. `columns` is how many fields it makes, so
/// the widest consistent delimiter wins.
struct Delimited {
    format: Format,
    delimiter: &'static str,
    columns: usize,
}

fn sniff_delimiter(text: &str) -> Option<Delimited> {
    let mut lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(SNIFF_LINES)
        .collect();
    if !text.ends_with('\n') && lines.len() > 2 {
        lines.pop(); // the head may have cut the last line in half
    }
    if lines.len() < 2 {
        return None;
    }
    let mut best: Option<Delimited> = None;
    for (delimiter, name) in DELIMITERS {
        let counts: Vec<usize> = lines
            .iter()
            .map(|line| count_outside_quotes(line, delimiter))
            .collect();
        let Some(&first) = counts.first() else {
            continue;
        };
        if first == 0 || counts.iter().any(|count| *count != first) {
            continue;
        }
        let candidate = Delimited {
            format: if delimiter == '\t' {
                Format::Tsv
            } else {
                Format::Csv
            },
            delimiter: name,
            columns: first + 1,
        };
        if best
            .as_ref()
            .is_none_or(|best| candidate.columns > best.columns)
        {
            best = Some(candidate);
        }
    }
    best
}

fn count_outside_quotes(line: &str, delimiter: char) -> usize {
    let mut quoted = false;
    let mut count = 0usize;
    for character in line.chars() {
        if character == '"' {
            quoted = !quoted;
        } else if character == delimiter && !quoted {
            count += 1;
        }
    }
    count
}

// ── saying what was found ───────────────────────────────────────────────────

/// What the bytes turned out to be, in a sentence.
fn describe_signature(format: Format) -> &'static str {
    match format {
        Format::Pdf => "a PDF",
        Format::Docx => "an OOXML document",
        Format::Doc | Format::Xls | Format::Msg => "an OLE compound file",
        Format::Odt => "an OpenDocument text document",
        Format::Rtf => "an RTF document",
        Format::Xlsx => "an OOXML workbook",
        Format::Ods => "an OpenDocument spreadsheet",
        Format::Csv => "delimited text",
        Format::Tsv => "tab-separated text",
        Format::Eml => "an RFC 5322 message",
        Format::Md => "markdown text",
        Format::Html => "HTML document",
        Format::Txt => "plain text",
        Format::Shp => "a shapefile header",
        Format::Kmz | Format::Zip | Format::Dsgrid => "a ZIP archive",
        Format::Kml => "KML text",
        Format::Gpkg => "a SQLite database",
        Format::Geojson => "GeoJSON text",
        Format::Jpg => "a JPEG image",
        Format::Png => "a PNG image",
        Format::Webp => "a WEBP image",
        Format::Tif => "a TIFF image",
        Format::SevenZ => "a 7z archive",
        Format::TarGz => "a gzip stream",
        Format::Unknown => "not a recognised format",
    }
}

/// What the bytes would have had to be for the name to be right.
fn expected_signature(format: Format) -> &'static str {
    match format {
        Format::Docx
        | Format::Xlsx
        | Format::Odt
        | Format::Ods
        | Format::Kmz
        | Format::Dsgrid
        | Format::Zip => "a ZIP archive",
        Format::Csv
        | Format::Tsv
        | Format::Md
        | Format::Html
        | Format::Txt
        | Format::Kml
        | Format::Geojson
        | Format::Eml => "text",
        other => describe_signature(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    use zip::write::{SimpleFileOptions, ZipWriter};

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (path, body) in entries {
            writer
                .start_file(
                    *path,
                    SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// An OpenDocument package: `mimetype` first and stored, as ODF requires.
    fn odf_of(media: &str) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                "mimetype",
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(media.as_bytes()).unwrap();
        writer
            .start_file(
                "content.xml",
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(b"<office:document-content/>").unwrap();
        writer.finish().unwrap().into_inner()
    }

    fn sqlite_header(application_id: &[u8; 4]) -> Vec<u8> {
        let mut header = vec![0u8; 100];
        header[..16].copy_from_slice(b"SQLite format 3\0");
        header[68..72].copy_from_slice(application_id);
        header
    }

    fn shapefile_header(bytes_len: u64) -> Vec<u8> {
        let mut header = vec![0u8; 100];
        header[..4].copy_from_slice(&[0x00, 0x00, 0x27, 0x0A]);
        let words = (bytes_len / 2) as u32;
        header[24..28].copy_from_slice(&words.to_be_bytes());
        header
    }

    fn seen(name: Option<&str>, head: &[u8]) -> Recognition {
        recognise(name, head, head.len() as u64)
    }

    fn reason(recognition: &Recognition) -> String {
        recognition.reason.clone().unwrap_or_default()
    }

    #[test]
    fn html_needs_html_document_bytes_and_is_not_a_kernel_preview() {
        let html = seen(
            Some("report.html"),
            b"<!doctype html><html><head></head><body>Report</body></html>",
        );
        assert_eq!(html.format, Format::Html);
        assert_eq!(html.kind, Kind::Note);
        assert!(!html.previewable);
        assert_eq!(
            seen(Some("report.html"), b"<root>XML</root>").format,
            Format::Txt
        );
        assert_eq!(
            seen(Some("report.txt"), b"<html><body>Report</body></html>").format,
            Format::Html
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_format_recognition() {
        // Magic beats the extension, and the reason names the conflict.
        let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\n";
        let found = seen(Some("payroll-2026.xlsx"), pdf);
        assert_eq!(found.format, Format::Pdf);
        assert_eq!(found.kind, Kind::Doc);
        assert!(found.previewable);
        let why = reason(&found);
        assert!(
            why.contains("the name says .xlsx but the bytes are a PDF; the bytes decide"),
            "{why}"
        );
        assert!(why.contains("pdf.js"), "{why}");

        // ".bak is not a format": the bytes decide, and the wrapper is named.
        let found = seen(Some("site.bak"), pdf);
        assert_eq!(found.format, Format::Pdf);
        assert!(
            reason(&found).contains(".bak is not a format; the bytes are a PDF"),
            "{}",
            reason(&found)
        );
        let found = seen(Some("EPC-Lot3-signed.pdf.bak"), pdf);
        assert_eq!(found.format, Format::Pdf);
        assert!(reason(&found).contains(".bak is not a format"));

        // A bare zip is a zip; the name only names the subtype, and a .shp
        // member does not make the archive a shapefile — the walk does that.
        let archive = zip_of(&[
            ("Lot3/drawings/site.pdf", b"%PDF-1.7"),
            ("Lot3/gis/poles.shp", &[0x00, 0x00, 0x27, 0x0A]),
        ]);
        assert_eq!(seen(Some("deliverable.zip"), &archive).format, Format::Zip);
        assert_eq!(seen(None, &archive).format, Format::Zip);
        assert_eq!(seen(Some("deliverable.bak"), &archive).format, Format::Zip);
        assert_eq!(seen(Some("Lot3.shp"), &archive).format, Format::Zip);
        assert_eq!(seen(Some("report.docx"), &archive).format, Format::Docx);
        assert_eq!(seen(Some("boq.xlsx"), &archive).format, Format::Xlsx);
        assert_eq!(seen(Some("site.kmz"), &archive).format, Format::Kmz);
        assert_eq!(
            seen(Some("mv-line.dsgrid"), &archive).format,
            Format::Dsgrid
        );
        let docx = seen(Some("report.docx"), &archive);
        assert_eq!(docx.kind, Kind::Doc);
        assert!(reason(&docx).contains("the name's .docx suffix named the docx subtype"));
        let pack = seen(Some("deliverable.zip"), &archive);
        assert_eq!(pack.kind, Kind::Pack);
        assert!(!pack.previewable);
        assert!(reason(&pack).contains("container_walk"));

        // An OpenDocument package declares its own type, and that is bytes.
        let sheet = seen(
            Some("letter.odt"),
            &odf_of("application/vnd.oasis.opendocument.spreadsheet"),
        );
        assert_eq!(sheet.format, Format::Ods);
        assert!(
            reason(&sheet).contains("opendocument.spreadsheet"),
            "{}",
            reason(&sheet)
        );
        // Recognised, and honestly not previewable in slice 1 (OOXML only).
        assert!(!sheet.previewable);
        assert!(
            reason(&sheet).contains("convert it to xlsx"),
            "{}",
            reason(&sheet)
        );
        assert!(!previewable(Format::Odt));
        assert_eq!(
            seen(
                Some("book.xlsx"),
                &odf_of("application/vnd.oasis.opendocument.text")
            )
            .format,
            Format::Odt
        );

        // A .xlsx whose bytes are not a ZIP archive is unknown, not a guess.
        let found = seen(Some("boq.xlsx"), &[0x89, 0x00, 0x1B, 0x00, 0xFE, 0x00]);
        assert_eq!(found.format, Format::Unknown);
        assert_eq!(found.kind, Kind::Other);
        assert!(!found.previewable);
        assert!(
            reason(&found).contains("the name says .xlsx but the bytes are not a ZIP archive"),
            "{}",
            reason(&found)
        );
        // Legible bytes are named for what they are, not for what they claim.
        let found = seen(Some("boq.xlsx"), b"this workbook is a note now\n");
        assert_eq!(found.format, Format::Txt);
        assert!(reason(&found).contains("the bytes are plain text; the bytes decide"));

        // One OLE container, three formats: only the name separates them.
        let cfb = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0x00, 0x00];
        assert_eq!(seen(Some("letter.doc"), &cfb).format, Format::Doc);
        assert_eq!(seen(Some("boq.xls"), &cfb).format, Format::Xls);
        assert_eq!(seen(Some("re Lot 3.msg"), &cfb).format, Format::Msg);
        let bare = seen(None, &cfb);
        assert_eq!(bare.format, Format::Msg);
        assert_eq!(bare.kind, Kind::Mail);
        assert!(!bare.previewable);
        assert!(reason(&bare).contains("slice 2"), "{}", reason(&bare));
        assert!(reason(&seen(Some("letter.doc"), &cfb)).contains("no kernel decoder"));

        // The remaining binary signatures.
        assert_eq!(
            seen(Some("a.bak"), b"7z\xBC\xAF\x27\x1C\x00").format,
            Format::SevenZ
        );
        assert_eq!(
            seen(Some("a.bak"), &[0x1F, 0x8B, 0x08, 0x00]).format,
            Format::TarGz
        );
        assert_eq!(
            seen(Some("photo.jpg"), &[0xFF, 0xD8, 0xFF, 0xE0]).format,
            Format::Jpg
        );
        assert_eq!(
            seen(None, &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]).format,
            Format::Png
        );
        assert_eq!(
            seen(None, b"RIFF\x00\x00\x00\x00WEBPVP8 ").format,
            Format::Webp
        );
        assert_eq!(seen(None, b"II*\0\x08\x00\x00\x00").format, Format::Tif);
        assert_eq!(seen(None, b"{\\rtf1\\ansi").format, Format::Rtf);
        assert_eq!(seen(None, b"{\\rtf1\\ansi").kind, Kind::Doc);
        assert!(!seen(None, b"{\\rtf1\\ansi").previewable);

        // A GeoPackage says GPKG in its SQLite header; other databases do not.
        let gpkg = seen(Some("poles.gpkg"), &sqlite_header(b"GPKG"));
        assert_eq!(gpkg.format, Format::Gpkg);
        assert_eq!(gpkg.kind, Kind::Geo);
        assert!(!gpkg.previewable);
        assert!(reason(&gpkg).contains("SQLite reader"), "{}", reason(&gpkg));
        let other = seen(Some("cache.db"), &sqlite_header(b"\0\0\0\0"));
        assert_eq!(other.format, Format::Unknown);
        assert!(reason(&other).contains("without the GPKG application id"));

        // A shapefile header, and its declared length cross-checked.
        let header = shapefile_header(100);
        let shp = recognise(Some("poles.shp"), &header, 100);
        assert_eq!(shp.format, Format::Shp);
        assert_eq!(shp.kind, Kind::Geo);
        assert!(shp.reason.is_none(), "{:?}", shp.reason);
        let truncated = recognise(Some("poles.shp"), &header, 44_112);
        assert!(
            reason(&truncated).contains("declares 100 bytes but the file is 44,112 bytes"),
            "{}",
            reason(&truncated)
        );

        // Text formats are sniffed from the head, and structure beats the name.
        let geojson = seen(
            Some("notes.txt"),
            br#"{"type":"FeatureCollection","features":[]}"#,
        );
        assert_eq!(geojson.format, Format::Geojson);
        assert_eq!(geojson.kind, Kind::Geo);
        assert!(geojson.previewable);
        let kml = seen(
            Some("site.bak"),
            br#"<?xml version="1.0"?><kml xmlns="http://www.opengis.net/kml/2.2"><Document/></kml>"#,
        );
        assert_eq!(kml.format, Format::Kml);
        let xml = seen(None, b"<?xml version=\"1.0\"?><gml:FeatureCollection/>");
        assert_eq!(xml.format, Format::Txt);
        assert!(reason(&xml).contains("no <kml> document"));
        let mail = seen(
            None,
            b"From: lead@epc.example\r\nTo: eng@ds.example\r\nSubject: Lot 3\r\nDate: Sat, 14 Mar 2026 09:00:00 +0200\r\n\r\nThe signed contract is attached.\r\n",
        );
        assert_eq!(mail.format, Format::Eml);
        assert_eq!(mail.kind, Kind::Mail);
        assert!(mail.previewable);
        let csv = seen(None, b"pole,lat,lon\nP1,-1.94,30.06\nP2,-1.95,30.07\n");
        assert_eq!(csv.format, Format::Csv);
        assert_eq!(csv.kind, Kind::Sheet);
        let tsv = seen(None, b"pole\tlat\tlon\nP1\t-1.94\t30.06\n");
        assert_eq!(tsv.format, Format::Tsv);
        let mislabelled = seen(Some("poles.tsv"), b"pole,lat\nP1,-1.94\nP2,-1.95\n");
        assert_eq!(mislabelled.format, Format::Csv);
        assert!(reason(&mislabelled).contains("the rows are comma-separated"));
        let quoted = seen(
            None,
            b"name,note\n\"Lot 3, phase 1\",ok\n\"Lot 4, phase 2\",ok\n",
        );
        assert_eq!(quoted.format, Format::Csv);
        let single_column = seen(Some("names.csv"), b"pole\nP1\nP2\n");
        assert_eq!(single_column.format, Format::Csv);
        assert!(reason(&single_column).contains("no delimiter was found"));
        assert_eq!(
            seen(Some("notes.md"), b"# Lot 3\n\nprose\n").format,
            Format::Md
        );
        assert_eq!(
            seen(Some("notes.md"), b"# Lot 3\n\nprose\n").kind,
            Kind::Note
        );
        assert_eq!(seen(None, b"just a line of prose\n").format, Format::Txt);
        assert_eq!(
            seen(None, "une reconnaissance élargie, en prose\n".as_bytes()).format,
            Format::Txt
        );

        // A container listing reads names, not members (§3.4).
        let member = recognise(Some("Lot3/gis/poles.shp"), &[], 44_112);
        assert_eq!(member.format, Format::Shp);
        assert!(
            reason(&member).contains("no bytes were read; the name's .shp suffix decided"),
            "{}",
            reason(&member)
        );

        // Every format keeps its table row.
        for format in Format::ALL {
            assert!(format_cap_bytes(*format) <= MAX_PREVIEW_INPUT_BYTES);
            assert_eq!(
                previewable(*format),
                preview_note(*format).is_none() || *format == Format::Pdf
            );
        }
        assert_eq!(kind_of(Format::Unknown), Kind::Other);
        assert_eq!(format_cap_bytes(Format::Pdf), MAX_PDF_BYTES);
        assert_eq!(format_cap_bytes(Format::Txt), MAX_TEXT_BYTES);
        assert_eq!(format_cap_bytes(Format::Geojson), MAX_GEO_BYTES);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_format_recognition_is_total_on_malformed_input() {
        // An empty file, an empty head, and a head with no name to help it.
        let empty = recognise(Some("contract.pdf"), &[], 0);
        assert_eq!(empty.format, Format::Unknown);
        assert!(
            reason(&empty).contains("the file is empty"),
            "{}",
            reason(&empty)
        );
        let headless = recognise(None, &[], 2_847_113);
        assert_eq!(headless.format, Format::Unknown);
        assert!(
            reason(&headless).contains("no bytes were read and the name claims no format"),
            "{}",
            reason(&headless)
        );
        assert!(reason(&headless).contains("64 KiB"));
        assert_eq!(recognise(Some("a.bak"), &[], 12).format, Format::Unknown);

        // Truncated and lying ZIP local headers.
        assert_eq!(seen(None, b"PK\x03\x04").format, Format::Zip);
        assert_eq!(seen(Some("a.docx"), b"PK\x03\x04").format, Format::Docx);
        let mut lying = b"PK\x03\x04".to_vec();
        lying.extend_from_slice(&[0u8; 4]);
        lying.extend_from_slice(&0u16.to_le_bytes()); // stored
        lying.extend_from_slice(&[0u8; 8]);
        lying.extend_from_slice(&u32::MAX.to_le_bytes()); // compressed size
        lying.extend_from_slice(&[0u8; 4]);
        lying.extend_from_slice(&8u16.to_le_bytes()); // name length
        lying.extend_from_slice(&u16::MAX.to_le_bytes()); // extra length
        lying.extend_from_slice(b"mimetype");
        assert_eq!(seen(Some("a.ods"), &lying).format, Format::Ods);
        assert_eq!(seen(None, b"PK\x05\x06").format, Format::Zip);

        // Truncated signatures of every other family.
        assert_eq!(seen(None, b"%PDF").format, Format::Txt);
        assert_eq!(seen(None, b"SQLite format 3\0").format, Format::Unknown);
        assert_eq!(seen(None, &[0x00, 0x00, 0x27, 0x0A]).format, Format::Shp);
        assert_eq!(seen(None, b"RIFF\x00\x00").format, Format::Unknown);
        assert_eq!(
            seen(None, &[0xD0, 0xCF, 0x11, 0xE0]).format,
            Format::Unknown
        );

        // Bytes that are not text and not a signature.
        assert_eq!(seen(None, &[0u8; 64]).format, Format::Unknown);
        assert_eq!(
            seen(None, &[0x01, 0x02, 0x03, 0x04, 0x05]).format,
            Format::Unknown
        );
        assert_eq!(seen(Some("x.csv"), &[0u8, 1, 2, 3]).format, Format::Unknown);
        assert!(reason(&seen(Some("x.csv"), &[0u8, 1, 2, 3])).contains("are not text"));
        // Invalid UTF-8 that is still legible bytes.
        let latin1 = seen(Some("poles.csv"), b"nom,val\nRubavu\xE9,1\nGisenyi\xE9,2\n");
        assert_eq!(latin1.format, Format::Csv);
        assert!(reason(&latin1).contains("not valid UTF-8"));
        // A head cut mid-character is still text.
        let cut = "prose élargie é".as_bytes();
        assert_eq!(
            seen(None, cut.get(..cut.len() - 1).unwrap_or(cut)).format,
            Format::Txt
        );

        // Names that are not names.
        for name in [
            "", ".", "...", "..bak", "a.", "/", "C:\\x\\", "  .PDF  ", ".7z",
        ] {
            let found = recognise(Some(name), b"%PDF-1.4", 8);
            assert_eq!(found.format, Format::Pdf, "{name}");
        }
        assert_eq!(
            seen(Some("ARCHIVE.TAR.GZ"), &[0x1F, 0x8B]).format,
            Format::TarGz
        );
        assert_eq!(seen(Some("x.tgz"), &[0x1F, 0x8B]).format, Format::TarGz);
        // A head longer than the file it claims to come from is still total.
        assert_eq!(recognise(Some("a.txt"), b"hello", 2).format, Format::Txt);
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_recognition_survives_every_truncation_of_a_zip_local_header() {
        // An OpenDocument head: local header, `mimetype` name, media type.
        let media = b"application/vnd.oasis.opendocument.text";
        let mut head =
            b"PK\x03\x04\x14\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        head.extend_from_slice(&(media.len() as u32).to_le_bytes());
        head.extend_from_slice(&(media.len() as u32).to_le_bytes());
        head.extend_from_slice(&8u16.to_le_bytes());
        head.extend_from_slice(&0u16.to_le_bytes());
        head.extend_from_slice(b"mimetype");
        head.extend_from_slice(media);
        assert_eq!(recognise(Some("x.odt"), &head, 4096).format, Format::Odt);
        assert_eq!(
            recognise(Some("x.zip"), &head, 4096).format,
            Format::Odt,
            "the bytes decide"
        );
        for length in 0..head.len() {
            let cut = &head[..length];
            for name in [Some("x.odt"), Some("x.docx"), Some("x.zip"), None] {
                let recognised = recognise(name, cut, 4096);
                assert!(
                    recognised.format != Format::Ods,
                    "{name:?}@{length}: a cut head never invents a subtype the bytes do not carry: {recognised:?}"
                );
            }
        }
        // A `compressed` field claiming 4 GiB with a 40-byte head.
        let mut lying = head.clone();
        lying[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(recognise(Some("x.odt"), &lying, 4096).format, Format::Odt);
        let short = &lying[..40];
        assert!(matches!(
            recognise(Some("x.odt"), short, 4096).format,
            Format::Odt | Format::Zip
        ));
    }
}
