//! Mail (§4.2 eml): RFC 5322 headers, the text part and an attachment list;
//! remote content is never fetched.
//!
//! Hand-rolled on purpose: no mail crate is in the offline registry and this
//! slice may not declare new dependencies. What it does: unfolds the header
//! block, decodes RFC 2047 encoded words (`=?utf-8?B?…?=`, `=?…?Q?…?=`),
//! walks one level of nested multipart boundaries, decodes the `text/plain`
//! part's `base64` or `quoted-printable` transfer encoding, bounds the body
//! at [`MAX_MAIL_BODY_BYTES`] and lists attachments by name, media type and
//! declared size.
//!
//! What it never does: fetch, resolve or emit a remote reference. Every
//! `http(s)://`, `ftp://` and `cid:` reference in the headers and the body is
//! replaced and counted (§4.2, §12), attachment bytes are never carried, and
//! a `text/html`-only message is reported as such rather than run through a
//! second, unowned HTML decoder.

use std::borrow::Cow;

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
    let mime = media_type(&content_type);
    if mime.starts_with("multipart/") {
        // An empty boundary would match every `--` in the body; it is no
        // boundary at all.
        let boundary = parameter(&content_type, "boundary").filter(|boundary| !boundary.is_empty());
        match boundary {
            Some(boundary) if depth < MAX_MULTIPART_DEPTH => {
                for section in split_sections(body, &boundary) {
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
            .map(media_type)
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
            Ok(charset_text(&decoded, part.charset.as_deref(), notes))
        }
        "quoted-printable" => {
            let decoded = decode_quoted_printable(part.body);
            Ok(charset_text(&decoded, part.charset.as_deref(), notes))
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

/// Header block and body, split at the first blank line. A message with no
/// blank line is all headers and an empty body.
fn split_head_body(message: &str) -> (&str, &str) {
    let crlf = message.find("\r\n\r\n").map(|at| (at, at + 4));
    let lf = message.find("\n\n").map(|at| (at, at + 2));
    let split = match (crlf, lf) {
        (Some(crlf), Some(lf)) => Some(if crlf.0 <= lf.0 { crlf } else { lf }),
        (found, None) | (None, found) => found,
    };
    match split {
        Some((at, from)) => (&message[..at], &message[from..]),
        None => (message, ""),
    }
}

/// Unfolds continuation lines (a header line beginning with space or tab
/// continues the one before it) into `(name, value)` pairs.
fn parse_headers(head: &str) -> Result<Vec<(String, String)>, String> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        match line.split_once(':') {
            Some((name, value)) => {
                headers.push((name.trim().to_string(), value.trim().to_string()))
            }
            None if headers.is_empty() => {
                return Err(format!(
                    "mail has no RFC 5322 header block; it begins \"{}\", which is not a header",
                    line.chars().take(40).collect::<String>()
                ));
            }
            None => continue,
        }
    }
    if headers.is_empty() {
        return Err(
            "mail has no RFC 5322 headers; a mail preview needs at least one header".into(),
        );
    }
    Ok(headers)
}

/// The first value of `name`, case-insensitively.
fn value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// `text/plain; charset=utf-8` → `text/plain`.
fn media_type(header: &str) -> String {
    header
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase()
}

/// One `; name=value` parameter, unquoted. RFC 2231 continuations
/// (`filename*=`) are not read; the part is then simply unnamed.
fn parameter(header: &str, name: &str) -> Option<String> {
    header.split(';').skip(1).find_map(|piece| {
        let (key, value) = piece.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().trim_matches('"').trim().to_string())
    })
}

/// The sections between `--boundary` markers, stopping at the closing
/// `--boundary--`. The line break that belongs to the boundary line is the
/// boundary's, not the part's, so exactly one is dropped at each end.
fn split_sections<'a>(body: &'a str, boundary: &str) -> Vec<&'a str> {
    let marker = format!("--{boundary}");
    body.split(marker.as_str())
        .skip(1)
        .take_while(|section| !section.starts_with("--"))
        .map(|section| {
            let section = section.trim_start_matches(['\r', '\n']);
            section
                .strip_suffix("\r\n")
                .or_else(|| section.strip_suffix('\n'))
                .unwrap_or(section)
        })
        .collect()
}

