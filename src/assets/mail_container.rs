//! Mail containers: an RFC 5322 message walked as a container, its MIME
//! parts the members (ds-brain `docs/contracts/correspondence.md` §Asset).
//!
//! ds-brain lists a `.eml`'s parts on the catalogue row at finalize
//! (`internal/services/assets_mail.go`) and the kernel extracts one part from
//! the downloaded bytes by the same `path`, so the two sides have to name
//! every part identically. **The member-name rule**, reproduced here verbatim
//! from that file and pinned by the shared fixtures under
//! `tests/fixtures/mail/`:
//!
//! * the `Content-Disposition` `filename`, else the `Content-Type` `name`,
//!   RFC 2047-decoded (and RFC 2231 `name*` forms decoded), reduced to its
//!   base name with backslashes read as separators, control characters
//!   dropped, trimmed, never empty (`part`), bounded at 200 bytes keeping an
//!   extension of at most 32;
//! * an unnamed part is `part-<section with dots as dashes><extension by
//!   media type>` — `part-1-1.txt`, `part-2.bin` — where the root of a
//!   single-part message is section `1`;
//! * a repeated name within one message becomes `name (2).ext`, `name (3).ext`;
//! * up to [`MAX_MAIL_PARTS`] parts are listed and every leaf is counted; the
//!   MIME tree is walked [`MAX_MAIL_DEPTH`] levels deep, a multipart below
//!   that is one leaf; a multipart with no boundary is one leaf; a broken
//!   boundary ends that level with what was listed.
//!
//! Sizes are DECODED sizes: a base64 or quoted-printable body is decoded to
//! count it, exactly as the row reports it. Nothing here fetches, resolves or
//! emits a remote reference; the bytes are the message the host handed over.

use std::collections::BTreeSet;

use super::preview::eml::{
    decode_encoded_words, decode_quoted_printable, media_type, parse_headers, split_head_body,
    value,
};
use super::{
    check_bytes, decode_base64, describe_bytes, format, thousands, Container, ContainerMember,
    Format, Kind,
};

/// Parts listed per message; the rest are counted in `member_count_total`.
pub const MAX_MAIL_PARTS: usize = 200;
/// Nested multipart levels walked.
pub const MAX_MAIL_DEPTH: usize = 8;
/// Bytes of one member name.
pub const MAX_MAIL_PATH_BYTES: usize = 200;

/// A part as the walk found it: its member name and how to decode its body.
struct Part<'a> {
    path: String,
    media_type: String,
    encoding: String,
    body: &'a str,
    decoded_bytes: u64,
}

struct Walker<'a> {
    parts: Vec<Part<'a>>,
    total: u64,
    truncated: bool,
    used: BTreeSet<String>,
}

/// Whether `bytes` are a message this module can walk: a header block with
/// at least one of `From`, `To`, `Subject` or `Message-ID`, as ds-brain
/// requires before it calls bytes a mail.
pub fn is_message(bytes: &[u8]) -> bool {
    parse_message(bytes).is_ok()
}

fn parse_message(bytes: &[u8]) -> Result<(String, Vec<(String, String)>), String> {
    let unreadable =
        |detail: String| format!("the bytes are not a readable RFC 5322 message: {detail}");
    if bytes.is_empty() {
        return Err(unreadable("the message is empty".into()));
    }
    let message = String::from_utf8_lossy(bytes).into_owned();
    let (head, _) = split_head_body(&message);
    let headers = parse_headers(head).map_err(unreadable)?;
    let named = ["From", "To", "Subject", "Message-ID"]
        .iter()
        .any(|name| value(&headers, name).is_some_and(|raw| !raw.trim().is_empty()));
    if !named {
        return Err(unreadable(
            "no From, To, Subject or Message-ID header".into(),
        ));
    }
    Ok((message, headers))
}

/// Lists the MIME parts of `bytes` as container members (§Asset). Every leaf
/// is counted; the listing stops at [`MAX_MAIL_PARTS`] and says so.
pub fn walk(bytes: &[u8]) -> Result<Container, String> {
    let (message, headers) = parse_message(bytes)?;
    let walker = walk_message(&message, &headers);
    let mut container = Container::new(Format::Eml);
    container.members = walker.parts.iter().map(member_row).collect();
    container.member_count_total = walker.total;
    container.truncated = walker.truncated;
    Ok(container)
}

