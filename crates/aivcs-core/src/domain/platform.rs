//! Platform detection and environment validation.

use std::fs;
use std::path::Path;

/// Supported platforms for AIVCS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    GenericLinux,
    MacOS,
    NixOS,
    NixOSWSL,
    Unknown,
}

impl Platform {
    /// Detect the current platform.
    pub fn detect() -> Self {
        if cfg!(target_os = "macos") {
            return Self::MacOS;
        }

        if cfg!(target_os = "linux") {
            let is_nixos = Path::new("/etc/NIXOS").exists()
                || fs::read_to_string("/etc/os-release")
                    .map(|v| v.contains("ID=nixos"))
                    .unwrap_or(false);

            let is_wsl = fs::read_to_string("/proc/version")
                .map(|v| v.to_lowercase().contains("microsoft") || v.to_lowercase().contains("wsl"))
                .unwrap_or(false);

            return match (is_nixos, is_wsl) {
                (true, true) => Self::NixOSWSL,
                (true, false) => Self::NixOS,
                (false, _) => Self::GenericLinux,
            };
        }

        Self::Unknown
    }

    pub fn is_wsl(&self) -> bool {
        matches!(self, Self::NixOSWSL)
    }

    pub fn is_nixos(&self) -> bool {
        matches!(self, Self::NixOS | Self::NixOSWSL)
    }
}

/// Validation result for the environment.
pub struct EnvValidation {
    pub platform: Platform,
    pub nix_available: bool,
    pub attic_available: bool,
    pub is_nix_shell: bool,
}

impl EnvValidation {
    pub fn check() -> Self {
        Self {
            platform: Platform::detect(),
            nix_available: is_command_available("nix"),
            attic_available: is_command_available("attic"),
            is_nix_shell: std::env::var("IN_NIX_SHELL").is_ok()
                || (std::env::var("SHLVL").map(|v| v == "2").unwrap_or(false)
                    && std::env::var("PATH")
                        .map(|v| v.contains("/nix/store"))
                        .unwrap_or(false)),
        }
    }
}

fn is_command_available(cmd: &str) -> bool {
    std::process::Command::new(cmd)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection_smoke() {
        let p = Platform::detect();
        // Just verify it doesn't panic and returns something plausible
        if cfg!(target_os = "macos") {
            assert_eq!(p, Platform::MacOS);
        }
    }
}
