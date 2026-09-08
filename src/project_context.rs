//! Project routing for commands that still need the existing UI runtime.
//! Background commands do not enter this boundary. Hosts observe identity,
//! execute an admitted switch, and fence the subsequent operation.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Context<'a> {
    pub uid: &'a str,
    pub lane: &'a str,
    pub audience: &'a str,
    pub project: Option<&'a str>,
}

impl Context<'_> {
    fn same_identity(self, other: Self) -> bool {
        self.uid == other.uid && self.lane == other.lane && self.audience == other.audience
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Requirement {
    DesktopUser,
    MapProject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route<'a> {
    CurrentDesktop,
    SwitchDesktop { project: &'a str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdentityMismatch;

/// A CLI project is the target when map work requires one. Otherwise the UI
/// supplies its current context. A project difference never changes identity.
pub fn route<'a>(
    requirement: Requirement,
    cli: Option<Context<'a>>,
    desktop: Context<'a>,
) -> Result<Route<'a>, IdentityMismatch> {
    let Some(cli) = cli else {
        return Ok(Route::CurrentDesktop);
    };
    if !cli.same_identity(desktop) {
        return Err(IdentityMismatch);
    }
    if requirement == Requirement::MapProject {
        if let Some(project) = cli.project {
            if desktop.project != Some(project) {
                return Ok(Route::SwitchDesktop { project });
            }
        }
    }
    Ok(Route::CurrentDesktop)
}

/// A successful switch must preserve the principal and reach the exact target.
/// The host additionally carries the observed session revision to dispatch.
pub fn switch_completed(before: Context<'_>, after: Context<'_>, target: &str) -> bool {
    before.same_identity(after) && after.project == Some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(project: Option<&str>) -> Context<'_> {
        Context {
            uid: "user",
            lane: "stable",
            audience: "audience",
            project,
        }
    }

    #[test]
    fn map_uses_cli_target_and_user_operations_leave_ui_alone() {
        let cli = Some(context(Some("cli-project")));
        let desktop = context(Some("ui-project"));
        assert_eq!(
            route(Requirement::MapProject, cli, desktop),
            Ok(Route::SwitchDesktop {
                project: "cli-project"
            })
        );
        assert_eq!(
            route(Requirement::DesktopUser, cli, desktop),
            Ok(Route::CurrentDesktop)
        );
        assert_eq!(
            route(Requirement::MapProject, cli, context(None)),
            Ok(Route::SwitchDesktop {
                project: "cli-project"
            })
        );
    }

    #[test]
    fn ui_supplies_context_when_cli_has_no_selection() {
        let desktop = context(Some("ui-project"));
        for cli in [None, Some(context(None)), Some(desktop)] {
            assert_eq!(
                route(Requirement::MapProject, cli, desktop),
                Ok(Route::CurrentDesktop)
            );
        }
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
                assert_eq!(
                    route(requirement, Some(original), changed),
                    Err(IdentityMismatch)
                );
            }
            assert!(!switch_completed(original, changed, "old-project"));
        }
        assert!(!switch_completed(original, original, "new-project"));
        assert!(switch_completed(
            original,
            context(Some("new-project")),
            "new-project"
        ));
    }
}
