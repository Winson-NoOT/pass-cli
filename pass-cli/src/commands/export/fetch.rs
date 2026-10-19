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

use crate::helpers::CliPassClient as PassClient;
use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use jiff::Timestamp;
use pass::domain::ItemState;
use pass::domain::archive::{
    ArchiveAttachment, ArchiveData, ArchiveExtraField, ArchiveExtraFieldContent, ArchiveFolder,
    ArchiveItem, ArchiveItemContent, ArchiveVault, base64_encode,
};
use std::collections::{HashMap, HashSet};

pub type ProgressCallback<'a> = &'a mut dyn FnMut(usize, usize, String);

const CONCURRENT_FETCHES: usize = 10;

pub struct FetchOptions<'a> {
    pub vault_names: &'a [String],
    pub include_attachments: bool,
    pub on_progress: ProgressCallback<'a>,
}

const SHARED_WITH_ME_VAULT_ID: &str = "shared-with-me";

pub async fn collect_archive(
    client: &PassClient,
    options: FetchOptions<'_>,
) -> Result<ArchiveData> {
    (options.on_progress)(0, 0, "Listing shares".to_string());

    let all_vaults = client.list_vaults().await.context("Error listing vaults")?;

    if !options.vault_names.is_empty() {
        for name in options.vault_names {
            if !all_vaults.iter().any(|v| &v.content.name == name) {
                return Err(anyhow!("Could not find vault {name}"));
            }
        }
    }

    let selected_vaults: Vec<_> = all_vaults
        .into_iter()
        .filter(|v| options.vault_names.is_empty() || options.vault_names.contains(&v.content.name))
        .collect();

    let mut vaults: Vec<ArchiveVault> = selected_vaults
        .iter()
        .map(|v| ArchiveVault {
            id: v.id.value().to_string(),
            share_id: v.share_id.value().to_string(),
            name: v.content.name.clone(),
            description: v.content.description.clone(),
        })
        .collect();

    // Collect folders per selected vault share
    let mut folders: Vec<ArchiveFolder> = Vec::new();
    for vault in &selected_vaults {
        (options.on_progress)(0, 0, format!("Listing folders of {}", vault.content.name));
        match client.list_folders(&vault.share_id).await {
            Ok(share_folders) => {
                for folder in share_folders {
                    folders.push(ArchiveFolder {
                        id: folder.id.value().to_string(),
                        share_id: vault.share_id.value().to_string(),
                        parent_folder_id: folder
                            .parent_folder_id
                            .as_ref()
                            .map(|id| id.value().to_string()),
                        name: folder.content.name.clone(),
                    });
                }
            }
            Err(e) => {
                warn!(
                    "Error listing folders for vault {}: {e:#}",
                    vault.content.name
                );
            }
        }
    }

    let selected_share_ids: HashSet<String> = selected_vaults
        .iter()
        .map(|v| v.share_id.value().to_string())
        .collect();
    let vault_id_by_share_id: HashMap<String, String> = selected_vaults
        .iter()
        .map(|v| (v.share_id.value().to_string(), v.id.value().to_string()))
        .collect();

    let shares = client.list_shares().await.context("Error listing shares")?;
    let mut target_share_ids: Vec<String> = Vec::new();
    let mut has_item_shares = false;

    for share in &shares {
        match &share.share_type {
            pass::domain::ShareType::Vault { .. } => {
                let share_id = share.id.value().to_string();
                if selected_share_ids.contains(&share_id) {
                    target_share_ids.push(share_id);
                }
            }
            pass::domain::ShareType::Item { .. } | pass::domain::ShareType::Folder { .. } => {
                if options.vault_names.is_empty() {
                    has_item_shares = true;
                    target_share_ids.push(share.id.value().to_string());
                }
            }
        }
    }

    if has_item_shares {
        vaults.push(ArchiveVault {
            id: SHARED_WITH_ME_VAULT_ID.to_string(),
            share_id: String::new(),
            name: "Shared with me".to_string(),
            description: "Items shared directly with you".to_string(),
        });
    }

    (options.on_progress)(0, 0, "Listing items".to_string());
    let listed: Vec<Result<(String, Vec<pass::domain::Item>)>> =
        futures_util::stream::iter(target_share_ids.iter().cloned())
            .map(|share_id| {
                let client = client.clone();
                async move {
                    let sid = pass::domain::ShareId::new(share_id.clone());
                    let items = client
                        .list_items(&sid)
                        .await
                        .with_context(|| format!("Error listing items for share {share_id}"));
                    items.map(|items| (share_id, items))
                }
            })
            .buffered(CONCURRENT_FETCHES)
            .collect()
            .await;

    let mut listed_ok: Vec<(String, Vec<pass::domain::Item>)> = Vec::new();
    let mut item_failures = 0usize;
    for result in listed {
        match result {
            Ok(pair) => listed_ok.push(pair),
            Err(e) => {
                item_failures += 1;
                tracing::warn!("{e:#}");
            }
        }
    }
    if item_failures > 0 {
        tracing::warn!("{item_failures} share(s) could not be listed");
    }

    let listed_len: usize = listed_ok.iter().map(|(_, items)| items.len()).sum();
    let total = listed_len;
    (options.on_progress)(0, total, "Processing items".to_string());

    let mut items: Vec<ArchiveItem> = Vec::with_capacity(listed_len);
    for (share_id, share_items) in listed_ok {
        for item in &share_items {
            items.push(convert_item(
                vault_id_by_share_id
                    .get(&share_id)
                    .map(String::as_str)
                    .unwrap_or(SHARED_WITH_ME_VAULT_ID),
                &share_id,
                item,
            ));
        }
    }
    if options.include_attachments {
        download_attachments(client, &mut items, options.on_progress).await?;
    }

    items.sort_by_key(|a| a.title.to_lowercase());

    Ok(ArchiveData {
        version: 2,
        exported_at: Timestamp::now().to_string(),
        vaults,
        folders,
        items,
        has_attachments: options.include_attachments,
    })
}

