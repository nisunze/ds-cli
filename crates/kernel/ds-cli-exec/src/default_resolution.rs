//! Default managed selection belongs to the packaged host lane, never PATH.
use super::*;

pub(super) struct Resolved {
    pub executable: PathBuf,
    pub managed_identity: Option<managed_modules::ManagedIdentity>,
    pub lane: Option<&'static str>,
}
impl Resolved {
    pub(super) fn verified<T>(
        &self,
        call: impl FnOnce() -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        let verify = || match &self.managed_identity {
            Some(identity) => managed_modules::verify(&self.executable, identity),
            None => Ok(()),
        };
        verify()?;
        let result = call();
        verify()?;
        result
    }
}

/// Match the packaged native client's lane, without interpreting an arbitrary
/// Linux target/release executable as an installed Stable host.
fn packaged_lane(
    debug: bool,
    root: &str,
    executable: Option<&Path>,
    windows: bool,
) -> Option<&'static str> {
    if debug {
        return None;
    }
    let root = root.to_ascii_lowercase();
    if !root.is_empty() {
        return Some(if root.contains("canary") {
            "canary"
        } else {
            "stable"
        });
    }
    if windows {
        let installed = executable
            .map(|path| path.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        return Some(if installed.contains("canary") {
            "canary"
        } else {
            "stable"
        });
    }
    (executable == Some(Path::new("/usr/bin/ds-canary"))).then_some("canary")
}

impl External {
    pub(super) fn resolve_default(&self) -> Result<Resolved, Failure> {
        let executable = std::env::current_exe().ok();
        let path = std::env::var_os("PATH");
        let lane = packaged_lane(
            cfg!(debug_assertions),
            option_env!("DS_NATIVE_CLIENT_PRODUCT_ROOT").unwrap_or_default(),
            executable.as_deref(),
            cfg!(windows),
        );
        self.resolve_default_for_paths(
            executable.as_deref(),
            path.as_deref(),
            lane,
            managed_modules::module_base,
        )
    }

    fn resolve_default_for_paths(
        &self,
        executable: Option<&Path>,
        path: Option<&OsStr>,
        lane: Option<&str>,
        module_base: impl FnOnce() -> Result<PathBuf, Failure>,
    ) -> Result<Resolved, Failure> {
        let lane = match lane {
            Some("stable") => Some("stable"),
            Some("canary") => Some("canary"),
            None => None,
            Some(_) => {
                return Err(Failure::invalid(
                    "module_lane_invalid",
                    "resolve exactly stable or canary",
                ));
            }
        };
        if std::env::var_os(self.env_override).is_none()
            && managed_modules::component(self).is_ok()
            && let Some(lane) = lane
        {
            let (selected, identity) = managed_modules::selected(self, &module_base()?, lane)?
                .ok_or_else(|| {
                    Failure::unavailable(
                        self.missing_code,
                        format!(
                            "the managed `{}` executable is not installed for {lane}",
                            self.name
                        ),
                    )
                    .remedy(self.remedy)
                })?;
            let executable = std::fs::canonicalize(selected)
                .ok()
                .filter(|path| is_executable(path))
                .ok_or_else(|| self.default_missing())?;
            return Ok(Resolved {
                executable,
                managed_identity: Some(identity),
                lane: Some(lane),
            });
        }
        let executable = self
            .locate_for_paths(executable, path)
            .ok_or_else(|| self.default_missing())?;
        Ok(Resolved {
            executable,
            managed_identity: None,
            lane,
        })
    }

    fn default_missing(&self) -> Failure {
        Failure::unavailable(self.missing_code, format!("`{}` was not found", self.name))
            .remedy(self.remedy)
            .detail(json!({"binary":self.name,"owner":self.owner,"override":self.env_override}))
    }
}

#[cfg(all(test, unix))]
mod tests;
