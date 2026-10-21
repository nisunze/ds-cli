//! Comparison must reject bad local inputs before any public acquisition.

use super::native_ds;

#[test]
fn elevation_compare_preserves_files_and_validates_coordinates_before_network() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("points.csv");
    let out = root.path().join("comparison.csv");
    std::fs::write(&source, "id,x,y\npole-a,29.75,-2.62\n").unwrap();
    std::fs::write(&out, "existing evidence\n").unwrap();
    let invoke = || {
        native_ds(&[
            "data",
            "elevation",
            "compare",
            "--source",
            source.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--output",
            "json",
        ])
    };
    let existing = invoke();
    assert_ne!(existing.code, 0);
    assert_eq!(
        existing.envelope["error"]["code"],
        "elevation_output_exists"
    );
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "existing evidence\n"
    );

    std::fs::remove_file(&out).unwrap();
    std::fs::write(&source, "id,x,y\npole-a,NaN,-2.62\n").unwrap();
    let invalid = invoke();
    assert_eq!(invalid.envelope["error"]["code"], "elevation_invalid_input");
    assert!(!out.exists());

    std::fs::write(
        &source,
        "id,x,y,rwanda_elevation_m\npole-a,29.75,-2.62,100\n",
    )
    .unwrap();
    let collision = invoke();
    assert_eq!(
        collision.envelope["error"]["code"],
        "elevation_output_column_collision"
    );
    assert!(!out.exists());

    let described = native_ds(&["capabilities", "data.elevation.compare", "--output", "json"]);
    assert_eq!(described.envelope["data"]["command"]["authority"], "none");
    assert_eq!(
        described.envelope["data"]["command"]["availability"],
        "available"
    );
}
