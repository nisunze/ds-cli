//! Transient Canvas computation left the native CLI (47583283): it belongs to
//! ds-interactions in the window, so `ds map canvas` is retired, and retired
//! means unreachable rather than a second name that still works.
mod common;
#[test]
fn retired_canvas_camera_is_not_a_command() {
    let run = common::invoke(&[
        "map",
        "canvas",
        "camera",
        "--request",
        "/nonexistent.json",
        "--output",
        "json",
    ]);
    assert_ne!(run.code, 0, "{}", run.stdout);
    assert!(run.stdout.contains("unknown_command"), "{}", run.stdout);
    let help = common::invoke(&["map", "--help"]);
    assert!(!help.stdout.contains("canvas"), "{}", help.stdout);
}
