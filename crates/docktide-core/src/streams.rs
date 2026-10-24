//! Ownership and cancellation of long-running Docker streams (spec M1 FR-5).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use tokio_util::sync::CancellationToken;

use crate::connection::DockerService;

/// Identifies one stream subscription across IPC.
pub type SubscriptionId = u64;

/// Every running producer, keyed by subscription.
#[derive(Debug, Default)]
pub(crate) struct StreamRegistry {
    next: AtomicU64,
    tokens: Mutex<HashMap<SubscriptionId, CancellationToken>>,
    /// The one events subscription; a new subscriber replaces it.
    live_updates: Mutex<Option<SubscriptionId>>,
    /// The one stats subscription, likewise.
    stats: Mutex<Option<SubscriptionId>>,
}

impl StreamRegistry {
    /// Registers a new producer and returns the token it must stop on.
    pub(crate) fn insert(&self) -> (SubscriptionId, CancellationToken) {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let token = CancellationToken::new();
        self.tokens().insert(id, token.clone());
        (id, token)
    }

    /// Registers the events producer and cancels the previous one.
    pub(crate) fn replace_live_updates(&self) -> (SubscriptionId, CancellationToken) {
        self.replace(&self.live_updates)
    }

    /// Registers the stats producer and cancels the previous one.
    pub(crate) fn replace_stats(&self) -> (SubscriptionId, CancellationToken) {
        self.replace(&self.stats)
    }

    fn replace(&self, slot: &Mutex<Option<SubscriptionId>>) -> (SubscriptionId, CancellationToken) {
        let (id, token) = self.insert();
        let previous = slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(id);
        if let Some(previous) = previous {
            self.cancel(previous);
        }
        (id, token)
    }

    /// Stops a producer. Unknown ids are ignored.
    pub(crate) fn cancel(&self, id: SubscriptionId) {
        if let Some(token) = self.tokens().remove(&id) {
            token.cancel();
        }
    }

    pub(crate) fn cancel_all(&self) {
        for (_, token) in self.tokens().drain() {
            token.cancel();
        }
    }

    /// Forgets a producer that ended by itself.
    pub(crate) fn remove(&self, id: SubscriptionId) {
        self.tokens().remove(&id);
    }

    fn tokens(&self) -> MutexGuard<'_, HashMap<SubscriptionId, CancellationToken>> {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl DockerService {
    /// Stops a stream started by this service. Unknown ids are ignored.
    pub fn cancel_stream(&self, id: SubscriptionId) {
        self.streams().cancel(id);
    }

    /// Stops every stream, for example when the page that subscribed reloads.
    pub fn cancel_all_streams(&self) {
        self.streams().cancel_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_distinct() {
        let registry = StreamRegistry::default();
        let (a, _) = registry.insert();
        let (b, _) = registry.insert();
        assert_ne!(a, b);
    }

    #[test]
    fn cancel_stops_only_that_producer() {
        let registry = StreamRegistry::default();
        let (a, token_a) = registry.insert();
        let (_, token_b) = registry.insert();
        registry.cancel(a);
        assert!(token_a.is_cancelled());
        assert!(!token_b.is_cancelled());
    }

    #[test]
    fn cancel_all_stops_every_producer() {
        let registry = StreamRegistry::default();
        let (_, token_a) = registry.insert();
        let (_, token_b) = registry.insert();
        registry.cancel_all();
        assert!(token_a.is_cancelled() && token_b.is_cancelled());
    }

    #[test]
    fn removed_producers_are_not_cancelled_later() {
        let registry = StreamRegistry::default();
        let (a, token) = registry.insert();
        registry.remove(a);
        registry.cancel_all();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn a_new_live_updates_producer_replaces_the_previous_one() {
        let registry = StreamRegistry::default();
        let (first, first_token) = registry.replace_live_updates();
        let (second, second_token) = registry.replace_live_updates();
        assert_ne!(first, second);
        assert!(first_token.is_cancelled());
        assert!(!second_token.is_cancelled());
    }

    #[test]
    fn a_new_stats_producer_replaces_only_the_previous_stats_one() {
        let registry = StreamRegistry::default();
        let (_, live_updates) = registry.replace_live_updates();
        let (_, first) = registry.replace_stats();
        let (_, second) = registry.replace_stats();
        assert!(first.is_cancelled());
        assert!(!second.is_cancelled() && !live_updates.is_cancelled());
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let registry = StreamRegistry::default();
        registry.cancel(42);
        registry.remove(42);
    }
}
