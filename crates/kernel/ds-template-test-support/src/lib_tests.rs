use super::*;
#[test]
fn literal_ratchet_requires_same_change_debt_reduction_and_reports_additions() {
    let first = LiteralOccurrence::new(Path::new("src/a.rs"), 3, "fixture-token", "first");
    let second = LiteralOccurrence::new(Path::new("src/a.rs"), 9, "fixture-token", "second");
    let baseline = LiteralBaseline {
        count: 1,
        occurrences: vec![first.clone()],
    };
    assert!(ratchet_diagnostics(&baseline, std::slice::from_ref(&first)).is_empty());
    let mut moved = first.clone();
    moved.line = 100;
    assert!(ratchet_diagnostics(&baseline, &[moved]).is_empty());
    // Path is information: the same literal in another file or crate is not new.
    let relocated = LiteralOccurrence::new(
        Path::new("other-crate/src/b.rs"),
        7,
        "fixture-token",
        "first",
    );
    assert!(ratchet_diagnostics(&baseline, std::slice::from_ref(&relocated)).is_empty());
    let added = ratchet_diagnostics(&baseline, &[first.clone(), second.clone()]);
    assert!(
        added
            .iter()
            .any(|e| e.contains("new occurrence:") && e.contains("src/a.rs:9")),
        "{added:?}"
    );
    // A second copy of an admitted literal lists every location it has.
    let copied = ratchet_diagnostics(&baseline, &[first.clone(), relocated]);
    assert!(
        copied.iter().any(|e| e.contains("count 2 exceeds 1")
            && e.contains("src/a.rs:3")
            && e.contains("other-crate/src/b.rs:7")),
        "{copied:?}"
    );
    assert!(!ratchet_diagnostics(&baseline, &[]).is_empty());
    let lowered = LiteralBaseline {
        count: 0,
        occurrences: vec![],
    };
    assert!(ratchet_diagnostics(&lowered, &[]).is_empty());
    // A replacement at the same total is still a newly introduced literal.
    assert!(
        ratchet_diagnostics(&baseline, &[second])
            .iter()
            .any(|e| e.contains("new occurrence"))
    );
    // Identical literals on one line are separate occurrences, never deduped.
    assert!(
        ratchet_diagnostics(&baseline, &[first.clone(), first])
            .iter()
            .any(|e| e.contains("new occurrence"))
    );
    let forged = LiteralBaseline {
        count: 0,
        occurrences: baseline.occurrences,
    };
    assert!(
        ratchet_diagnostics(&forged, &[])
            .iter()
            .any(|e| e.contains("disagrees"))
    );
}
#[test]
fn source_ratchet_excludes_root_job_copies_but_scans_nested_source_out() {
    let root = std::path::PathBuf::from(std::env::var_os("TMPDIR").unwrap()).join(format!(
        "literal-job-copies-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    for file in [
        "src/lib.rs",
        "src/out/real.rs",
        "out/gate/source/src/lib.rs",
        "_discardable/gate/src/lib.rs",
    ] {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "pub const LABEL: &str = \"S140\";\n").unwrap();
    }
    let found = source_occurrences(&root);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found.iter().all(|row| row.path.starts_with("src/")),
        "{found:?}"
    );
    assert!(
        found.iter().any(|row| row.path == "src/out/real.rs"),
        "{found:?}"
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn historical_ratchet_rejects_raised_working_and_committed_baselines() {
    let root = std::env::var_os("TMPDIR").expect("tests require explicit TMPDIR");
    let repo = std::path::PathBuf::from(root).join(format!(
        "literal-ratchet-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(repo.join("src")).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let path = Path::new("baseline.json");
    let save = || {
        std::fs::write(
            repo.join(path),
            serde_json::to_vec_pretty(&literal_baseline(&repo)).unwrap(),
        )
        .unwrap()
    };
    git(&["init", "--quiet"]);
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub const LABEL: &str = \"Residential\";\n",
    )
    .unwrap();
    save();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "initial baseline"]);
    assert_source_ratchet(&repo, path);
    // A depth-one checkout cannot establish a historical ceiling.
    git(&[
        "commit",
        "--quiet",
        "--allow-empty",
        "-m",
        "history predecessor",
    ]);
    let shallow = repo.with_file_name(format!(
        "{}-shallow",
        repo.file_name().unwrap().to_str().unwrap()
    ));
    let cloned = std::process::Command::new("git")
        .args(["clone", "--quiet", "--depth", "1", "--no-local"])
        .arg(&repo)
        .arg(&shallow)
        .output()
        .unwrap();
    assert!(
        cloned.status.success(),
        "{}",
        String::from_utf8_lossy(&cloned.stderr)
    );
    let rejection = std::panic::catch_unwind(|| assert_source_ratchet(&shallow, path)).unwrap_err();
    let message = rejection
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| rejection.downcast_ref::<&str>().copied())
        .expect("guard panic message");
    assert!(message.contains("requires full Git history"), "{message}");
    std::fs::remove_dir_all(shallow).unwrap();

    std::fs::write(
        repo.join("src/lib.rs"),
        "pub const LABEL: &str = \"S140\";\n",
    )
    .unwrap();
    save();
    assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "forged replacement"]);
    assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub const LABEL: &str = \"fixture-token\";\n",
    )
    .unwrap();
    assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
    save();
    assert_source_ratchet(&repo, path);
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "lower baseline"]);
    assert_source_ratchet(&repo, path);
    std::fs::write(
        repo.join("src/lib.rs"),
        "pub const LABEL: &str = \"Residential\";\n",
    )
    .unwrap();
    save();
    assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "forged increase"]);
    assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
    // Formatting-only commits must not reset an older, lower ceiling.
    std::fs::write(
        repo.join(path),
        serde_json::to_vec(&literal_baseline(&repo)).unwrap(),
    )
    .unwrap();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "format forged baseline"]);
    assert!(std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).is_err());
    std::fs::remove_dir_all(repo).unwrap();
}
#[test]
fn simultaneous_transform_is_bijective_and_preserves_shape() {
    let config = fixture_config();
    let alien = Alien::from_template(&config);
    assert!(alien.forward.len() > 25);
    assert!(alien.forward.values().all(|v| v.len() <= 64));
    assert_eq!(alien.restore(&alien.apply(&config)), config);
    assert_eq!(alien.restore(&alien.apply(&fixture_job())), fixture_job());
    let loads = serde_json::json!({format!("{}, 173",config["cust_category"][0]["clean_name"].as_str().unwrap()):2});
    assert_eq!(alien.restore(&alien.apply(&loads)), loads);
    assert_ne!(alien.apply(&loads), loads);
    assert!(alien.forward.values().all(|v| !v.is_ascii()));
}
#[test]
fn the_bijection_commutes_with_unicode_case_normalization() {
    let template = serde_json::json!({"cust_category":[{"clean_name":"Résidentiel","aliases":["RÉSIDENTIEL","résidentiel"]}]});
    let alien = Alien::from_template(&template);
    for original in alien.forward.keys() {
        assert_eq!(
            alien.rename(&original.to_lowercase()),
            alien.rename(original).to_lowercase()
        );
    }
    assert_eq!(alien.restore(&alien.apply(&template)), template);
    assert_eq!(
        alien.forward.len(),
        alien.forward.values().collect::<BTreeSet<_>>().len()
    );
}

