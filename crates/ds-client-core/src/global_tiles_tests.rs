use super::*;

#[test]
fn every_action_sends_the_keys_the_backend_reads() {
    let catalog = Command::Catalog {
        domain: Domain::SolarTemplate,
        search: Some("roads".into()),
        page_size: 50,
        page_token: None,
    };
    let payload = catalog.payload();
    assert_eq!(payload["action"], "global_catalog");
    assert_eq!(payload["domain"], "solar_template");
    assert_eq!(payload["gcp_project"], DEFAULT_GCP_PROJECT);
    assert_eq!(payload["page_size"], 50);
    assert_eq!(payload["search"], "roads");
    assert!(payload["page_token"].is_null());

    // A global action never carries a project: the catalog has none, and a
    // project id on this wire would be a fence nothing enforces.
    for command in [
        &catalog,
        &Command::List {
            domain: Domain::NetworkTemplate,
        },
        &Command::Status {
            tile_id: "tile-1".into(),
        },
        &Command::Access {
            tile_id: "tile-1".into(),
            visibility: Visibility::Everyone,
            allowed: Allowlists::default(),
        },
    ] {
        let payload = command.payload();
        let object = payload.as_object().expect("payload is an object");
        assert!(
            !object.contains_key("eds_project_id") && !object.contains_key("project_id"),
            "{} carries a project",
            command.action()
        );
    }
}

#[test]
fn hiding_a_publication_states_who_still_reads_it() {
    let access = |visibility: Visibility, allowed: Allowlists| Command::Access {
        tile_id: "rwanda_data_elementary_schools".into(),
        visibility,
        allowed,
    };
    let restricted = Allowlists {
        users: vec!["operator@example.com".into()],
        app_roles: vec!["platform_admin".into()],
        ..Allowlists::default()
    };
    assert!(access(Visibility::Restricted, restricted.clone())
        .validate()
        .is_ok());
    // The whole point of the lever: restricted with nobody named leaves the
    // archive and the publication intact and stops the product mounting it.
    assert!(access(Visibility::Restricted, Allowlists::default())
        .validate()
        .is_ok());
    assert!(
        access(Visibility::Everyone, restricted).validate().is_err(),
        "an allowlist under `all` records a grant nothing reads"
    );
    for bad in [
        Allowlists {
            users: vec!["operator".into()],
            ..Allowlists::default()
        },
        Allowlists {
            users: vec!["operator@example.com".into(), "Operator@Example.com".into()],
            ..Allowlists::default()
        },
        Allowlists {
            projects: vec!["two words".into()],
            ..Allowlists::default()
        },
        Allowlists {
            app_roles: (0..=MAX_ALLOWED).map(|n| format!("role{n}")).collect(),
            ..Allowlists::default()
        },
    ] {
        assert!(
            access(Visibility::Restricted, bad).validate().is_err(),
            "an unusable allowlist was accepted"
        );
    }

    let payload = access(
        Visibility::Restricted,
        Allowlists {
            projects: vec!["czgmdwth_gisagara".into()],
            ..Allowlists::default()
        },
    )
    .payload();
    assert_eq!(payload["action"], "global_access");
    assert_eq!(payload["tile_id"], "rwanda_data_elementary_schools");
    assert_eq!(payload["access_policy"]["mode"], "restricted");
    assert_eq!(payload["access_policy"]["projects"][0], "czgmdwth_gisagara");
    // Every list travels, empty included, or a dropped grant would be kept
    // by the document this call is supposed to replace.
    for list in ["users", "app_roles", "project_roles"] {
        assert_eq!(
            payload["access_policy"][list],
            json!([]),
            "{list} did not travel"
        );
    }
    assert_eq!(Visibility::parse("all").unwrap(), Visibility::Everyone);
    assert!(Visibility::parse("hidden").is_err());
}

#[test]
fn only_access_writes() {
    assert!(!Command::List {
        domain: Domain::NetworkTemplate
    }
    .writes());
    assert!(!Command::Status {
        tile_id: "t".into()
    }
    .writes());
    assert!(Command::Access {
        tile_id: "t".into(),
        visibility: Visibility::Restricted,
        allowed: Allowlists::default(),
    }
    .writes());
}
