//! One visible identity for the packaged MCP server.
//!
//! Host configuration keys, MCP protocol identity, and human titles describe
//! the same executable without changing any canonical `ds` command or tool id.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimePlatform {
    Windows,
    Macos,
    Linux,
    Wsl,
}

impl RuntimePlatform {
    pub(crate) fn current() -> Self {
        let native = match std::env::consts::OS {
            "windows" => Self::Windows,
            "macos" => Self::Macos,
            _ => Self::Linux,
        };
        #[cfg(target_os = "linux")]
        {
            let detected = linux_platform_from_release(
                std::fs::read_to_string("/proc/sys/kernel/osrelease")
                    .ok()
                    .as_deref(),
            );
            if detected == Self::Wsl {
                detected
            } else {
                native
            }
        }
        #[cfg(not(target_os = "linux"))]
        native
    }

    pub(crate) const fn token(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Wsl => "wsl",
        }
    }

    const fn camel(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Macos => "Macos",
            Self::Linux => "Linux",
            Self::Wsl => "Wsl",
        }
    }

    const fn title(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Macos => "macOS",
            Self::Linux => "Linux",
            Self::Wsl => "WSL",
        }
    }
}

#[cfg(target_os = "linux")]
fn linux_platform_from_release(release: Option<&str>) -> RuntimePlatform {
    if release.is_some_and(|value| value.to_ascii_lowercase().contains("microsoft")) {
        RuntimePlatform::Wsl
    } else {
        RuntimePlatform::Linux
    }
}

pub(crate) const fn packaged_lane() -> &'static str {
    env!("DS_MCP_RELEASE_LANE")
}

fn lane_camel(lane: &str) -> &'static str {
    match lane {
        "stable" => "Stable",
        "canary" => "Canary",
        "headless" => "Headless",
        "local" => "Local",
        "development" => "Development",
        _ => unreachable!("build.rs admits only closed MCP release lanes"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerIdentity {
    lane: &'static str,
    platform: RuntimePlatform,
}

impl ServerIdentity {
    pub(crate) fn current() -> Self {
        Self {
            lane: packaged_lane(),
            platform: RuntimePlatform::current(),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(lane: &'static str, platform: RuntimePlatform) -> Self {
        Self { lane, platform }
    }

    pub(crate) const fn lane(&self) -> &'static str {
        self.lane
    }

    pub(crate) const fn platform(&self) -> &'static str {
        self.platform.token()
    }

    /// MCP's protocol-facing, machine-safe server identity.
    pub(crate) fn protocol_name(&self) -> String {
        format!("ds-{}-{}", self.lane, self.platform.token())
    }

    /// Cross-host configuration key. VS Code recommends camelCase server ids.
    pub(crate) fn registration_name(&self) -> String {
        format!(
            "dsGridDesign{}{}",
            lane_camel(self.lane),
            self.platform.camel()
        )
    }

    /// Human-facing title shown by hosts that implement MCP `serverInfo.title`.
    pub(crate) fn title(&self) -> String {
        format!(
            "DS GridDesign — {} on {}",
            lane_camel(self.lane),
            self.platform.title()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_windows_identity_has_distinct_machine_config_and_human_names() {
        let identity = ServerIdentity::new("stable", RuntimePlatform::Windows);
        assert_eq!(identity.protocol_name(), "ds-stable-windows");
        assert_eq!(identity.registration_name(), "dsGridDesignStableWindows");
        assert_eq!(identity.title(), "DS GridDesign — Stable on Windows");
    }

    #[test]
    fn canary_wsl_identity_never_collides_with_native_linux() {
        let wsl = ServerIdentity::new("canary", RuntimePlatform::Wsl);
        let linux = ServerIdentity::new("canary", RuntimePlatform::Linux);
        assert_eq!(wsl.protocol_name(), "ds-canary-wsl");
        assert_eq!(wsl.registration_name(), "dsGridDesignCanaryWsl");
        assert_ne!(wsl.protocol_name(), linux.protocol_name());
        assert_ne!(wsl.registration_name(), linux.registration_name());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn wsl_detection_uses_kernel_evidence_not_a_mutable_lane_flag() {
        assert_eq!(
            linux_platform_from_release(Some("5.15.153.1-microsoft-standard-WSL2")),
            RuntimePlatform::Wsl
        );
        assert_eq!(
            linux_platform_from_release(Some("6.8.0-79-generic")),
            RuntimePlatform::Linux
        );
        assert_eq!(linux_platform_from_release(None), RuntimePlatform::Linux);
    }
}
