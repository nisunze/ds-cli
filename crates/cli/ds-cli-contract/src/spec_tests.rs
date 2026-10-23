use super::{
    Arg, Authority, AuthorityCapability, Availability, Chapter, Command, Effect, Execution,
    Requires,
};

fn available() -> Availability {
    Availability::Available
}

static WRITE_SWITCH: Arg = Arg::switch("write", "Select the writing path.");
static WRITE_ARGS: &[Arg] = &[WRITE_SWITCH];

fn confirmation_fixture(effect: Effect) -> Command {
    Command {
        id: "test.confirmation-trigger",
        path: &["test", "confirmation-trigger"],
        contract: 1,
        chapter: Chapter::Catalog,
        summary: "Regression fixture.",
        purpose: "Proves only machine writes can use a conditional write trigger.",
        effect,
        authority: Authority::None,
        execution: Execution::Sync,
        args: WRITE_ARGS,
        output: "Nothing.",
        examples: &[],
        refusals: &[],
        reference: None,
        search: &[],
        requires: Requires::Server,
        availability: available,
    }
}

#[test]
fn chapter_tokens_are_unique_stable_and_round_trip() {
    assert_eq!(Chapter::ALL.len(), 13);
    for (index, chapter) in Chapter::ALL.iter().enumerate() {
        let token = chapter.token();
        assert_eq!(Chapter::from_token(token), Some(*chapter));
        assert_eq!(chapter.to_string(), token);
        assert!(
            !token.is_empty()
                && token
                    .chars()
                    .all(|value| value.is_ascii_lowercase() || value == '-')
                && !token.starts_with('-')
                && !token.ends_with('-')
        );
        assert!(
            Chapter::ALL[..index]
                .iter()
                .all(|previous| previous.token() != token),
            "duplicate chapter token `{token}`"
        );
    }
}

#[test]
fn unknown_chapter_tokens_are_not_guessed() {
    assert_eq!(Chapter::from_token("grid_model"), None);
    assert_eq!(Chapter::from_token("Catalog"), None);
    assert_eq!(Chapter::from_token(""), None);
}

#[test]
fn compatibility_authorities_normalize_without_weakening_desktop_proof() {
    use AuthorityCapability::{Desktop, Map, None, Project, User};

    let cases: &[(Authority, &[AuthorityCapability])] = &[
        (Authority::None, &[None]),
        (Authority::HeadlessUser, &[User]),
        (Authority::HeadlessProject, &[User, Project]),
        (Authority::DesktopPairing, &[Desktop]),
        (Authority::DesktopUser, &[User, Desktop]),
        (Authority::Project, &[User, Project, Desktop]),
    ];
    for (authority, expected) in cases {
        assert_eq!(authority.capabilities(), *expected, "{authority}");
        assert!(
            !authority.requires_capability(Map),
            "legacy authority `{authority}` must not invent active-map proof"
        );
    }
    assert!(Authority::Project.requires_desktop());
    assert!(Authority::Project.requires_signed_in_user());
    assert!(Authority::Project.requires_project());
    assert!(!Authority::HeadlessProject.requires_desktop());
}

#[test]
fn effect_and_execution_tokens_round_trip() {
    for effect in Effect::ALL {
        assert_eq!(Effect::from_token(effect.token()), Some(*effect));
    }
    assert_eq!(Effect::ALL.len(), 9, "a new effect class must join `ALL`");
    assert_eq!(Effect::from_token("write"), None);
    for execution in [Execution::Sync, Execution::Job] {
        assert_eq!(Execution::from_token(execution.token()), Some(execution));
    }
    assert_eq!(Execution::from_token("async"), None);
}

#[test]
fn authority_tokens_remain_exactly_backward_compatible() {
    for authority in [
        Authority::None,
        Authority::DesktopPairing,
        Authority::DesktopUser,
        Authority::Project,
        Authority::HeadlessUser,
        Authority::HeadlessProject,
    ] {
        assert_eq!(Authority::from_token(authority.token()), Some(authority));
    }
    assert_eq!(Authority::from_token("user"), None);
    assert_eq!(Authority::from_token("map"), None);
}

#[test]
fn typed_publication_plans_confirm_only_the_applied_project_path() {
    static ARGS: [Arg; 2] = [
        Arg::switch("apply", "Apply the candidate."),
        Arg::value("publication", "<json>", "Typed project binding."),
    ];
    let mut command = confirmation_fixture(Effect::GlobalWrite);
    command.args = &ARGS;
    for (args, confirmed) in [
        (vec![], false),
        (vec!["--publication", "binding.json"], false),
        (vec!["--apply"], false),
        (vec!["--apply", "--publication", "binding.json"], true),
    ] {
        let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
        let inputs = crate::parse(&command, &args).unwrap();
        assert_eq!(command.confirmation_required_for(&inputs), confirmed);
    }
    let descriptor = crate::help::command_json(&command);
    assert_eq!(descriptor["confirmation_trigger"], "--apply");
    assert_eq!(descriptor["confirmation_parameter"], "--publication");
    assert!(crate::help::command(&command).contains("required with --apply and --publication"));
    // Merely naming an apply switch cannot bypass an existing write gate.
    command.args = &ARGS[..1];
    assert!(command.confirmation_required_for(&crate::parse(&command, &[]).unwrap()));
}