/// The decoded bytes of the part named `path`, refused above `max_bytes` by
/// name. `path` is the member name the walk lists, never a section number.
pub fn extract_part(bytes: &[u8], path: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    let (message, headers) = parse_message(bytes)?;
    let walker = walk_message(&message, &headers);
    let part = walker
        .parts
        .iter()
        .find(|part| part.path == path)
        .ok_or_else(|| format!("the mail has no part named {path}"))?;
    check_bytes(
        &format!("mail part {path}"),
        part.decoded_bytes,
        "member",
        max_bytes,
    )?;
    let decoded = decode_body(part.body, &part.encoding);
    if decoded.len() as u64 > max_bytes {
        return Err(format!(
            "mail part {path} decodes past {} bytes; the member bound is {}",
            thousands(max_bytes),
            describe_bytes(max_bytes)
        ));
    }
    Ok(decoded)
}

fn walk_message<'a>(message: &'a str, headers: &[(String, String)]) -> Walker<'a> {
    let (_, body) = split_head_body(message);
    let mut walker = Walker {
        parts: Vec::new(),
        total: 0,
        truncated: false,
        used: BTreeSet::new(),
    };
    walker.node(body, headers, "", 0);
    walker
}

impl<'a> Walker<'a> {
    /// Descends one MIME node. `section` is its index path: `""` for the
    /// root, then `1`, `1.2`, … for the members of each multipart.
    fn node(&mut self, body: &'a str, headers: &[(String, String)], section: &str, depth: usize) {
        let content_type = value(headers, "content-type").unwrap_or("");
        let (mut media, type_params) = parse_media_type(content_type);
        if media.is_empty() {
            media = "text/plain".to_string();
        }
        if media.starts_with("multipart/") && depth < MAX_MAIL_DEPTH {
            let boundary = type_params
                .iter()
                .find(|(key, _)| key == "boundary")
                .map(|(_, boundary)| boundary.clone())
                .unwrap_or_default();
            if !boundary.is_empty() {
                for (index, part) in sections(body, &boundary).into_iter().enumerate() {
                    let child = if section.is_empty() {
                        (index + 1).to_string()
                    } else {
                        format!("{section}.{}", index + 1)
                    };
                    let (head, part_body) = split_head_body(part);
                    let part_headers = parse_headers(head).unwrap_or_default();
                    self.node(part_body, &part_headers, &child, depth + 1);
                }
                return;
            }
        }
        self.leaf(body, headers, &media, &type_params, section);
    }

    fn leaf(
        &mut self,
        body: &'a str,
        headers: &[(String, String)],
        media: &str,
        type_params: &[(String, String)],
        section: &str,
    ) {
        self.total = self.total.saturating_add(1);
        let encoding = value(headers, "content-transfer-encoding")
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let decoded_bytes = decoded_size(body, &encoding);
        if self.parts.len() >= MAX_MAIL_PARTS {
            self.truncated = true;
            return;
        }
        let disposition_header = value(headers, "content-disposition").unwrap_or("");
        let (disposition_kind, disposition_params) = if disposition_header.trim().is_empty() {
            (String::new(), Vec::new())
        } else {
            parse_media_type(disposition_header)
        };
        let mut attachment = matches!(disposition_kind.as_str(), "attachment" | "inline");
        let raw_name = param(&disposition_params, "filename")
            .or_else(|| param(type_params, "name"))
            .unwrap_or_default();
        let mut notes = Vec::new();
        let name = decode_encoded_words(&raw_name, &mut notes)
            .trim()
            .to_string();
        let named = !name.is_empty();
        let section = if section.is_empty() { "1" } else { section };
        let name = if named {
            attachment = true;
            name
        } else {
            format!("part-{}{}", section.replace('.', "-"), extension_for(media))
        };
        let _ = attachment;
        let path = self.unique(sanitize_name(&name));
        self.parts.push(Part {
            path,
            media_type: media.to_string(),
            encoding,
            body,
            decoded_bytes,
        });
    }

