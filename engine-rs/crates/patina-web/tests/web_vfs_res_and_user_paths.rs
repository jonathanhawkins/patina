//! The web asset VFS, exercised natively (pat-u3l8c.113).
//!
//! The bead's acceptance is a browser one — "browser demo loads scene+textures
//! from a served `.pck` via fetch; `user://` persists across reload via
//! IndexedDB/OPFS" — but almost none of it actually needs a browser. What a
//! browser contributes is the `fetch` that produces the bytes and the
//! IndexedDB puts that persist them; the decisions are all in
//! `patina_web::vfs`, so they are pinned here instead of only in a tab.
//!
//! These tests deliberately run with **no path roots configured**
//! (`set_path_roots(None, None)`), which is precisely the browser's situation:
//! no filesystem under `res://` or `user://`. On the old code path every
//! assertion here was unreachable, because `resolve_path` turned `res://` into
//! a `PathBuf` and failed outright without a root.
//!
//! What they do NOT prove: that `fetch` and IndexedDB behave as assumed. The
//! `user://` persistence test replays the mutation queue through a fresh store,
//! which is the same sequence a page reload performs against IndexedDB, and
//! `user_store_glue_matches_the_binding_names` pins the one thing a native test
//! otherwise cannot — that the JS glue calls the names `wasm_bindgen` actually
//! exports. The wasm half itself is gated by
//! `cargo check -p patina-web --target wasm32-unknown-unknown`.

