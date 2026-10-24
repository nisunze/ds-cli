use super::*;

#[test]
fn progress_line_keeps_the_fixed_grammar_older_shells_parse() {
    assert_eq!(
        progress_line(
            UiComponent::CommandKernel,
            "downloading",
            56,
            12_400_000,
            22_100_000
        ),
        "command-kernel: downloading 56% (12400000/22100000)"
    );
}

#[test]
fn version_travels_on_its_own_line_and_hostile_text_is_refused() {
    assert_eq!(
        version_line(UiComponent::CommandKernel, "0.1.0-r8").as_deref(),
        Some("command-kernel: version 0.1.0-r8")
    );
    for bad in ["", "0.1 0", "r8\nfake: x", "a".repeat(65).as_str()] {
        assert!(
            version_line(UiComponent::Interactions, bad).is_none(),
            "{bad:?}"
        );
    }
}

#[test]
fn pace_passes_step_changes_and_the_end_but_spaces_the_rest() {
    let t0 = Instant::now();
    let ms = |n| t0 + Duration::from_millis(n);
    let mut pace = Pace::new();
    assert!(pace.admit("checking", 0, 0, ms(0)));
    assert!(pace.admit("downloading", 0, 10, ms(1)));
    assert!(!pace.admit("downloading", 1, 10, ms(10)));
    assert!(!pace.admit("downloading", 2, 10, ms(200)));
    assert!(pace.admit("downloading", 3, 10, ms(300)));
    assert!(pace.admit("downloading", 10, 10, ms(310)));
    assert!(pace.admit("verifying", 10, 10, ms(311)));
}
