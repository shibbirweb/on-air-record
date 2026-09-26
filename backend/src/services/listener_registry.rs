//! Every open audio stream, for the admin's realtime listener list.
//!
//! Each stream session registers itself when it opens and holds a [`ListenerHandle`]; dropping the handle,
//! however the session ends, removes the entry, so the list cannot keep a listener who has gone. Every
//! change bumps a version on a watch channel, and each session that may see the list sends a fresh copy
//! when it notices. A watch channel rather than a broadcast one, because a session that falls behind only
//! ever needs the newest list, never the ones in between.
//!
//! This is also the one count of listeners. The broadcast hub counts only those on the live feed, and
//! somebody listening back through history is a listener too.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;

use crate::models::{ListenerAccount, ListenerActivity, ListenerEntry, PlayerState};
use crate::util::time::now_ms;

pub struct ListenerRegistry {
    entries: Mutex<HashMap<u64, ListenerEntry>>,
    next_id: Mutex<u64>,
    version: watch::Sender<u64>,
}

impl Default for ListenerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ListenerRegistry {
    pub fn new() -> Self {
        let (version, _) = watch::channel(0);
        Self {
            entries: Mutex::new(HashMap::new()),
            next_id: Mutex::new(1),
            version,
        }
    }

    /// Add a connection, live until the returned handle is dropped.
    pub fn register(
        self: &Arc<Self>,
        account: Option<ListenerAccount>,
        address: IpAddr,
        user_agent: Option<String>,
    ) -> ListenerHandle {
        let id = match self.next_id.lock() {
            Ok(mut next) => {
                let id = *next;
                *next += 1;
                id
            }
            Err(_) => 0,
        };

        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(
                id,
                ListenerEntry {
                    id,
                    account,
                    address,
                    user_agent,
                    connected_at_ms: now_ms(),
                    activity: ListenerActivity::Live,
                    player: PlayerState::Playing,
                },
            );
        }
        self.bump();

        ListenerHandle {
            registry: self.clone(),
            id,
        }
    }

    /// Every connection, oldest first, which is the order the list shows them in.
    pub fn snapshot(&self) -> Vec<ListenerEntry> {
        let mut listed: Vec<ListenerEntry> = match self.entries.lock() {
            Ok(entries) => entries.values().cloned().collect(),
            Err(_) => Vec::new(),
        };
        listed.sort_by_key(|entry| (entry.connected_at_ms, entry.id));
        listed
    }

    pub fn count(&self) -> usize {
        self.entries
            .lock()
            .map(|entries| entries.len())
            .unwrap_or(0)
    }

    /// Notified whenever the list changes. Starts marked as seen, so a new subscriber is only woken by
    /// changes after it subscribed; it sends its first copy itself.
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        let mut receiver = self.version.subscribe();
        receiver.borrow_and_update();
        receiver
    }

    /// Apply `change` to one entry, waking watchers only if it actually differs afterwards.
    fn modify(&self, id: u64, change: impl FnOnce(&mut ListenerEntry)) {
        let changed = match self.entries.lock() {
            Ok(mut entries) => match entries.get_mut(&id) {
                Some(entry) => {
                    let before = (entry.activity, entry.player);
                    change(entry);
                    before != (entry.activity, entry.player)
                }
                None => false,
            },
            Err(_) => false,
        };
        if changed {
            self.bump();
        }
    }

    fn remove(&self, id: u64) {
        let removed = self
            .entries
            .lock()
            .map(|mut entries| entries.remove(&id).is_some())
            .unwrap_or(false);
        if removed {
            self.bump();
        }
    }

    fn bump(&self) {
        self.version
            .send_modify(|version| *version = version.wrapping_add(1));
    }
}

/// A stream session's place in the registry. Dropping it removes the entry.
pub struct ListenerHandle {
    registry: Arc<ListenerRegistry>,
    id: u64,
}

impl ListenerHandle {
    /// Record what the connection is doing. Only a real change wakes anybody watching.
    pub fn set_activity(&self, activity: ListenerActivity) {
        self.registry
            .modify(self.id, |entry| entry.activity = activity);
    }

    /// Record whether the person is hearing it, as their browser reports. Repeats wake nobody.
    pub fn set_player(&self, player: PlayerState) {
        self.registry.modify(self.id, |entry| entry.player = player);
    }
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        self.registry.remove(self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Role;
    use std::net::Ipv4Addr;

    const ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 24));

    fn account(email: &str) -> Option<ListenerAccount> {
        Some(ListenerAccount {
            email: email.to_string(),
            role: Role::Listener,
        })
    }

    #[test]
    fn a_connection_is_listed_until_its_handle_is_dropped() {
        let registry = Arc::new(ListenerRegistry::new());
        let first = registry.register(account("kitchen@example.com"), ADDRESS, None);
        let second = registry.register(None, ADDRESS, Some("Firefox".to_string()));
        assert_eq!(registry.count(), 2);

        let listed = registry.snapshot();
        assert_eq!(listed[0].account, account("kitchen@example.com"));
        assert_eq!(listed[1].account, None);
        assert_eq!(listed[1].user_agent.as_deref(), Some("Firefox"));
        assert_ne!(listed[0].id, listed[1].id);

        drop(first);
        assert_eq!(registry.count(), 1);
        drop(second);
        assert!(registry.snapshot().is_empty());
    }

    #[test]
    fn watchers_are_woken_by_changes_and_not_by_repeats() {
        let registry = Arc::new(ListenerRegistry::new());
        let mut watcher = registry.subscribe();
        assert!(!watcher.has_changed().expect("open"));

        let handle = registry.register(None, ADDRESS, None);
        assert!(watcher.has_changed().expect("open"));
        watcher.borrow_and_update();

        // Already live, so saying so again changes nothing and wakes nobody.
        handle.set_activity(ListenerActivity::Live);
        assert!(!watcher.has_changed().expect("open"));

        handle.set_activity(ListenerActivity::Playback { from_ms: 1_000 });
        assert!(watcher.has_changed().expect("open"));
        watcher.borrow_and_update();
        assert_eq!(
            registry.snapshot()[0].activity,
            ListenerActivity::Playback { from_ms: 1_000 }
        );

        drop(handle);
        assert!(watcher.has_changed().expect("open"));
    }

    #[test]
    fn the_player_state_is_tracked_apart_from_the_stream() {
        let registry = Arc::new(ListenerRegistry::new());
        let handle = registry.register(None, ADDRESS, None);
        // Assumed to be listening until the client says otherwise.
        assert_eq!(registry.snapshot()[0].player, PlayerState::Playing);

        let mut watcher = registry.subscribe();
        handle.set_player(PlayerState::Idle);
        assert!(watcher.has_changed().expect("open"));
        watcher.borrow_and_update();

        handle.set_player(PlayerState::Idle);
        assert!(!watcher.has_changed().expect("open"));

        // Moving through history while paused keeps both halves.
        handle.set_activity(ListenerActivity::Playback { from_ms: 5_000 });
        let entry = &registry.snapshot()[0];
        assert_eq!(entry.player, PlayerState::Idle);
        assert_eq!(
            entry.activity,
            ListenerActivity::Playback { from_ms: 5_000 }
        );
    }

    #[test]
    fn a_new_watcher_starts_with_nothing_pending() {
        let registry = Arc::new(ListenerRegistry::new());
        let _existing = registry.register(None, ADDRESS, None);
        let watcher = registry.subscribe();
        assert!(!watcher.has_changed().expect("open"));
    }
}
