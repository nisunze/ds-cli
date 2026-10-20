use super::*;

fn parse(tokens: &[&str]) -> Inputs {
    let tokens: Vec<String> = tokens.iter().map(|token| (*token).to_string()).collect();
    ds_cli_contract::parse(&COMMAND, &tokens).expect("declared inputs")
}

#[test]
fn a_missing_empty_or_mistyped_file_is_refused_before_any_network() {
    let root = std::env::temp_dir().join(format!("pm-attach-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let empty = root.join("empty.pdf");
    std::fs::write(&empty, b"").unwrap();
    let good = root.join("plan.pdf");
    std::fs::write(&good, b"%PDF-1.7").unwrap();
    for (file, content_type) in [
        (root.join("absent.pdf"), None),
        (empty.clone(), None),
        (root.clone(), None),
        (good.clone(), Some("pdf")),
    ] {
        let file = file.to_str().unwrap().to_owned();
        let mut tokens = vec!["--project", "p", "--task", "t", "--file", file.as_str()];
        if let Some(content_type) = content_type {
            tokens.extend(["--content-type", content_type]);
        }
        let error = attachment(&parse(&tokens)).unwrap_err();
        assert_eq!(error.code(), FILE_INVALID.code, "{file}");
    }
    let file = good.to_str().unwrap();
    let attached =
        attachment(&parse(&["--project", "p", "--task", "t", "--file", file])).unwrap();
    assert_eq!(attached.name(), "plan.pdf");
    assert_eq!(attached.content_type(), "application/pdf");
    assert_eq!(attached.len(), 8);
    let _ = std::fs::remove_dir_all(&root);
}
