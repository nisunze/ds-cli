use super::*;

#[test]
fn brain_parameter_precedence_is_independent_of_header_order() {
    for (header, expected) in [
        (
            "attachment; filename*=utf-8''r%C3%A9sum%C3%A9.pdf; filename=plain.pdf",
            "résumé.pdf",
        ),
        (
            "attachment; filename=plain.pdf; filename*=utf-8''r%C3%A9sum%C3%A9.pdf",
            "résumé.pdf",
        ),
        (
            "attachment; filename*0*=utf-8''continued; filename*1=.pdf; filename=plain.pdf",
            "continued.pdf",
        ),
    ] {
        let (_, parameters) = parse_media_type(header);
        let filename = parameters
            .iter()
            .find(|(name, _)| name == "filename")
            .unwrap();
        assert_eq!(filename.1, expected, "{header}");
    }
}

#[test]
fn transfer_decoding_preserves_unknown_escapes_and_exact_newlines() {
    for (wire, expected) in [
        ("left=20right=\r\nnext", "left rightnext"),
        ("=Z=ZZ=4=\r\n==\r\n=", "=Z=ZZ=4=="),
        ("line\r\n", "line\r\n"),
        ("line \t\r\nnext\nlast=", "line \t\r\nnext\nlast="),
    ] {
        assert_eq!(
            decode_quoted_printable(wire),
            expected.as_bytes(),
            "{wire:?}"
        );
    }
}

#[test]
fn header_words_preserve_the_preview_charset_and_malformed_qp_contract() {
    let mut notes = Vec::new();
    assert_eq!(
        decode_encoded_words("=?latin-1?Q?caf=E9_40?=", &mut notes),
        "café 40"
    );
    assert_eq!(
        decode_encoded_words("=?utf-8?Q?=Z=ZZ=4?=", &mut notes),
        "=Z=ZZ=4"
    );
    assert!(notes.is_empty());
    assert_eq!(
        decode_encoded_words("=?x-unknown?Q?a?=", &mut notes),
        "=?x-unknown?Q?a?="
    );
    assert!(notes[0].contains("x-unknown"));
}

#[test]
fn delimiter_candidates_inside_content_are_never_container_members() {
    let body = "preamble --b\r\n--b\r\nSubject: first\r\n\r\nkeep --b inside\r\n--bad\r\nkept\r\n--b \t\r\nSubject: second\r\n\r\nbody\r\n--b--\r\nepilogue";
    assert_eq!(
        sections(body, "b").collect::<Vec<_>>(),
        [
            "Subject: first\r\n\r\nkeep --b inside\r\n--bad\r\nkept",
            "Subject: second\r\n\r\nbody",
        ]
    );
}
