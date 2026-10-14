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
use crate::helpers::CliPassClient as PassClient;
use anyhow::Result;
use clap::Subcommand;

mod create;
mod delete;
mod list;
mod r#move;
mod update;

#[derive(Subcommand)]
pub enum FolderCommands {
    #[command(about = "List folders in a vault")]
    List {
        #[arg(long, help = "Share ID of the vault")]
        share_id: Option<String>,
        #[arg(long, help = "Name of the vault")]
        vault_name: Option<String>,
        #[arg(long, help = "Parent folder ID to filter by")]
        parent_id: Option<String>,
        #[arg(long, default_value = "human")]
        output: OutputFormat,
    },
    #[command(about = "Create a new folder")]
    Create {
        #[arg(long, help = "Share ID of the vault")]
        share_id: Option<String>,
        #[arg(long, help = "Name of the vault")]
        vault_name: Option<String>,
        #[arg(long, help = "Name of the folder")]
        name: String,
        #[arg(long, help = "Parent folder ID (if creating under another folder)")]
        parent_id: Option<String>,
    },
    #[command(about = "Update a folder")]
    Update {
        #[arg(long, help = "Share ID of the vault")]
        share_id: Option<String>,
        #[arg(long, help = "Name of the vault")]
        vault_name: Option<String>,
        #[arg(long, help = "Folder ID to update")]
        folder_id: String,
        #[arg(long, help = "New name for the folder")]
        name: String,
    },
    #[command(about = "Delete a folder")]
    Delete {
        #[arg(long, help = "Share ID of the vault")]
        share_id: Option<String>,
        #[arg(long, help = "Name of the vault")]
        vault_name: Option<String>,
        #[arg(long, help = "Folder ID to delete")]
        folder_id: String,
    },
    #[command(about = "Move a folder")]
    Move {
        #[arg(long, help = "Share ID of the vault")]
        share_id: Option<String>,
        #[arg(long, help = "Name of the vault")]
        vault_name: Option<String>,
        #[arg(long, help = "Folder ID to move")]
        folder_id: String,
        #[arg(long, help = "New parent folder ID (omit to move to root)")]
        parent_folder_id: Option<String>,
    },
}

pub async fn run(command: FolderCommands, client: PassClient) -> Result<()> {
    match command {
        FolderCommands::List {
            share_id,
            vault_name,
            parent_id,
            output,
        } => list::run(client, share_id, vault_name, parent_id, output).await,
        FolderCommands::Create {
            share_id,
            vault_name,
            name,
            parent_id,
        } => create::run(client, share_id, vault_name, name, parent_id).await,
        FolderCommands::Update {
            share_id,
            vault_name,
            folder_id,
            name,
        } => update::run(client, share_id, vault_name, folder_id, name).await,
        FolderCommands::Delete {
            share_id,
            vault_name,
            folder_id,
        } => delete::run(client, share_id, vault_name, folder_id).await,
        FolderCommands::Move {
            share_id,
            vault_name,
            folder_id,
            parent_folder_id,
        } => r#move::run(client, share_id, vault_name, folder_id, parent_folder_id).await,
    }
}