    /// `name`, then `name (2).ext`, `name (3).ext` — distinct within one
    /// message, so `--member` names exactly one part.
    fn unique(&mut self, name: String) -> String {
        if self.used.insert(name.clone()) {
            return name;
        }
        let ext = extension_of(&name);
        let stem = &name[..name.len() - ext.len()];
        let mut n = 2u64;
        loop {
            let candidate = format!("{stem} ({n}){ext}");
            if self.used.insert(candidate.clone()) {
                return candidate;
            }
            n += 1;
        }
    }
}

fn member_row(part: &Part<'_>) -> ContainerMember {
    let recognised = format::recognise(Some(&part.path), &[], part.decoded_bytes).format;
    let format = if recognised == Format::Unknown {
        format_by_suffix(&part.path).unwrap_or(match part.media_type.as_str() {
            "message/rfc822" => Format::Eml,
            "application/pdf" => Format::Pdf,
            "image/png" => Format::Png,
            "image/jpeg" => Format::Jpg,
            "image/webp" => Format::Webp,
            "text/plain" => Format::Txt,
            _ => Format::Unknown,
        })
    } else {
        recognised
    };
    let kind = format::kind_of(format);
    ContainerMember {
        path: part.path.clone(),
        bytes: part.decoded_bytes,
        kind,
        format,
        companions: Vec::new(),
        // A pack or a forwarded message inside the mail is a container of
        // its own: extract it, then walk the bytes that come back.
        nested: kind == Kind::Pack || format == Format::Eml,
    }
}

/// The closed `Format` a member name maps to when the name alone did not
/// decide, so `report.pdf` lists as `pdf` rather than `unknown`.
fn format_by_suffix(path: &str) -> Option<Format> {
    let suffix = path.rsplit('.').next()?.to_ascii_lowercase();
    if suffix.len() == path.len() {
        return None;
    }
    Format::ALL
        .iter()
        .copied()
        .find(|format| *format != Format::Unknown && format.as_str() == suffix)
        .or(match suffix.as_str() {
            "jpeg" => Some(Format::Jpg),
            "tiff" => Some(Format::Tif),
            "text" => Some(Format::Txt),
            "markdown" => Some(Format::Md),
            "json" => None,
            _ => None,
        })
}

/// The decoded bytes of one body under its transfer encoding. An undecodable
/// base64 body counts and extracts as its raw text, as ds-brain counts it.
fn decode_body(body: &str, encoding: &str) -> Vec<u8> {
    match encoding {
        "base64" => {
            let packed: String = body
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            decode_base64("mail part", &packed).unwrap_or_else(|_| body.as_bytes().to_vec())
        }
        "quoted-printable" => decode_quoted_printable(body),
        _ => body.as_bytes().to_vec(),
    }
}

fn decoded_size(body: &str, encoding: &str) -> u64 {
    decode_body(body, encoding).len() as u64
}

/// The sections between `--boundary` delimiter lines, up to the closing
/// `--boundary--`. A delimiter is one whole line; the line break before it
/// belongs to the delimiter, not to the part (RFC 2046 §5.1.1), which is why
/// a quoted-printable body before a boundary has no trailing CRLF.
fn sections<'a>(body: &'a str, boundary: &str) -> Vec<&'a str> {
    let delimiter = format!("--{boundary}");
    let closing = format!("--{boundary}--");
    let mut out = Vec::new();
    let mut current: Option<usize> = None;
    let mut at = 0usize;
    while at <= body.len() {
        let line_end = body[at..]
            .find('\n')
            .map_or(body.len(), |offset| at + offset);
        let line = body[at..line_end].trim_end_matches('\r');
        let trimmed = line.trim_end();
        let is_closing = trimmed == closing;
        let is_delimiter = trimmed == delimiter;
        if is_delimiter || is_closing {
            if let Some(start) = current.take() {
                // The CRLF (or LF) that ends the previous line is the
                // delimiter's, not the part's.
                let end = body[..at]
                    .strip_suffix("\r\n")
                    .or_else(|| body[..at].strip_suffix('\n'))
                    .map_or(at, str::len);
                out.push(&body[start.min(end)..end]);
            }
            if is_closing {
                return out;
            }
            current = Some((line_end + 1).min(body.len()));
        }
        if line_end >= body.len() {
            break;
        }
        at = line_end + 1;
    }
    // No closing delimiter: what was opened stays listed with what it holds.
    if let Some(start) = current {
        out.push(&body[start.min(body.len())..]);
    }
    out
}

