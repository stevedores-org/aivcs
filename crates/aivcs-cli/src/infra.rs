//! `aivcs infra` — sovereign infra reconcilers (no GitHub Actions).

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;

use aivcs_core::infra::cloudflare_lb::{
    build_audit_report, fetch_load_balancers, fetch_pools, parse_allowlist, prune_orphans,
    render_audit_markdown, resolve_cf_credentials,
};
use aivcs_core::infra::gsm::{resolve_project_id, vault_kv_path_to_gsm_secret_id, GsmClient};

#[derive(clap::Subcommand)]
pub enum InfraAction {
    /// Cloudflare Load Balancer pool hygiene
    CloudflareLb {
        #[command(subcommand)]
        action: CloudflareLbAction,
    },
    /// Flux GitOps reconcile (in-cluster; replaces GHA kubectl jobs)
    Flux {
        #[command(subcommand)]
        action: FluxAction,
    },
    /// Google Cloud Secret Manager operator-controlled writes
    Gsm {
        /// GCP Project ID override
        #[arg(long, global = true)]
        project: Option<String>,

        /// GCP Secret Manager canonical prefix override
        #[arg(long, default_value = "aivcs-secrets--", global = true)]
        prefix: String,

        #[command(subcommand)]
        action: GsmAction,
    },
}

#[derive(clap::Subcommand)]
pub enum CloudflareLbAction {
    /// Compare live CF pools against a git allowlist
    Audit {
        /// Allowlist file (one pool name per line; `#` comments)
        #[arg(short, long)]
        allowlist: PathBuf,

        /// Emit JSON instead of Markdown
        #[arg(long)]
        json: bool,
    },
    /// Delete unreferenced orphan pools (respects LB references)
    Prune {
        #[arg(short, long)]
        allowlist: PathBuf,

        /// List candidates without calling DELETE
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(clap::Subcommand)]
pub enum FluxAction {
    /// `flux reconcile kustomization <name> --with-source`
    Reconcile {
        #[arg(short, long)]
        kustomization: String,

        #[arg(short, long, default_value = "flux-system")]
        namespace: String,

        #[arg(long, default_value_t = true)]
        with_source: bool,
    },
}

#[derive(clap::Subcommand)]
pub enum GsmAction {
    /// Create or update a secret in GSM under the canonical prefix
    #[command(alias = "write")]
    StoreSecret {
        /// Vault KV v2 secret path (e.g., ci/repos/lornu-ai/aivcs-lornu-demo)
        path: String,

        /// Key-value pairs in KEY=VALUE or KEY format (looks up KEY in env)
        #[arg(num_args = 0..)]
        key_values: Vec<String>,

        /// Read secret payload from a JSON file
        #[arg(long)]
        from_file: Option<PathBuf>,
    },
    /// List all secrets in the GCP project matching the canonical prefix
    List {
        /// Emit JSON instead of clean Vault-compatible layout
        #[arg(long)]
        json: bool,
    },
    /// Delete/destroy a secret in GSM
    Delete {
        /// Vault KV v2 secret path to delete
        path: String,

        /// Force deletion without confirmation prompts
        #[arg(long)]
        force: bool,
    },
}

pub async fn run(action: InfraAction) -> Result<()> {
    match action {
        InfraAction::CloudflareLb { action } => run_cloudflare_lb(action).await,
        InfraAction::Flux { action } => run_flux(action),
        InfraAction::Gsm {
            project,
            prefix,
            action,
        } => run_gsm(project.as_deref(), &prefix, action).await,
    }
}

async fn run_cloudflare_lb(action: CloudflareLbAction) -> Result<()> {
    match action {
        CloudflareLbAction::Audit { allowlist, json } => {
            let content = std::fs::read_to_string(&allowlist)
                .with_context(|| format!("read allowlist {:?}", allowlist))?;
            let allow = parse_allowlist(&content);
            let (token, account_id) = resolve_cf_credentials()?;
            let http = reqwest::Client::new();
            let pools = fetch_pools(&http, &account_id, &token).await?;
            let lbs = fetch_load_balancers(&http, &account_id, &token).await?;
            let report = build_audit_report(&allow, &pools, &lbs);
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", render_audit_markdown(&report));
            }
            if !report.orphans.is_empty() {
                std::process::exit(2);
            }
            Ok(())
        }
        CloudflareLbAction::Prune { allowlist, dry_run } => {
            let content = std::fs::read_to_string(&allowlist)
                .with_context(|| format!("read allowlist {:?}", allowlist))?;
            let allow = parse_allowlist(&content);
            let (token, account_id) = resolve_cf_credentials()?;
            let http = reqwest::Client::new();
            let pools = fetch_pools(&http, &account_id, &token).await?;
            let lbs = fetch_load_balancers(&http, &account_id, &token).await?;
            let report = build_audit_report(&allow, &pools, &lbs);
            let deleted = prune_orphans(&http, &account_id, &token, &report, dry_run).await?;
            for name in &deleted {
                if dry_run {
                    println!("would prune: {name}");
                } else {
                    println!("pruned: {name}");
                }
            }
            Ok(())
        }
    }
}

