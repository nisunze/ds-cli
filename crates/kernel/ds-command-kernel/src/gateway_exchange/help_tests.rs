use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn request<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        _ => panic!("expected a fetch"),
    }
}

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    match resume(
        continuation,
        Observed::Response {
            status: 200,
            content_type: Some("application/json".into()),
            retry_after: None,
        },
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(library) => Ok(library),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn svg(id: &str, data: &str) -> Value {
    json!({"id": id, "filename": format!("{id}.svg"), "contentType": "image/svg+xml",
        "encoding": "utf8", "data": data, "order": 0})
}

fn article(slug: &str, content: &str) -> Value {
    json!({"slug": slug, "title": "Data Cleaning", "description": "", "category": "data-cleaning",
        "order": 0, "content": content})
}

fn envelope(articles: Value, assets: Value) -> Value {
    json!({"success": true, "data": {"articles": articles, "categories": [
        {"key": "data-cleaning", "label": "Data cleaning", "order": 1, "collapsible": true, "defaultOpen": false}
    ], "count": 1, "assets": assets}})
}

#[test]
fn help_and_tutorials_name_this_clients_audience() {
    let help = request(HelpArticles {
        language: "fr".into(),
    });
    assert_eq!(help.path, "/api/v1/help");
    assert_eq!(
        help.body,
        Some(
            json!({"action": "list", "app": "ds_griddesign", "language": "fr",
            "requester": "ds-web", "platform": "web", "includeAssets": true})
        )
    );
    assert_eq!(help.response_cap_bytes, 32 * 1024 * 1024);
    let tutorials = request(TutorialArticles {
        language: "en".into(),
    });
    assert_eq!(tutorials.path, "/api/v1/tutorials");
    assert_eq!(
        tutorials.body,
        Some(json!({"action": "list", "language": "en",
            "requester": "ds-web", "platform": "web", "includeAssets": true}))
    );
}

#[test]
fn a_language_is_one_short_tag() {
    for bad in ["", "en fr", "en/../x", &"e".repeat(17)] {
        let Step::Refused(refusal) = start(HelpArticles {
            language: bad.into(),
        }) else {
            panic!("{bad:?} is not a language tag")
        };
        assert_eq!(refusal.code, LANGUAGE_INVALID);
    }
    assert!(matches!(
        start(HelpArticles {
            language: "pt-BR".into()
        }),
        Step::Fetch { .. }
    ));
}

#[test]
fn a_utf8_asset_becomes_a_uri_component_encoded_data_url() {
    let library = answered(
        HelpArticles {
            language: "en".into(),
        },
        envelope(
            json!([article("a", "help-asset:shot")]),
            json!({"shot": svg("shot", "<svg><text>Data Cleaning</text></svg>")}),
        ),
    )
    .unwrap();
    assert_eq!(
        library.articles[0].content,
        "data:image/svg+xml;charset=utf-8,%3Csvg%3E%3Ctext%3EData%20Cleaning%3C%2Ftext%3E%3C%2Fsvg%3E"
    );
    assert_eq!(
        utf8_percent_encode("a-b_c.d!e~f*g'h(i)j k/é", URI_COMPONENT).to_string(),
        "a-b_c.d!e~f*g'h(i)j%20k%2F%C3%A9",
        "the same characters encodeURIComponent keeps"
    );
}

#[test]
fn references_resolve_without_touching_unresolved_ids() {
    let content =
        "![workspace](help-asset:upload-cleaning-workspace)\n![missing](help-asset:missing)";
    let library = answered(
        HelpArticles {
            language: "en".into(),
        },
        envelope(
            json!([article("a", content)]),
            json!({"upload-cleaning-workspace": svg("upload-cleaning-workspace", "<svg />")}),
        ),
    )
    .unwrap();
    let resolved = &library.articles[0].content;
    assert!(resolved.contains("](data:image/svg+xml;charset=utf-8,%3Csvg%20%2F%3E)"));
    assert!(resolved.contains("](help-asset:missing)"));
}

#[test]
fn an_articles_own_assets_win_over_the_shared_ones() {
    let mut own = article(
        "a",
        r#"<img src="help-asset:fill" /><img src="help-asset:url" />"#,
    );
    own["assetData"] = json!({
        "fill": {"id": "fill", "filename": "fill.png", "contentType": "image/png", "encoding": "base64", "data": "iVBOR", "order": 0},
        "url": {"id": "url", "filename": "", "contentType": "", "encoding": "url", "url": "  https://cdn.example/x.png ", "order": 1},
    });
    let library = answered(
        TutorialArticles {
            language: "en".into(),
        },
        envelope(json!([own]), json!({"fill": svg("fill", "<svg />")})),
    )
    .unwrap();
    assert_eq!(
        library.articles[0].content,
        r#"<img src="data:image/png;base64,iVBOR" /><img src="https://cdn.example/x.png" />"#
    );
}

#[test]
fn an_asset_without_bytes_or_url_leaves_its_reference() {
    let library = answered(
        HelpArticles {
            language: "en".into(),
        },
        envelope(
            json!([article("a", "help-asset:empty help-asset:blank")]),
            json!({
                "empty": {"id": "empty", "filename": "", "contentType": "", "encoding": "utf8", "order": 0},
                "blank": {"id": "blank", "filename": "", "contentType": "", "encoding": "url", "url": " ", "order": 0},
            }),
        ),
    )
    .unwrap();
    assert_eq!(
        library.articles[0].content,
        "help-asset:empty help-asset:blank"
    );
}

#[test]
fn the_library_carries_only_what_a_page_renders() {
    let mut html = article("a", "<p>x</p>");
    html["format"] = json!("html");
    html["sourceFormat"] = json!("docx");
    html["assets"] = json!(["shot"]);
    let mut other = article("b", "x");
    other["format"] = json!("rst");
    let library = answered(
        HelpArticles {
            language: "en".into(),
        },
        envelope(json!([html, other, article("c", "x")]), json!({})),
    )
    .unwrap();
    assert_eq!(
        library
            .articles
            .iter()
            .map(|article| article.format)
            .collect::<Vec<_>>(),
        [
            ArticleFormat::Html,
            ArticleFormat::Markdown,
            ArticleFormat::Markdown
        ]
    );
    assert_eq!(
        serde_json::to_value(&library).unwrap(),
        json!({"articles": [
            {"slug": "a", "title": "Data Cleaning", "description": "", "category": "data-cleaning", "order": 0, "content": "<p>x</p>", "format": "html"},
            {"slug": "b", "title": "Data Cleaning", "description": "", "category": "data-cleaning", "order": 0, "content": "x", "format": "markdown"},
            {"slug": "c", "title": "Data Cleaning", "description": "", "category": "data-cleaning", "order": 0, "content": "x", "format": "markdown"},
        ], "categories": [
            {"key": "data-cleaning", "label": "Data cleaning", "order": 1, "collapsible": true, "defaultOpen": false}
        ]})
    );
}

#[test]
fn a_library_without_its_lists_is_refused() {
    let refused = answered(
        HelpArticles {
            language: "en".into(),
        },
        json!({"success": true, "data": {"articles": []}}),
    )
    .unwrap_err();
    assert_eq!(refused.code, INVALID);
}
