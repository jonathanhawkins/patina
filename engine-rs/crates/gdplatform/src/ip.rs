//! The `IP` singleton, modelled on Godot's **web** target.
//!
//! Godot registers `IP` on every platform (`core/register_core_types.cpp`), but
//! the web build substitutes `IPWeb` (`platform/web/ip_web.h`), whose
//! `_resolve_hostname` and `get_local_interfaces` have **empty bodies**. Web
//! parity is therefore *present-and-empty*, not absent: the singleton exists,
//! every method answers, and the answers are blank.
//!
//! The resolver queue is modelled faithfully because its status transitions are
//! observable from script. In `core/io/ip.cpp`:
//!
//! * `resolve_hostname_queue_item` sets `RESOLVER_STATUS_DONE` on a cache hit
//!   (:171-173) and `RESOLVER_STATUS_WAITING` otherwise (:176).
//! * The queue then resolves — synchronously when no resolver thread is running
//!   (:180) — and :105 sets
//!   `response.is_empty() ? RESOLVER_STATUS_ERROR : RESOLVER_STATUS_DONE`.
//!
//! ⚠️ On web `_resolve_hostname` adds nothing, so an *uncached* lookup is always
//! empty and its terminal status is **`RESOLVER_STATUS_ERROR`**, not `DONE`.
//! `DONE` is reachable only through the cache. Since Patina has no resolver
//! thread, the status is terminal by the time the call returns.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

// `IP.ResolverStatus` — core/io/ip.h. Unvalued in the C++ enum, so sequential.
pub const RESOLVER_STATUS_NONE: i64 = 0;
pub const RESOLVER_STATUS_WAITING: i64 = 1;
pub const RESOLVER_STATUS_DONE: i64 = 2;
pub const RESOLVER_STATUS_ERROR: i64 = 3;

// core/io/ip.h:60-61.
pub const RESOLVER_MAX_QUERIES: i64 = 256;
pub const RESOLVER_INVALID_ID: i64 = -1;

// `IP.Type` — core/io/ip.h, explicitly valued.
pub const TYPE_NONE: i64 = 0;
pub const TYPE_IPV4: i64 = 1;
pub const TYPE_IPV6: i64 = 2;
pub const TYPE_ANY: i64 = 3;

/// One slot of the resolver queue.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolveItem {
    /// The hostname the caller queued.
    pub hostname: String,
    /// The `IP.Type` filter the caller asked for.
    pub ip_type: i64,
    /// Resolved addresses — always empty on web unless the cache was seeded.
    pub addresses: Vec<String>,
    /// One of the `RESOLVER_STATUS_*` values.
    pub status: i64,
}

/// One entry of `IP.get_local_interfaces()`.
///
/// Always absent on web, but the shape is part of the API.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InterfaceInfo {
    /// Adapter name.
    pub name: String,
    /// Human-readable adapter name.
    pub friendly_name: String,
    /// Adapter index, as a string (Godot reports it that way).
    pub index: String,
    /// Addresses bound to the adapter.
    pub addresses: Vec<String>,
}

/// Godot's cache key pairs the hostname with the requested type, so the same
/// host queued as IPv4 and IPv6 occupies two entries.
fn cache_key(hostname: &str, ip_type: i64) -> String {
    format!("{ip_type}:{hostname}")
}

/// The `IP` singleton's state.
#[derive(Debug, Default)]
pub struct Ip {
    queue: HashMap<i64, ResolveItem>,
    cache: HashMap<String, Vec<String>>,
}

impl Ip {
    /// Creates an empty resolver.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mirrors `_IP_ResolverPrivate::find_empty_id` — the first free slot, or
    /// `RESOLVER_INVALID_ID` when all `RESOLVER_MAX_QUERIES` are taken.
    fn find_empty_id(&self) -> i64 {
        (0..RESOLVER_MAX_QUERIES)
            .find(|id| !self.queue.contains_key(id))
            .unwrap_or(RESOLVER_INVALID_ID)
    }