/// RFC 2047 encoded words. An unknown charset or encoding is kept raw and
/// noted rather than guessed at.
fn decode_encoded_words(value: &str, notes: &mut Vec<String>) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    let mut adjacent = false;
    while let Some(start) = rest.find("=?") {
        let after = &rest[start + 2..];
        let Some(charset_end) = after.find('?') else {
            break;
        };
        let charset = &after[..charset_end];
        let after_charset = &after[charset_end + 1..];
        let Some(encoding_end) = after_charset.find('?') else {
            break;
        };
        let encoding = &after_charset[..encoding_end];
        let encoded = &after_charset[encoding_end + 1..];
        let Some(word_end) = encoded.find("?=") else {
            break;
        };
        let consumed = start + 2 + charset_end + 1 + encoding_end + 1 + word_end + 2;
        let gap = &rest[..start];
        // RFC 2047 §6.2: whitespace between two adjacent encoded words is a
        // separator, not text.
        if !(adjacent && gap.trim().is_empty()) {
            out.push_str(gap);
        }
        match decode_word(charset, encoding, &encoded[..word_end]) {
            Some(text) => out.push_str(&text),
            None => {
                notes.push(format!(
                    "an encoded header word declares charset {charset} and encoding {encoding}, which the kernel does not decode; it is shown as received"
                ));
                out.push_str(&rest[start..consumed]);
            }
        }
        adjacent = true;
        rest = &rest[consumed..];
    }
    out.push_str(rest);
    out
}

fn decode_word(charset: &str, encoding: &str, encoded: &str) -> Option<String> {
    let bytes = match encoding.to_ascii_lowercase().as_str() {
        "b" => decode_base64("encoded header word", encoded).ok()?,
        "q" => decode_quoted_printable(&encoded.replace('_', " ")),
        _ => return None,
    };
    charset_decode(&bytes, &charset.to_ascii_lowercase())
}

