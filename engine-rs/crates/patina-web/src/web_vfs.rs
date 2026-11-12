//! Browser bindings for the web asset VFS (pat-u3l8c.113).
//!
//! The counterpart to [`crate::web_audio`], and the same split: every decision
//! lives in [`crate::vfs`], which is portable and natively tested, and this
//! module is only the handle JS holds.
//!
//! It deliberately imports **nothing** from JS. The browser's two async jobs —
//! fetching the `.pck` and writing `user://` into IndexedDB — are driven from
//! the JS side instead:
//!
//! 1. `fetch(url)` → `arrayBuffer()` → [`WebVfs::mount_pack`] with the bytes,
//!    so `res://` resolves out of the archive from then on;
//! 2. on boot, read the object store and replay it through
//!    [`WebVfs::hydrate_user_file`] *before* the main loop starts;
//! 3. after each frame (or on an interval), drain
//!    [`WebVfs::take_pending_write`] and persist each entry.
//!
//! [`USER_STORE_GLUE`](crate::vfs::USER_STORE_GLUE) is the IndexedDB source for
//! steps 2 and 3, the way [`gdaudio::audio_worklet_glue`] is the worklet source
//! — handed to the host as a string so the wasm module needs no JS import of
//! its own.
//!
//! ⚠️ Why a queue rather than a direct call into JS: `UserStorage` is
//! synchronous because a script's `store_string()` has to return on the calling
//! frame, while IndexedDB is asynchronous, and a promise cannot be awaited on
//! the thread that resolves it. See [`crate::vfs`] for the full reasoning and
//! the durability caveat it implies.
//!
//! Build it with:
//! `cargo check -p patina-web --target wasm32-unknown-unknown`

use std::sync::Arc;
use wasm_bindgen::prelude::*;

use crate::vfs::{mount_pack_bytes, queued_user_store, UserStoreWrite, WebUserStore, WriteQueue};

/// One `user://` mutation for the host to persist.
///
/// `bytes` is empty for a removal; check [`PendingWrite::is_remove`] rather
/// than the length, because an empty file is a legitimate `Put`.
#[wasm_bindgen]
pub struct PendingWrite {
    path: String,
    bytes: Vec<u8>,
    remove: bool,
}

#[wasm_bindgen]
impl PendingWrite {
    /// The full `user://…` path this mutation applies to.
    #[wasm_bindgen(getter)]
    pub fn path(&self) -> String {
        self.path.clone()
    }

    /// The new contents — empty when this is a removal.
    #[wasm_bindgen(getter)]
    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    /// Whether the host should delete the key instead of writing it.
    #[wasm_bindgen(getter, js_name = isRemove)]
    pub fn is_remove(&self) -> bool {
        self.remove
    }
}

impl From<UserStoreWrite> for PendingWrite {
    fn from(write: UserStoreWrite) -> Self {
        match write {
            UserStoreWrite::Put { path, bytes } => Self {
                path,
                bytes,
                remove: false,
            },
            UserStoreWrite::Remove { path } => Self {
                path,
                bytes: Vec::new(),
                remove: true,
            },
        }
    }
}

/// The page's view of the engine filesystem: a mounted `res://` pack and a
/// persistent `user://`.
///
/// Constructing it installs the `user://` backend process-wide, so the host
/// should make exactly one and keep it for the lifetime of the page — dropping
/// it leaves `gdcore` holding the store through its own `Arc`, but the queue
/// stops being drained, so writes accumulate unpersisted.
#[wasm_bindgen]
pub struct WebVfs {
    store: Arc<WebUserStore>,
    queue: Arc<WriteQueue>,
}

#[wasm_bindgen]
impl WebVfs {
    /// Installs a fresh, empty `user://` store and returns the handle.
    ///
    /// Hydrate it before starting the main loop: a script that reads
    /// `user://settings.cfg` on the first frame must not see an empty store
    /// just because the IndexedDB read had not finished.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let (store, queue) = queued_user_store();
        WebUserStore::install(Arc::clone(&store));
        Self { store, queue }
    }

    /// Mounts fetched `.pck` bytes over `res://` under `label`.
    ///
    /// `priority` orders overlapping mounts, so a patch pack fetched after the
    /// base one can shadow it (`ProjectSettings.load_resource_pack`).
    #[wasm_bindgen(js_name = mountPack)]
    pub fn mount_pack(&self, label: &str, bytes: &[u8], priority: i32) -> Result<(), JsValue> {
        mount_pack_bytes(label, bytes.to_vec(), priority).map_err(|err| JsValue::from_str(&err))
    }

    /// Loads one previously persisted `user://` file, without queueing it for
    /// persistence again.
    #[wasm_bindgen(js_name = hydrateUserFile)]
    pub fn hydrate_user_file(&self, user_path: &str, bytes: &[u8]) {
        self.store.hydrate(user_path, bytes.to_vec());
    }

    /// The oldest unpersisted mutation, or `undefined` when there is nothing to
    /// do. Drain in a loop; the order is the engine's write order and must be
    /// preserved.
    #[wasm_bindgen(js_name = takePendingWrite)]
    pub fn take_pending_write(&self) -> Option<PendingWrite> {
        self.queue.take_next().map(PendingWrite::from)
    }

    /// How many mutations are waiting to be persisted.
    #[wasm_bindgen(js_name = pendingWriteCount)]
    pub fn pending_write_count(&self) -> usize {
        self.queue.len()
    }

    /// How many `user://` files the store currently holds.
    #[wasm_bindgen(js_name = userFileCount)]
    pub fn user_file_count(&self) -> usize {
        self.store.len()
    }

    /// The IndexedDB source for the host's half of `user://`.
    ///
    /// A getter rather than a free function so the host does not need a second
    /// import.
    #[wasm_bindgen(getter, js_name = userStoreGlue)]
    pub fn user_store_glue(&self) -> String {
        crate::vfs::USER_STORE_GLUE.to_string()
    }
}

impl Default for WebVfs {
    /// Same as [`WebVfs::new`] — present because `new()` takes no arguments.
    fn default() -> Self {
        Self::new()
    }
}
