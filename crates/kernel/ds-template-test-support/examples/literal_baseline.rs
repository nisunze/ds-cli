//! Explicit review tool. Redirect stdout to a repo's checked-in fixture baseline.
fn main() {
    let repo = std::env::args_os()
        .nth(1)
        .expect("usage: literal_baseline REPO");
    println!(
        "{}",
        serde_json::to_string_pretty(&ds_template_test_support::literal_baseline(
            std::path::Path::new(&repo)
        ))
        .unwrap()
    );
}
