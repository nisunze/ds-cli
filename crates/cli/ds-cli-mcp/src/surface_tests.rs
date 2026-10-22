#[test]
fn member_form_grant_workflow_is_on_the_existing_project_survey_profile() {
    for id in [
        "survey.member-grant.read",
        "survey.member-grant.plan",
        "survey.member-grant.apply",
    ] {
        assert!(
            super::Profile::SurveyProjects.command_ids().contains(&id),
            "missing {id}"
        );
    }
}

use super::*;

#[test]
fn mcp_advertises_account_connect_for_every_exposure() {
    for (exposure, profile) in [
        (Exposure::Chapters, None),
        (Exposure::Commands, None),
        (Exposure::Commands, Some(Profile::AuthContext)),
    ] {
        let surface = Surface::new(exposure, profile, vec![]).expect("surface");
        let instructions = surface.instructions();
        assert!(
            instructions.contains("ds account connect"),
            "{instructions}"
        );
        assert!(instructions.contains("account.connect"), "{instructions}");
        assert!(
            instructions.contains("Link a trusted device"),
            "{instructions}"
        );
        assert!(!names_terminal_sign_in(&instructions), "{instructions}");
    }
}

#[test]
fn mcp_guides_project_work_and_regression_triage_for_every_exposure() {
    for (exposure, profile) in [
        (Exposure::Chapters, None),
        (Exposure::Commands, None),
        (Exposure::Commands, Some(Profile::Project)),
        (Exposure::Commands, Some(Profile::Correspondence)),
    ] {
        let instructions = Surface::new(exposure, profile, vec![])
            .expect("surface")
            .instructions();
        for expected in [
            "ds-project-work",
            "signed-in user",
            "exact project",
            "pm.task",
            "verified completion",
            "verify asset links",
            "next condition",
            "notification reached",
            "negative control",
            "unclear CLI/MCP guidance",
            "official feedback",
            "ds-printout",
            "governed A3 landscape cover and naming template",
            "project-owned parties",
            "proved member mapping",
            "exact placed native names",
            "inaccessible through the declared DS surface",
            "do not redraw a cover",
            "atomic booklet job",
        ] {
            assert!(
                instructions.contains(expected),
                "missing {expected}: {instructions}"
            );
        }
        assert!(!PROJECT_WORK_GUIDANCE.contains("feedback"));
        assert!(REGRESSION_GUIDANCE.contains("official feedback"));
    }
}

#[test]
fn mcp_teaches_unprompted_feedback_outside_coding_for_every_exposure() {
    for (exposure, profile) in [
        (Exposure::Chapters, None),
        (Exposure::Commands, None),
        (Exposure::Commands, Some(Profile::AuthContext)),
        (Exposure::Commands, Some(Profile::Project)),
    ] {
        let instructions = Surface::new(exposure, profile, vec![])
            .expect("surface")
            .instructions();
        for expected in [
            "Report as you go",
            "feedback.submit",
            "unasked",
            "second search",
            "slow",
            "idea",
            "list the report ids",
            "never secrets or customer data",
            "Coding sessions never submit feedback",
            "owning backlogs",
        ] {
            assert!(
                instructions.contains(expected),
                "missing {expected}: {instructions}"
            );
        }
        assert!(!names_terminal_sign_in(&instructions), "{instructions}");
    }
}

