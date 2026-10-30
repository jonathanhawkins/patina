//! The web asset VFS: `res://` served out of a fetched `.pck`, `user://` kept
//! in the browser's persistent storage (pat-u3l8c.113).
//!
//! Godot's web export solves both halves the same way this module does: the
//! exported `.pck` is fetched over HTTP and mounted as the resource pack
//! (`PackedData`), and `user://` is routed to an Emscripten IDBFS mount instead
//! of a host filesystem (`platform/web/*`). A browser has no filesystem under
//! either prefix, so `gdcore::file_access` exposes two seams —
//! [`gdcore::file_access::mount_pack`] for `res://` and
//! [`gdcore::file_access::set_user_storage`] for `user://` — and this module is
//! the web implementation of both.
//!
//! **Everything here is portable Rust with no browser bindings**, which is the
//! same split [`crate::audio`] uses: the contract is exercised by native
//! `cargo test`, and only the plumbing that actually needs a browser (the
//! `fetch` call and the IndexedDB puts) lives in [`crate::web_vfs`]. Bytes
//! arrive here as a `Vec<u8>` whatever fetched them.
//!
//! ## Why `user://` is a write-through cache rather than a direct backend
//!
//! [`gdcore::file_access::UserStorage`] is **synchronous** — it has to be,
//! because `FileAccess.store_string()` returns a value to a script on the
//! calling frame — while IndexedDB and the Origin Private File System are both
//! **asynchronous**. The two cannot be bridged by blocking: there is no way to
//! await a promise on the browser's main thread without deadlocking the event
//! loop that would resolve it.
//!
//! So [`WebUserStore`] holds the live bytes in memory and reports every
//! mutation to a persist hook, which the browser half turns into a
//! fire-and-forget IndexedDB write. A page load hydrates the map back
//! ([`WebUserStore::hydrate`]) before the main loop starts. That is exactly the
//! shape of Emscripten's IDBFS, which Godot's own web export depends on: a
//! synchronous in-memory filesystem that persists asynchronously.
//!
//! ⚠️ The consequence is worth stating plainly rather than discovering later: a
//! write is durable only once the browser has run the queued put, so a tab
//! killed in the same tick as `store_string()` can lose that write. Godot's web
//! export has the same property.

use gdcore::file_access::{mount_pack, set_user_storage, PackDirListing, UserStorage};
use gdresource::pck::PckMount;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// One mutation the engine made to `user://`, handed to the persist hook.
///
/// `Remove` carries the path alone, so the hook can delete a key without
/// reading it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserStoreWrite {
    /// `path` now holds `bytes` (a create or an overwrite).
    Put {
        /// The full `user://…` path.
        path: String,
        /// The new contents.
        bytes: Vec<u8>,
    },
    /// `path` no longer exists.
    Remove {
        /// The full `user://…` path.
        path: String,
    },
}

/// What [`WebUserStore`] calls after every mutation, to persist it.
///
/// `Send + Sync` because `UserStorage` is: `gdcore` stores the registered
/// backend in a process-global, and the engine may touch `user://` from any
/// thread (a browser is single-threaded, but the trait is shared with native).
pub type PersistHook = Box<dyn Fn(&UserStoreWrite) + Send + Sync>;

/// A `user://` backend that answers synchronously from memory and reports each
/// mutation for asynchronous persistence.
pub struct WebUserStore {
    files: RwLock<BTreeMap<String, Vec<u8>>>,
    persist: PersistHook,
}

impl std::fmt::Debug for WebUserStore {
    /// Hand-written because a `dyn Fn` hook is not `Debug`; the useful part is
    /// how many files the store is holding, not the closure's address.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebUserStore")
            .field("files", &self.len())
            .finish_non_exhaustive()
    }
}

impl WebUserStore {
    /// An empty store whose mutations are reported to `persist`.
    pub fn new(persist: impl Fn(&UserStoreWrite) + Send + Sync + 'static) -> Self {
        Self {
            files: RwLock::new(BTreeMap::new()),
            persist: Box::new(persist),
        }
    }

    /// A store that discards its writes — for tests and for a page that has no
    /// persistent storage available (a browser can refuse IndexedDB in private
    /// mode). `user://` still works for the lifetime of the page, which is what
    /// Godot does when IDBFS is unavailable.
    pub fn ephemeral() -> Self {
        Self::new(|_| {})
    }

