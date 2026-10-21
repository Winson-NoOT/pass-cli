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

use crate::auth::store::PassSessionStore;
use muon::cookie_store::{CookieExpiration, CookieStore, StoredCookie};
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct PassCookieStore {
    session_store: Arc<RwLock<PassSessionStore>>,
    saved: Vec<StoredCookie>,
}

impl PassCookieStore {
    pub(crate) fn new(session_store: Arc<RwLock<PassSessionStore>>) -> Self {
        let saved = session_store.read().cookies.read().clone();
        Self {
            session_store,
            saved,
        }
    }
}

impl CookieStore for PassCookieStore {
    fn set_all(&mut self, cookies: impl IntoIterator<Item = StoredCookie>) {
        let mut snapshot: Vec<StoredCookie> = cookies.into_iter().collect();
        snapshot.sort_by(|left, right| identity(left).cmp(&identity(right)));

        let store = self.session_store.read();
        *store.cookies.write() = snapshot.clone();

        if needs_save(&self.saved, &snapshot, now_secs()) {
            self.saved = snapshot;
            store.schedule_persist();
        }
    }

    fn get_all(&self) -> impl IntoIterator<Item = StoredCookie> {
        self.session_store.read().cookies.read().clone()
    }
}

fn needs_save(saved: &[StoredCookie], current: &[StoredCookie], now: i64) -> bool {
    saved.len() != current.len()
        || saved.iter().zip(current).any(|(saved, current)| {
            identity(saved) != identity(current)
                || expiration_is_stale(saved.expiration, current.expiration, now)
        })
}

fn expiration_is_stale(saved: CookieExpiration, current: CookieExpiration, now: i64) -> bool {
    match (saved, current) {
        (CookieExpiration::AtUtc(saved), CookieExpiration::AtUtc(current)) => {
            2 * (saved.as_secs() - now) < current.as_secs() - now
        }
        (saved, current) => saved != current,
    }
}

fn identity(cookie: &StoredCookie) -> (&str, &str, bool, bool, &Option<String>, &Option<String>) {
    (
        &cookie.name,
        &cookie.value,
        cookie.secure,
        cookie.http_only,
        &cookie.domain_attribute,
        &cookie.path_attribute,
    )
}

#[cfg(wasm_runtime)]
fn now_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

#[cfg(not(wasm_runtime))]
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::storage::SessionStorage;
    use crate::domain::LocalKeyProvider;
    use futures::task::{FutureObj, Spawn, SpawnError};
    use muon::cookie_store::UnixTimestamp;
    use muon::env::Environment;

    #[derive(Clone, Default)]
    struct CountingSpawner {
        spawned: Arc<parking_lot::Mutex<usize>>,
    }

    impl CountingSpawner {
        fn count(&self) -> usize {
            *self.spawned.lock()
        }
    }

    impl Spawn for CountingSpawner {
        fn spawn_obj(&self, _future: FutureObj<'static, ()>) -> Result<(), SpawnError> {
            *self.spawned.lock() += 1;
            Ok(())
        }
    }

    struct NoStorage;

    #[async_trait::async_trait]
    impl SessionStorage for NoStorage {
        async fn load(&self) -> anyhow::Result<Option<Vec<u8>>> {
            Ok(None)
        }
        async fn save(&self, _data: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        async fn exists(&self) -> bool {
            false
        }
        async fn delete(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct FixedKeyProvider(Vec<u8>);

    #[async_trait::async_trait]
    impl LocalKeyProvider for FixedKeyProvider {
        async fn get_key(&self) -> anyhow::Result<crate::domain::LocalKey> {
            Ok(crate::domain::LocalKey::new(self.0.clone()))
        }
        async fn remove_key(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    fn cookie(name: &str) -> StoredCookie {
        StoredCookie {
            name: name.to_owned(),
            value: "value".to_owned(),
            secure: false,
            http_only: false,
            origin_url: "https://pass-api.proton.me/auth/v4/sessions"
                .parse()
                .unwrap(),
            domain_attribute: None,
            path_attribute: Some("/".to_owned()),
            expiration: CookieExpiration::AtUtc(UnixTimestamp::from_secs(1_900_000_000)),
        }
    }

    fn store() -> (PassCookieStore, CountingSpawner) {
        let spawner = CountingSpawner::default();
        let session_store = PassSessionStore::new(
            Environment::new_prod(),
            Arc::new(NoStorage),
            Arc::new(FixedKeyProvider(
                crate::domain::crypto::generate_encryption_key(),
            )),
            Arc::new(spawner.clone()),
        );
        (
            PassCookieStore::new(Arc::new(RwLock::new(session_store))),
            spawner,
        )
    }

    fn names(store: &PassCookieStore) -> Vec<String> {
        store.get_all().into_iter().map(|c| c.name).collect()
    }

    #[test]
    fn set_all_replaces_the_snapshot() {
        let (mut store, _) = store();

        store.set_all([cookie("first")]);
        store.set_all([cookie("second")]);

        assert_eq!(names(&store), ["second"]);
    }

    #[test]
    fn get_all_is_order_independent() {
        let (mut store, _) = store();

        store.set_all([cookie("b"), cookie("a")]);

        assert_eq!(names(&store), ["a", "b"]);
    }

    #[test]
    fn unchanged_snapshot_does_not_schedule_a_write() {
        let (mut store, spawner) = store();

        store.set_all([cookie("a")]);
        store.set_all([cookie("a")]);

        assert_eq!(spawner.count(), 1);
    }

    #[test]
    fn changed_value_schedules_a_write() {
        let (mut store, spawner) = store();
        let mut rotated = cookie("Session-Id");
        rotated.value = "rotated".to_owned();

        store.set_all([cookie("Session-Id")]);
        store.set_all([rotated]);

        assert_eq!(spawner.count(), 2);
    }

    #[test]
    fn refreshed_origin_is_kept_without_a_write() {
        let (mut store, spawner) = store();
        let mut refreshed = cookie("Session-Id");
        refreshed.origin_url = "https://pass-api.proton.me/core/v4/users".parse().unwrap();

        store.set_all([cookie("Session-Id")]);
        store.set_all([refreshed.clone()]);

        assert_eq!(store.get_all().into_iter().collect::<Vec<_>>(), [refreshed]);
        assert_eq!(spawner.count(), 1);
    }

    fn expiring_at(secs: i64) -> StoredCookie {
        let mut cookie = cookie("Session-Id");
        cookie.expiration = CookieExpiration::AtUtc(UnixTimestamp::from_secs(secs));
        cookie
    }

    #[test]
    fn expiration_refresh_within_half_the_lifetime_is_not_saved() {
        let now = 1_000;
        let saved = [expiring_at(now + 3_600)];

        assert!(!needs_save(&saved, &[expiring_at(now + 3_600)], now));
        assert!(!needs_save(&saved, &[expiring_at(now + 7_200)], now));
    }

    #[test]
    fn expiration_refresh_beyond_half_the_lifetime_is_saved() {
        let now = 1_000;
        let saved = [expiring_at(now + 3_600)];

        assert!(needs_save(&saved, &[expiring_at(now + 7_201)], now));
    }

    #[test]
    fn short_lived_cookie_refreshed_after_half_its_lifetime_is_saved() {
        let saved = [expiring_at(1_060)];

        assert!(!needs_save(&saved, &[expiring_at(1_089)], 1_029));
        assert!(needs_save(&saved, &[expiring_at(1_091)], 1_031));
    }
}