#[test]
fn alien_category_ids_scramble_the_fixture_lexical_order() {
    let config = fixture_config();
    let alien = Alien::from_template(&config);
    let mut original: Vec<_> = config["cust_category"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["clean_name"].as_str().unwrap().to_owned())
        .collect();
    original.sort();
    let mut reordered = original.clone();
    reordered.sort_by_key(|name| alien.rename(name));
    assert_ne!(
        original, reordered,
        "the control must expose name-order defaults"
    );
    for name in reordered {
        assert_eq!(alien.restore(&Value::String(alien.rename(&name))), name);
    }
}

#[test]
fn encoded_sheet_cells_collect_and_rename_vocabulary_without_format_drift() {
    let template = serde_json::json!({"transfo_sizes":[{"electrical_params":"{ \"vector_group\": \"Groupe Ω\", \"vk_percent\": 4.70 }"}]});
    let alien = Alien::from_template(&template);
    assert!(alien.forward.contains_key("Groupe Ω"));
    assert_ne!(alien.apply(&template), template);
    assert_eq!(alien.restore(&alien.apply(&template)), template);
}

#[test]
fn alias_cell_encodings_preserve_their_delimiters_and_references() {
    let template = serde_json::json!({"cust_category":[
        {"clean_name":"Catégorie Ω","aliases":"  [ \"résumé\", \"住宅\" ]  "},
        {"clean_name":"Catégorie β","misspelled_names":"ancien Ω##ancien β"}
    ]});
    let project = serde_json::json!({"category":"résumé", "another_category":"ancien β"});
    let alien = Alien::from_template(&template);
    let translated = alien.apply(&template);
    assert!(
        serde_json::from_str::<Value>(translated["cust_category"][0]["aliases"].as_str().unwrap())
            .unwrap()
            .is_array()
    );
    assert_eq!(
        translated["cust_category"][1]["misspelled_names"]
            .as_str()
            .unwrap()
            .split("##")
            .count(),
        2
    );
    assert_ne!(alien.apply(&project), project);
    assert_eq!(alien.restore(&translated), template);
    assert_eq!(alien.restore(&alien.apply(&project)), project);
}