/// `type/subtype; key=value; key*=charset''pct-encoded` → the lowercase media
/// type and its parameters, RFC 2231 forms decoded: `key*` (single, decoded)
/// wins over `key*0`/`key*1*` continuations, which win over a plain `key`,
/// which is the precedence Go's `mime.ParseMediaType` — the row's parser —
/// resolves them with.
fn parse_media_type(header: &str) -> (String, Vec<(String, String)>) {
    let media = media_type(header);
    let mut plain: Vec<(String, String)> = Vec::new();
    let mut single: Vec<(String, String)> = Vec::new();
    let mut pieces: Vec<(String, u32, bool, String)> = Vec::new();
    for piece in split_parameters(header) {
        let Some((key, raw)) = piece.split_once('=') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        let raw = unquote(raw.trim());
        if let Some(base) = key.strip_suffix('*') {
            match base.rsplit_once('*') {
                Some((name, index)) if index.chars().all(|c| c.is_ascii_digit()) => {
                    if let Ok(index) = index.parse::<u32>() {
                        pieces.push((name.to_string(), index, true, raw));
                    }
                }
                _ => {
                    if let Some(decoded) = decode_2231(&raw) {
                        single.push((base.to_string(), decoded));
                    }
                }
            }
        } else if let Some((name, index)) = key.rsplit_once('*') {
            if index.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(index) = index.parse::<u32>() {
                    pieces.push((name.to_string(), index, false, raw));
                }
                continue;
            }
            plain.push((key, raw));
        } else {
            plain.push((key, raw));
        }
    }
    let mut params = plain;
    let mut names: Vec<String> = pieces.iter().map(|(name, ..)| name.clone()).collect();
    names.sort();
    names.dedup();
    for name in names {
        let mut joined = String::new();
        let mut next = 0u32;
        while let Some((_, _, encoded, raw)) = pieces
            .iter()
            .find(|(candidate, index, ..)| *candidate == name && *index == next)
        {
            if *encoded {
                let Some(decoded) = decode_2231_value(raw, next == 0) else {
                    break;
                };
                joined.push_str(&decoded);
            } else {
                joined.push_str(raw);
            }
            next += 1;
        }
        if next > 0 {
            params.retain(|(key, _)| *key != name);
            params.push((name, joined));
        }
    }
    for (name, decoded) in single {
        params.retain(|(key, _)| *key != name);
        params.push((name, decoded));
    }
    (media, params)
}

/// Splits a header's parameters on `;` outside quotes.
fn split_parameters(header: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut first = true;
    for character in header.chars() {
        match character {
            '\\' if quoted && !escaped => {
                escaped = true;
                current.push(character);
            }
            '"' if !escaped => {
                quoted = !quoted;
                current.push(character);
            }
            ';' if !quoted => {
                if first {
                    first = false;
                } else {
                    out.push(current.clone());
                }
                current.clear();
            }
            _ => {
                escaped = false;
                current.push(character);
            }
        }
    }
    if !first {
        out.push(current);
    }
    out
}

fn unquote(value: &str) -> String {
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return value.to_string();
    };
    let mut out = String::with_capacity(inner.len());
    let mut escaped = false;
    for character in inner.chars() {
        if escaped {
            out.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            out.push(character);
        }
    }
    out
}

/// `charset'language'percent-encoded` → the text, for utf-8 and latin-1.
fn decode_2231(value: &str) -> Option<String> {
    decode_2231_value(value, true)
}

