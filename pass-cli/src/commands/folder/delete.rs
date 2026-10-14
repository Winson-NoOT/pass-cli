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

use crate::commands::item::ShareQuery;
use crate::helpers::CliPassClient as PassClient;
use anyhow::{Context, Result};
use pass::domain::FolderId;

pub async fn run(
    client: PassClient,
    share_id: Option<String>,
    vault_name: Option<String>,
    folder_id: String,
) -> Result<()> {
    let share_query = ShareQuery::new(share_id, vault_name)?;
    let share_id = share_query.share_id(&client).await?;
    let folder_id = FolderId::new(folder_id);

    client
        .delete_folder(&share_id, &folder_id)
        .await
        .context("Error deleting folder")?;

    println!("Folder deleted successfully");
    Ok(())
}
