//! Project routing for commands that still need the existing UI runtime.
//! Background commands do not enter this boundary. Hosts observe identity,
//! execute an admitted switch, and fence the subsequent operation.
//!
//! This is a consumer of [`crate::desktop_instance`]: the instance is chosen
//! there, first, and the [`Context`] below describes the instance that was
//! chosen. What is decided here is only what happens *inside* it.
//!
//! **A saved CLI selection never moves a live map.** A caller may have project
//! A selected while the instance shows B; that difference narrows which
//! instance may serve the work (`desktop_instance::select`) and, once one is
//! chosen, refuses — it is never an instruction to switch the window the
//! operator is looking at. Only the switch command itself carries
//! [`Switch::Explicit`], and it names its instance. The superseded behaviour is
//! recorded in `docs/contracts/ds-desktop-instance.md` §"Decisions to
//! challenge".

use crate::desktop_instance::{Candidate, DESKTOP_PROJECT_NOT_OPEN};

/// The typed code a host raises when the paired instance is signed in
/// elsewhere. Named here so the two owners of desktop routing publish one
/// vocabulary between them.
pub const AUTH_CONTEXT_MISMATCH: &str = "auth_context_mismatch";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context<'a> {
    pub uid: &'a str,
    pub lane: &'a str,
    pub audience: &'a str,
    pub project: Option<&'a str>,
    /// The live instance this context describes, when it describes one. A
    /// caller's own saved context names none: it is an identity and a default,
    /// not a runtime.
    pub instance: Option<&'a str>,
}

impl<'a> Context<'a> {
    /// The context of the instance [`crate::desktop_instance::select`] chose.
    /// Hosts build it from the decision rather than from a handshake by hand,
    /// so the fields that fence an operation and the fields that chose the
    /// instance are the same fields.
    pub fn of(candidate: &'a Candidate) -> Self {
        Self {
            uid: &candidate.uid,
            lane: &candidate.lane,
            audience: &candidate.audience_sha256,
            project: candidate.project.as_deref(),
            instance: Some(&candidate.instance_id),
        }
    }

    fn same_identity(self, other: Self) -> bool {
        self.uid == other.uid && self.lane == other.lane && self.audience == other.audience
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requirement {
    DesktopUser,
    MapProject,
}

/// Whether this operation is allowed to move the instance's project at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Switch {
    /// Every ordinary paired operation. The caller's project selection is a
    /// condition on where the work may go, never an instruction to the UI.
    #[default]
    Never,
    /// `ds desktop project switch --target desktop:<instance_id> --project <p>`
    /// and nothing else: the operator asked for this window to move.
    Explicit,
}

/// How a verified paired window was named. Its host lane remains unchanged;
/// a Local development window is not a provisioned credential lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairedHostScope<'a> {
    Provisioned,
    ExplicitLocalDevelopment {
        target_instance: &'a str,
        native_credential_lane: &'a str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route<'a> {
    CurrentDesktop,
    SwitchDesktop { project: &'a str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal<'a> {
    /// The instance is signed in as another account, on another lane, or
    /// against another credential audience.
    IdentityMismatch,
    /// The operation is about a project this instance does not have open, and
    /// nothing here may move a live map to it.
    ProjectNotOpen { project: &'a str },
}

impl Refusal<'_> {
    pub const fn code(self) -> &'static str {
        match self {
            Self::IdentityMismatch => AUTH_CONTEXT_MISMATCH,
            Self::ProjectNotOpen { .. } => DESKTOP_PROJECT_NOT_OPEN,
        }
    }
}

/// What one operation does inside the instance that was chosen for it.
///
/// A project difference is a refusal, not a switch: `desktop_instance::select`
/// has already refused to route project work to an instance that does not hold
/// the project, so reaching this with a difference means the caller named the
/// instance itself — and naming an instance is not asking it to move.
pub fn route<'a>(
    requirement: Requirement,
    switch: Switch,
    cli: Option<Context<'a>>,
    desktop: Context<'a>,
) -> Result<Route<'a>, Refusal<'a>> {
    let Some(cli) = cli else {
        return Ok(Route::CurrentDesktop);
    };
    if !cli.same_identity(desktop) {
        return Err(Refusal::IdentityMismatch);
    }
    project_route(requirement, switch, cli, desktop)
}

/// Explicit Local development pairing with a separately captured native
/// credential lane. Only an exact named, observed Local instance may use this
/// scope; automatic routing and provisioned lanes retain `route` unchanged.
pub fn route_paired<'a>(
    requirement: Requirement,
    switch: Switch,
    cli: Option<Context<'a>>,
    desktop: Context<'a>,
    scope: PairedHostScope<'a>,
) -> Result<Route<'a>, Refusal<'a>> {
    let PairedHostScope::ExplicitLocalDevelopment {
        target_instance,
        native_credential_lane,
    } = scope
    else {
        return route(requirement, switch, cli, desktop);
    };
    let Some(cli) = cli else {
        return Err(Refusal::IdentityMismatch);
    };
    if desktop.lane != "local"
        || desktop.instance != Some(target_instance)
        || target_instance.is_empty()
        || !matches!(native_credential_lane, "stable" | "canary")
        || cli.lane != native_credential_lane
        || cli.uid != desktop.uid
        || cli.audience != desktop.audience
    {
        return Err(Refusal::IdentityMismatch);
    }
    project_route(requirement, switch, cli, desktop)
}

fn project_route<'a>(
    requirement: Requirement,
    switch: Switch,
    cli: Context<'a>,
    desktop: Context<'a>,
) -> Result<Route<'a>, Refusal<'a>> {
    if requirement == Requirement::MapProject {
        if let Some(project) = cli.project {
            if desktop.project != Some(project) {
                return match switch {
                    Switch::Explicit => Ok(Route::SwitchDesktop { project }),
                    Switch::Never => Err(Refusal::ProjectNotOpen { project }),
                };
            }
        }
    }
    Ok(Route::CurrentDesktop)
}