    /// Loads a previously persisted file **without** reporting it back to the
    /// persist hook.
    ///
    /// ⚠️ The "without" is the whole point: hydrating through `write_file`
    /// would queue an IndexedDB put for every file the page just read out of
    /// IndexedDB, turning each reload into a full rewrite.
    pub fn hydrate(&self, user_path: &str, bytes: Vec<u8>) {
        self.files_mut().insert(user_path.to_string(), bytes);
    }

    /// How many files the store currently holds.
    pub fn len(&self) -> usize {
        self.files_ref().len()
    }

    /// Whether the store holds no files.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every path the store holds, in sorted order.
    pub fn paths(&self) -> Vec<String> {
        self.files_ref().keys().cloned().collect()
    }

    /// Registers `self` as the process-wide `user://` backend.
    ///
    /// Takes an `Arc` because `gdcore` keeps the backend alive behind a
    /// trait object and the browser half needs its own handle to hydrate
    /// through.
    pub fn install(store: Arc<Self>) {
        // Spelled out rather than coerced inside `Some(..)`: the unsizing to
        // `Arc<dyn UserStorage>` is what `set_user_storage` wants, and naming
        // it keeps the coercion at a site that cannot be ambiguous.
        let backend: Arc<dyn UserStorage> = store;
        set_user_storage(Some(backend));
    }

    fn files_ref(&self) -> std::sync::RwLockReadGuard<'_, BTreeMap<String, Vec<u8>>> {
        self.files
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn files_mut(&self) -> std::sync::RwLockWriteGuard<'_, BTreeMap<String, Vec<u8>>> {
        self.files
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl UserStorage for WebUserStore {
    fn file_exists(&self, user_path: &str) -> bool {
        self.files_ref().contains_key(user_path)
    }

    fn read_file(&self, user_path: &str) -> Option<Vec<u8>> {
        self.files_ref().get(user_path).cloned()
    }

    fn write_file(&self, user_path: &str, bytes: &[u8]) -> Result<(), String> {
        // The guard is dropped before the hook runs: the hook reaches into JS,
        // and a JS callback that touched `user://` again would deadlock on a
        // lock we were still holding.
        self.files_mut()
            .insert(user_path.to_string(), bytes.to_vec());
        (self.persist)(&UserStoreWrite::Put {
            path: user_path.to_string(),
            bytes: bytes.to_vec(),
        });
        Ok(())
    }

    fn remove(&self, user_path: &str) -> Result<(), String> {
        if self.files_mut().remove(user_path).is_none() {
            return Err(format!("{user_path} does not exist"));
        }
        (self.persist)(&UserStoreWrite::Remove {
            path: user_path.to_string(),
        });
        Ok(())
    }

    fn dir_exists(&self, user_dir: &str) -> bool {
        let prefix = format!("{user_dir}/");
        self.files_ref().keys().any(|key| key.starts_with(&prefix))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let bytes = {
            let mut files = self.files_mut();
            let bytes = files
                .remove(from)
                .ok_or_else(|| format!("{from} does not exist"))?;
            files.insert(to.to_string(), bytes.clone());
            bytes
        };
        // Two mutations, because the store underneath is a flat key-value map
        // with no rename of its own.
        (self.persist)(&UserStoreWrite::Remove {
            path: from.to_string(),
        });
        (self.persist)(&UserStoreWrite::Put {
            path: to.to_string(),
            bytes,
        });
        Ok(())
    }

    /// Immediate children only, the way a `.pck` is listed: a flat key-value
    /// store would otherwise surface every nested file as a sibling of the
    /// directory being listed.
    fn list_dir(&self, user_dir: &str) -> PackDirListing {
        let prefix = if user_dir == "user://" {
            "user://".to_string()
        } else {
            format!("{user_dir}/")
        };
        let mut listing = PackDirListing::default();
        for key in self.files_ref().keys() {
            let Some(rest) = key.strip_prefix(&prefix) else {
                continue;
            };
            match rest.split_once('/') {
                Some((dir, _)) => {
                    let dir = dir.to_string();
                    if !listing.dirs.contains(&dir) {
                        listing.dirs.push(dir);
                    }
                }
                None => listing.files.push(rest.to_string()),
            }
        }
        listing
    }
}

/// The host's IndexedDB half of `user://`, as a JS module source.
///
/// Handed to the page as a string for the same reason
/// [`gdaudio::audio_worklet_glue`] is: the wasm module then needs no JS import
/// of its own, and the browser code lives next to the Rust that defines its
/// contract instead of in a separate asset that can drift.
///
/// The host's responsibilities, in order:
/// 1. `openUserStore()` once, before the engine starts;
/// 2. `hydrateUserStore(db, vfs)` — replay persisted files into the store
///    **before** the first frame, or a script reading `user://settings.cfg` on
///    frame one sees an empty store;
/// 3. `drainPendingWrites(db, vfs)` after each frame or on an interval.
///
/// ⚠️ `store.put(value, key)` passes the key out-of-line because the object
/// store is created without a `keyPath`; the `user://…` path IS the key.
pub const USER_STORE_GLUE: &str = r#"// patina user:// persistence (pat-u3l8c.113)
const DB_NAME = 'patina-user';
const STORE = 'user';

export function openUserStore(name = DB_NAME) {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(name, 1);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(STORE)) db.createObjectStore(STORE);
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export function readAllUserFiles(db) {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, 'readonly');
    const req = tx.objectStore(STORE).openCursor();
    const out = [];
    req.onsuccess = () => {
      const cursor = req.result;
      if (!cursor) { resolve(out); return; }
      out.push([cursor.key, new Uint8Array(cursor.value)]);
      cursor.continue();
    };
    req.onerror = () => reject(req.error);
  });
}

