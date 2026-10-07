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

use crate::domain::{FolderId, Item, ShareId};
use crate::item::open::ItemWithItemKey;
use crate::pagination::SincePagination;
use crate::{PassClient, PassClientContext};
use anyhow::{Context, Result};
use muon::GET;
use pass_derive::sdk_export;
use std::collections::HashMap;
use std::convert::Infallible;
use std::ops::ControlFlow;

struct ItemsForShareCacheType;
type ItemsForShareCache = HashMap<ShareId, SerializedItemsWithKey>;

#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct SerializedItemsWithKey(Vec<u8>);

impl SerializedItemsWithKey {
    pub fn new(content: &[ItemWithItemKey], xor_key: u8) -> Result<Self> {
        let serialized = serde_json::to_vec(content).context("Error serializing content")?;
        let xored = Self::perform_xor(serialized, xor_key);
        Ok(Self(xored))
    }

    pub fn deserialize(&self, xor_key: u8) -> Result<Vec<ItemWithItemKey>> {
        let xored = Self::perform_xor(self.0.clone(), xor_key);
        let deserialized = serde_json::from_slice(&xored).context("Error deserializing content")?;
        Ok(deserialized)
    }

    fn perform_xor(mut data: Vec<u8>, xor_key: u8) -> Vec<u8> {
        for i in &mut data {
            *i ^= xor_key;
        }
        data
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct GetItemsResponse {
    #[serde(rename = "Items")]
    pub items: ItemsList,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
struct ItemsList {
    #[serde(rename = "RevisionsData")]
    pub revisions: Vec<ItemRevision>,
    #[serde(rename = "LastToken")]
    pub last_token: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[allow(dead_code)]
pub(crate) struct ItemRevision {
    #[serde(rename = "ItemID")]
    pub item_id: String,
    #[serde(rename = "Revision")]
    pub revision: u64,
    #[serde(rename = "ContentFormatVersion")]
    pub content_format_version: i32,
    #[serde(rename = "KeyRotation")]
    pub key_rotation: u8,
    #[serde(rename = "Content")]
    pub content: String,
    #[serde(rename = "ItemKey")]
    pub item_key: Option<String>,
    #[serde(rename = "State")]
    pub state: u8,
    #[serde(rename = "Flags")]
    pub flags: u64,
    #[serde(rename = "AliasEmail")]
    pub alias_email: Option<String>,
    #[serde(rename = "CreateTime")]
    pub create_time: u64,
    #[serde(rename = "ModifyTime")]
    pub modify_time: u64,
    #[serde(rename = "FolderID")]
    pub folder_id: Option<String>,
}

#[sdk_export]
impl<C: PassClientContext> PassClient<C> {
    /// Lists the items available for a given ShareId
    #[sdk_export]
    pub async fn list_items(&self, share_id: &ShareId) -> Result<Vec<Item>> {
        let opened = match self
            .scan_items(share_id, |_| ControlFlow::<Infallible>::Continue(()))
            .await?
        {
            ControlFlow::Continue(opened) => opened,
            ControlFlow::Break(never) => match never {},
        };
        Ok(opened.into_iter().map(|i| i.item).collect())
    }

    /// Lists the items directly inside `folder_id` (not in its subfolders)
    /// of the vault behind `share_id`.
    #[sdk_export]
    pub async fn list_items_in_folder(
        &self,
        share_id: &ShareId,
        folder_id: &FolderId,
    ) -> Result<Vec<Item>> {
        let items = self
            .list_items(share_id)
            .await
            .context("Error listing items")?;
        Ok(items
            .into_iter()
            .filter(|item| item.folder_id.as_ref() == Some(folder_id))
            .collect())
    }

    #[sdk_export]
    pub async fn clear_items_cache(&self, share_id: &ShareId) {
        self.cache
            .update(ItemsForShareCacheType, |cache: &mut ItemsForShareCache| {
                cache.remove(share_id);
            })
            .await;
    }
}

impl<C: PassClientContext> PassClient<C> {
    // Visits the items of `share_id` until `visit` breaks, returning the break value.
    // Item pages are fetched and decrypted one at a time, so breaking skips the remaining
    // requests. The items cache for the share is only populated once all pages were visited.
    pub async fn list_items_until<B>(
        &self,
        share_id: &ShareId,
        mut visit: impl FnMut(&Item) -> ControlFlow<B>,
    ) -> Result<Option<B>> {
        match self
            .scan_items(share_id, |opened| visit(&opened.item))
            .await?
        {
            ControlFlow::Break(value) => Ok(Some(value)),
            ControlFlow::Continue(_) => Ok(None),
        }
    }

    /// Returns the first item of `share_id` matching `predicate`, without listing past it.
    pub async fn find_item_in_share(
        &self,
        share_id: &ShareId,
        mut predicate: impl FnMut(&Item) -> bool,
    ) -> Result<Option<Item>> {
        self.list_items_until(share_id, |item| {
            if predicate(item) {
                ControlFlow::Break(item.clone())
            } else {
                ControlFlow::Continue(())
            }
        })
        .await
    }

    async fn scan_items<B>(
        &self,
        share_id: &ShareId,
        mut visit: impl FnMut(&ItemWithItemKey) -> ControlFlow<B>,
    ) -> Result<ControlFlow<B, Vec<ItemWithItemKey>>> {
        if let Some(cached) = self.cached_items(share_id).await {
            trace!("Visiting cached items for share {share_id}");
            for item in &cached {
                if let ControlFlow::Break(value) = visit(item) {
                    return Ok(ControlFlow::Break(value));
                }
            }
            return Ok(ControlFlow::Continue(cached));
        }

        let mut opened = Vec::new();
        let mut pagination = SincePagination::default();
        loop {
            let page = self
                .fetch_items_page(share_id, &pagination)
                .await
                .context("Error fetching items")?;
            let page_size = page.revisions.len();
            trace!("Retrieved {page_size} items");
            if page_size == 0 {
                break;
            }

            let opened_page = self
                .open_items(share_id, page.revisions)
                .await
                .context("Error opening items")?;
            for item in opened_page {
                if let ControlFlow::Break(value) = visit(&item) {
                    trace!("Stopped visiting items for share {share_id}, not caching them");
                    return Ok(ControlFlow::Break(value));
                }
                opened.push(item);
            }

            if page_size < pagination.page_size {
                break;
            }
            match pagination.next(page.last_token) {
                Some(next) => pagination = next,
                None => break,
            }
        }

        trace!(
            "Finished item retrieval process. Retrieved {} items",
            opened.len()
        );
        self.store_cached_items(share_id, &opened).await?;
        Ok(ControlFlow::Continue(opened))
    }

    async fn fetch_items_page(
        &self,
        share_id: &ShareId,
        pagination: &SincePagination,
    ) -> Result<ItemsList> {
        let mut req = GET!("/pass/v1/share/{}/item", share_id)
            .query(("PageSize".to_string(), format!("{}", pagination.page_size)));
        if let Some(ref since) = pagination.since {
            req = req.query(("Since".to_string(), since.to_string()));
        }

        let res = self.send(req).await.context("Error fetching items page")?;
        let response: GetItemsResponse = assert_response!(res);
        Ok(response.items)
    }

    async fn cached_items(&self, share_id: &ShareId) -> Option<Vec<ItemWithItemKey>> {
        self.cache
            .ensure_has_value(ItemsForShareCacheType, ItemsForShareCache::new)
            .await;
        let cached: ItemsForShareCache = self.cache.get(ItemsForShareCacheType).await?;
        match cached.get(share_id)?.deserialize(self.memory_xor_key) {
            Ok(items) => Some(items),
            Err(e) => {
                warn!("Error deserializing cached items: {e:#}");
                None
            }
        }
    }

    async fn store_cached_items(
        &self,
        share_id: &ShareId,
        items: &[ItemWithItemKey],
    ) -> Result<()> {
        let serialized = SerializedItemsWithKey::new(items, self.memory_xor_key)
            .context("Error caching items")?;
        self.cache
            .ensure_has_value(ItemsForShareCacheType, ItemsForShareCache::new)
            .await;
        self.cache
            .update(ItemsForShareCacheType, |cache: &mut ItemsForShareCache| {
                cache.insert(share_id.clone(), serialized);
            })
            .await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::CodeResponse;
    use crate::domain::{Item, ItemState};
    use crate::test_tools::*;
    use muon_test::server::Request as RecordedRequest;
    use std::collections::VecDeque;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn setup_list_items_endpoint(
        server: &ProtonAPI,
        share_id: &str,
        revisions: Vec<ItemRevision>,
    ) -> Arc<AtomicBool> {
        server.handler_with_method(
            Method::GET,
            format!("/pass/v1/share/{}/item", share_id),
            move |_| {
                success(GetItemsResponse {
                    items: ItemsList {
                        revisions: revisions.clone(),
                        last_token: None,
                    },
                })
            },
        )
    }

    const PAGED_SHARE_ID: &str = "PagedShareID";
    const SECOND_PAGE_TOKEN: &str = "SecondPage";
    // The test server drops a handler once it has answered, so register a few copies
    const PAGED_HANDLER_COPIES: usize = 5;

    struct PagedItems {
        first_title: String,
        last_title: String,
        total: usize,
    }

    fn revision_for(item_id: String, encrypted: &EncryptItemContentsResult) -> ItemRevision {
        ItemRevisionBuilder::new(item_id)
            .with_content(crate::utils::b64_encode(&encrypted.encrypted_contents))
            .with_item_key(Some(crate::utils::b64_encode(
                &encrypted.encrypted_item_key,
            )))
            .with_state(ItemState::Active as u8)
            .build()
    }

    /// Serves a full first page whose first item is unique, then a second page with one more
    /// unique item. `fail_second_page` makes the second page fail while it is set.
    fn setup_paged_items_endpoint(
        api: &ProtonAPI,
        fail_second_page: Arc<AtomicBool>,
    ) -> PagedItems {
        let page_size = SincePagination::default().page_size;
        let first = create_random_item();
        let filler = encrypt_item_contents(create_random_item());
        let last = create_random_item();

        let mut first_page = vec![revision_for(
            "First".to_string(),
            &encrypt_item_contents(first.clone()),
        )];
        first_page.extend((1..page_size).map(|i| revision_for(format!("Filler{i}"), &filler)));
        let second_page = vec![revision_for(
            "Last".to_string(),
            &encrypt_item_contents(last.clone()),
        )];

        for _ in 0..PAGED_HANDLER_COPIES {
            let first_page = first_page.clone();
            let second_page = second_page.clone();
            let fail_second_page = fail_second_page.clone();
            api.handler_with_method(
                Method::GET,
                format!("/pass/v1/share/{PAGED_SHARE_ID}/item"),
                move |req| {
                    let is_second_page = req
                        .uri()
                        .query()
                        .is_some_and(|q| q.contains(&format!("Since={SECOND_PAGE_TOKEN}")));
                    if !is_second_page {
                        return success(GetItemsResponse {
                            items: ItemsList {
                                revisions: first_page.clone(),
                                last_token: Some(SECOND_PAGE_TOKEN.to_string()),
                            },
                        });
                    }
                    if fail_second_page.load(Ordering::Relaxed) {
                        return response_from_status_and_payload(422, CodeResponse { code: 2001 });
                    }
                    success(GetItemsResponse {
                        items: ItemsList {
                            revisions: second_page.clone(),
                            last_token: None,
                        },
                    })
                },
            );
        }

        PagedItems {
            first_title: first.title,
            last_title: last.title,
            total: page_size + 1,
        }
    }

    async fn paged_client(server: muon_test::Server) -> (TestPassClient, ProtonAPI) {
        let (raw_client, api) = server.client::<()>();
        let client = make_test_pass_client_with_setup(raw_client, &api, PlanType::Free).await;
        setup_vault_share(&api, PAGED_SHARE_ID);
        setup_share_keys(&api, PAGED_SHARE_ID);
        (client, api)
    }

    fn item_page_requests(requests: &VecDeque<RecordedRequest<Vec<u8>>>) -> usize {
        let path = format!("/pass/v1/share/{PAGED_SHARE_ID}/item");
        requests.iter().filter(|r| r.uri().path() == path).count()
    }

    #[test]
    fn test_serialized_items_with_key_round_trip() {
        const XOR_KEY: u8 = 0x42;

        // Create test item data
        let item_data = create_random_item();
        let encrypted_data = encrypt_item_contents(item_data.clone());

        // Create an ItemWithItemKey
        let item_with_key = ItemWithItemKey {
            item: Item {
                id: item_id!("test_item_id"),
                share_id: share_id!("test_share_id"),
                vault_id: vault_id!("test_vault_id"),
                content: item_data,
                state: ItemState::Active,
                flags: vec![],
                alias_email: None,
                create_time: jiff::Timestamp::from_second(1234567890)
                    .unwrap()
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .datetime(),
                modify_time: jiff::Timestamp::from_second(1234567890)
                    .unwrap()
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .datetime(),
                folder_id: None,
            },
            item_key: crate::item::item_keys::OpenedItemKey {
                key_rotation: 1,
                key: encrypted_data.item_key,
            },
        };

        let items = vec![item_with_key.clone()];

        // Serialize with XOR key
        let serialized =
            SerializedItemsWithKey::new(&items, XOR_KEY).expect("Should be able to serialize");

        // Deserialize with same XOR key
        let deserialized = serialized
            .deserialize(XOR_KEY)
            .expect("Should be able to deserialize");

        // Verify round-trip works
        assert_eq!(1, deserialized.len());
        assert_eq!(
            item_with_key.item.id.value(),
            deserialized[0].item.id.value()
        );
        assert_eq!(
            item_with_key.item.share_id.value(),
            deserialized[0].item.share_id.value()
        );
        assert_eq!(
            item_with_key.item_key.key_rotation,
            deserialized[0].item_key.key_rotation
        );
    }

    #[test]
    fn test_serialized_items_with_key_wrong_key_fails() {
        const XOR_KEY: u8 = 0x42;
        const WRONG_KEY: u8 = 0x99;

        // Create test item data
        let item_data = create_random_item();
        let encrypted_data = encrypt_item_contents(item_data.clone());

        let item_with_key = ItemWithItemKey {
            item: Item {
                id: item_id!("test_item_id"),
                share_id: share_id!("test_share_id"),
                vault_id: vault_id!("test_vault_id"),
                content: item_data,
                state: ItemState::Active,
                flags: vec![],
                alias_email: None,
                create_time: jiff::Timestamp::from_second(1234567890)
                    .unwrap()
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .datetime(),
                modify_time: jiff::Timestamp::from_second(1234567890)
                    .unwrap()
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .datetime(),
                folder_id: None,
            },
            item_key: crate::item::item_keys::OpenedItemKey {
                key_rotation: 1,
                key: encrypted_data.item_key,
            },
        };

        let items = vec![item_with_key];

        // Serialize with one key
        let serialized =
            SerializedItemsWithKey::new(&items, XOR_KEY).expect("Should be able to serialize");

        // Try to deserialize with wrong key - should fail
        let result = serialized.deserialize(WRONG_KEY);
        assert!(
            result.is_err(),
            "Deserialization with wrong key should fail"
        );
    }

    #[test]
    fn test_serialized_items_with_key_empty_list() {
        const XOR_KEY: u8 = 0x42;

        let items: Vec<ItemWithItemKey> = vec![];

        // Serialize empty list
        let serialized = SerializedItemsWithKey::new(&items, XOR_KEY)
            .expect("Should be able to serialize empty list");

        // Deserialize
        let deserialized = serialized
            .deserialize(XOR_KEY)
            .expect("Should be able to deserialize empty list");

        assert!(deserialized.is_empty());
    }

    #[test]
    fn test_serialized_items_xor_actually_encrypts() {
        const XOR_KEY: u8 = 0x42;

        // Create test item data
        let item_data = create_random_item();
        let encrypted_data = encrypt_item_contents(item_data.clone());

        let item_with_key = ItemWithItemKey {
            item: Item {
                id: item_id!("test_item_id"),
                share_id: share_id!("test_share_id"),
                vault_id: vault_id!("test_vault_id"),
                content: item_data,
                state: ItemState::Active,
                flags: vec![],
                alias_email: None,
                create_time: jiff::Timestamp::from_second(1234567890)
                    .unwrap()
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .datetime(),
                modify_time: jiff::Timestamp::from_second(1234567890)
                    .unwrap()
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .datetime(),
                folder_id: None,
            },
            item_key: crate::item::item_keys::OpenedItemKey {
                key_rotation: 1,
                key: encrypted_data.item_key,
            },
        };

        let items = vec![item_with_key];

        // Get original serialized data without XOR
        let original_json = serde_json::to_vec(&items).expect("Should serialize to JSON");

        // Serialize with XOR key
        let serialized =
            SerializedItemsWithKey::new(&items, XOR_KEY).expect("Should be able to serialize");

        // Verify that the XOR'd data is different from the original
        assert_ne!(
            &original_json, &serialized.0,
            "XOR encrypted data should be different from original"
        );

        // Verify that all bytes are XOR'd
        for (orig_byte, xor_byte) in original_json.iter().zip(serialized.0.iter()) {
            assert_eq!(orig_byte ^ XOR_KEY, *xor_byte, "Each byte should be XOR'd");
        }
    }

    #[muon_test::test]
    async fn test_list_items_empty_response(server: muon_test::Server) {
        let (raw_client, api) = server.client::<()>();
        const SHARE_ID: &str = "TestShareID1";

        let client = make_test_pass_client_with_setup(raw_client, &api, PlanType::Free).await;

        // Setup vault share and keys
        setup_vault_share(&api, SHARE_ID);
        setup_share_keys(&api, SHARE_ID);

        // Setup empty items list
        let handled = setup_list_items_endpoint(&api, SHARE_ID, vec![]);

        let recorder = api.new_recorder();
        let items = client
            .list_items(&share_id!(SHARE_ID))
            .await
            .expect("Should be able to list items");

        assert_hit!(handled);
        let requests = recorder.read();

        // Only request items. As it's empty, the share is not requested
        assert_eq!(requests.len(), 1);
        let req = requests.iter().next().unwrap();
        assert_eq!(
            format!("/pass/v1/share/{}/item", SHARE_ID),
            req.uri().path()
        );

        assert!(items.is_empty(), "Expected empty items list");
    }

    #[muon_test::test]
    async fn test_list_items_two_shares_with_one_item_each(server: muon_test::Server) {
        let (raw_client, api) = server.client::<()>();
        const SHARE_ID_1: &str = "TestShareID1";
        const SHARE_ID_2: &str = "TestShareID2";
        const ITEM_ID_1: &str = "Item1";
        const ITEM_ID_2: &str = "Item2";

        let client = make_test_pass_client_with_setup(raw_client, &api, PlanType::Free).await;

        // Setup first share
        setup_vault_share(&api, SHARE_ID_1);
        setup_share_keys(&api, SHARE_ID_1);

        // Setup second share
        setup_vault_share(&api, SHARE_ID_2);
        setup_share_keys(&api, SHARE_ID_2);

        // Create item data for first share
        let item_data_1 = create_random_item();
        let encrypted_data_1 = encrypt_item_contents(item_data_1.clone());
        let revision_1 = ItemRevisionBuilder::new(ITEM_ID_1.to_string())
            .with_content(crate::utils::b64_encode(
                &encrypted_data_1.encrypted_contents,
            ))
            .with_item_key(Some(crate::utils::b64_encode(
                &encrypted_data_1.encrypted_item_key,
            )))
            .with_state(ItemState::Active as u8)
            .build();

        // Create item data for second share
        let item_data_2 = create_random_item();
        let encrypted_data_2 = encrypt_item_contents(item_data_2.clone());
        let revision_2 = ItemRevisionBuilder::new(ITEM_ID_2.to_string())
            .with_content(crate::utils::b64_encode(
                &encrypted_data_2.encrypted_contents,
            ))
            .with_item_key(Some(crate::utils::b64_encode(
                &encrypted_data_2.encrypted_item_key,
            )))
            .with_state(ItemState::Active as u8)
            .build();

        // Setup endpoints
        let handled_1 = setup_list_items_endpoint(&api, SHARE_ID_1, vec![revision_1]);
        let handled_2 = setup_list_items_endpoint(&api, SHARE_ID_2, vec![revision_2]);

        // Fetch items for first share
        let items_1 = client
            .list_items(&share_id!(SHARE_ID_1))
            .await
            .expect("Should be able to list items for share 1");

        assert_hit!(handled_1);
        assert_eq!(1, items_1.len(), "Expected 1 item in share 1");
        assert_eq!(ITEM_ID_1, items_1[0].id.value());
        assert_eq!(item_data_1.title, items_1[0].content.title);

        // Fetch items for second share
        let items_2 = client
            .list_items(&share_id!(SHARE_ID_2))
            .await
            .expect("Should be able to list items for share 2");

        assert_hit!(handled_2);
        assert_eq!(1, items_2.len(), "Expected 1 item in share 2");
        assert_eq!(ITEM_ID_2, items_2[0].id.value());
        assert_eq!(item_data_2.title, items_2[0].content.title);
    }

    #[muon_test::test]
    async fn test_list_items_cache_is_used(server: muon_test::Server) {
        let (raw_client, api) = server.client::<()>();
        const SHARE_ID: &str = "TestShareID1";
        const ITEM_ID: &str = "Item1";

        let client = make_test_pass_client_with_setup(raw_client, &api, PlanType::Free).await;

        // Setup vault share and keys
        setup_vault_share(&api, SHARE_ID);
        setup_share_keys(&api, SHARE_ID);

        // Create item data
        let item_data = create_random_item();
        let encrypted_data = encrypt_item_contents(item_data.clone());
        let revision = ItemRevisionBuilder::new(ITEM_ID.to_string())
            .with_content(crate::utils::b64_encode(&encrypted_data.encrypted_contents))
            .with_item_key(Some(crate::utils::b64_encode(
                &encrypted_data.encrypted_item_key,
            )))
            .with_state(ItemState::Active as u8)
            .build();

        // Setup endpoint
        let handled = setup_list_items_endpoint(&api, SHARE_ID, vec![revision]);

        let recorder = api.new_recorder();

        // First fetch - should hit the endpoint
        let items_1 = client
            .list_items(&share_id!(SHARE_ID))
            .await
            .expect("Should be able to list items (first call)");

        assert_hit!(handled);
        assert_eq!(1, items_1.len());
        assert_eq!(ITEM_ID, items_1[0].id.value());

        let requests_after_first = recorder.read().len();
        assert!(requests_after_first > 0, "First call should make requests");

        // Second fetch - should use cache, no new requests
        let items_2 = client
            .list_items(&share_id!(SHARE_ID))
            .await
            .expect("Should be able to list items (second call)");

        let requests_after_second = recorder.read().len();
        assert_eq!(
            requests_after_first, requests_after_second,
            "Second call should not make new requests (cache should be used)"
        );

        // Verify the items are the same
        assert_eq!(1, items_2.len());
        assert_eq!(ITEM_ID, items_2[0].id.value());
        assert_eq!(item_data.title, items_2[0].content.title);
    }

    #[muon_test::test]
    async fn test_find_item_in_share_stops_at_first_page(server: muon_test::Server) {
        let (client, api) = paged_client(server).await;
        let paged = setup_paged_items_endpoint(&api, Arc::new(AtomicBool::new(false)));
        let share_id = share_id!(PAGED_SHARE_ID);

        let recorder = api.new_recorder();
        let found = client
            .find_item_in_share(&share_id, |i| i.content.title == paged.first_title)
            .await
            .expect("Should be able to search items")
            .expect("Item should be found");

        assert_eq!("First", found.id.value());
        assert_eq!(1, item_page_requests(&recorder.read()));
    }

    #[muon_test::test]
    async fn test_list_items_until_break_does_not_cache(server: muon_test::Server) {
        let (client, api) = paged_client(server).await;
        let paged = setup_paged_items_endpoint(&api, Arc::new(AtomicBool::new(false)));
        let share_id = share_id!(PAGED_SHARE_ID);

        let recorder = api.new_recorder();
        let mut visited = 0;
        let found = client
            .list_items_until(&share_id, |_| {
                visited += 1;
                ControlFlow::Break(visited)
            })
            .await
            .expect("Should be able to search items");
        assert_eq!(Some(1), found);

        let items = client
            .list_items(&share_id)
            .await
            .expect("Should be able to list items");

        assert_eq!(paged.total, items.len());
        assert_eq!(3, item_page_requests(&recorder.read()));
    }

    #[muon_test::test]
    async fn test_list_items_until_exhausted_caches_all_pages(server: muon_test::Server) {
        let (client, api) = paged_client(server).await;
        let paged = setup_paged_items_endpoint(&api, Arc::new(AtomicBool::new(false)));
        let share_id = share_id!(PAGED_SHARE_ID);

        let recorder = api.new_recorder();
        let missing = client
            .find_item_in_share(&share_id, |i| i.content.title == "missing")
            .await
            .expect("Should be able to search items");
        assert!(missing.is_none());
        assert_eq!(2, item_page_requests(&recorder.read()));

        let found = client
            .find_item_in_share(&share_id, |i| i.content.title == paged.last_title)
            .await
            .expect("Should be able to search items")
            .expect("Item should be found in cache");
        let items = client
            .list_items(&share_id)
            .await
            .expect("Should be able to list items");

        assert_eq!("Last", found.id.value());
        assert_eq!(paged.total, items.len());
        assert_eq!(2, item_page_requests(&recorder.read()));
    }

    #[muon_test::test]
    async fn test_list_items_failed_page_does_not_cache(server: muon_test::Server) {
        let (client, api) = paged_client(server).await;
        let fail_second_page = Arc::new(AtomicBool::new(true));
        let paged = setup_paged_items_endpoint(&api, fail_second_page.clone());
        let share_id = share_id!(PAGED_SHARE_ID);

        client
            .list_items(&share_id)
            .await
            .expect_err("Second page failure should fail the listing");

        fail_second_page.store(false, Ordering::Relaxed);
        let recorder = api.new_recorder();
        let items = client
            .list_items(&share_id)
            .await
            .expect("Should be able to list items");

        assert_eq!(paged.total, items.len());
        assert_eq!(2, item_page_requests(&recorder.read()));
    }
}
