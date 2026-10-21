//! Bounded RFC/MIME primitives shared by preview and container projection.

use crate::assets::decode_base64;

/// Header block and body, split at the first blank line. A message with no
/// blank line is all headers and an empty body.
pub(crate) fn split_head_body(message: &str) -> (&str, &str) {
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

/// Header-only parsing never constructs a MIME body tree. The format's byte
/// admission happens before callers make an owned message or parse any header.
pub(crate) fn parse_headers(head: &str) -> Result<Vec<(String, String)>, String> {
    if let Some(line) = head
        .lines()
        .find(|line| !line.trim().is_empty() && !line.starts_with([' ', '\t']))
    {
        if !line.contains(':') {
            return Err(format!(
                "mail has no RFC 5322 header block; it begins \"{}\", which is not a header",
                line.chars().take(40).collect::<String>()
            ));
        }
    }
    // Installing one explicit parser also selects the configured raw default
    // for known headers: addresses, IDs and Date stay exactly display text.
    let parser = mail_parser::MessageParser::new()
        .header_raw(mail_parser::HeaderName::Subject)
        .default_header_raw();
    let parsed = parser
        .parse_headers(head.as_bytes())
        .ok_or("mail has no RFC 5322 headers; a mail preview needs at least one header")?;
    Ok(parsed.parts[0]
        .headers
        .iter()
        .map(|header| {
            (
                header.name.as_str().to_string(),
                header
                    .value
                    .as_text()
                    .unwrap_or_default()
                    .lines()
                    .map(str::trim)
                    .collect::<Vec<_>>()
                    .join(" "),
            )
        })
        .collect())
}

/// The first value of `name`, case-insensitively.
pub(crate) fn value<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header, _)| header.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// RFC 2047 encoded words. An unknown charset or encoding is kept raw and
/// noted rather than guessed at.
pub(crate) fn decode_encoded_words(value: &str, notes: &mut Vec<String>) -> String {
    static WORD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let word =
        WORD.get_or_init(|| regex::Regex::new(r"=\?([^?]*)\?([^?]*)\?([\s\S]*?)\?=").unwrap());
    let mut out = String::with_capacity(value.len());
    let mut end = 0;
    let mut adjacent = false;
    for captures in word.captures_iter(value) {
        let matched = captures.get(0).unwrap();
        let gap = &value[end..matched.start()];
        if !(adjacent && gap.trim().is_empty()) {
            out.push_str(gap);
        }
        let charset = &captures[1];
        let encoding = &captures[2];
        let supported = charset_decode(&[], &charset.to_ascii_lowercase()).is_some()
            && matches!(encoding.to_ascii_lowercase().as_str(), "b" | "q")
            && (!encoding.eq_ignore_ascii_case("b")
                || decode_base64("encoded header word", &captures[3]).is_ok());
        if supported {
            let parsed = if encoding.eq_ignore_ascii_case("q") {
                charset_decode(
                    &decode_quoted_printable(&captures[3].replace('_', " ")),
                    &charset.to_ascii_lowercase(),
                )
            } else {
                mail_parser::parsers::MessageStream::new(&matched.as_str().as_bytes()[1..])
                    .decode_rfc2047()
            };
            out.push_str(parsed.as_deref().unwrap_or(matched.as_str()));
        } else {
            notes.push(format!(
                "an encoded header word declares charset {charset} and encoding {encoding}, which the kernel does not decode; it is shown as received"
            ));
            out.push_str(matched.as_str());
        }
        end = matched.end();
        adjacent = true;
    }
    out.push_str(&value[end..]);
    out
}

