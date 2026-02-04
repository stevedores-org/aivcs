//! Nix-Env-Manager: Nix Flakes and Attic Integration for AIVCS
//!
//! This crate provides the environment versioning layer for AIVCS.
//! It interfaces with Nix Flakes for deterministic builds and
//! Attic for binary caching.
//!
//! ## Layer 2 - Environment/Tooling
//!
//! Focus: Correct hash generation and dependency resolution.

use anyhow::Result;
use sha2::{Digest, Sha256};
use std::path::Path;
use std::process::Command;

/// Nix environment hash
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NixHash(pub String);

impl std::fmt::Display for NixHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Generate environment hash from a Nix Flake
///
/// # TDD: test_changing_flake_input_changes_hash
pub fn generate_environment_hash(flake_path: &Path) -> Result<NixHash> {
    let lock_path = flake_path.join("flake.lock");

    if lock_path.exists() {
        // Hash the flake.lock file
        let content = std::fs::read(&lock_path)?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        let hash = hex::encode(hasher.finalize());
        Ok(NixHash(hash))
    } else {
        // Try to generate hash via nix command
        let output = Command::new("nix")
            .args(["flake", "metadata", "--json"])
            .current_dir(flake_path)
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let mut hasher = Sha256::new();
                hasher.update(&out.stdout);
                let hash = hex::encode(hasher.finalize());
                Ok(NixHash(hash))
            }
            _ => {
                // Fallback: hash the flake.nix itself
                let flake_nix = flake_path.join("flake.nix");
                if flake_nix.exists() {
                    let content = std::fs::read(&flake_nix)?;
                    let mut hasher = Sha256::new();
                    hasher.update(&content);
                    let hash = hex::encode(hasher.finalize());
                    Ok(NixHash(hash))
                } else {
                    anyhow::bail!("No flake.lock or flake.nix found at {:?}", flake_path)
                }
            }
        }
    }
}

/// Check if environment is cached in Attic
///
/// # TDD: test_pull_nonexistent_hash_fails_gracefully
pub async fn is_environment_cached(hash: &NixHash) -> bool {
    // TODO: Implement Attic client query
    // For now, always return false (not cached)
    let _ = hash;
    false
}

/// Pull environment from Attic cache
pub async fn pull_environment(hash: &NixHash) -> Result<std::path::PathBuf> {
    // TODO: Implement Attic pull
    anyhow::bail!("Environment {} not found in cache", hash)
}

/// Push environment to Attic cache
pub async fn push_environment(hash: &NixHash, _build_path: &Path) -> Result<()> {
    // TODO: Implement Attic push
    tracing::info!("Would push environment {} to Attic", hash);
    Ok(())
}

/// Generate logic hash from source code
///
/// # TDD: test_changing_rust_source_changes_logic_hash
pub fn generate_logic_hash(source_path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();

    // Walk the source directory and hash all .rs files
    if source_path.is_dir() {
        for entry in walkdir(source_path)? {
            if entry.extension().map(|e| e == "rs").unwrap_or(false) {
                let content = std::fs::read(&entry)?;
                hasher.update(&content);
            }
        }
    } else if source_path.is_file() {
        let content = std::fs::read(source_path)?;
        hasher.update(&content);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Simple directory walker (no external dependency)
fn walkdir(dir: &Path) -> Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();

    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                files.extend(walkdir(&path)?);
            } else {
                files.push(path);
            }
        }
    }

    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_generate_logic_hash_deterministic() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        std::fs::write(&file_path, b"fn main() {}").unwrap();

        let hash1 = generate_logic_hash(&file_path).unwrap();
        let hash2 = generate_logic_hash(&file_path).unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_changing_rust_source_changes_logic_hash() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.rs");

        std::fs::write(&file_path, b"fn main() { println!(\"v1\"); }").unwrap();
        let hash1 = generate_logic_hash(&file_path).unwrap();

        std::fs::write(&file_path, b"fn main() { println!(\"v2\"); }").unwrap();
        let hash2 = generate_logic_hash(&file_path).unwrap();

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_nix_hash_from_lock_file() {
        let dir = tempdir().unwrap();
        let lock_path = dir.path().join("flake.lock");
        let mut file = std::fs::File::create(&lock_path).unwrap();
        file.write_all(b"{\"nodes\": {}}").unwrap();

        let hash = generate_environment_hash(dir.path()).unwrap();
        assert!(!hash.0.is_empty());
        assert_eq!(hash.0.len(), 64); // SHA256 hex
    }
}