    /// `IP.resolve_hostname` — the first address, or `""` on web.
    pub fn resolve_hostname(&self, hostname: &str, ip_type: i64) -> String {
        self.resolve_hostname_addresses(hostname, ip_type)
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    /// `IP.resolve_hostname_addresses` — empty on web unless cached.
    pub fn resolve_hostname_addresses(&self, hostname: &str, ip_type: i64) -> Vec<String> {
        self.cache
            .get(&cache_key(hostname, ip_type))
            .cloned()
            .unwrap_or_default()
    }

    /// `IP.resolve_hostname_queue_item`.
    ///
    /// Returns the slot id, or `RESOLVER_INVALID_ID` when the queue is full.
    /// Because there is no resolver thread, the slot's status is already
    /// terminal when this returns: `DONE` for a cache hit, `ERROR` otherwise.
    pub fn resolve_hostname_queue_item(&mut self, hostname: &str, ip_type: i64) -> i64 {
        let id = self.find_empty_id();
        if id == RESOLVER_INVALID_ID {
            return id;
        }
        let addresses = self.resolve_hostname_addresses(hostname, ip_type);
        let status = if addresses.is_empty() {
            RESOLVER_STATUS_ERROR
        } else {
            RESOLVER_STATUS_DONE
        };
        self.queue.insert(
            id,
            ResolveItem {
                hostname: hostname.to_string(),
                ip_type,
                addresses,
                status,
            },
        );
        id
    }

    /// `IP.get_resolve_item_status`. An unknown id reports `NONE`.
    pub fn get_resolve_item_status(&self, id: i64) -> i64 {
        self.queue
            .get(&id)
            .map(|item| item.status)
            .unwrap_or(RESOLVER_STATUS_NONE)
    }

    /// `IP.get_resolve_item_address` — `""` unless the item reached `DONE`
    /// (ip.cpp:203 refuses any other status).
    pub fn get_resolve_item_address(&self, id: i64) -> String {
        self.get_resolve_item_addresses(id)
            .into_iter()
            .next()
            .unwrap_or_default()
    }

    /// `IP.get_resolve_item_addresses` — empty unless the item reached `DONE`
    /// (ip.cpp:222).
    pub fn get_resolve_item_addresses(&self, id: i64) -> Vec<String> {
        match self.queue.get(&id) {
            Some(item) if item.status == RESOLVER_STATUS_DONE => item.addresses.clone(),
            _ => Vec::new(),
        }
    }

    /// `IP.erase_resolve_item` — frees the slot for reuse.
    pub fn erase_resolve_item(&mut self, id: i64) {
        self.queue.remove(&id);
    }

    /// `IP.get_local_addresses` — always empty on web.
    pub fn get_local_addresses(&self) -> Vec<String> {
        Vec::new()
    }

    /// `IP.get_local_interfaces` — always empty on web
    /// (`IPWeb::get_local_interfaces` has an empty body).
    pub fn get_local_interfaces(&self) -> Vec<InterfaceInfo> {
        Vec::new()
    }

    /// `IP.clear_cache`. An empty hostname clears everything, matching Godot.
    pub fn clear_cache(&mut self, hostname: &str) {
        if hostname.is_empty() {
            self.cache.clear();
            return;
        }
        let suffix = format!(":{hostname}");
        self.cache.retain(|key, _| !key.ends_with(&suffix));
    }

    /// Seeds the resolver cache.
    ///
    /// Not a Godot API. It stands in for the platform resolver that web does
    /// not have, and it is the only way `RESOLVER_STATUS_DONE` is reachable —
    /// which makes the `DONE` branch testable instead of dead.
    pub fn cache_hostname(&mut self, hostname: &str, ip_type: i64, addresses: Vec<String>) {
        self.cache.insert(cache_key(hostname, ip_type), addresses);
    }

    /// Number of occupied resolver slots.
    pub fn queued_count(&self) -> usize {
        self.queue.len()
    }
}

static IP_SINGLETON: OnceLock<Mutex<Ip>> = OnceLock::new();

/// The process-global `IP` singleton.
///
/// Poisoning is recovered from rather than propagated: a panic in one script
/// must not make `IP` permanently unusable for the rest of the process.
pub fn global() -> MutexGuard<'static, Ip> {
    IP_SINGLETON
        .get_or_init(|| Mutex::new(Ip::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_uncached_lookup_is_empty_like_ipweb() {
        let ip = Ip::new();
        assert_eq!(ip.resolve_hostname("example.com", TYPE_ANY), "");
        assert!(ip
            .resolve_hostname_addresses("example.com", TYPE_ANY)
            .is_empty());
        assert!(ip.get_local_addresses().is_empty());
        assert!(ip.get_local_interfaces().is_empty());
    }

    #[test]
    fn an_uncached_queue_item_terminates_in_error_not_done() {
        // ip.cpp:105 — `response.is_empty() ? ERROR : DONE`, and IPWeb always
        // resolves to nothing. This is the case the bead's acceptance text got
        // wrong by asserting DONE.
        let mut ip = Ip::new();
        let id = ip.resolve_hostname_queue_item("example.com", TYPE_ANY);
        assert_ne!(id, RESOLVER_INVALID_ID);
        assert_eq!(ip.get_resolve_item_status(id), RESOLVER_STATUS_ERROR);
        assert_eq!(ip.get_resolve_item_address(id), "");
        assert!(ip.get_resolve_item_addresses(id).is_empty());
    }

    #[test]
    fn a_cached_queue_item_reaches_done() {
        let mut ip = Ip::new();
        ip.cache_hostname("example.com", TYPE_ANY, vec!["93.184.216.34".into()]);
        let id = ip.resolve_hostname_queue_item("example.com", TYPE_ANY);
        assert_eq!(ip.get_resolve_item_status(id), RESOLVER_STATUS_DONE);
        assert_eq!(ip.get_resolve_item_address(id), "93.184.216.34");
    }

    #[test]
    fn the_cache_key_separates_types() {
        let mut ip = Ip::new();
        ip.cache_hostname("example.com", TYPE_IPV4, vec!["1.2.3.4".into()]);
        assert_eq!(ip.resolve_hostname("example.com", TYPE_IPV4), "1.2.3.4");
        // The same host under a different type is a different entry.
        assert_eq!(ip.resolve_hostname("example.com", TYPE_IPV6), "");
    }

    #[test]
    fn clear_cache_with_a_hostname_spares_the_others() {
        let mut ip = Ip::new();
        ip.cache_hostname("a.test", TYPE_ANY, vec!["1.1.1.1".into()]);
        ip.cache_hostname("b.test", TYPE_ANY, vec!["2.2.2.2".into()]);
        ip.clear_cache("a.test");
        assert_eq!(ip.resolve_hostname("a.test", TYPE_ANY), "");
        assert_eq!(ip.resolve_hostname("b.test", TYPE_ANY), "2.2.2.2");
        ip.clear_cache("");
        assert_eq!(ip.resolve_hostname("b.test", TYPE_ANY), "");
    }

    #[test]
    fn an_erased_item_frees_its_slot_and_reports_none() {
        let mut ip = Ip::new();
        let id = ip.resolve_hostname_queue_item("example.com", TYPE_ANY);
        ip.erase_resolve_item(id);
        assert_eq!(ip.get_resolve_item_status(id), RESOLVER_STATUS_NONE);
        assert_eq!(ip.queued_count(), 0);
        // The freed slot is handed out again rather than leaking.
        assert_eq!(ip.resolve_hostname_queue_item("other.test", TYPE_ANY), id);
    }

    #[test]
    fn a_full_queue_reports_the_invalid_id() {
        let mut ip = Ip::new();
        for _ in 0..RESOLVER_MAX_QUERIES {
            assert_ne!(
                ip.resolve_hostname_queue_item("x.test", TYPE_ANY),
                RESOLVER_INVALID_ID
            );
        }
        assert_eq!(
            ip.resolve_hostname_queue_item("x.test", TYPE_ANY),
            RESOLVER_INVALID_ID
        );
    }
}