fn decode_2231_value(value: &str, leading_charset: bool) -> Option<String> {
    let (charset, encoded) = if leading_charset {
        let mut parts = value.splitn(3, '\'');
        let charset = parts.next()?.to_ascii_lowercase();
        let _language = parts.next()?;
        (charset, parts.next()?)
    } else {
        ("utf-8".to_string(), value)
    };
    let mut bytes = Vec::with_capacity(encoded.len());
    let raw = encoded.as_bytes();
    let mut at = 0;
    while at < raw.len() {
        if raw[at] == b'%' && at + 2 < raw.len() {
            let hex = std::str::from_utf8(&raw[at + 1..at + 3]).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            at += 3;
        } else {
            bytes.push(raw[at]);
            at += 1;
        }
    }
    match charset.as_str() {
        "utf-8" | "utf8" | "us-ascii" | "ascii" | "" => {
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        "iso-8859-1" | "latin-1" | "latin1" => Some(bytes.iter().map(|b| *b as char).collect()),
        _ => None,
    }
}

fn param(params: &[(String, String)], name: &str) -> Option<String> {
    params
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty())
}

/// The extension an unnamed part takes from its media type: the explicit
/// six ds-brain names first, then Go's built-in `mime` table (sorted, first
/// wins), else `.bin`.
pub fn extension_for(media_type: &str) -> &'static str {
    match media_type {
        "text/plain" => ".txt",
        "text/html" => ".html",
        "text/calendar" => ".ics",
        "message/rfc822" => ".eml",
        "image/jpeg" => ".jpg",
        "application/pdf" => ".pdf",
        "image/avif" => ".avif",
        "text/css" => ".css",
        "image/gif" => ".gif",
        "text/javascript" => ".js",
        "application/json" => ".json",
        "image/png" => ".png",
        "image/svg+xml" => ".svg",
        "application/wasm" => ".wasm",
        "image/webp" => ".webp",
        "text/xml" => ".xml",
        _ => ".bin",
    }
}

/// One member name: the base name only, no control characters, bounded,
/// never empty.
pub fn sanitize_name(name: &str) -> String {
    let replaced = name.replace('\\', "/");
    let base = path_base(replaced.trim());
    let cleaned: String = base.chars().filter(|c| !c.is_control()).collect();
    let mut cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." || cleaned == "/" {
        cleaned = "part".to_string();
    }
    if cleaned.len() > MAX_MAIL_PATH_BYTES {
        let mut ext = extension_of(&cleaned).to_string();
        if ext.len() > 32 {
            ext.clear();
        }
        let mut cut = MAX_MAIL_PATH_BYTES - ext.len();
        while cut > 0 && !cleaned.is_char_boundary(cut) {
            cut -= 1;
        }
        cleaned = format!("{}{ext}", &cleaned[..cut]);
    }
    cleaned
}