/// A successful switch must preserve the principal, stay in the instance it
/// was sent to, and reach the exact target. The host additionally carries the
/// observed session revision to dispatch.
pub fn switch_completed(before: Context<'_>, after: Context<'_>, target: &str) -> bool {
    before.same_identity(after)
        && before.instance == after.instance
        && after.project == Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const INSTANCE: &str = "11111111111111111111111111111111";
    const AUDIENCE: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn context(project: Option<&str>) -> Context<'_> {
        Context {
            uid: "user",
            lane: "stable",
            audience: "audience",
            project,
            instance: Some(INSTANCE),
        }
    }
    fn cli(project: Option<&str>) -> Context<'_> {
        Context {
            instance: None,
            ..context(project)
        }
    }

    /// Supersedes `map_uses_cli_target_and_user_operations_leave_ui_alone`: the
    /// CLI's saved selection used to switch a live map's project on any map
    /// command. The owner's ruling of 2026-09-12 is that it may never do that.
    #[test]
    fn a_cli_selection_narrows_the_target_and_never_moves_a_live_map() {
        let caller = Some(cli(Some("cli-project")));
        let desktop = context(Some("ui-project"));
        assert_eq!(
            route(Requirement::MapProject, Switch::Never, caller, desktop),
            Err(Refusal::ProjectNotOpen {
                project: "cli-project"
            })
        );
        assert_eq!(
            route(
                Requirement::MapProject,
                Switch::Never,
                caller,
                context(None)
            ),
            Err(Refusal::ProjectNotOpen {
                project: "cli-project"
            })
        );
        // A user operation was never about the project and still is not.
        assert_eq!(
            route(Requirement::DesktopUser, Switch::Never, caller, desktop),
            Ok(Route::CurrentDesktop)
        );
        // The refusal is the same code the instance owner raises for the same
        // situation, so one remedy answers both.
        assert_eq!(
            Refusal::ProjectNotOpen { project: "p" }.code(),
            DESKTOP_PROJECT_NOT_OPEN
        );
    }

    #[test]
    fn only_the_switch_command_moves_the_project_it_names() {
        let caller = Some(cli(Some("cli-project")));
        assert_eq!(
            route(
                Requirement::MapProject,
                Switch::Explicit,
                caller,
                context(Some("ui-project"))
            ),
            Ok(Route::SwitchDesktop {
                project: "cli-project"
            })
        );
        // Even asked explicitly, a switch is about the project — not a way to
        // reach a user operation in another account's window.
        assert_eq!(
            route(
                Requirement::DesktopUser,
                Switch::Explicit,
                caller,
                context(Some("ui-project"))
            ),
            Ok(Route::CurrentDesktop)
        );
    }

    #[test]
    fn ui_supplies_context_when_cli_has_no_selection() {
        let desktop = context(Some("ui-project"));
        for caller in [None, Some(cli(None)), Some(cli(Some("ui-project")))] {
            for switch in [Switch::Never, Switch::Explicit] {
                assert_eq!(
                    route(Requirement::MapProject, switch, caller, desktop),
                    Ok(Route::CurrentDesktop)
                );
            }
        }
    }

    #[test]
    fn explicit_local_pairing_keeps_host_and_credential_scopes_distinct() {
        let caller = Context {
            lane: "canary",
            ..cli(Some("project"))
        };
        let local = Context {
            lane: "local",
            ..context(Some("project"))
        };
        let scope = PairedHostScope::ExplicitLocalDevelopment {
            target_instance: INSTANCE,
            native_credential_lane: "canary",
        };
        assert_eq!(
            route(Requirement::MapProject, Switch::Never, Some(caller), local),
            Err(Refusal::IdentityMismatch)
        );
        assert_eq!(
            route_paired(
                Requirement::MapProject,
                Switch::Never,
                Some(caller),
                local,
                scope
            ),
            Ok(Route::CurrentDesktop)
        );
        assert_eq!(local.lane, "local");
        for (changed, scope) in [
            (
                Context {
                    uid: "foreign",
                    ..local
                },
                scope,
            ),
            (
                Context {
                    audience: "foreign",
                    ..local
                },
                scope,
            ),
            (
                Context {
                    lane: "stable",
                    ..local
                },
                scope,
            ),
            (
                local,
                PairedHostScope::ExplicitLocalDevelopment {
                    target_instance: "another-instance",
                    native_credential_lane: "canary",
                },
            ),
            (
                local,
                PairedHostScope::ExplicitLocalDevelopment {
                    target_instance: INSTANCE,
                    native_credential_lane: "stable",
                },
            ),
            (
                local,
                PairedHostScope::ExplicitLocalDevelopment {
                    target_instance: INSTANCE,
                    native_credential_lane: "local",
                },
            ),
        ] {
            assert_eq!(
                route_paired(
                    Requirement::MapProject,
                    Switch::Explicit,
                    Some(caller),
                    changed,
                    scope
                ),
                Err(Refusal::IdentityMismatch)
            );
        }
        assert_eq!(
            route_paired(Requirement::MapProject, Switch::Never, None, local, scope),
            Err(Refusal::IdentityMismatch)
        );
        let elsewhere = Context {
            project: Some("other"),
            ..local
        };
        assert_eq!(
            route_paired(
                Requirement::MapProject,
                Switch::Never,
                Some(caller),
                elsewhere,
                scope
            ),
            Err(Refusal::ProjectNotOpen { project: "project" })
        );
        assert_eq!(
            route_paired(
                Requirement::MapProject,
                Switch::Explicit,
                Some(caller),
                elsewhere,
                scope
            ),
            Ok(Route::SwitchDesktop { project: "project" })
        );
        assert_eq!(
            route_paired(
                Requirement::MapProject,
                Switch::Never,
                Some(caller),
                Context {
                    lane: "stable",
                    ..local
                },
                PairedHostScope::Provisioned
            ),
            Err(Refusal::IdentityMismatch)
        );
    }

    #[test]
    fn identity_changes_never_admit_a_switch() {
        let original = context(Some("old-project"));
        for changed in [
            Context {
                uid: "another",
                ..original
            },
            Context {
                lane: "canary",
                ..original
            },
            Context {
                audience: "another",
                ..original
            },
        ] {
            for requirement in [Requirement::MapProject, Requirement::DesktopUser] {
                for switch in [Switch::Never, Switch::Explicit] {
                    assert_eq!(
                        route(requirement, switch, Some(original), changed),
                        Err(Refusal::IdentityMismatch)
                    );
                }
            }
            assert!(!switch_completed(original, changed, "old-project"));
        }
        assert_eq!(Refusal::IdentityMismatch.code(), AUTH_CONTEXT_MISMATCH);
        assert!(!switch_completed(original, original, "new-project"));
        assert!(switch_completed(
            original,
            context(Some("new-project")),
            "new-project"
        ));
    }

    #[test]
    fn a_switch_must_come_back_from_the_instance_it_was_sent_to() {
        let before = context(Some("old-project"));
        let elsewhere = Context {
            instance: Some("22222222222222222222222222222222"),
            ..context(Some("new-project"))
        };
        assert!(!switch_completed(before, elsewhere, "new-project"));
        assert!(!switch_completed(
            before,
            Context {
                instance: None,
                ..context(Some("new-project"))
            },
            "new-project"
        ));
    }

    #[test]
    fn the_context_that_fences_an_operation_is_the_one_that_chose_the_instance() {
        let candidate: Candidate = serde_json::from_value(json!({
            "instance_id": INSTANCE,
            "profile": "canary",
            "lane": "canary",
            "uid": "uid-a",
            "audience_sha256": AUDIENCE,
            "project": "project-a",
            "session_revision": 3,
        }))
        .expect("a candidate");
        assert_eq!(
            Context::of(&candidate),
            Context {
                uid: "uid-a",
                lane: "canary",
                audience: AUDIENCE,
                project: Some("project-a"),
                instance: Some(INSTANCE),
            }
        );
        assert_eq!(
            route(
                Requirement::MapProject,
                Switch::Never,
                Some(Context {
                    project: Some("project-a"),
                    instance: None,
                    ..Context::of(&candidate)
                }),
                Context::of(&candidate)
            ),
            Ok(Route::CurrentDesktop)
        );
    }
}
