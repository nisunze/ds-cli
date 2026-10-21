use super::*;
use crate::printing::{named_layout, ElementKind};

fn layout() -> Layout {
    named_layout(
        Some("project-overview-a3".into()),
        Some("Project overview · A3".into()),
        Paper::A3,
        Orientation::Landscape,
    )
    .unwrap()
}

fn sheet(incomplete: bool) -> Sheet<'static> {
    Sheet {
        caption: Some("Rwanda Template"),
        country: "Rwanda",
        date: "2026-10-20",
        page: (1, 1),
        incomplete,
    }
}

#[test]
fn an_overview_sheet_binds_every_key_its_layout_prints() {
    let layout = layout();
    assert_eq!(
        keys(&layout),
        ["title", "project", "drawing", "date", "source"]
    );
    let text = bindings(&layout, &sheet(false)).unwrap();
    assert_eq!(text["title"], "Project overview · A3");
    assert_eq!(text["project"], "Rwanda Template");
    assert_eq!(text["drawing"], "A3 landscape · 1/1");
    assert_eq!(text["date"], "2026-10-20");
    assert_eq!(
        text["source"],
        "Project designs and governed Rwanda context"
    );
}

#[test]
fn authored_title_block_values_win_and_missing_context_is_said() {
    let mut layout = layout();
    layout
        .title_block
        .insert("title".into(), "Ndora sector — MV and LV".into());
    let text = bindings(&layout, &sheet(true)).unwrap();
    assert_eq!(text["title"], "Ndora sector — MV and LV");
    assert!(text["source"].starts_with("INCOMPLETE INPUTS"));
}

#[test]
fn a_key_nobody_binds_refuses_but_renderer_keys_pass() {
    let mut layout = layout();
    let mut element = layout.elements[0].clone();
    element.id = "client".into();
    element.kind = ElementKind::Text;
    element.text = "{client} · {scale} · {admin_subtitle}".into();
    layout.elements.push(element);
    let error = bindings(&layout, &sheet(false)).unwrap_err();
    assert!(error.starts_with("print_text_missing"), "{error}");
    assert!(error.contains("{client}"), "{error}");
    layout.title_block.insert("client".into(), "EDCL".into());
    assert_eq!(bindings(&layout, &sheet(false)).unwrap()["client"], "EDCL");
}

#[test]
fn without_a_sealed_caption_the_project_line_must_be_authored() {
    let mut layout = layout();
    let uncaptioned = Sheet {
        caption: None,
        ..sheet(false)
    };
    let error = bindings(&layout, &uncaptioned).unwrap_err();
    assert!(error.contains("{project}"), "{error}");
    layout
        .title_block
        .insert("project".into(), "Aurora Rural Electrification".into());
    assert_eq!(
        bindings(&layout, &uncaptioned).unwrap()["project"],
        "Aurora Rural Electrification"
    );
}