/// Decodes `bytes` in `charset`, noting (never hiding) a charset the kernel
/// does not know.
fn charset_text(bytes: &[u8], charset: Option<&str>, notes: &mut Vec<String>) -> String {
    let name = charset.unwrap_or("utf-8").to_ascii_lowercase();
    match charset_decode(bytes, &name) {
        Some(text) => text,
        None => {
            notes.push(format!(
                "the body declares charset {name}, which the kernel does not decode; it is read as UTF-8 with invalid sequences replaced"
            ));
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
}

fn charset_decode(bytes: &[u8], charset: &str) -> Option<String> {
    match charset {
        "utf-8" | "utf8" | "us-ascii" | "ascii" => {
            Some(String::from_utf8_lossy(bytes).into_owned())
        }
        "iso-8859-1" | "latin-1" | "latin1" => {
            Some(bytes.iter().map(|byte| *byte as char).collect())
        }
        _ => None,
    }
}

/// `=XX` hex escapes and soft line breaks, per RFC 2045 §6.7. A malformed
/// escape is kept as the literal `=` it was written as.
fn decode_quoted_printable(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0usize;
    while at < bytes.len() {
        if bytes[at] != b'=' {
            out.push(bytes[at]);
            at += 1;
            continue;
        }
        let rest = &bytes[at + 1..];
        if rest.starts_with(b"\r\n") {
            at += 3;
        } else if rest.starts_with(b"\n") {
            at += 2;
        } else if let (Some(high), Some(low)) = (
            rest.first().copied().and_then(hex_value),
            rest.get(1).copied().and_then(hex_value),
        ) {
            out.push(high * 16 + low);
            at += 3;
        } else {
            out.push(b'=');
            at += 1;
        }
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{Format, REQUEST_SCHEMA};

    fn request() -> PreviewRequest {
        PreviewRequest {
            schema: REQUEST_SCHEMA.to_string(),
            format: Format::Eml,
            member: None,
            sheet: None,
            pages: None,
            rows: None,
            name: None,
        }
    }

    fn mail_block(doc: &PreviewDoc) -> (&Vec<(String, String)>, &String, &Vec<MailAttachment>) {
        match &doc.blocks[0] {
            Block::Mail {
                headers,
                text,
                attachments,
            } => (headers, text, attachments),
            other => panic!("the first block is the mail, not {other:?}"),
        }
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_eml_headers_and_text_part() {
        let message = concat!(
            "From: Site Team <site@example.test>\r\n",
            "To: Nisunze <owner@example.test>\r\n",
            "Subject: =?utf-8?B?UsOpc3Vtw6kgZGUgY2hhbnRpZXI=?=\r\n",
            " =?utf-8?Q?_lot_3?=\r\n",
            "Date: Mon, 09 Mar 2026 08:12:00 +0000\r\n",
            "Message-ID: <a1@example.test>\r\n",
            "MIME-Version: 1.0\r\n",
            "Content-Type: multipart/mixed; boundary=\"OUTER\"\r\n",
            "\r\n",
            "preamble\r\n",
            "--OUTER\r\n",
            "Content-Type: multipart/alternative; boundary=\"INNER\"\r\n",
            "\r\n",
            "--INNER\r\n",
            "Content-Type: text/plain; charset=utf-8\r\n",
            "Content-Transfer-Encoding: quoted-printable\r\n",
            "\r\n",
            "Bonjour =E2=82=AC 40 poteaux pos=C3=A9s,=\r\n",
            " tension OK\r\n",
            "--INNER\r\n",
            "Content-Type: text/html; charset=utf-8\r\n",
            "\r\n",
            "<p>ignored</p>\r\n",
            "--INNER--\r\n",
            "--OUTER\r\n",
            "Content-Type: application/pdf; name=\"staking.pdf\"\r\n",
            "Content-Disposition: attachment; filename=\"staking.pdf\"; size=811233\r\n",
            "Content-Transfer-Encoding: base64\r\n",
            "\r\n",
            "JVBERi0xLjcK\r\n",
            "--OUTER--\r\n",
        );
        let doc = mail(message.as_bytes(), &request()).expect("mail decodes");
        let (headers, text, attachments) = mail_block(&doc);
        assert_eq!(
            headers[0],
            (
                "From".to_string(),
                "Site Team <site@example.test>".to_string()
            )
        );
        assert_eq!(
            headers
                .iter()
                .find(|(name, _)| name == "Subject")
                .map(|(_, value)| value.as_str()),
            Some("Résumé de chantier lot 3")
        );
        assert_eq!(
            headers
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            vec!["From", "To", "Date", "Subject", "Message-ID"]
        );
        assert_eq!(text, "Bonjour € 40 poteaux posés, tension OK");
        assert_eq!(
            attachments,
            &vec![MailAttachment {
                name: "staking.pdf".into(),
                mime: "application/pdf".into(),
                bytes: 811_233,
            }]
        );
        assert!(!doc.truncated);
        assert!(doc.bounded);

        // A single-part base64 message, and a header-only message.
        let simple = "From: a@b.test\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8gc2l0ZQ==\r\n";
        let doc = mail(simple.as_bytes(), &request()).expect("mail decodes");
        assert_eq!(mail_block(&doc).1, "hello site");

        let html_only = "From: a@b.test\r\nContent-Type: text/html\r\n\r\n<p>hi</p>\r\n";
        let doc = mail(html_only.as_bytes(), &request()).expect("mail decodes");
        assert_eq!(
            doc.blocks[0],
            Block::Paragraph {
                text: "html body not rendered".into()
            }
        );
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("no HTML decoder")));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_eml_malformed_input() {
        assert!(mail(&[], &request())
            .expect_err("empty is refused")
            .contains("no RFC 5322 headers"));
        assert!(mail(b"just a line of text\r\nand another\r\n", &request())
            .expect_err("headerless is refused")
            .contains("which is not a header"));
        assert!(mail(
            b"From: a@b.test\r\nContent-Transfer-Encoding: base64\r\n\r\n!!!! not base64 !!!!\r\n",
            &request()
        )
        .expect_err("a malformed base64 body is refused")
        .contains("not valid base64"));

        let broken = [
            b"From: a@b.test\r\nContent-Type: text/plain\r\n\r\nsite ".as_slice(),
            &[0xff, 0xfe],
        ]
        .concat();
        assert!(mail(&broken, &request())
            .expect("invalid UTF-8 never panics")
            .notes
            .iter()
            .any(|note| note.contains("not valid UTF-8")));

        let over = vec![b'x'; 4 * 1024 * 1024 + 1];
        let refusal = mail(&over, &request()).expect_err("over the eml bound");
        assert!(
            refusal.contains("4,194,305") && refusal.contains("4 MiB"),
            "{refusal}"
        );
    }

    // ── adversarial review (stage 4) ────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_eml_ten_thousand_headers_and_broken_encoded_words() {
        let mut message = String::new();
        for index in 0..10_000 {
            message.push_str(&format!("X-Trace-{index}: hop {index}\r\n"));
        }
        message.push_str(
            "Subject: =?utf-8?B?!!!not base64!!!?= =?x-unknown?Q?a?= =?utf-8?Q?trunc\r\n",
        );
        message.push_str("From: a@example.test\r\n\r\nbody\r\n");
        let doc = mail(message.as_bytes(), &request()).expect("ten thousand headers decode");
        let (headers, text, attachments) = mail_block(&doc);
        assert_eq!(headers.len(), 2, "only the display headers are shown");
        assert_eq!(text, "body\r\n");
        assert!(attachments.is_empty());
        assert!(
            doc.notes.iter().any(|note| note.contains("x-unknown")),
            "an undecodable encoded word is noted: {:?}",
            doc.notes
        );
        // A 3 MiB single folded header line, and a header block with no body.
        let mut folded = String::from("Subject: start\r\n");
        for _ in 0..30_000 {
            folded.push_str(" continued and continued and continued and continued and continued and continued and continued and\r\n");
        }
        let doc = mail(folded.as_bytes(), &request()).expect("a long folded header decodes");
        let (headers, text, _) = mail_block(&doc);
        assert!(headers[0].1.starts_with("start continued"));
        assert!(text.is_empty());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_eml_part_count_is_bounded_and_an_empty_boundary_is_no_boundary() {
        // 20,000 sections on a one-letter boundary: at most MAX_MAIL_PARTS
        // parts are read, the rest are counted into a note, and the
        // attachment list is bounded with them.
        let mut message = String::from(
            "From: a@example.test\r\nContent-Type: multipart/mixed; boundary=\"b\"\r\n\r\n",
        );
        for index in 0..20_000 {
            message.push_str(&format!(
                "--b\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=\"p{index}.bin\"\r\n\r\nxx\r\n"
            ));
        }
        message.push_str("--b--\r\n");
        let doc = mail(message.as_bytes(), &request()).expect("bounded, not exhausted");
        let (_, _, attachments) = mail_block(&doc);
        assert_eq!(attachments.len(), MAX_MAIL_PARTS);
        assert!(doc.truncated);
        assert!(
            doc.notes.iter().any(|note| note
                == "the message has 20,000 MIME parts; the first 1,000 are read (the part bound is 1,000)"),
            "{:?}",
            doc.notes
        );
        // `boundary=""` would split on every `--`; it is treated as none.
        let message = "From: a@example.test\r\nContent-Type: multipart/mixed; boundary=\"\"\r\n\r\n--\r\n--\r\n--\r\n";
        let doc = mail(message.as_bytes(), &request()).expect("decodes");
        let (_, text, attachments) = mail_block(&doc);
        assert!(
            attachments.is_empty(),
            "a part that could not be split is never an attachment of itself"
        );
        assert_eq!(text, "--\r\n--\r\n--\r\n", "its text is shown as received");
        assert!(doc
            .notes
            .iter()
            .any(|note| note.contains("declares no boundary")));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn assets_review_eml_invalid_base64_body_is_a_named_refusal() {
        let message = "From: a@example.test\r\nContent-Type: text/plain\r\nContent-Transfer-Encoding: base64\r\n\r\n!!! not base64 at all !!!\r\n";
        let error = mail(message.as_bytes(), &request()).expect_err("named refusal");
        assert!(
            error.starts_with("mail body is not valid base64"),
            "{error}"
        );
        // Quoted-printable with every malformed escape shape: never a panic.
        let message = "From: a@example.test\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n=\r\n=Z=ZZ=4=\r\n==\r\n=";
        let doc = mail(message.as_bytes(), &request()).expect("malformed escapes are kept literal");
        let (_, text, _) = mail_block(&doc);
        assert!(text.contains("=Z=ZZ=4"), "{text}");
    }
}