#[test]
fn vocabulary_collisions_never_rename_schema_or_lifecycle_wire_values() {
    let template = serde_json::json!({
        "cust_category":[{"clean_name":"existing", "aliases":["category", "service", "1", "calculated"]}],
        "lv_poles_rules":{"table":{"table":[], "voltage_drop":{"standard":"existing"}}}
    });
    let project = serde_json::json!({"gdfs":{"service_cables":{"features":[{"properties":{
        "category":"category", "existing_status":"existing", "connection_phase":"1", "layer_name":"service_cables"
    }}]}}, "status":"calculated", "initial_load_w":{"existing":211}});
    let alien = Alien::from_template(&template);
    let translated = alien.apply(&project);
    let props = &translated["gdfs"]["service_cables"]["features"][0]["properties"];
    assert_eq!(props["category"], alien.rename("category"));
    assert_eq!(props["existing_status"], "existing");
    assert_eq!(props["connection_phase"], "1");
    assert_eq!(props["layer_name"], "service_cables");
    assert_eq!(translated["status"], "calculated");
    assert_eq!(translated["initial_load_w"][alien.rename("existing")], 211);
    let renamed = alien.apply(&template);
    assert!(renamed["lv_poles_rules"][alien.rename("table")]["table"].is_array());
    assert_eq!(alien.restore(&renamed), template);
    assert_eq!(alien.restore(&translated), project);
}