/// Decodes `bytes` in `charset`, noting (never hiding) a charset the kernel
/// does not know.
pub(super) fn charset_text(bytes: &[u8], charset: Option<&str>, notes: &mut Vec<String>) -> String {
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

/// Preserve DS's malformed-escape and exact-newline dialect. The complete
/// upstream MIME decoder strips whitespace and drops incomplete escapes;
/// only its public hex decoder is appropriate at this compatibility boundary.
pub(crate) fn decode_quoted_printable(text: &str) -> Vec<u8> {
    let mut bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    while let Some(at) = bytes.iter().position(|byte| *byte == b'=') {
        out.extend_from_slice(&bytes[..at]);
        bytes = &bytes[at + 1..];
        if bytes.starts_with(b"\r\n") {
            bytes = &bytes[2..];
        } else if bytes.starts_with(b"\n") {
            bytes = &bytes[1..];
        } else if let Some(value) = bytes.get(..2).and_then(|hex| {
            mail_parser::decoders::quoted_printable::quoted_printable_decode_char(hex[0], hex[1])
        }) {
            out.push(value);
            bytes = &bytes[2..];
        } else {
            out.push(b'=');
        }
    }
    out.extend_from_slice(bytes);
    out
}

/// Decode the media type and RFC 2231 parameters through the upstream parser.
pub(crate) fn parse_media_type(header: &str) -> (String, Vec<(String, String)>) {
    // Brain/Go gives single extended values precedence over contiguous
    // continuations and plain values, regardless of their source order.
    // Select those fields before upstream unquoting/percent/charset decoding.
    static PARAMETER: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let parameter = PARAMETER.get_or_init(|| {
        regex::Regex::new(r#";\s*([^;=\s]+)\s*=\s*("(?:\\.|[^"\\])*"|[^;]*)"#).unwrap()
    });
    #[derive(Default)]
    struct Fields<'a> {
        plain: Option<&'a str>,
        extended: Option<&'a str>,
        pieces: std::collections::BTreeMap<u32, &'a str>,
    }
    let mut fields = std::collections::BTreeMap::<String, Fields<'_>>::new();
    for capture in parameter.captures_iter(header) {
        let key = capture[1].to_ascii_lowercase();
        let (name, suffix) = key
            .split_once('*')
            .map_or((key.as_str(), None), |(name, suffix)| (name, Some(suffix)));
        let raw = capture.get(0).unwrap().as_str();
        let group = fields.entry(name.to_string()).or_default();
        match suffix {
            None => {
                group.plain.get_or_insert(raw);
            }
            Some("") => group.extended = Some(raw),
            Some(piece) => {
                if let Ok(index) = piece.trim_end_matches('*').parse::<u32>() {
                    group.pieces.entry(index).or_insert(raw);
                }
            }
        }
    }
    let mut wire = header.split(';').next().unwrap_or_default().to_string();
    for group in fields.values() {
        if let Some(raw) = group.extended {
            wire.push_str(raw);
            continue;
        }
        let mut index = 0;
        while let Some(raw) = group.pieces.get(&index) {
            wire.push_str(raw);
            index += 1;
        }
        if index == 0 {
            if let Some(raw) = group.plain {
                wire.push_str(raw);
            }
        }
    }
    wire.push('\n');
    match mail_parser::parsers::MessageStream::new(wire.as_bytes()).parse_content_type() {
        mail_parser::HeaderValue::ContentType(parsed) => {
            let media = match parsed.c_subtype {
                Some(subtype) => format!("{}/{subtype}", parsed.c_type),
                None => parsed.c_type.into_owned(),
            };
            let parameters = parsed
                .attributes
                .unwrap_or_default()
                .into_iter()
                .map(|a| (a.name.into_owned(), a.value.into_owned()))
                .collect();
            (media, parameters)
        }
        _ => (String::new(), Vec::new()),
    }
}

/// A lazy part iterator: no Vec proportional to the message's MIME leaf count.
/// The upstream scanner finds delimiter candidates; the adapter admits only
/// whole delimiter lines, matching Brain's mime/multipart member boundaries.
pub(crate) fn sections<'a, 'b>(
    body: &'a str,
    boundary: &'b str,
) -> impl Iterator<Item = &'a str> + use<'a, 'b> {
    let mut stream = mail_parser::parsers::MessageStream::new(body.as_bytes());
    let mut current = None;
    let mut finished = boundary.is_empty();
    std::iter::from_fn(move || {
        while !finished {
            if !stream.seek_next_part(boundary.as_bytes()) {
                finished = true;
                return current.take().map(|start| &body[start..]);
            }
            let end = stream.offset();
            let begin = end - boundary.len() - 2;
            if begin != 0 && body.as_bytes()[begin - 1] != b'\n' {
                continue;
            }
            let tail = body[end..]
                .split('\n')
                .next()
                .unwrap_or_default()
                .trim_end();
            if !tail.is_empty() && tail != "--" {
                continue;
            }
            let previous = current.take();
            let close = tail == "--";
            stream.skip_crlf();
            if close {
                finished = true;
            } else {
                current = Some(stream.offset());
            }
            if let Some(start) = previous {
                let end = body[..begin]
                    .strip_suffix("\r\n")
                    .or_else(|| body[..begin].strip_suffix('\n'))
                    .map_or(begin, str::len);
                return Some(&body[start.min(end)..end]);
            }
        }
        None
    })
}

#[cfg(test)]
mod tests;