#[test]
fn mcp_scrubs_terminal_sign_in_advice_from_every_advisory_shape() {
    let mut descriptor = json!({
        "data": {"command": {
            "id": "x.y",
            "purpose": "Exchanges an email and hidden TTY password for a session.",
            "inputs": [{"name": "email", "summary": "Pass --email <address>."}],
            "examples": [{"command": "ds auth login --email a@b", "note": "Prompts for a password."}],
            "refusals": [{
                "code": "headless_signed_out",
                "when": "no password session",
                "remedy": "run ds auth login --email <address>"
            }]
        }}
    });
    mcp_device_link_guidance(&mut descriptor);
    let command = &descriptor["data"]["command"];
    assert_eq!(command["refusals"][0]["remedy"], DEVICE_LINK_REMEDY);
    assert_eq!(command["refusals"][0]["when"], DEVICE_LINK_REMEDY);
    assert_eq!(command["purpose"], DEVICE_LINK_REMEDY);
    assert_eq!(command["inputs"][0]["summary"], "Pass --email <address>.");
    assert_eq!(command["examples"][0]["command"], DEVICE_LINK_REMEDY);
    assert_eq!(command["examples"][0]["note"], DEVICE_LINK_REMEDY);
    assert!(!names_terminal_sign_in(&descriptor.to_string()));

    let mut error = json!({
        "error": {
            "code": "headless_signed_out",
            "message": "this command needs the password session",
            "remedy": "run ds auth login --email <address>",
            "next": ["ds auth login --email <address>", "ds auth status"]
        },
        "data": {"next": "ds auth login --email <address>"}
    });
    mcp_device_link_guidance(&mut error);
    assert_eq!(error["error"]["remedy"], DEVICE_LINK_REMEDY);
    assert_eq!(error["error"]["next"][0], DEVICE_LINK_NEXT);
    assert_eq!(error["error"]["next"][1], "ds auth status");
    assert_eq!(error["data"]["next"], DEVICE_LINK_NEXT);
    assert!(
        error["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("sign-in is needed")
    );
    assert!(!names_terminal_sign_in(&error.to_string()));
}

#[test]
fn mcp_scrub_leaves_facts_and_unrelated_advice_alone() {
    // A feedback report quoting a person's complaint is a fact, not
    // advice; the scrub must not rewrite it. And advice that never
    // mentions a terminal sign-in is exactly what it was.
    let mut envelope = json!({
        "data": {
            "reports": [{"detail": "the remedy said ds auth login --email; a password prompt is intimidating"}],
            "next": "ds feedback list"
        },
        "error": {"remedy": "repair the package", "next": "ds doctor"}
    });
    let before = envelope.clone();
    mcp_device_link_guidance(&mut envelope);
    assert_eq!(envelope, before);
}

use ds_cli_contract::spec::Authority;

fn tool(id: &str, chapter: Chapter, confirmation_required: bool) -> Tool {
    Tool {
        name: tools::tool_name(id),
        id: id.to_string(),
        chapter,
        authority: Authority::None,
        effect: if confirmation_required {
            Effect::GlobalWrite
        } else {
            Effect::ReadOnly
        },
        execution: ds_cli_contract::spec::Execution::Sync,
        requires_window: false,
        path: id.split('.').map(str::to_string).collect(),
        description: format!("{id} purpose"),
        input_schema: json!({ "type": "object" }),
        confirmation_required,
        confirmation_trigger: None,
        preview_switch: None,
        inputs: Vec::new(),
        descriptor: json!({
            "id": id,
            "summary": format!("{id} summary"),
            "availability": "available",
            "effect": if confirmation_required { "global_write" } else { "read_only" }
        }),
    }
}

fn conditional_tool(id: &str, chapter: Chapter) -> Tool {
    let mut tool = tool(id, chapter, true);
    tool.effect = Effect::MachineWrite;
    tool.descriptor["effect"] = json!("machine_write");
    tool.confirmation_trigger = Some("write".to_string());
    tool.inputs.push(tools::Input {
        name: "write".to_string(),
        kind: "switch".to_string(),
    });
    tool
}

#[test]
fn broad_surface_is_exactly_the_declared_stable_chapter_tools() {
    let surface = Surface::new(
        Exposure::Chapters,
        None,
        vec![tool("map.profile.view", Chapter::MapPresentation, false)],
    )
    .expect("surface");
    let names = surface
        .tool_list()
        .into_iter()
        .map(|value| value["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    // Derived: the catalogue and diagnostics bootstrap plus one router
    // per routed chapter. A new chapter that nobody routed fails here.
    assert_eq!(names.len(), Chapter::ALL.len() + 1);
    assert_eq!(names[0], "ds_catalog");
    for chapter in Chapter::ALL {
        assert!(
            names.contains(&chapter_tool_name(*chapter).to_string()),
            "chapter `{chapter}` publishes no tool"
        );
    }
}

#[test]
fn every_declared_chapter_except_the_catalog_is_routed() {
    // F36: `ROUTED_CHAPTERS` is hand-maintained beside a declaration that
    // already enumerates every chapter. Adding a chapter and forgetting
    // this list makes its commands unreachable through MCP with nothing
    // failing, because the surface's own count matches the list, not the
    // registry.
    let expected: Vec<Chapter> = Chapter::ALL
        .iter()
        .copied()
        .filter(|chapter| *chapter != Chapter::Catalog)
        .collect();
    assert_eq!(
        ROUTED_CHAPTERS.to_vec(),
        expected,
        "`ROUTED_CHAPTERS` must be `Chapter::ALL` minus the catalogue, in declaration order"
    );
}

#[test]
fn profiles_are_typed_filtered_views_and_require_command_exposure() {
    let tools = vec![
        tool("map.profile.view", Chapter::MapPresentation, false),
        tool("tile.generate", Chapter::VectorTiles, true),
    ];
    let profile = Surface::new(
        Exposure::Commands,
        Some(Profile::GridProfile),
        tools.clone(),
    )
    .expect("profile");
    let names = profile
        .tool_list()
        .into_iter()
        .map(|value| value["name"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names, ["ds_catalog", "ds_diagnostics", "map_profile_view"]);
    let error = Surface::new(Exposure::Chapters, Some(Profile::GridProfile), tools).unwrap_err();
    assert_eq!(error.code(), "mcp_profile_exposure_invalid");
}

#[test]
fn local_model_profile_is_four_scoped_tools_with_exact_catalogue_fences() {
    let surface = Surface::new(
        Exposure::Commands,
        Some(Profile::GridLocalModel),
        vec![
            tool("dsgrid.model.list", Chapter::GridModel, false),
            tool("dsgrid.model.combine", Chapter::GridModel, false),
            tool("design.version.list", Chapter::Design, false),
            tool("dsgrid.inspect", Chapter::GridModel, false),
            tool("design.lv.process", Chapter::Design, true),
        ],
    )
    .unwrap();
    let names: Vec<_> = surface
        .tool_list()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        names,
        ["ds_catalog", "ds_diagnostics", "ds_grid_model", "ds_design"]
    );
    assert_eq!(surface.published_count(), 4);
    let executable = PathBuf::from("unused-catalogue-does-not-spawn");
    for (command, router) in [
        ("dsgrid.model.combine", "ds_grid_model"),
        ("design.version.list", "ds_design"),
    ] {
        let result = surface
            .call("ds_catalog", &json!({"command": command}), &executable)
            .unwrap();
        assert_eq!(result["structuredContent"]["next"]["tool"], router);
    }
    for command in ["dsgrid.inspect", "design.lv.process"] {
        assert!(
            surface
                .call("ds_catalog", &json!({"command": command}), &executable)
                .is_err()
        );
        assert!(
            surface
                .call(
                    "ds_grid_model",
                    &json!({"operation":"invoke", "command":command}),
                    &executable
                )
                .is_err()
        );
    }
    assert!(
        surface
            .call("dsgrid_model_combine", &json!({}), &executable)
            .is_err()
    );
    assert!(surface.call("ds_reports", &json!({}), &executable).is_err());
}

#[test]
fn correction_profile_exposes_guarded_batch_and_review_reads_only() {
    let tools = vec![
        tool("dsgrid.inspect", Chapter::GridModel, false),
        tool("dsgrid.run", Chapter::GridModel, false),
        tool("dsgrid.apply-correction", Chapter::GridModel, false),
        tool("dsgrid.apply-batch", Chapter::GridModel, false),
        tool("dsgrid.apply", Chapter::GridModel, false),
        tool("dsgrid-exchange.sync", Chapter::GridModel, false),
    ];
    let surface = Surface::new(Exposure::Commands, Some(Profile::GridCorrections), tools)
        .expect("bounded correction profile");
    let names: Vec<_> = surface
        .tool_list()
        .into_iter()
        .map(|value| value["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "ds_catalog",
            "ds_diagnostics",
            "dsgrid_apply-correction",
            "dsgrid_inspect",
            "dsgrid_run"
        ]
    );
}

#[test]
fn wrong_chapter_and_nested_confirmation_fail_closed() {
    let surface = Surface::new(
        Exposure::Chapters,
        None,
        vec![tool("tile.generate", Chapter::VectorTiles, true)],
    )
    .expect("surface");
    let executable = PathBuf::from("not-called");
    // Routing is the protocol's: the wrong router is a protocol error
    // naming the right one.
    let wrong = surface
        .call_chapter(
            Chapter::Survey,
            &json!({ "operation": "invoke", "command": "tile.generate", "arguments": {} }),
            &executable,
            &mut |_| {},
        )
        .unwrap_err();
    assert!(wrong.1.contains("ds_vector_tiles"), "{}", wrong.1);
    // Once the command is resolved, a misplaced confirmation is that
    // command's refusal, with a code and a remedy, and nothing runs.
    let nested = surface
            .call_chapter(
                Chapter::VectorTiles,
                &json!({ "operation": "invoke", "command": "tile.generate", "arguments": { "confirm": true } }),
                &executable,
                &mut |_| {},
            )
            .expect("a refusal is a tool result");
    assert_eq!(nested["isError"], true);
    let error = &nested["structuredContent"]["error"];
    assert_eq!(error["code"], "mcp_arguments_invalid");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("chapter envelope"),
        "{error}"
    );
    assert_eq!(error["remedy"], crate::ARGUMENTS_INVALID.remedy);
    assert_eq!(nested["structuredContent"]["command"], "tile.generate");
}

#[test]
fn conditional_tool_annotations_stay_conservative_and_preview_rejects_confirm() {
    let conditional = conditional_tool("operations.install", Chapter::Operations);
    assert_eq!(
        leaf_tool_json(&conditional)["annotations"]["readOnlyHint"],
        false
    );
    let surface = Surface::new(Exposure::Chapters, None, vec![conditional]).unwrap();
    let refused = surface
        .call_chapter(
            Chapter::Operations,
            &json!({
                "operation": "invoke",
                "command": "operations.install",
                "arguments": { "write": false },
                "confirm": true,
            }),
            &PathBuf::from("not-called"),
            &mut |_| {},
        )
        .expect("a refusal is a tool result");
    let error = &refused["structuredContent"]["error"];
    assert_eq!(error["code"], "mcp_arguments_invalid");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("does not accept confirmation for this invocation"),
        "{error}"
    );
}

#[test]
fn non_confirming_local_ui_tools_are_not_annotated_read_only() {
    let mut local_ui = tool("data.admin-bounds.read", Chapter::Data, false);
    local_ui.effect = Effect::LocalUi;
    local_ui.descriptor["effect"] = json!("local_ui");
    assert_eq!(
        leaf_tool_json(&local_ui)["annotations"]["readOnlyHint"],
        false
    );
}

/// An answer above the bound is trimmed where it is long — its largest
/// array — and stays one envelope a host can branch on, with the trim
/// reported in `more` rather than hidden.
#[test]
fn an_oversized_answer_is_trimmed_to_a_well_formed_envelope_and_says_so() {
    let rows: Vec<Value> = (0..5_000)
        .map(|index| json!({ "id": index, "text": "x".repeat(100) }))
        .collect();
    let mut envelope = json!({
        "v": 1, "command": "x.list", "contract": 1, "status": "ok",
        "data": { "count": 5_000, "rows": rows, "small": [1, 2, 3] },
    });
    let original = serde_json::to_vec(&envelope).unwrap().len();
    bound_envelope(&mut envelope, 64 * 1024);
    let bounded = serde_json::to_vec(&envelope).unwrap().len();
    assert!(bounded <= 64 * 1024, "{bounded}");
    assert_eq!(envelope["status"], "ok");
    assert_eq!(envelope["data"]["count"], 5_000);
    assert_eq!(envelope["data"]["small"], json!([1, 2, 3]));
    let kept = envelope["data"]["rows"].as_array().unwrap().len();
    assert!(kept > 0 && kept < 5_000, "{kept}");
    let note = &envelope["more"]["mcp_truncation"];
    assert_eq!(note["original_bytes"], original);
    assert_eq!(note["arrays"][0]["pointer"], "/data/rows");
    assert_eq!(note["arrays"][0]["kept"], kept);
    assert_eq!(note["arrays"][0]["total"], 5_000);
    assert_eq!(note["remedy"], crate::RESULT_TOO_LARGE.remedy);

    // A CLI continuation already in `more` is kept beside the note.
    let mut continued = json!({
        "status": "ok", "data": { "rows": vec!["y".repeat(1_000); 100] },
        "more": { "next": "ds x list --after 100" },
    });
    bound_envelope(&mut continued, 8 * 1024);
    assert_eq!(continued["more"]["next"], "ds x list --after 100");
    assert!(continued["more"]["mcp_truncation"].is_object());

    // One oversized value with no array to shorten is withheld, and the
    // envelope still says what happened.
    let mut blob = json!({ "status": "ok", "data": { "blob": "z".repeat(100_000) } });
    bound_envelope(&mut blob, 8 * 1024);
    assert_eq!(blob["data"], Value::Null);
    assert_eq!(blob["more"]["mcp_truncation"]["withheld"], json!(["/data"]));

    // An answer inside the bound is untouched.
    let mut small = json!({ "status": "ok", "data": { "rows": [1] } });
    let before = small.clone();
    bound_envelope(&mut small, 8 * 1024);
    assert_eq!(small, before);
}

#[test]
fn credentials_never_reach_a_host_and_ordinary_tokens_do() {
    let mut envelope = json!({
        "status": "error",
        "error": {
            "message": "gateway said 401 for Authorization: Bearer eyJhbGciOiJSUzI1NiJ9.payload.sig",
            "detail": { "Refresh_Token": "r-123", "nested": [{ "password": "hunter2" }] }
        },
        "data": {
            "token": "LV-POLE",
            "access_token": "",
            "note": "the bearer of this letter",
            "session": { "device_code": "dc-1", "code_verifier": "cv-1" }
        }
    });
    let replaced = redact_credentials(&mut envelope);
    assert_eq!(replaced, 5);
    let text = envelope.to_string();
    for secret in ["eyJhbGciOiJSUzI1NiJ9", "r-123", "hunter2", "dc-1", "cv-1"] {
        assert!(!text.contains(secret), "{secret} leaked: {text}");
    }
    // A survey feature-code token is data, an empty value hides nothing,
    // and prose that says "bearer" is not a header.
    assert_eq!(envelope["data"]["token"], "LV-POLE");
    assert_eq!(envelope["data"]["access_token"], "");
    assert_eq!(envelope["data"]["note"], "the bearer of this letter");
}

#[test]
fn chapter_routers_are_annotated_from_the_commands_they_route() {
    let surface = Surface::new(
        Exposure::Chapters,
        None,
        vec![
            tool("shell.status", Chapter::Operations, false),
            tool("tile.generate", Chapter::VectorTiles, true),
            tool("tile.status", Chapter::VectorTiles, false),
        ],
    )
    .expect("surface");
    let listed = surface.tool_list();
    let annotations = |name: &str| {
        listed
            .iter()
            .find(|tool| tool["name"] == name)
            .map(|tool| tool["annotations"].clone())
            .expect(name)
    };
    let operations = annotations("ds_operations");
    assert_eq!(operations["readOnlyHint"], true);
    assert_eq!(operations["destructiveHint"], false);
    assert_eq!(operations["idempotentHint"], true);
    let tiles = annotations("ds_vector_tiles");
    assert_eq!(tiles["readOnlyHint"], false);
    assert_eq!(tiles["destructiveHint"], true);
    assert_eq!(tiles["idempotentHint"], false);
    for tool in &listed {
        assert_eq!(tool["annotations"]["openWorldHint"], false, "{tool}");
    }
}

#[test]
fn leaf_annotations_are_the_effect_hints() {
    let write = tool("tile.generate", Chapter::VectorTiles, true);
    let annotations = &leaf_tool_json(&write)["annotations"];
    assert_eq!(annotations["readOnlyHint"], false);
    assert_eq!(annotations["destructiveHint"], true);
    assert_eq!(annotations["idempotentHint"], false);
    let read = tool("tile.status", Chapter::VectorTiles, false);
    let annotations = &leaf_tool_json(&read)["annotations"];
    assert_eq!(annotations["readOnlyHint"], true);
    assert_eq!(annotations["destructiveHint"], false);
    assert_eq!(annotations["idempotentHint"], true);
}

#[test]
fn a_leaf_argument_mistake_is_the_commands_refusal_not_a_protocol_error() {
    let surface = Surface::new(
        Exposure::Commands,
        None,
        vec![tool("shell.status", Chapter::Operations, false)],
    )
    .expect("surface");
    let refused = surface
        .call(
            "shell_status",
            &json!({ "nope": 1 }),
            &PathBuf::from("not-called"),
        )
        .expect("a refusal is a tool result");
    assert_eq!(refused["isError"], true);
    let envelope = &refused["structuredContent"];
    assert_eq!(envelope["status"], "error");
    assert_eq!(envelope["command"], "shell.status");
    assert_eq!(envelope["error"]["code"], "mcp_arguments_invalid");
    assert_eq!(envelope["error"]["class"], "invalid_input");
    assert!(
        envelope["error"]["message"]
            .as_str()
            .unwrap()
            .contains("`nope`")
    );
    assert_eq!(
        envelope["error"]["next"],
        json!(["ds capabilities shell.status"])
    );
    // An unknown tool is still the protocol's to refuse.
    let unknown = surface
        .call("shell_nope", &json!({}), &PathBuf::from("not-called"))
        .unwrap_err();
    assert_eq!(unknown.0, -32602);
}

/// A shell stands in for `ds`: the tool's path becomes `-c <script>`, and
/// the adapter's own trailing tokens become the script's positional
/// parameters, which it ignores.
#[cfg(unix)]
fn shell_tool(script: &str, effect: Effect) -> Tool {
    let mut tool = tool("fixture.shell", Chapter::Operations, false);
    tool.path = vec!["-c".to_string(), script.to_string()];
    tool.effect = effect;
    tool
}

#[cfg(unix)]
#[test]
fn a_call_past_its_bound_is_stopped_and_refused_by_what_it_may_have_changed() {
    let executable = PathBuf::from("/bin/sh");
    for (effect, class) in [
        (Effect::ReadOnly, "unavailable"),
        (Effect::GlobalWrite, "conflict"),
    ] {
        let surface = Surface::new(
            Exposure::Commands,
            None,
            vec![shell_tool("sleep 5", effect)],
        )
        .expect("surface")
        .with_call_timeout(Duration::from_millis(300));
        let started = std::time::Instant::now();
        let refused = surface
            .call("fixture_shell", &json!({}), &executable)
            .expect("a timeout is a tool result");
        assert!(started.elapsed() < Duration::from_secs(4));
        assert_eq!(refused["isError"], true);
        let error = &refused["structuredContent"]["error"];
        assert_eq!(error["code"], "mcp_call_timed_out");
        assert_eq!(error["class"], class, "{effect}");
        assert_eq!(error["detail"]["effect"], effect.token());
        if !tools::hints(effect).read_only {
            assert!(
                error["remedy"]
                    .as_str()
                    .unwrap()
                    .contains("re-read any state")
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn an_answer_past_the_capture_bound_is_refused_not_cut() {
    let surface = Surface::new(
        Exposure::Commands,
        None,
        vec![shell_tool(
            &format!("head -c {} /dev/zero", tools::STDOUT_CAPTURE_LIMIT + 1),
            Effect::ReadOnly,
        )],
    )
    .expect("surface");
    let refused = surface
        .call("fixture_shell", &json!({}), &PathBuf::from("/bin/sh"))
        .expect("a refusal is a tool result");
    assert_eq!(
        refused["structuredContent"]["error"]["code"],
        "mcp_result_too_large"
    );
    assert!(refused["content"][0]["text"].as_str().unwrap().len() < 4096);
}

#[cfg(unix)]
#[test]
fn text_that_is_no_envelope_is_bounded_and_scrubbed() {
    let executable = PathBuf::from("/bin/sh");
    let long = Surface::new(
        Exposure::Commands,
        None,
        vec![shell_tool(
            "head -c 20000 /dev/zero | tr '\\0' x >&2; exit 3",
            Effect::ReadOnly,
        )],
    )
    .expect("surface")
    .call("fixture_shell", &json!({}), &executable)
    .expect("result");
    assert_eq!(long["isError"], true);
    assert!(long.get("structuredContent").is_none());
    let text = long["content"][0]["text"].as_str().unwrap();
    assert!(text.len() < MAX_FALLBACK_TEXT_BYTES + 100, "{}", text.len());
    assert!(text.contains("bytes omitted by ds mcp"), "{text}");

    let quoted = Surface::new(
            Exposure::Commands,
            None,
            vec![shell_tool(
                "echo 'gateway said 401 to Authorization: Bearer abcdefghijklmnopqrstuvwxyz0123' >&2; exit 3",
                Effect::ReadOnly,
            )],
        )
        .expect("surface")
        .call("fixture_shell", &json!({}), &executable)
        .expect("result");
    let text = quoted["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("gateway said 401"), "{text}");
    assert!(!text.contains("abcdefghijklmnopqrstuvwxyz0123"), "{text}");
    assert!(text.contains(REDACTED), "{text}");
}

fn catalog_fixture(id: &str, chapter: Chapter, summary: &str, purpose: &str) -> Tool {
    let mut command = tool(id, chapter, false);
    command.descriptor["summary"] = json!(summary);
    command.descriptor["purpose"] = json!(purpose);
    command.description = format!(
        "{summary}\n\n{purpose}\nUse the profile; Solar workspace and survey refusals require a descriptor."
    );
    command
}

fn catalog_query(surface: &Surface, arguments: Value) -> Value {
    surface
        .call_catalog(&arguments, &PathBuf::from("/no-executable-for-catalogue"))
        .expect("descriptor-only catalogue query")["structuredContent"]
        .clone()
}

#[test]
fn catalog_query_excludes_generated_host_guidance() {
    let surface = Surface::new(
        Exposure::Chapters,
        None,
        vec![
            catalog_fixture(
                "account.status",
                Chapter::Operations,
                "Read account status",
                "Inspect the signed-in account.",
            ),
            catalog_fixture(
                "assets.list",
                Chapter::Assets,
                "List documents",
                "List held project assets.",
            ),
            catalog_fixture(
                "map.profile.view",
                Chapter::MapPresentation,
                "Read Profile view",
                "Inspect the mounted scene.",
            ),
        ],
    )
    .unwrap();
    let data = catalog_query(&surface, json!({"query":"profile"}));
    assert_eq!(data["matched"], 1);
    assert_eq!(data["results"][0]["id"], "map.profile.view");
    assert_eq!(data["identity"], surface.identity);
    assert_eq!(
        data["skill_resources"],
        surface
            .identity
            .get("skills")
            .cloned()
            .unwrap_or(Value::Null)
    );
}

#[test]
fn catalog_query_prefers_id_then_summary_over_incidental_purpose() {
    let surface = Surface::new(
        Exposure::Chapters,
        None,
        vec![
            catalog_fixture(
                "account.inspect",
                Chapter::Operations,
                "Read account status",
                "Inspect the account used by a Profile request.",
            ),
            catalog_fixture(
                "zz.section",
                Chapter::GridModel,
                "Read Profile section",
                "Read the authored section.",
            ),
            catalog_fixture(
                "map.profile.view",
                Chapter::MapPresentation,
                "Read the mounted scene",
                "Inspect current presentation.",
            ),
        ],
    )
    .unwrap();
    let data = catalog_query(&surface, json!({"query":"PrOfIlE"}));
    let ids: Vec<_> = data["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["map.profile.view", "zz.section", "account.inspect"]);
}

#[test]
fn catalog_query_ranks_full_primary_term_coverage_and_keeps_or_matches() {
    let surface = Surface::new(
        Exposure::Chapters,
        None,
        vec![
            catalog_fixture(
                "core.cancel",
                Chapter::Operations,
                "Cancel a job",
                "Cancel a running request.",
            ),
            catalog_fixture(
                "solar.cities",
                Chapter::Solar,
                "Read Solar cities",
                "Inspect cities using the workspace.",
            ),
            catalog_fixture(
                "solar.workspace.read",
                Chapter::Solar,
                "Read the approved workspace",
                "Read the held selection.",
            ),
            catalog_fixture(
                "data.workspace",
                Chapter::Data,
                "Read workspace metadata",
                "Inspect retained metadata.",
            ),
        ],
    )
    .unwrap();
    let data = catalog_query(&surface, json!({"query":"Solar workspace"}));
    assert_eq!(data["matched"], 3);
    assert_eq!(data["results"][0]["id"], "solar.workspace.read");
    assert!(
        data["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "data.workspace")
    );
    let scoped = catalog_query(
        &surface,
        json!({"query":"Solar workspace","chapter":"solar"}),
    );
    assert_eq!(scoped["matched"], 2);
    assert_eq!(scoped["results"][0]["id"], "solar.workspace.read");
}

#[test]
fn catalog_query_keeps_ten_result_bound_ties_and_exact_command_routing() {
    let commands = (0..12)
        .rev()
        .map(|n| {
            catalog_fixture(
                &format!("survey.read.{n:02}"),
                Chapter::Survey,
                "Read entries",
                "Read retained entries.",
            )
        })
        .collect();
    let surface = Surface::new(Exposure::Chapters, None, commands).unwrap();
    let data = catalog_query(&surface, json!({"query":"survey"}));
    assert_eq!(data["matched"], 12);
    assert_eq!(data["results"].as_array().unwrap().len(), 10);
    assert_eq!(data["results"][0]["id"], "survey.read.00");
    assert_eq!(data["results"][9]["id"], "survey.read.09");
    let exact = catalog_query(&surface, json!({"command":"survey.read.11"}));
    assert_eq!(exact["command"]["id"], "survey.read.11");
    assert_eq!(exact["next"]["tool"], "ds_survey");
    assert!(
        surface
            .call_catalog(
                &json!({"command":"survey.read.11","chapter":"solar"}),
                &PathBuf::from("/no-executable")
            )
            .is_err()
    );
}

#[test]
fn catalog_discovery_never_enters_the_desktop_launch_gate() {
    let mut paired = tool("desktop.project.list", Chapter::Project, false);
    paired.authority = Authority::DesktopUser;
    let surface = Surface::new(Exposure::Chapters, None, vec![paired]).expect("surface");
    let response = surface
        .call_catalog(
            &json!({ "command": "desktop.project.list" }),
            &PathBuf::from("ds"),
        )
        .expect("catalogue is descriptor-only");
    assert_eq!(
        response["structuredContent"]["command"]["id"],
        "desktop.project.list"
    );
}

#[test]
fn every_declared_profile_token_round_trips() {
    for token in PROFILE_IDS {
        let profile = Profile::from_token(token).expect("known profile");
        assert_eq!(profile.token(), *token);
    }
    assert!(Profile::from_token("all").is_none());
}

#[test]
fn bundled_skills_name_only_known_chapters_and_compatible_profiles() {
    let skills = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../skills");
    let mut declared = 0usize;
    for entry in std::fs::read_dir(skills).expect("skills directory") {
        let path = entry.expect("skill entry").path().join("SKILL.md");
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("skill text");
        let chapters = text.lines().find_map(|line| {
            line.trim_start().strip_prefix("ds-chapters:").map(|value| {
                value
                    .split(',')
                    .map(str::trim)
                    .map(|token| {
                        Chapter::from_token(token).unwrap_or_else(|| {
                            panic!("{} names unknown chapter `{token}`", path.display())
                        })
                    })
                    .collect::<Vec<_>>()
            })
        });
        // A skill that teaches several workflows names each workflow's
        // typed profile. Together they carry every declared chapter, and
        // none is named that carries no chapter of the skill.
        let profiles = text.lines().find_map(|line| {
            line.trim_start()
                .strip_prefix("ds-mcp-profile:")
                .map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .map(|token| {
                            Profile::from_token(token).unwrap_or_else(|| {
                                panic!("{} names unknown profile `{token}`", path.display())
                            })
                        })
                        .collect::<Vec<_>>()
                })
        });
        if let Some(chapters) = chapters {
            declared += 1;
            assert!(!chapters.is_empty(), "{} has no chapters", path.display());
            if let Some(profiles) = profiles {
                for chapter in &chapters {
                    assert!(
                        profiles
                            .iter()
                            .any(|profile| profile.includes_chapter(*chapter)),
                        "{} requires `{}` but no named profile carries it",
                        path.display(),
                        chapter.token()
                    );
                }
                for profile in &profiles {
                    assert!(
                        chapters
                            .iter()
                            .any(|chapter| profile.includes_chapter(*chapter)),
                        "{} names profile `{}`, which carries none of its chapters",
                        path.display(),
                        profile.token()
                    );
                }
            }
        } else {
            assert!(
                profiles.is_none(),
                "{} names a profile without declaring chapters",
                path.display()
            );
        }
    }
    assert!(declared >= 10, "workflow skills must declare chapters");
}

/// Which chapters a profile may publish from; the routing tests read it.
impl Profile {
    pub fn includes_chapter(self, chapter: Chapter) -> bool {
        match self {
            Self::AuthContext => chapter == Chapter::Project,
            Self::AdminBounds => chapter == Chapter::Data,
            Self::Datasets => matches!(
                chapter,
                Chapter::Data | Chapter::GridModel | Chapter::MapPresentation | Chapter::Reports
            ),
            Self::Installations => chapter == Chapter::Operations,
            Self::Grid => matches!(chapter, Chapter::GridModel | Chapter::Reports),
            Self::GridNative => chapter == Chapter::GridModel,
            Self::Printing | Self::PrintingMaps => {
                matches!(chapter, Chapter::Reports | Chapter::MapPresentation)
            }
            // Version attachments are Design commands carried by this model
            // lifecycle profile beside publication.
            Self::GridLocalModel => matches!(chapter, Chapter::GridModel | Chapter::Design),
            Self::GridClearance | Self::GridCorrections => chapter == Chapter::GridModel,
            Self::GridProfile => matches!(chapter, Chapter::GridModel | Chapter::MapPresentation),
            Self::Survey
            | Self::FormFactory
            | Self::SurveyProjects
            | Self::SurveyMedia
            | Self::SurveyMigration
            | Self::Layers => chapter == Chapter::Survey,
            Self::DesignEdit | Self::DesignRun | Self::DesignMigration => {
                chapter == Chapter::Design
            }
            Self::Map | Self::Styles | Self::PrintStyles => chapter == Chapter::MapPresentation,
            Self::Tiling => chapter == Chapter::VectorTiles,
            Self::Project
            | Self::ProjectTaskOperations
            | Self::ProjectChecklists
            | Self::Notes
            | Self::Correspondence
            | Self::Messaging => chapter == Chapter::Project,
            Self::SolarInput
            | Self::SolarMigration
            | Self::SolarApplication
            | Self::SolarDashboard
            | Self::SolarRun
            | Self::SolarDelivery
            | Self::SolarPortfolioBatch => chapter == Chapter::Solar,
            Self::Operations => chapter == Chapter::Operations,
            Self::ProjectOperations => matches!(chapter, Chapter::Design | Chapter::Reports),
        }
    }
}