#[test]
fn encoded_vocabulary_maps_keep_the_same_context_and_exact_cell_bytes() {
    let template = serde_json::json!({"cust_category":[{"clean_name":"table", "aliases":["status", "calculated"]}],
        "lv_poles_rules":{"rule Ω":{"voltage_drop":" { \"category_loads\": { \"table\": { \"initial_w\": 173.0, \"saturation_w\": 611.00 } }, \"status\": \"calculated\" } "}}});
    let mut template = template;
    template["category_loads"] =
        serde_json::json!(" { \"extérieur Ω\": { \"initial_w\": 211.0 } } ");
    let alien = Alien::from_template(&template);
    assert!(alien.forward.contains_key("extérieur Ω"));
    let translated = alien.apply(&template);
    let encoded: Value =
        serde_json::from_str(translated["category_loads"].as_str().unwrap()).unwrap();
    assert_eq!(encoded[alien.rename("extérieur Ω")]["initial_w"], 211.0);
    let cell = translated["lv_poles_rules"][alien.rename("rule Ω")]["voltage_drop"]
        .as_str()
        .unwrap();
    let decoded: Value = serde_json::from_str(cell).unwrap();
    assert_eq!(
        decoded["category_loads"][alien.rename("table")]["initial_w"],
        173.0
    );
    assert_eq!(decoded["status"], "calculated");
    assert!(cell.contains("173.0") && cell.contains("611.00"));
    assert_eq!(alien.restore(&translated), template);
    let project = serde_json::json!({"category_loads":" { \"table\": { \"initial_w\": 173.0 } } "});
    let renamed = alien.apply(&project);
    let decoded: Value = serde_json::from_str(renamed["category_loads"].as_str().unwrap()).unwrap();
    assert!(decoded.get(alien.rename("table")).is_some());
    assert_eq!(alien.restore(&renamed), project);
}

#[test]
fn authored_column_schema_survives_alias_collisions_in_values_and_encoded_json() {
    let template = serde_json::json!({
        "cust_category":[{"clean_name":"fixture Ω", "aliases":["category", "status", "service"]}],
        "know_columns":[{"customers":"category", "common":"status", "service_cables":"service"}],
        "customer":{"category":"category"}
    });
    let alien = Alien::from_template(&template);
    for value in [
        template.clone(),
        Value::String(serde_json::to_string(&template).unwrap()),
    ] {
        let translated = alien.apply(&value);
        let decoded = match &translated {
            Value::String(text) => serde_json::from_str::<Value>(text).unwrap(),
            value => value.clone(),
        };
        assert_eq!(decoded["know_columns"], template["know_columns"]);
        assert_eq!(decoded["customer"]["category"], alien.rename("category"));
        assert_eq!(alien.restore(&translated), value);
    }
}