use gdcore::file_access::{
    clear_mounted_packs, set_path_roots, set_user_storage, DirAccess, FileAccess, FileMode,
};
use gdresource::pck::PckPacker;
use patina_web::vfs::{
    mount_pack_bytes, queued_user_store, UserStoreWrite, WebUserStore, WriteQueue, USER_STORE_GLUE,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// Mounts, the registered `user://` store and the path roots are all
/// process-global, so these tests cannot run concurrently with each other.
///
/// ⚠️ Without this they pass under `--test-threads=1` and flake under nextest's
/// per-test processes only by luck of scheduling — the failure mode is one test
/// clearing the mounts another is mid-read on.
static VFS_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    VFS_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The browser's starting state: no filesystem under either prefix, nothing
/// mounted, no `user://` backend.
fn reset() {
    set_user_storage(None);
    clear_mounted_packs();
    set_path_roots(None, None);
}

/// A `.pck` holding `files`, as `fetch().arrayBuffer()` would deliver it.
fn packed(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut packer = PckPacker::new();
    for (path, bytes) in files {
        packer.add_file(*path, bytes.to_vec());
    }
    packer.pack().expect("pack the fixture")
}

/// Writes `text` to `path` the way a script does, and closes the handle.
fn write_user_file(path: &str, text: &str) {
    let mut file = FileAccess::open(path, FileMode::Write).expect("write open creates the file");
    file.store_string(text).expect("store_string");
    // Dropped here: close() is what flushes the buffer into the store.
}

// ---------------------------------------------------------------------------
// res:// out of a fetched pack
// ---------------------------------------------------------------------------

/// The whole point of the res:// half: with no project directory at all, a
/// `res://` read resolves out of the bytes the page fetched.
#[test]
fn res_reads_resolve_out_of_fetched_pack_bytes() {
    let _guard = lock();
    reset();

    let bytes = packed(&[
        ("res://main.tscn", b"[gd_scene]"),
        ("res://art/icon.png", b"PNGDATA"),
    ]);
    mount_pack_bytes("game", bytes, 0).expect("the fetched bytes are a valid pck");

    assert!(
        FileAccess::file_exists("res://main.tscn"),
        "a packed scene must exist with no filesystem under res://"
    );
    assert_eq!(
        FileAccess::read_file_bytes("res://art/icon.png").expect("read the packed texture"),
        b"PNGDATA".to_vec(),
        "the texture bytes must come back byte-for-byte out of the pack"
    );
    assert!(
        !FileAccess::file_exists("res://absent.tscn"),
        "a path in no pack must not report as existing"
    );

    reset();
}

/// A later pack at a higher priority shadows the base one, which is how a
/// patch delivered as a second fetch replaces content
/// (`ProjectSettings.load_resource_pack`).
#[test]
fn a_higher_priority_pack_shadows_the_base_one() {
    let _guard = lock();
    reset();

    mount_pack_bytes("base", packed(&[("res://data.txt", b"base")]), 0).expect("base mounts");
    mount_pack_bytes("patch", packed(&[("res://data.txt", b"patched")]), 10).expect("patch mounts");

    assert_eq!(
        FileAccess::read_file_bytes("res://data.txt").expect("read"),
        b"patched".to_vec(),
        "the higher-priority mount must win"
    );

    reset();
}

/// Bytes that are not a pack are reported as an error rather than mounted
/// empty — a 404 page delivered instead of the `.pck` is the common case, and
/// silently mounting nothing would surface later as "every asset is missing".
#[test]
fn non_pack_bytes_are_rejected_with_the_label() {
    let _guard = lock();
    reset();

    let err = mount_pack_bytes("game", b"<!doctype html>".to_vec(), 0)
        .expect_err("an HTML error page is not a pck");
    assert!(
        err.contains("game"),
        "the error must name the mount that failed, got {err:?}"
    );
    assert!(
        !FileAccess::file_exists("res://main.tscn"),
        "a rejected pack must not be mounted"
    );

    reset();
}

// ---------------------------------------------------------------------------
// user:// through the write-through store
// ---------------------------------------------------------------------------

/// A `user://` write is readable immediately (synchronously, on the calling
/// frame) and is queued exactly once for the host to persist.
#[test]
fn a_user_write_is_readable_at_once_and_queued_once() {
    let _guard = lock();
    reset();

    let (store, queue) = queued_user_store();
    WebUserStore::install(Arc::clone(&store));

    write_user_file("user://save.cfg", "level=7");

    assert_eq!(
        FileAccess::read_file_string("user://save.cfg").expect("read back"),
        "level=7",
        "the write must be visible without waiting for persistence"
    );
    assert_eq!(
        queue.drain(),
        vec![UserStoreWrite::Put {
            path: "user://save.cfg".to_string(),
            bytes: b"level=7".to_vec(),
        }],
        "exactly one put must be queued for IndexedDB"
    );

    reset();
}

/// Hydrating must not queue anything. This is the one that matters on every
/// reload: hydrating through `write_file` would queue a put for each file the
/// page had just *read* out of IndexedDB, rewriting the whole store at boot.
#[test]
fn hydration_does_not_queue_a_write() {
    let _guard = lock();
    reset();

    let (store, queue) = queued_user_store();
    store.hydrate("user://save.cfg", b"level=7".to_vec());
    WebUserStore::install(Arc::clone(&store));

    assert_eq!(
        FileAccess::read_file_string("user://save.cfg").expect("hydrated file is readable"),
        "level=7"
    );
    assert!(
        queue.is_empty(),
        "hydration must not queue writes, got {:?}",
        queue.drain()
    );

    reset();
}

/// The acceptance's persistence clause, native stand-in: drain the queue into a
/// map (what the host's IndexedDB holds), throw the engine-side store away, and
/// rebuild from that map — the sequence a page reload performs.
#[test]
fn user_data_survives_a_reload() {
    let _guard = lock();
    reset();

    // --- first page load ---
    let (store, queue) = queued_user_store();
    WebUserStore::install(Arc::clone(&store));
    write_user_file("user://save.cfg", "level=7");
    write_user_file("user://profile/name.txt", "ada");

    let persisted = persist(&queue);
    assert_eq!(persisted.len(), 2, "both writes reached the host");
    drop(store);
    set_user_storage(None);

    // --- reload: nothing in memory, everything in "IndexedDB" ---
    assert!(
        FileAccess::read_file_string("user://save.cfg").is_err(),
        "with no store installed the data must be unreachable — otherwise this \
         test would pass on leftover state instead of on hydration"
    );

    let (fresh, fresh_queue) = queued_user_store();
    for (path, bytes) in &persisted {
        fresh.hydrate(path, bytes.clone());
    }
    WebUserStore::install(Arc::clone(&fresh));

    assert_eq!(
        FileAccess::read_file_string("user://save.cfg").expect("survives the reload"),
        "level=7"
    );
    assert_eq!(
        FileAccess::read_file_string("user://profile/name.txt").expect("survives the reload"),
        "ada"
    );
    assert!(
        fresh_queue.is_empty(),
        "a reload must not re-queue what it just loaded"
    );

    reset();
}

/// A removal persists as a removal, so a deleted save does not come back on the
/// next reload.
#[test]
fn a_removal_persists_and_does_not_resurrect() {
    let _guard = lock();
    reset();

    let (store, queue) = queued_user_store();
    WebUserStore::install(Arc::clone(&store));
    write_user_file("user://save.cfg", "level=7");
    DirAccess::open("user://")
        .expect("user:// opens through the store")
        .remove("save.cfg")
        .expect("remove the save");

    let persisted = persist(&queue);
    assert!(
        persisted.is_empty(),
        "the put must be undone by the remove, leaving nothing persisted, got {:?}",
        persisted.keys().collect::<Vec<_>>()
    );

    reset();
}

/// A rename is two mutations in order — the old key removed, the new key
/// written — because the store underneath is a flat key-value map. Order
/// matters: replayed the other way round, the file would be gone.
#[test]
fn a_rename_queues_a_remove_then_a_put() {
    let _guard = lock();
    reset();

    let (store, queue) = queued_user_store();
    WebUserStore::install(Arc::clone(&store));
    store.hydrate("user://old.cfg", b"keep".to_vec());

    let mut dir = DirAccess::open("user://").expect("user:// opens through the store");
    dir.rename("old.cfg", "new.cfg").expect("rename");

    assert_eq!(
        queue.drain(),
        vec![
            UserStoreWrite::Remove {
                path: "user://old.cfg".to_string()
            },
            UserStoreWrite::Put {
                path: "user://new.cfg".to_string(),
                bytes: b"keep".to_vec(),
            },
        ],
        "a rename must queue the remove before the put"
    );
    assert_eq!(
        FileAccess::read_file_string("user://new.cfg").expect("read the renamed file"),
        "keep"
    );

    reset();
}

/// Listing a `user://` directory returns immediate children only. A flat
/// key-value store would otherwise report every nested file as a sibling of the
/// directory it lives under.
#[test]
fn listing_returns_immediate_children_only() {
    let _guard = lock();
    reset();

    let (store, _queue) = queued_user_store();
    for path in [
        "user://save.cfg",
        "user://profile/name.txt",
        "user://profile/avatars/a.png",
    ] {
        store.hydrate(path, b"x".to_vec());
    }
    WebUserStore::install(Arc::clone(&store));

    let mut dir = DirAccess::open("user://").expect("user:// opens through the store");
    let mut files = dir.get_files();
    files.sort();
    let mut dirs = dir.get_directories();
    dirs.sort();

    assert_eq!(files, vec!["save.cfg".to_string()]);
    assert_eq!(
        dirs,
        vec!["profile".to_string()],
        "a nested file must surface as its directory, not as a sibling"
    );

    reset();
}

// ---------------------------------------------------------------------------
// The JS contract
// ---------------------------------------------------------------------------

/// The glue and the binding have to agree on names, and nothing else checks
/// that: the glue is a string here and the `js_name` attributes it calls are
/// compiled only for wasm32, so a rename on either side is invisible to every
/// native test and to the wasm build alike.
#[test]
fn user_store_glue_matches_the_binding_names() {
    for needle in [
        // The methods `web_vfs` exports under these exact `js_name`s.
        "vfs.hydrateUserFile(",
        "vfs.takePendingWrite()",
        "write.isRemove",
        "write.path",
        "write.bytes",
        // The entry points the host is documented to call, in order.
        "export function openUserStore",
        "export async function hydrateUserStore",
        "export function persistUserWrite",
        "export async function drainPendingWrites",
    ] {
        assert!(
            USER_STORE_GLUE.contains(needle),
            "the IndexedDB glue no longer references `{needle}`; if a binding was \
             renamed, rename it in both places"
        );
    }
    assert!(
        USER_STORE_GLUE.contains("store.put(write.bytes, write.path)"),
        "the object store has no keyPath, so the key must be passed out-of-line"
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Applies everything queued to a map, the way the host's IndexedDB would:
/// puts write a key, removes delete one, oldest first.
fn persist(queue: &Arc<WriteQueue>) -> BTreeMap<String, Vec<u8>> {
    let mut persisted: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for write in queue.drain() {
        match write {
            UserStoreWrite::Put { path, bytes } => {
                persisted.insert(path, bytes);
            }
            UserStoreWrite::Remove { path } => {
                persisted.remove(&path);
            }
        }
    }
    persisted
}