export async function hydrateUserStore(db, vfs) {
  for (const [path, bytes] of await readAllUserFiles(db)) {
    vfs.hydrateUserFile(path, bytes);
  }
}

export function persistUserWrite(db, write) {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, 'readwrite');
    const store = tx.objectStore(STORE);
    if (write.isRemove) {
      store.delete(write.path);
    } else {
      store.put(write.bytes, write.path);
    }
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

export async function drainPendingWrites(db, vfs) {
  for (let write = vfs.takePendingWrite(); write; write = vfs.takePendingWrite()) {
    await persistUserWrite(db, write);
  }
}
"#;

/// A queue of mutations waiting to be persisted by the browser half.
///
/// The engine writes synchronously; the browser persists asynchronously. This
/// is the handoff between the two, and it is deliberately the *only* thing the
/// JS side has to understand: it drains the queue whenever it likes and turns
/// each entry into an IndexedDB put or delete.
///
/// Keeping the queue here rather than calling an imported JS function from the
/// persist hook is what makes the whole `user://` path natively testable — the
/// same reason [`crate::audio`] keeps the mixer chain out of
/// [`crate::web_audio`].
#[derive(Debug, Default)]
pub struct WriteQueue {
    pending: std::sync::Mutex<std::collections::VecDeque<UserStoreWrite>>,
}

impl WriteQueue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a mutation.
    pub fn push(&self, write: UserStoreWrite) {
        self.lock().push_back(write);
    }

    /// Removes and returns the oldest mutation, oldest first so replaying the
    /// queue reproduces the engine's write order — a `Put` followed by a
    /// `Remove` of the same path must not persist in the other order.
    pub fn take_next(&self) -> Option<UserStoreWrite> {
        self.lock().pop_front()
    }

    /// Removes and returns everything queued, oldest first.
    pub fn drain(&self) -> Vec<UserStoreWrite> {
        self.lock().drain(..).collect()
    }

    /// How many mutations are waiting.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// Whether nothing is waiting.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, std::collections::VecDeque<UserStoreWrite>> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A `user://` store whose mutations land in a [`WriteQueue`], plus that queue.
///
/// This is the pairing the browser half installs: the engine gets a
/// synchronous store, and JS gets a queue to drain into IndexedDB.
pub fn queued_user_store() -> (Arc<WebUserStore>, Arc<WriteQueue>) {
    let queue = Arc::new(WriteQueue::new());
    let hook_queue = Arc::clone(&queue);
    let store = Arc::new(WebUserStore::new(move |write| {
        hook_queue.push(write.clone());
    }));
    (store, queue)
}

/// Mounts the bytes of a `.pck` over `res://` under `label`.
///
/// This is the browser's whole asset story: the exported pack is fetched into
/// memory and every `res://` read resolves out of it, because there is no
/// filesystem to fall back to. `priority` orders overlapping mounts the way
/// `ProjectSettings.load_resource_pack` does, so a patch pack fetched later can
/// shadow the base one.
///
/// Returns the parse error as a `String` rather than an `io::Error` so the
/// browser binding can hand it straight to JS.
pub fn mount_pack_bytes(label: &str, bytes: Vec<u8>, priority: i32) -> Result<(), String> {
    let mount = PckMount::from_bytes(bytes).map_err(|err| format!("{label}: {err}"))?;
    mount_pack(label, Arc::new(mount), priority);
    Ok(())
}