#[test]
fn scanner_ignores_tests_and_comments_but_finds_production_macros() {
    let dir = std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("disk TMPDIR required"))
        .join(format!("template-guard-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(
        dir.join("src/lib.rs"),
        r#"// Residential
/// Residential documentation is allowed, including Rust's lowered doc attribute.
#[cfg(test)] mod tests { fn fixture(){ let _ = "Residential"; } }
#[cfg(all(test, feature="test-helper"))] mod deep_tests { const NAME: &str = "Residential"; }
fn real() { let _=serde_json::json!({"category":"Residential"}); let _="S140 / S190 / S255"; let _="nonResidentialSuffix"; }
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("assets/labels.json"),
        r#"{"label":"\u0052esidential"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("assets/print.html"),
        "<!-- \"Residential\" -->\n<p>Residential / S140</p>",
    )
    .unwrap();
    let violations = source_violations(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(
        violations
            .iter()
            .filter(|v| v.contains("Residential"))
            .count(),
        3,
        "{violations:?}"
    );
    for token in ["S140", "S190", "S255"] {
        assert!(
            violations.iter().any(|v| v.contains(token)),
            "{violations:?}"
        );
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("TMPDIR").expect("tests require explicit TMPDIR"))
        .join(format!(
            "{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
}
fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn crate_group_scan_covers_every_crate_with_group_relative_paths() {
    let root = scratch("literal-crate-group");
    const LABEL: &str = "pub const LABEL: &str = \"S140\";\n";
    for file in [
        "kernel/src/lib.rs",
        "kernel/src/out/real.rs",
        "kernel/out/job/src/lib.rs",
        "kernel/_discardable/job/src/lib.rs",
        "kernel/examples/src/demo.rs",
        "kernel/benches/src/bench.rs",
        "kernel/tests/src/it.rs",
        "kernel/backlog/src/note.rs",
        "kernel/src/thing_tests.rs",
        "store/src/lib.rs",
        "ds-template-test-support/src/lib.rs",
    ] {
        write(&root, file, LABEL);
    }
    write(&root, "kernel/Cargo.toml", "");
    write(&root, "store/Cargo.toml", "");
    let found = source_occurrences(&root);
    std::fs::remove_dir_all(&root).unwrap();
    let paths: Vec<&str> = found.iter().map(|row| row.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "kernel/src/lib.rs",
            "kernel/src/out/real.rs",
            "store/src/lib.rs"
        ]
    );
}

#[test]
fn group_ratchet_follows_moved_literals_and_refuses_planted_ones() {
    let repo = scratch("literal-group-history");
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    // The baseline lives in one crate and accounts for the whole group.
    let path = Path::new("kernel/tests/fixtures/baseline.json");
    let save = || {
        write(
            &repo,
            path.to_str().unwrap(),
            &serde_json::to_string_pretty(&literal_baseline(&repo)).unwrap(),
        )
    };
    let refusal = || {
        let panic = std::panic::catch_unwind(|| assert_source_ratchet(&repo, path)).unwrap_err();
        panic.downcast_ref::<String>().cloned().unwrap_or_default()
    };
    write(&repo, "kernel/Cargo.toml", "");
    write(&repo, "store/Cargo.toml", "");
    write(
        &repo,
        "kernel/src/lib.rs",
        "pub const LABEL: &str = \"Residential\";\n",
    );
    write(&repo, "store/src/lib.rs", "pub const OTHER: u8 = 1;\n");
    git(&["init", "--quiet"]);
    save();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "group baseline"]);
    assert_source_ratchet(&repo, path);
    // Moving the literal to a sibling crate is not an addition, before or
    // after the inventory's informational paths are refreshed and committed.
    write(&repo, "kernel/src/lib.rs", "pub const OTHER: u8 = 2;\n");
    write(
        &repo,
        "store/src/lib.rs",
        "pub const LABEL: &str = \"Residential\";\n",
    );
    assert_source_ratchet(&repo, path);
    save();
    assert_source_ratchet(&repo, path);
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "move the literal"]);
    assert_source_ratchet(&repo, path);
    // A planted copy fails against the scan and, once recorded, against history.
    write(
        &repo,
        "store/src/planted.rs",
        "pub const PLANTED: &str = \"Residential\";\n",
    );
    let message = refusal();
    assert!(message.contains("new occurrence:"), "{message}");
    assert!(message.contains("store/src/planted.rs:1"), "{message}");
    save();
    let message = refusal();
    assert!(message.contains("historical baseline admits"), "{message}");
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "forged planted literal"]);
    assert!(refusal().contains("historical baseline admits"));
    std::fs::remove_dir_all(repo).unwrap();
}

#[test]
fn json_member_names_are_wire_names_but_json_values_still_count() {
    let dir = scratch("template-json-members");
    write(
        &dir,
        "src/lib.rs",
        r#"
fn wire() -> serde_json::Value {
    let _ = serde_json::json!({"exists": false, "nested": {"service": 1}, "rows": [{"exists": true}]});
    let _ = vec![json!({"exists": null})];
    let _ = json!({"kind": "service"});
    let _ = json!(["exists"]);
    let _ = other!({"exists": 1});
    let _ = json!({"a": format!("{}", "Residential")});
    json!({"a": tagged!{"service": 1}})
}
"#,
    );
    let found = source_occurrences(&dir);
    std::fs::remove_dir_all(&dir).unwrap();
    let count = |token: &str| found.iter().filter(|row| row.token == token).count();
    // Members of every `json!` object, however nested, are wire names.
    // A `json!` value or array element, another macro's members and a nested
    // non-json macro inside `json!` still count.
    assert_eq!(count("exists"), 2, "{found:?}");
    assert_eq!(count("service"), 2, "{found:?}");
    assert_eq!(count("Residential"), 1, "{found:?}");
}
