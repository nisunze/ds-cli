use super::*;

#[test]
fn oversized_mail_refuses_before_an_owned_message_or_mime_tree() {
    let bytes = vec![b'x'; crate::assets::MAX_MAIL_BYTES as usize + 1];
    let error = walk(&bytes).expect_err("input byte admission");
    assert!(
        error.contains("4,194,305") && error.contains("4 MiB"),
        "{error}"
    );
}

fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/mail")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
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
    // `html` joined the kernel's format vocabulary in af1ab34: the part now
    // lists as an HTML note; the NAME stays the shared contract with ds-brain's row.
    let html = by_path(&container, "part-1-2.html");
    assert_eq!(html.format, Format::Html);
    assert_eq!(html.kind, Kind::Note);
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
