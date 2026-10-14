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

use crate::commands::OutputFormat;
use crate::commands::item::ShareQuery;
use crate::helpers::CliPassClient as PassClient;
use anyhow::{Context, Result};

pub async fn run(
    client: PassClient,
    share_id: Option<String>,
    vault_name: Option<String>,
    parent_id: Option<String>,
    output: OutputFormat,
) -> Result<()> {
    let share_query = ShareQuery::new(share_id, vault_name)?;
    let share_id = share_query.share_id(&client).await?;

    let mut folders = client
        .list_folders(&share_id)
        .await
        .context("Error listing folders")?;

    // Filter by parent_id if provided
    if let Some(parent_id_filter) = parent_id {
        folders.retain(|folder| {
            folder
                .parent_folder_id
                .as_ref()
                .map(|id| id.to_string() == parent_id_filter)
                .unwrap_or(false)
        });
    }

    match output {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&folders).context("Error serializing folders")?;
            println!("{json}");
        }
        OutputFormat::Human => {
            for folder in folders {
                let parent_info = folder
                    .parent_folder_id
                    .as_ref()
                    .map(|id| format!(" (parent: {})", id))
                    .unwrap_or_default();
                println!("- {}: {}{}", folder.content.name, folder.id, parent_info);
            }
        }
    }

    Ok(())
}
