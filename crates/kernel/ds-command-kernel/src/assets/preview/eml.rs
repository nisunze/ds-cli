//! Mail (§4.2 eml): RFC 5322 headers, the text part and an attachment list;
//! remote content is never fetched.
//!
//! Generic RFC/MIME decoding delegates to mail-parser. The projection keeps
//! its own part/depth admission, text-only body selection and display bounds.
//!
//! What it never does: fetch, resolve or emit a remote reference. Every
//! `http(s)://`, `ftp://` and `cid:` reference in the headers and the body is
//! replaced and counted (§4.2, §12), attachment bytes are never carried, and
//! a `text/html`-only message is reported as such rather than run through a
//! second, unowned HTML decoder.

use std::borrow::Cow;

pub(crate) mod mime;
use mime::{decode_encoded_words, decode_quoted_printable, parse_headers, split_head_body, value};

use super::super::{
    check_format_cap, decode_base64, thousands, Block, MailAttachment, PreviewDoc, PreviewRequest,
    MAX_MAIL_BODY_BYTES,
};
use super::text::strip_remote_references;

/// Multipart levels walked: the message itself and one nesting inside it
/// (`multipart/mixed` holding a `multipart/alternative` is the common shape).
/// Deeper nesting is reported, never walked.
const MAX_MULTIPART_DEPTH: usize = 2;

/// MIME leaves read from one message. A 4 MiB body split on a one-letter
/// boundary would otherwise become a million parts (and a million attachment
/// rows); past this bound the rest are counted and reported, never walked.
pub const MAX_MAIL_PARTS: usize = 1_000;

/// The headers a mail preview shows, in this order.
const DISPLAY_HEADERS: [&str; 6] = ["From", "To", "Cc", "Date", "Subject", "Message-ID"];

pub fn mail(bytes: &[u8], request: &PreviewRequest) -> Result<PreviewDoc, String> {
    check_format_cap(bytes.len() as u64, request.format)?;
    let mut doc = PreviewDoc::new(request.format);
    let message = String::from_utf8_lossy(bytes);
    if matches!(message, Cow::Owned(_)) {
        doc.notes.push(
            "the message is not valid UTF-8; invalid sequences are shown as the replacement character"
                .into(),
        );
    }
    if message.trim().is_empty() {
        return Err("mail is empty; there are no RFC 5322 headers to read".into());
    }

    let (head, body) = split_head_body(&message);
    let headers = parse_headers(head)?;
    let mut notes: Vec<String> = Vec::new();
    let mut removed = 0usize;

    let shown_headers: Vec<(String, String)> = DISPLAY_HEADERS
        .iter()
        .filter_map(|name| {
            let raw = value(&headers, name)?;
            let decoded = decode_encoded_words(raw, &mut notes);
            let (text, stripped) = strip_remote_references(&decoded);
            removed += stripped;
            Some(((*name).to_string(), text))
        })
        .collect();

    let mut parts: Vec<Part> = Vec::new();
    let mut parts_left_out: u64 = 0;
    collect_parts(
        body,
        &headers,
        0,
        &mut parts,
        &mut parts_left_out,
        &mut notes,
    );
    if parts_left_out > 0 {
        doc.truncated = true;
        notes.push(format!(
            "the message has {} MIME parts; the first {} are read (the part bound is {})",
            thousands(parts.len() as u64 + parts_left_out),
            thousands(parts.len() as u64),
            thousands(MAX_MAIL_PARTS as u64)
        ));
    }

    let text_part = parts
        .iter()
        .find(|part| part.mime == "text/plain" && !part.is_attachment());
    let mut text = match text_part {
        Some(part) => body_text(part, &mut notes)?,
        None => {
            if parts.iter().any(|part| part.mime == "text/html") {
                notes.push(
                    "the message carries only a text/html body; the kernel has no HTML decoder and never strips tags into text, so no body text is shown"
                        .into(),
                );
                doc.blocks.push(Block::Paragraph {
                    text: "html body not rendered".into(),
                });
            } else {
                notes.push(
                    "the message carries no text/plain part; only its headers and attachments are shown"
                        .into(),
                );
            }
            String::new()
        }
    };

    let body_bytes = text.len();
    if body_bytes > MAX_MAIL_BODY_BYTES {
        let mut end = MAX_MAIL_BODY_BYTES;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        doc.truncated = true;
        notes.push(format!(
            "mail body is {} bytes; the first {} are shown (the mail body bound is {} bytes)",
            thousands(body_bytes as u64),
            thousands(end as u64),
            thousands(MAX_MAIL_BODY_BYTES as u64)
        ));
    }
    let (text, stripped) = strip_remote_references(&text);
    removed += stripped;

    let attachments: Vec<MailAttachment> = parts
        .iter()
        .filter(|part| part.is_attachment())
        .map(Part::attachment)
        .collect();
    if !attachments.is_empty() {
        notes.push(
            "attachment sizes are the declared or encoded sizes; a preview never carries attachment bytes"
                .into(),
        );
    }
    if removed > 0 {
        notes.push(format!(
            "{} remote reference(s) were removed from the mail; a preview never fetches remote content",
            thousands(removed as u64)
        ));
    }

    let body_mime = text_part.map_or("none".to_string(), |part| part.mime.clone());
    let body_encoding = text_part.map_or("none".to_string(), |part| part.encoding.clone());
    doc.blocks.push(Block::Mail {
        headers: shown_headers,
        text,
        attachments: attachments.clone(),
    });
    doc.blocks.push(Block::Meta {
        entries: vec![
            (
                "bytes".into(),
                format!("{} bytes", thousands(bytes.len() as u64)),
            ),
            ("parts".into(), thousands(parts.len() as u64)),
            ("body".into(), body_mime),
            ("transfer encoding".into(), body_encoding),
            ("attachments".into(), thousands(attachments.len() as u64)),
        ],
    });
    doc.notes.extend(notes);
    Ok(doc)
}

