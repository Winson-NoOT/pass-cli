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

use crate::commands::item::list::{FilterType, ItemSummary, SortBy};
use crate::commands::{OutputFormat, settings_helper};
use crate::helpers::CliPassClient as PassClient;
use anyhow::{Context, Result, bail};
use pass::domain::{Item, ItemContent, ItemState, ShareId, Vault};
use std::collections::HashMap;

#[derive(serde::Serialize)]
struct SearchResults {
    items: Vec<SearchItemSummary>,
}

#[derive(serde::Serialize)]
struct SearchItemSummary {
    #[serde(flatten)]
    item: ItemSummary,
    vault_name: String,
}

pub struct SearchOptions {
    pub query: String,
    pub share_id: Option<String>,
    pub vault_name: Option<String>,
    pub filter_type: Option<FilterType>,
    pub sort_by: Option<SortBy>,
    pub limit: Option<usize>,
    pub output: Option<OutputFormat>,
}

pub async fn run(client: PassClient, options: SearchOptions) -> Result<()> {
    if options.share_id.is_some() && options.vault_name.is_some() {
        bail!("Please provide either --share-id or --vault-name, not both");
    }

    let query = options.query.trim().to_string();
    if query.is_empty() {
        bail!("Search query must not be empty");
    }
    let output = match options.output {
        Some(fmt) => fmt,
        None => settings_helper::get_default_format(&client)
            .await?
            .unwrap_or(OutputFormat::Human),
    };

    let all_vaults = client.list_vaults().await.context("Error listing vaults")?;

    let vaults = select_vaults(
        all_vaults,
        options.share_id.as_deref(),
        options.vault_name.as_deref(),
    )?;
    let vault_names: HashMap<ShareId, String> = vaults
        .iter()
        .map(|vault| (vault.share_id.clone(), vault.content.name.clone()))
        .collect();

    let mut items = Vec::new();
    for vault in &vaults {
        let vault_items = match client.list_items(&vault.share_id).await {
            Ok(items) => items,
            Err(e) => {
                error!("Error listing items in vault {}: {e:#}", vault.content.name);
                continue;
            }
        };

        for item in vault_items {
            if item.state != ItemState::Active {
                continue;
            }
            if let Some(filter) = options.filter_type
                && !filter.matches(&item.content.content)
            {
                continue;
            }
            if matches_query(&item, &query) {
                items.push(item);
            }
        }
    }

    if let Some(sort) = options.sort_by {
        sort.sort_items(&mut items);
    }
    if let Some(limit) = options.limit {
        items.truncate(limit);
    }

    match output {
        OutputFormat::Json => {
            let results = SearchResults {
                items: items
                    .iter()
                    .map(|item| SearchItemSummary {
                        item: ItemSummary::from(item),
                        vault_name: vault_names.get(&item.share_id).cloned().unwrap_or_default(),
                    })
                    .collect(),
            };
            let json = serde_json::to_string_pretty(&results)
                .context("Error serializing search results")?;
            println!("{json}");
        }
        OutputFormat::Human => {
            if items.is_empty() {
                eprintln!("No items found for '{query}'.");
                return Ok(());
            }
            for item in &items {
                let vault_name = vault_names
                    .get(&item.share_id)
                    .map(String::as_str)
                    .unwrap_or("unknown");
                println!(
                    "- [{}]: {} (vault: {})",
                    item.id, item.content.title, vault_name
                );
            }
        }
    }

    Ok(())
}

fn select_vaults(
    vaults: Vec<Vault>,
    share_id: Option<&str>,
    vault_name: Option<&str>,
) -> Result<Vec<Vault>> {
    match (share_id, vault_name) {
        (Some(share_id), None) => {
            let selected: Vec<Vault> = vaults
                .into_iter()
                .filter(|vault| vault.share_id.value() == share_id)
                .collect();
            if selected.is_empty() {
                bail!("Could not find vault with share id {share_id}");
            }
            Ok(selected)
        }
        (None, Some(vault_name)) => {
            let selected: Vec<Vault> = vaults
                .into_iter()
                .filter(|vault| vault.content.name == vault_name)
                .collect();
            if selected.is_empty() {
                bail!("Could not find vault {vault_name}");
            }
            Ok(selected)
        }
        (None, None) => Ok(vaults),
        (Some(_), Some(_)) => bail!("Please provide either --share-id or --vault-name, not both"),
    }
}

fn matches_query(item: &Item, query: &str) -> bool {
    let query_lower = query.to_lowercase();
    if contains(&item.content.title, &query_lower) {
        return true;
    }

    match &item.content.content {
        ItemContent::Login(login) => {
            any_contains(
                [login.email.as_str(), login.username.as_str()],
                &query_lower,
            ) || any_contains(login.urls.iter().map(String::as_str), &query_lower)
                || any_contains(
                    login.autofill_urls.iter().map(|url| url.url.as_str()),
                    &query_lower,
                )
        }
        ItemContent::Alias(_) => item
            .alias_email
            .as_deref()
            .is_some_and(|email| contains(email, &query_lower)),
        ItemContent::Identity(identity) => any_contains(
            [
                identity.email.as_str(),
                identity.work_email.as_str(),
                identity.website.as_str(),
                identity.personal_website.as_str(),
            ],
            &query_lower,
        ),
        ItemContent::Note(_)
        | ItemContent::CreditCard(_)
        | ItemContent::SshKey(_)
        | ItemContent::Wifi(_)
        | ItemContent::Custom(_) => false,
    }
}