fn convert_item(vault_id: &str, share_id: &str, item: &pass::domain::Item) -> ArchiveItem {
    let has_files = item.flags.contains(&pass::domain::ItemFlag::ItemHasFiles);
    ArchiveItem {
        id: item.id.value().to_string(),
        vault_id: vault_id.to_string(),
        share_id: share_id.to_string(),
        item_type: item_type_str(&item.content.content).to_string(),
        title: item.content.title.clone(),
        note: item.content.note.clone(),
        item_uuid: item.content.item_uuid.clone(),
        alias_email: item.alias_email.clone(),
        content: ArchiveItemContent::from(&item.content.content),
        extra_fields: item
            .content
            .extra_fields
            .iter()
            .map(|f| ArchiveExtraField {
                name: f.name.clone(),
                content: ArchiveExtraFieldContent::from(&f.content),
            })
            .collect(),
        create_time: item.create_time.to_string(),
        modify_time: item.modify_time.to_string(),
        attachments: Vec::new(), // populated in download_attachments when has_files
        trashed: item.state == ItemState::Trashed,
        has_files,
        folder_id: item.folder_id.as_ref().map(|id| id.value().to_string()),
    }
}

fn item_type_str(content: &pass::domain::ItemContent) -> &'static str {
    match content {
        pass::domain::ItemContent::Note(_) => "note",
        pass::domain::ItemContent::Login(_) => "login",
        pass::domain::ItemContent::Alias(_) => "alias",
        pass::domain::ItemContent::CreditCard(_) => "credit_card",
        pass::domain::ItemContent::Identity(_) => "identity",
        pass::domain::ItemContent::SshKey(_) => "ssh_key",
        pass::domain::ItemContent::Wifi(_) => "wifi",
        pass::domain::ItemContent::Custom(_) => "custom",
    }
}

async fn download_attachments(
    client: &PassClient,
    items: &mut [ArchiveItem],
    on_progress: ProgressCallback<'_>,
) -> Result<()> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Only items flagged `ItemHasFiles` carry attachments.
    let total_jobs: usize = items.iter().filter(|i| i.has_files).count();
    (on_progress)(0, total_jobs, "Downloading attachments".to_string());

    let done = Arc::new(AtomicUsize::new(0));
    let failures = Arc::new(AtomicUsize::new(0));

    // One `view_item` per file-bearing item (obtains the item key needed to
    // decrypt attachment keys), then download all of its files. Jobs run
    // concurrently via a buffered stream; progress is reported through shared
    // atomic counters (the callback is `FnMut`, not `Send`).
    futures_util::stream::iter(items.iter_mut().enumerate())
        .filter(|(_idx, item)| futures_util::future::ready(item.has_files))
        .map(|(idx, item)| {
            let client = client.clone();
            let done = done.clone();
            let failures = failures.clone();
            async move {
                let sid = pass::domain::ShareId::new(item.share_id.clone());
                let iid = pass::domain::ItemId::new(item.id.clone());

                let details = match client.view_item(&sid, &iid).await {
                    Ok(d) => d,
                    Err(e) => {
                        failures.fetch_add(1, Ordering::Relaxed);
                        tracing::warn!("Error loading attachments for item {}: {e:#}", item.id);
                        done.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };

                let count = details.attachments.len();
                let mut slots = Vec::with_capacity(count);
                for attachment in &details.attachments {
                    let mut data: Vec<u8> =
                        Vec::with_capacity((attachment.size as usize).min(8 * 1024 * 1024));
                    let dl = client
                        .download_attachment(&sid, &iid, attachment, |chunk: Vec<u8>| {
                            data.extend_from_slice(&chunk);
                            futures_util::future::ready(Ok(()))
                        })
                        .await;
                    let mut slot = ArchiveAttachment::pending(
                        attachment.id.value().to_string(),
                        attachment.content.name.clone(),
                        attachment.content.mime_type.clone(),
                        attachment.size,
                    );
                    match dl {
                        Ok(()) => {
                            slot.data_base64 = base64_encode(&data);
                            slot.size = data.len() as u64;
                        }
                        Err(e) => {
                            failures.fetch_add(1, Ordering::Relaxed);
                            tracing::warn!("Error downloading attachment {}: {e:#}", slot.id);
                        }
                    }
                    slots.push(slot);
                }
                item.attachments = slots;
                done.fetch_add(1, Ordering::Relaxed);
                let _ = idx;
            }
        })
        .buffered(CONCURRENT_FETCHES)
        .collect::<Vec<()>>()
        .await;

    let failure_count = failures.load(Ordering::Relaxed);
    if failure_count > 0 {
        tracing::warn!("{failure_count} attachment(s) could not be downloaded");
    }
    let _ = done;

    Ok(())
}