fn run_flux(action: FluxAction) -> Result<()> {
    match action {
        FluxAction::Reconcile {
            kustomization,
            namespace,
            with_source,
        } => {
            aivcs_core::run_reconcile(&kustomization, &namespace, with_source)?;
            println!("✓ reconciled kustomization {kustomization} in {namespace}");
            Ok(())
        }
    }
}

async fn run_gsm(override_project: Option<&str>, prefix: &str, action: GsmAction) -> Result<()> {
    let project_id = resolve_project_id(override_project)?;
    let client = GsmClient::new(project_id).await?;

    match action {
        GsmAction::StoreSecret {
            path,
            key_values,
            from_file,
        } => {
            let secret_id = vault_kv_path_to_gsm_secret_id(&path, prefix).with_context(|| {
                format!(
                    "invalid secret path: '{}'. Only ci/gcp, ci/repos/*, agents/*, kubernetes/*, or prod/* are allowed.",
                    path
                )
            })?;

            let data = parse_key_values(&key_values, from_file.as_deref())?;
            client.store_secret(&secret_id, data).await?;
            println!(
                "✓ secret stored successfully: {} (GSM ID: {})",
                path, secret_id
            );
            Ok(())
        }
        GsmAction::List { json } => {
            let secrets = client.list_secrets(prefix).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&secrets)?);
            } else {
                if secrets.is_empty() {
                    println!("No secrets found matching prefix '{}'", prefix);
                } else {
                    for s in secrets {
                        println!("{}", s);
                    }
                }
            }
            Ok(())
        }
        GsmAction::Delete { path, force } => {
            let secret_id = vault_kv_path_to_gsm_secret_id(&path, prefix)
                .with_context(|| format!("invalid secret path: '{}'", path))?;

            if !force && !confirm_deletion(&path, &secret_id)? {
                println!("Operation cancelled.");
                return Ok(());
            }

            client.delete_secret(&secret_id).await?;
            println!(
                "✓ secret deleted successfully: {} (GSM ID: {})",
                path, secret_id
            );
            Ok(())
        }
    }
}

fn parse_key_values(
    key_values: &[String],
    from_file: Option<&std::path::Path>,
) -> Result<HashMap<String, String>> {
    let mut map = HashMap::new();

    // 1. Read flat JSON if provided
    if let Some(file_path) = from_file {
        let content = std::fs::read_to_string(file_path)
            .with_context(|| format!("failed to read file: {:?}", file_path))?;
        let file_map: HashMap<String, String> =
            serde_json::from_str(&content).with_context(|| {
                format!(
                    "failed to parse file {:?} as JSON (must be flat string-to-string object)",
                    file_path
                )
            })?;
        map.extend(file_map);
    }

    // 2. Parse inline key-value arguments
    for kv in key_values {
        if let Some(pos) = kv.find('=') {
            let key = kv[..pos].trim().to_string();
            let val = kv[pos + 1..].trim();
            if key.is_empty() {
                bail!("invalid empty key in argument: '{}'", kv);
            }
            if let Some(env_var) = val.strip_prefix("env:") {
                let env_val = std::env::var(env_var).with_context(|| {
                    format!(
                        "environment variable '{}' not set for key '{}'",
                        env_var, key
                    )
                })?;
                map.insert(key, env_val);
            } else {
                map.insert(key, val.to_string());
            }
        } else {
            let key = kv.trim().to_string();
            if key.is_empty() {
                bail!("invalid empty argument");
            }
            let env_val = std::env::var(&key)
                .with_context(|| format!("environment variable '{}' not set", key))?;
            map.insert(key, env_val);
        }
    }

    if map.is_empty() {
        bail!("no secret key-value pairs provided. Pass KEY=VALUE arguments, set environment variables, or use --from-file.");
    }

    Ok(map)
}

fn confirm_deletion(path: &str, secret_id: &str) -> Result<bool> {
    print!(
        "WARNING: You are about to permanently delete the secret '{}' (GSM ID: {}).\nThis operation cannot be undone. Are you sure? [y/N]: ",
        path, secret_id
    );
    io::stdout().flush().context("flush stdout")?;

    let mut response = String::new();
    io::stdin().read_line(&mut response).context("read stdin")?;
    let response = response.trim().to_lowercase();
    Ok(response == "y" || response == "yes")
}