fn any_contains<'a>(values: impl IntoIterator<Item = &'a str>, query_lower: &str) -> bool {
    values.into_iter().any(|value| contains(value, query_lower))
}

fn contains(value: &str, query_lower: &str) -> bool {
    value.to_lowercase().contains(query_lower)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pass::domain::{
        AliasItem, IdentityItem, ItemContent, ItemData, ItemId, ItemState, LoginItem, NoteItem,
        ShareId, VaultId,
    };

    fn make_item(content: ItemContent) -> Item {
        Item {
            id: ItemId::new("item-1".to_string()),
            share_id: ShareId::new("share-1".to_string()),
            vault_id: VaultId::new("vault-1".to_string()),
            content: ItemData {
                title: "My Item".to_string(),
                note: "a secret note".to_string(),
                item_uuid: "uuid-1".to_string(),
                content,
                extra_fields: vec![],
                platform_specific: None,
                custom_icon: None,
            },
            state: ItemState::Active,
            flags: vec![],
            create_time: jiff::civil::DateTime::constant(2026, 1, 1, 0, 0, 0, 0),
            modify_time: jiff::civil::DateTime::constant(2026, 1, 1, 0, 0, 0, 0),
            folder_id: None,
            alias_email: None,
        }
    }

    fn login_item() -> Item {
        make_item(ItemContent::Login(LoginItem {
            email: "octo@example.com".to_string(),
            username: "octocat".to_string(),
            password: "hunter2".to_string(),
            urls: vec!["https://github.com/login".to_string()],
            totp_uri: String::new(),
            passkeys: vec![],
            autofill_urls: vec![],
        }))
    }

    fn identity_item() -> Item {
        make_item(ItemContent::Identity(Box::new(IdentityItem {
            full_name: String::new(),
            email: "identity@example.com".to_string(),
            phone_number: String::new(),
            first_name: String::new(),
            middle_name: String::new(),
            last_name: String::new(),
            birthdate: String::new(),
            gender: String::new(),
            extra_personal_details: vec![],
            organization: String::new(),
            street_address: String::new(),
            zip_or_postal_code: String::new(),
            city: String::new(),
            state_or_province: String::new(),
            country_or_region: String::new(),
            floor: String::new(),
            county: String::new(),
            extra_address_details: vec![],
            social_security_number: String::new(),
            passport_number: String::new(),
            license_number: String::new(),
            website: "https://example.com".to_string(),
            x_handle: String::new(),
            second_phone_number: String::new(),
            linkedin: String::new(),
            reddit: String::new(),
            facebook: String::new(),
            yahoo: String::new(),
            instagram: String::new(),
            extra_contact_details: vec![],
            company: String::new(),
            job_title: String::new(),
            personal_website: String::new(),
            work_phone_number: String::new(),
            work_email: "work@example.com".to_string(),
            extra_work_details: vec![],
            extra_sections: vec![],
        })))
    }

    #[test]
    fn matches_title_case_insensitively() {
        assert!(matches_query(&login_item(), "my item"));
        assert!(matches_query(&login_item(), "MY ITEM"));
    }

    #[test]
    fn matches_login_username_email_and_urls() {
        let item = login_item();
        assert!(matches_query(&item, "octocat"));
        assert!(matches_query(&item, "octo@example.com"));
        assert!(matches_query(&item, "github.com"));
    }

    #[test]
    fn matches_identity_email_and_urls() {
        let item = identity_item();
        assert!(matches_query(&item, "identity@example.com"));
        assert!(matches_query(&item, "work@example.com"));
        assert!(matches_query(&item, "example.com"));
    }

    #[test]
    fn matches_alias_email() {
        let mut item = make_item(ItemContent::Alias(AliasItem));
        item.alias_email = Some("alias@example.com".to_string());
        assert!(matches_query(&item, "alias@example.com"));
    }

    #[test]
    fn ignores_passwords_notes_and_unselected_content() {
        let item = login_item();
        assert!(!matches_query(&item, "hunter2"));
        assert!(!matches_query(&item, "secret note"));
    }

    #[test]
    fn note_items_only_match_on_title() {
        let item = make_item(ItemContent::Note(NoteItem));
        assert!(matches_query(&item, "my item"));
        assert!(!matches_query(&item, "secret note"));
    }

    #[test]
    fn search_summary_includes_vault_name_without_secrets() {
        let item = login_item();
        let summary = SearchItemSummary {
            item: ItemSummary::from(&item),
            vault_name: "Personal".to_string(),
        };

        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"vault_name\":\"Personal\""));
        assert!(json.contains("\"title\":\"My Item\""));
        assert!(!json.contains("hunter2"));
        assert!(!json.contains("\"content\""));
        assert!(!json.contains("\"password\""));
        assert!(!json.contains("\"note\""));
    }
}