/// Go's `path.Base`: the last element after trailing slashes are removed;
/// `"."` for an empty path, `"/"` for a path of slashes only.
fn path_base(path: &str) -> &str {
    if path.is_empty() {
        return ".";
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/";
    }
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

/// Go's `path.Ext`: the suffix from the final dot of the last element,
/// including the dot; empty when there is none.
fn extension_of(name: &str) -> &str {
    let last = name.rsplit('/').next().unwrap_or(name);
    match last.rfind('.') {
        Some(dot) => &name[name.len() - (last.len() - dot)..],
        None => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mail")
            .join(name);
        let bytes =
            std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert!(
            bytes.windows(2).any(|pair| pair == b"\r\n"),
            "{name} lost its CRLF line endings on checkout; tests/fixtures/mail must stay binary in .gitattributes"
        );
        bytes
    }

    fn by_path<'a>(container: &'a Container, path: &str) -> &'a ContainerMember {
        container
            .members
            .iter()
            .find(|member| member.path == path)
            .unwrap_or_else(|| {
                panic!(
                    "no member {path}; members are {:?}",
                    container
                        .members
                        .iter()
                        .map(|m| &m.path)
                        .collect::<Vec<_>>()
                )
            })
    }

    /// The same six leaves ds-brain's `TestParseAssetMailReadsHeadersAndWalksTheMIMETree`
    /// lists from the same bytes, under the same names and decoded sizes.
    #[test]
    fn the_mixed_fixture_walks_to_the_six_members_ds_brain_lists() {
        let container = walk(&fixture("mixed.eml")).expect("walk");
        assert_eq!(container.format, Format::Eml);
        assert_eq!(container.member_count_total, 6);
        assert_eq!(container.members.len(), 6);
        assert!(!container.truncated);
        let text = by_path(&container, "part-1-1.txt");
        assert_eq!(
            text.bytes,
            "Please see the five review points — details attached.".len() as u64,
            "quoted-printable is decoded before counting; the CRLF before a boundary belongs to the boundary"
        );
        assert_eq!(text.format, Format::Txt);
        // `html` is not in the kernel's closed format vocabulary, so the part
        // lists as `unknown`/`other` here where the row says `doc`/`html`:
        // the NAME is the shared contract, the kind is each side's vocabulary.
        let html = by_path(&container, "part-1-2.html");
        assert_eq!(html.format, Format::Unknown);
        assert_eq!(html.kind, Kind::Other);
        let picture = by_path(&container, "picture.png");
        assert_eq!(picture.bytes, 128, "base64 is decoded before counting");
        assert_eq!(picture.kind, Kind::Image);
        assert_eq!(picture.format, Format::Png);
        let pdf = by_path(&container, "Shaker review.pdf");
        assert_eq!(pdf.bytes, "%PDF-1.4 tiny".len() as u64);
        assert_eq!(
            pdf.format,
            Format::Pdf,
            "an encoded filename is decoded; the extension decides over application/octet-stream"
        );
        let kmz = by_path(&container, "route.kmz");
        assert_eq!(
            kmz.kind,
            Kind::Geo,
            "a traversal filename is reduced to its base name; a geographic part is promotable"
        );
        assert!(
            !kmz.nested,
            "a geographic package is previewed as geo, not walked as a pack — as in a zip"
        );
        let second = by_path(&container, "picture (2).png");
        assert_eq!(
            second.bytes, 128,
            "member names are distinct within the message"
        );
        let order: Vec<&str> = container.members.iter().map(|m| m.path.as_str()).collect();
        assert_eq!(
            order,
            [
                "part-1-1.txt",
                "part-1-2.html",
                "picture.png",
                "Shaker review.pdf",
                "route.kmz",
                "picture (2).png"
            ],
            "members list in MIME order"
        );
    }

    #[test]
    fn a_member_extracts_to_its_decoded_bytes_by_the_listed_name() {
        let bytes = fixture("mixed.eml");
        let pdf = extract_part(&bytes, "Shaker review.pdf", 1024).expect("pdf");
        assert_eq!(pdf, b"%PDF-1.4 tiny");
        let text = extract_part(&bytes, "part-1-1.txt", 1024).expect("text");
        assert_eq!(
            String::from_utf8(text).expect("utf-8"),
            "Please see the five review points — details attached."
        );
        let picture = extract_part(&bytes, "picture (2).png", 1024).expect("second picture");
        assert_eq!(picture.len(), 128);
        assert!(picture.starts_with(b"\x89PNG"));
        let error = extract_part(&bytes, "picture.png", 16).expect_err("bounded");
        assert!(error.contains("the member bound is"), "{error}");
        let error = extract_part(&bytes, "2", 1024).expect_err("a section is not a name");
        assert!(error.starts_with("the mail has no part named 2"), "{error}");
    }

    #[test]
    fn a_plain_message_is_one_part_and_a_broken_boundary_keeps_what_it_listed() {
        let plain = walk(&fixture("plain.eml")).expect("plain");
        assert_eq!(plain.member_count_total, 1);
        assert_eq!(plain.members[0].path, "part-1.txt");
        assert_eq!(
            plain.members[0].bytes,
            "Poles delivered to Nyamagabe stockyard.\r\n".len() as u64,
            "a single-part body keeps its final line break; there is no boundary to own it"
        );
        let broken = walk(&fixture("broken.eml")).expect("broken");
        assert!(broken.member_count_total >= 1, "{broken:?}");
        // Text without headers is not a mail — a named refusal, never a guess.
        let error = walk(&fixture("not-a-mail.txt")).expect_err("not a mail");
        assert!(
            error.starts_with("the bytes are not a readable RFC 5322 message"),
            "{error}"
        );
        assert!(!is_message(b""));
        assert!(!is_message(b"PK\x03\x04"));
    }

    #[test]
    fn member_names_follow_the_one_rule_ds_brain_names_them_by() {
        assert_eq!(
            sanitize_name("..\\..\\C:\\Users\\x\\drawing.pdf"),
            "drawing.pdf"
        );
        assert_eq!(sanitize_name("bad\u{0}name\u{7}.png"), "badname.png");
        assert_eq!(sanitize_name("   "), "part");
        assert_eq!(sanitize_name("../"), "part");
        let long = sanitize_name(&format!("{}.pdf", "a".repeat(400)));
        assert_eq!(long.len(), MAX_MAIL_PATH_BYTES);
        assert!(long.ends_with(".pdf"));
        let mut walker = Walker {
            parts: Vec::new(),
            total: 0,
            truncated: false,
            used: BTreeSet::new(),
        };
        assert_eq!(walker.unique("a.png".into()), "a.png");
        assert_eq!(walker.unique("a.png".into()), "a (2).png");
        assert_eq!(walker.unique("a.png".into()), "a (3).png");
        assert_eq!(walker.unique("noext".into()), "noext");
        assert_eq!(walker.unique("noext".into()), "noext (2)");
        assert_eq!(extension_for("application/x-unknown-thing"), ".bin");
        assert_eq!(extension_for("image/png"), ".png");
        assert_eq!(extension_for("text/html"), ".html");
    }

    #[test]
    fn rfc_2231_parameters_decode_as_the_row_parser_decodes_them() {
        let (media, params) = parse_media_type(
            "application/octet-stream; name*=utf-8''r%C3%A9sum%C3%A9.pdf; name=\"plain.pdf\"",
        );
        assert_eq!(media, "application/octet-stream");
        assert_eq!(param(&params, "name").as_deref(), Some("résumé.pdf"));
        let (_, params) =
            parse_media_type("attachment; filename*0*=utf-8''long%20; filename*1=\"name.pdf\"");
        assert_eq!(param(&params, "filename").as_deref(), Some("long name.pdf"));
        let (media, params) = parse_media_type("Multipart/Mixed; Boundary=\"a;b\"");
        assert_eq!(media, "multipart/mixed");
        assert_eq!(param(&params, "boundary").as_deref(), Some("a;b"));
    }

    #[test]
    fn a_deep_tree_is_walked_eight_levels_and_the_listing_is_bounded() {
        // Nine nested multiparts: the ninth is a leaf, listed under its
        // section path rather than walked.
        let mut body = String::new();
        let mut closers = Vec::new();
        for level in 0..9 {
            let boundary = format!("b{level}");
            body.push_str(&format!(
                "--{boundary}\r\nContent-Type: multipart/mixed; boundary=\"b{}\"\r\n\r\n",
                level + 1
            ));
            closers.push(boundary);
        }
        body.push_str("--b9\r\nContent-Type: text/plain\r\n\r\nleaf\r\n--b9--\r\n");
        for boundary in closers.iter().rev() {
            body.push_str(&format!("--{boundary}--\r\n"));
        }
        let message = format!(
            "From: a@example.com\r\nContent-Type: multipart/mixed; boundary=\"b0\"\r\n\r\n{body}"
        );
        let container = walk(message.as_bytes()).expect("walk");
        assert_eq!(container.member_count_total, 1);
        assert_eq!(
            container.members[0].path, "part-1-1-1-1-1-1-1-1.bin",
            "the multipart below level eight is one leaf, named by its section"
        );

        let mut many = String::from(
            "From: a@example.com\r\nContent-Type: multipart/mixed; boundary=\"m\"\r\n\r\n",
        );
        for _ in 0..(MAX_MAIL_PARTS + 5) {
            many.push_str("--m\r\nContent-Type: text/plain\r\n\r\nx\r\n");
        }
        many.push_str("--m--\r\n");
        let container = walk(many.as_bytes()).expect("walk");
        assert_eq!(container.members.len(), MAX_MAIL_PARTS);
        assert_eq!(container.member_count_total, (MAX_MAIL_PARTS + 5) as u64);
        assert!(container.truncated);
    }
}