#[test]
fn only_machine_write_can_use_the_declared_write_trigger() {
    let machine = confirmation_fixture(Effect::MachineWrite);
    assert_eq!(machine.confirmation_trigger(), Some("--write"));
    assert_eq!(
        crate::help::command_json(&machine)["confirmation_trigger"],
        "--write"
    );
    let preview = crate::parse(&machine, &[]).unwrap();
    assert!(!machine.confirmation_required_for(&preview));
    let write = crate::parse(&machine, &["--write".to_string()]).unwrap();
    assert!(machine.confirmation_required_for(&write));

    for effect in [Effect::GlobalWrite, Effect::ArtifactWrite] {
        let command = confirmation_fixture(effect);
        assert_eq!(command.confirmation_trigger(), None);
        assert!(
            crate::help::command_json(&command)
                .get("confirmation_trigger")
                .is_none(),
            "{effect:?} must remain unconditionally gated in its descriptor"
        );
        let absent = crate::parse(&command, &[]).unwrap();
        assert!(command.confirmation_required_for(&absent));
    }
}

/// A writing command that declares a `--dry-run` switch has a previewing
/// path: with the switch set nothing is written, so the confirmation gate
/// stands aside; without it the gate is exactly what it always was. A
/// `dry-run` VALUE is not a switch and opens no such path.
#[test]
fn a_declared_dry_run_switch_is_the_one_confirmation_free_path_of_a_write() {
    static PREVIEW: Command = Command {
        id: "fixture.propose",
        path: &["fixture", "propose"],
        contract: 1,
        summary: "fixture",
        purpose: "fixture",
        chapter: Chapter::Project,
        effect: Effect::GlobalWrite,
        authority: Authority::None,
        execution: Execution::Sync,
        args: &[Arg::switch(
            "dry-run",
            "Answer the proposal; write nothing.",
        )],
        output: "",
        examples: &[],
        refusals: &[],
        reference: None,
        search: &[],
        requires: Requires::Server,
        availability: available,
    };
    assert_eq!(PREVIEW.preview_switch(), Some("--dry-run"));
    assert_eq!(
        crate::help::command_json(&PREVIEW)["preview_switch"],
        "--dry-run"
    );
    let proposal = crate::parse(&PREVIEW, &["--dry-run".to_string()]).unwrap();
    assert!(!PREVIEW.confirmation_required_for(&proposal));
    let write = crate::parse(&PREVIEW, &[]).unwrap();
    assert!(PREVIEW.confirmation_required_for(&write));

    let guarded_ui = Command {
        effect: Effect::LocalUi,
        refusals: &[super::Refusal {
            code: "confirmation_required",
            when: "live edit",
            remedy: "preview or confirm",
        }],
        ..PREVIEW
    };
    assert!(!guarded_ui.confirmation_required_for(&proposal));
    assert!(guarded_ui.confirmation_required_for(&write));
    let descriptor = crate::help::command_json(&guarded_ui);
    assert_eq!(descriptor["confirmation_required"], true);
    assert_eq!(descriptor["preview_switch"], "--dry-run");
    let unguarded_ui = Command {
        refusals: &[],
        ..guarded_ui
    };
    assert!(!unguarded_ui.confirmation_required_for(&write));
    let read = Command {
        effect: Effect::ReadOnly,
        ..guarded_ui
    };
    assert!(!read.confirmation_required_for(&write));

    static VALUE: Command = Command {
        id: "fixture.value",
        path: &["fixture", "value"],
        contract: 1,
        summary: "fixture",
        purpose: "fixture",
        chapter: Chapter::Project,
        effect: Effect::GlobalWrite,
        authority: Authority::None,
        execution: Execution::Sync,
        args: &[Arg::value("dry-run", "<x>", "not a switch")],
        output: "",
        examples: &[],
        refusals: &[],
        reference: None,
        search: &[],
        requires: Requires::Server,
        availability: available,
    };
    assert_eq!(VALUE.preview_switch(), None);
    assert!(
        crate::help::command_json(&VALUE)
            .get("preview_switch")
            .is_none()
    );
    let gated = crate::parse(&VALUE, &["--dry-run".to_string(), "x".to_string()]).unwrap();
    assert!(VALUE.confirmation_required_for(&gated));
}

/// A placeholder is the declared type of a value: the sensitive shapes mark
/// their inputs sensitive, and a `<min-max>` placeholder declares its bound.
#[test]
fn a_value_shape_declares_sensitivity_and_a_bound() {
    for shape in super::SENSITIVE_VALUES {
        assert!(Arg::value("x", shape, "s").is_sensitive(), "{shape}");
    }
    for plain in ["<project-id>", "<name>", "<path>", "<token>", "<text>", ""] {
        assert!(!Arg::value("x", plain, "s").is_sensitive(), "{plain}");
    }
    assert!(!Arg::switch("x", "s").is_sensitive());
    assert_eq!(Arg::value("limit", "<1-50>", "s").range(), Some((1, 50)));
    assert_eq!(
        Arg::value("timeout", "<0-900>", "s").range(),
        Some((0, 900))
    );
    for unbounded in ["<count>", "<50-1>", "<a-b>", "<1->", "1-50", "<1-50"] {
        assert_eq!(Arg::value("x", unbounded, "s").range(), None, "{unbounded}");
    }
    static MARKED: &[Arg] = &[
        Arg::value("limit", "<1-50>", "Rows."),
        Arg::value("email", "<email>", "Who."),
        Arg::value("project", "<project-id>", "Where."),
    ];
    let json = crate::help::command_json_unchecked(&Command {
        args: MARKED,
        ..confirmation_fixture(Effect::ReadOnly)
    });
    let inputs = json["inputs"].as_array().expect("inputs");
    assert_eq!(
        (inputs[0]["minimum"].as_i64(), inputs[0]["maximum"].as_i64()),
        (Some(1), Some(50))
    );
    assert!(inputs[0].get("sensitive").is_none());
    assert_eq!(inputs[1]["sensitive"], true);
    assert!(inputs[2].get("sensitive").is_none() && inputs[2].get("minimum").is_none());
}
