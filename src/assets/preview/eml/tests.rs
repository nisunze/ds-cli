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
    message.push_str("Subject: =?utf-8?B?!!!not base64!!!?= =?x-unknown?Q?a?= =?utf-8?Q?trunc\r\n");
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
