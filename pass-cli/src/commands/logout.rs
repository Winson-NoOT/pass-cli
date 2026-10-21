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

use crate::helpers::{CliPassClient as PassClient, SessionExt as _};
use crate::telemetry::event::CommandEvent;
use crate::utils::get_base_dir;
use anyhow::{Context, Result, anyhow};
use parking_lot::RwLock;
use pass::auth::PassSessionStore;
use std::sync::Arc;
use tracing::warn;

async fn remove_local_data() -> Result<()> {
    let base_dir = get_base_dir().context("Error getting base dir")?;
    if !base_dir.exists() {
        println!("There was no data to be removed");
        return Ok(());
    }

    if base_dir.is_dir() {
        tokio::fs::remove_dir_all(&base_dir)
            .await
            .context("Error deleting base dir")?;
        Ok(())
    } else {
        Err(anyhow!(
            "Base directory is not a directory: {}",
            base_dir.display()
        ))
    }
}

pub async fn run(client: PassClient, store: Arc<RwLock<PassSessionStore>>) -> Result<()> {
    client.emit_telemetry(&CommandEvent::new("logout")).await;
    if let Err(e) = client.logout().await {
        eprintln!("Error logging out: {}", e);
        eprintln!(
            "There has been an error during the logout process. If it persists, you may run 'pass-cli logout --force'"
        );
        std::process::exit(1);
    }

    store.flush().await;

    let key_provider = client
        .get_key_provider()
        .await
        .context("Error getting key provider")?;
    if let Err(e) = key_provider.remove_key().await {
        warn!("Error removing local key: {e:#}");
    }

    remove_local_data().await?;
    println!("Successfully logged out");
    Ok(())
}

pub async fn force_logout() -> Result<()> {
    println!("Executing force logout");

    if let Ok(base_dir) = get_base_dir() {
        crate::features::try_cleanup_all_key_providers(base_dir).await;
    }

    cleanup().await?;
    println!("Successfully performed force logout");
    Ok(())
}

pub async fn cleanup() -> Result<()> {
    remove_local_data().await?;
    Ok(())
}
