/*
 *  Copyright (c) 2026 Proton AG
 *  This file is part of Proton AG and Proton Pass.
 *
 *  Proton Pass is free software: you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation, either version 3 of the License, or
 *  (at your option) any later version.
 *
 *  Proton Pass is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with Proton Pass.  If not, see <https://www.gnu.org/licenses/>.
 *
 */

mod fetch;

use pass::domain::archive as archive_lib;

use crate::helpers::CliPassClient as PassClient;
use crate::utils::ask_for_input;
use anyhow::{Context, Result, anyhow};
use clap::{Args, Subcommand};
use std::io::Write as _;
use std::path::PathBuf;
use zeroize::{Zeroize, Zeroizing};

#[derive(Subcommand)]
pub enum ExportCommands {
    #[command(about = "Export vaults and items to an offline HTML archive")]
    Archive(ArchiveArgs),
}

pub async fn run(command: ExportCommands, client: PassClient) -> Result<()> {
    match command {
        ExportCommands::Archive(args) => archive(args, client).await,
    }
}

#[derive(Args, Debug)]
pub struct ArchiveArgs {
    #[arg(
        long = "vault-name",
        value_name = "NAME",
        help = "Vault to export (repeatable; omit to export all vaults plus shared items)"
    )]
    pub vault_names: Vec<String>,

    #[arg(
        long,
        help = "Also download and embed file attachments (increases file size)"
    )]
    pub include_attachments: bool,

    #[arg(
        long,
        help = "Encrypt the archive with a password (AES-256-GCM + Argon2id)"
    )]
    pub encrypt: bool,

    #[arg(
        long,
        help = "Environment variable holding the encryption password (requires --encrypt)"
    )]
    pub password_env: Option<String>,

    #[arg(
        short,
        long,
        help = "Output path for the generated HTML file",
        default_value = "proton-pass-archive.html"
    )]
    pub output: PathBuf,
}

async fn archive(args: ArchiveArgs, client: PassClient) -> Result<()> {
    let ArchiveArgs {
        vault_names,
        include_attachments,
        encrypt,
        password_env,
        output,
    } = args;

    if password_env.is_some() && !encrypt {
        return Err(anyhow!("--password-env requires --encrypt"));
    }

    let password: Option<Zeroizing<String>> = if encrypt {
        Some(resolve_password(password_env.as_deref())?)
    } else {
        None
    };

    let mut spin_step = 0usize;
    let mut progress_print = |done: usize, total: usize, label: String| {
        // Single-line, in-place progress with a spinning frame:
        //  - determinate, in progress: `⠙ Downloading attachments: 3/8 (37%)`
        //  - determinate, unknown count yet: `⠙ Processing items (58)`
        //  - indeterminate: `⠙ Listing shares...`
        let spinner = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        spin_step = (spin_step + 1) % spinner.len();
        let frame = spinner[spin_step];

        let line = match (total, done) {
            (0, _) => format!("{frame} {label}..."),
            (t, 0) => format!("{frame} {label} ({t})"),
            (t, d) if d >= t => format!("{frame} {label}: {t}/{t} ✓"),
            (t, d) => {
                let pct = (d * 100) / t;
                format!("{frame} {label}: {d}/{t} ({pct}%)")
            }
        };

        eprint!("\r\x1b[2K{line}");
        let _ = std::io::stderr().flush();
    };

    eprintln!("Collecting items from Pass...");
    let data = fetch::collect_archive(
        &client,
        fetch::FetchOptions {
            vault_names: &vault_names,
            include_attachments,
            on_progress: &mut progress_print,
        },
    )
    .await
    .context("Error collecting archive data")?;
    // Clear the in-place progress line before printing the summary.
    eprint!("\r\x1b[2K");
    let _ = std::io::stderr().flush();

    eprintln!(
        "Collected {} item(s) across {} vault(s){}",
        data.items.len(),
        data.vaults.len(),
        if include_attachments {
            format!(
                " with {} attachment(s)",
                data.items
                    .iter()
                    .map(|i| i.attachments.len())
                    .sum::<usize>()
            )
        } else {
            String::new()
        }
    );

    let data_json = serde_json::to_string(&data).context("Error serializing archive data")?;

    let envelope = if let Some(password) = &password {
        let salt = pass::domain::crypto::random_bytes(archive_lib::SALT_LEN);
        let kdf = archive_lib::ArchiveKdfParams::current();
        let mut key = archive_lib::derive_key(password, &salt, &kdf)?;
        let encrypted = archive_lib::encrypt_blob(data_json.as_bytes(), &key)?;
        let envelope = archive_lib::ArchiveEnvelope {
            version: 1,
            kdf,
            salt: archive_lib::hex_encode(&salt),
            blob: archive_lib::base64_encode(&encrypted),
        };
        // Best-effort scrubbing of the derived key.
        key.zeroize();
        Some(envelope)
    } else {
        None
    };

    let html = archive_lib::render(archive_lib::ViewerInput {
        data_json: &data_json,
        envelope: envelope.as_ref(),
        exported_at: &data.exported_at,
        has_attachments: include_attachments,
        item_count: data.items.len(),
        vault_count: data.vaults.len(),
    })?;

    write_output(&output, html.as_bytes())?;

    println!(
        "Archive written to {} ({} item(s), {} vault(s), encrypted: {})",
        output.display(),
        data.items.len(),
        data.vaults.len(),
        if password.is_some() { "yes" } else { "no" }
    );

    Ok(())
}

fn resolve_password(password_env: Option<&str>) -> Result<Zeroizing<String>> {
    if let Some(env_name) = password_env {
        let value = std::env::var(env_name)
            .map_err(|_| anyhow!("Environment variable {env_name} must be set and non-empty"))?;
        if value.is_empty() {
            return Err(anyhow!(
                "Environment variable {env_name} must be set and non-empty"
            ));
        }
        Ok(Zeroizing::new(value))
    } else {
        let password = Zeroizing::new(ask_for_input("Enter archive password: ", true)?);
        if password.is_empty() {
            return Err(anyhow!("Password cannot be empty"));
        }
        let confirmation = Zeroizing::new(ask_for_input("Confirm archive password: ", true)?);
        if password.as_str() != confirmation.as_str() {
            return Err(anyhow!("Passwords do not match"));
        }
        Ok(password)
    }
}

fn write_output(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    // Restrict permissions on Unix: the archive contains all secrets.
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("Error creating {}", path.display()))?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .context("Error restricting archive permissions")?;
        file.write_all(bytes)
            .context("Error writing archive output")?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes).with_context(|| format!("Error writing {}", path.display()))?;
    }
    Ok(())
}