/// One MIME leaf: its headers already parsed, its body still encoded.
struct Part<'a> {
    mime: String,
    charset: Option<String>,
    encoding: String,
    disposition: String,
    filename: Option<String>,
    declared_size: Option<u64>,
    body: &'a str,
}

impl Part<'_> {
    fn is_attachment(&self) -> bool {
        self.disposition.starts_with("attachment")
            || (self.filename.is_some() && self.mime != "text/plain")
            || !self.mime.starts_with("text/")
    }

    fn attachment(&self) -> MailAttachment {
        MailAttachment {
            name: self
                .filename
                .clone()
                .unwrap_or_else(|| "unnamed attachment".to_string()),
            mime: self.mime.clone(),
            bytes: self.declared_size.unwrap_or_else(|| self.encoded_bytes()),
        }
    }

    /// The size the part declares, or what its encoding implies. No
    /// attachment is decoded to find out.
    fn encoded_bytes(&self) -> u64 {
        if self.encoding == "base64" {
            let characters = self
                .body
                .chars()
                .filter(|character| !character.is_whitespace() && *character != '=')
                .count() as u64;
            characters * 3 / 4
        } else {
            self.body.len() as u64
        }
    }
}

/// Walks `body` as the part described by `headers`, flattening one level of
/// nested multipart into `parts`.
fn collect_parts<'a>(
    body: &'a str,
    headers: &[(String, String)],
    depth: usize,
    parts: &mut Vec<Part<'a>>,
    left_out: &mut u64,
    notes: &mut Vec<String>,
) {
    if parts.len() >= MAX_MAIL_PARTS {
        *left_out = left_out.saturating_add(1);
        return;
    }
    let content_type = value(headers, "content-type")
        .map(str::trim)
        .filter(|declared| !declared.is_empty())
        .unwrap_or("text/plain")
        .to_string();
    let mime = mime::parse_media_type(&content_type).0;
    if mime.starts_with("multipart/") {
        // An empty boundary would match every `--` in the body; it is no
        // boundary at all.
        let boundary = parameter(&content_type, "boundary").filter(|boundary| !boundary.is_empty());
        match boundary {
            Some(boundary) if depth < MAX_MULTIPART_DEPTH => {
                for section in mime::sections(body, &boundary) {
                    if parts.len() >= MAX_MAIL_PARTS {
                        *left_out = left_out.saturating_add(1);
                        continue;
                    }
                    let (head, part_body) = split_head_body(section);
                    let part_headers = parse_headers(head).unwrap_or_default();
                    collect_parts(
                        part_body,
                        &part_headers,
                        depth + 1,
                        parts,
                        left_out,
                        notes,
                    );
                }
                return;
            }
            Some(_) => notes.push(format!(
                "a multipart part nested deeper than {MAX_MULTIPART_DEPTH} levels was not walked; its text is shown as received"
            )),
            None => notes.push(
                "a multipart part declares no boundary; its text is shown as received".into(),
            ),
        }
    }
    // A multipart part that could not be split (no boundary, or too deep) is
    // shown as the text it is, exactly as the note above says — never listed
    // as an attachment of itself.
    let mime = if mime.starts_with("multipart/") {
        "text/plain".to_string()
    } else {
        mime
    };
    parts.push(Part {
        mime,
        charset: parameter(&content_type, "charset"),
        encoding: value(headers, "content-transfer-encoding")
            .unwrap_or("7bit")
            .trim()
            .to_ascii_lowercase(),
        disposition: value(headers, "content-disposition")
            .map(|header| mime::parse_media_type(header).0)
            .unwrap_or_default(),
        filename: value(headers, "content-disposition")
            .and_then(|disposition| parameter(disposition, "filename"))
            .or_else(|| parameter(&content_type, "name")),
        declared_size: value(headers, "content-disposition")
            .and_then(|disposition| parameter(disposition, "size"))
            .and_then(|size| size.parse::<u64>().ok()),
        body,
    });
}

/// The text a part carries, with its transfer encoding and charset undone.
fn body_text(part: &Part, notes: &mut Vec<String>) -> Result<String, String> {
    match part.encoding.as_str() {
        "base64" => {
            let packed: String = part
                .body
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            let decoded = decode_base64("mail body", &packed)?;
            Ok(mime::charset_text(&decoded, part.charset.as_deref(), notes))
        }
        "quoted-printable" => {
            let decoded = decode_quoted_printable(part.body);
            Ok(mime::charset_text(&decoded, part.charset.as_deref(), notes))
        }
        "7bit" | "8bit" | "binary" => Ok(part.body.to_string()),
        other => {
            notes.push(format!(
                "content-transfer-encoding {other} is not decoded; the body is shown as received"
            ));
            Ok(part.body.to_string())
        }
    }
}

fn parameter(header: &str, name: &str) -> Option<String> {
    mime::parse_media_type(header)
        .1
        .into_iter()
        .find_map(|(key, value)| (key == name && !value.is_empty()).then_some(value))
}

#[cfg(test)]
mod tests;
