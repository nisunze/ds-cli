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

use super::preview::eml::mime::{
    decode_encoded_words, decode_quoted_printable, parse_headers, parse_media_type, sections,
    split_head_body, value,
};
use super::{
    check_bytes, check_format_cap, decode_base64, describe_bytes, format, thousands, Container,
    ContainerMember, Format, Kind,
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
    check_format_cap(bytes.len() as u64, Format::Eml)?;
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
                for (index, part) in sections(body, &boundary).enumerate() {
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
        let (_, disposition_params) = if disposition_header.trim().is_empty() {
            (String::new(), Vec::new())
        } else {
            parse_media_type(disposition_header)
        };
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
            name
        } else {
            format!("part-{}{}", section.replace('.', "-"), extension_for(media))
        };
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
            "text/html" => Format::Html,
            "audio/mpeg" => Format::Mp3,
            "audio/wav" | "audio/x-wav" => Format::Wav,
            "video/mp4" => Format::Mp4,
            "video/webm" => Format::Webm,
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
        "audio/mpeg" => ".mp3",
        "audio/wav" | "audio/x-wav" => ".wav",
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
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
mod tests;
