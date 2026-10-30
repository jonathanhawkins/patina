//! `user://` routes through a registered [`UserStorage`] instead of the
//! filesystem (pat-u3l8c.113).
//!
//! The web half of the async asset VFS needs `user://` to live in IndexedDB or
//! the Origin Private File System, because a browser has no filesystem under
//! it. Before this, `gdcore::file_access::resolve_path` mapped `user://`
//! straight to a `PathBuf` under `user_root()`, so there was no seam a browser
//! backend could be installed at: every `user://` open either hit the host FS
//! or — with no root configured, which is the browser case — failed outright.
//!
//! These tests pin the seam itself, with an in-memory store standing in for
//! IndexedDB. They deliberately run with **no user root configured**, which is
//! what makes them meaningful: every assertion here would be unreachable on the
//! old code path, since `resolve_path` would have returned
//! `InvalidOperation("user:// path used but no user root configured")`.
//!
//! What they do *not* prove is the browser clause of the bead's acceptance
//! ("persists across reload via IndexedDB/OPFS") — that needs a wasm build and
//! a real browser. `user_data_survives_a_store_rehydration` is the native
//! stand-in: it drops every engine-side handle, rebuilds the store from the
//! bytes it had persisted, and reads through the fresh instance, which is the
//! same sequence a page reload performs against IndexedDB.

use gdcore::file_access::{
    set_path_roots, set_user_storage, DirAccess, FileAccess, FileMode, PackDirListing, UserStorage,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// `set_user_storage` and `set_path_roots` both write process-global state, so
/// the tests in this binary must not overlap.
///
/// This is not belt-and-braces: nextest gives each *binary* its own process but
/// runs the tests inside one concurrently, so without this lock a test that
/// clears the store would race one that just registered it.
static STORE_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    STORE_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

// ---------------------------------------------------------------------------
// An in-memory stand-in for the browser store
// ---------------------------------------------------------------------------

/// The persisted bytes, shared so a "reload" can rebuild a store over them —
/// exactly what an IndexedDB object store is to the page that reopens it.
type Persisted = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

struct MemoryUserStorage {
    files: Persisted,
}

impl MemoryUserStorage {
    fn new(files: Persisted) -> Self {
        Self { files }
    }

    fn files(&self) -> MutexGuard<'_, BTreeMap<String, Vec<u8>>> {
        self.files.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl UserStorage for MemoryUserStorage {
    fn file_exists(&self, user_path: &str) -> bool {
        self.files().contains_key(user_path)
    }

    fn read_file(&self, user_path: &str) -> Option<Vec<u8>> {
        self.files().get(user_path).cloned()
    }

    fn write_file(&self, user_path: &str, bytes: &[u8]) -> Result<(), String> {
        self.files().insert(user_path.to_string(), bytes.to_vec());
        Ok(())
    }

    fn remove(&self, user_path: &str) -> Result<(), String> {
        if self.files().remove(user_path).is_none() {
            return Err(format!("{user_path} does not exist"));
        }
        Ok(())
    }

    fn dir_exists(&self, user_dir: &str) -> bool {
        let prefix = format!("{user_dir}/");
        self.files().keys().any(|k| k.starts_with(&prefix))
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        let mut files = self.files();
        let bytes = files
            .remove(from)
            .ok_or_else(|| format!("{from} does not exist"))?;
        files.insert(to.to_string(), bytes);
        Ok(())
    }

    /// A flat key-value store, listed the way a `.pck` is: immediate children
    /// only, so a nested file does not surface as a sibling of its directory.
    fn list_dir(&self, user_dir: &str) -> PackDirListing {
        let prefix = if user_dir == "user://" {
            "user://".to_string()
        } else {
            format!("{user_dir}/")
        };
        let mut listing = PackDirListing::default();
        for key in self.files().keys() {
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

/// Registers a store over `files` and clears any configured path roots, so the
/// test is running in the browser's situation: `user://` with no filesystem
/// under it.
fn install(files: &Persisted) {
    set_path_roots(None, None);
    set_user_storage(Some(Arc::new(MemoryUserStorage::new(files.clone()))));
}

fn uninstall() {
    set_user_storage(None);
    set_path_roots(None, None);
}

fn empty_store() -> Persisted {
    Arc::new(Mutex::new(BTreeMap::new()))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A `user://` write goes into the store, not onto disk, and comes back out
/// through the ordinary read helpers.
#[test]
fn user_writes_round_trip_through_the_registered_store() {
    let _guard = lock();
    let files = empty_store();
    install(&files);

    {
        let mut f = FileAccess::open("user://save.cfg", FileMode::Write)
            .expect("a write open creates the file in the store");
        f.store_string("level=7").expect("store_string");
    } // dropped: close() flushes the buffer to the store

    // Read the store directly — the engine, not the test, is what put this here.
    assert_eq!(
        files
            .lock()
            .unwrap()
            .get("user://save.cfg")
            .map(|b| String::from_utf8_lossy(b).into_owned()),
        Some("level=7".to_string()),
        "close() must flush the handle's buffer into the store"
    );

    assert!(FileAccess::file_exists("user://save.cfg"));
    assert_eq!(
        FileAccess::read_file_string("user://save.cfg").unwrap(),
        "level=7"
    );

    // Nothing was written to a filesystem path: there is no user root at all,
    // which on the old code path made this whole sequence impossible.
    assert!(
        !std::path::Path::new("user://save.cfg").exists(),
        "the `user://` spelling must never be treated as a literal path"
    );

    uninstall();
}

/// A read-only open of an absent file fails, and a non-truncating write mode
/// sees what is already there — the same two rules the filesystem branch has.
#[test]
fn open_modes_match_the_filesystem_branch() {
    let _guard = lock();
    let files = empty_store();
    install(&files);

    assert!(
        FileAccess::open("user://missing.dat", FileMode::Read).is_err(),
        "a read-only open of an absent file must fail, as it does on disk"
    );
    assert!(
        FileAccess::read_file_bytes("user://missing.dat").is_err(),
        "reading an absent file must fail rather than yield empty bytes"
    );
    assert!(!FileAccess::file_exists("user://missing.dat"));

    {
        let mut f = FileAccess::open("user://log.txt", FileMode::Write).unwrap();
        f.store_string("first").unwrap();
    }
    // WriteRead does not truncate, so the existing bytes are still there and an
    // append lands after them.
    {
        let mut f = FileAccess::open("user://log.txt", FileMode::WriteRead).unwrap();
        let len = f.get_length().expect("stored length");
        assert_eq!(len, 5, "WriteRead must not truncate the stored bytes");
        f.seek(len).expect("seek to end");
        f.store_string("+second").unwrap();
    }
    assert_eq!(
        FileAccess::read_file_string("user://log.txt").unwrap(),
        "first+second",
        "a non-truncating write mode must open over the stored bytes"
    );

    // Write truncates, as `OpenOptions::truncate(true)` does on disk.
    {
        let mut f = FileAccess::open("user://log.txt", FileMode::Write).unwrap();
        f.store_string("replaced").unwrap();
    }
    assert_eq!(
        FileAccess::read_file_string("user://log.txt").unwrap(),
        "replaced"
    );

    uninstall();
}

/// The native stand-in for the bead's "persists across reload" clause: every
/// engine-side handle is dropped, a *new* store is built over the bytes that
/// were persisted, and the data is still there.
#[test]
fn user_data_survives_a_store_rehydration() {
    let _guard = lock();
    let files = empty_store();
    install(&files);

    {
        let mut f = FileAccess::open("user://profile/slot1.sav", FileMode::Write).unwrap();
        f.store_buffer(&[1, 2, 3, 4]).unwrap();
    }
    // Tear the whole engine-side registration down, as closing the page does.
    uninstall();
    assert!(
        !FileAccess::file_exists("user://profile/slot1.sav"),
        "with no store registered and no user root there is nothing to find"
    );

    // Reload: a fresh store instance over the same persisted bytes.
    install(&files);
    assert_eq!(
        FileAccess::read_file_bytes("user://profile/slot1.sav").unwrap(),
        vec![1, 2, 3, 4],
        "a rehydrated store must still serve what the previous session wrote"
    );

    uninstall();
}

/// `DirAccess` lists and mutates the store, including through a handle opened
/// on a `user://` directory that exists only inside it.
#[test]
fn dir_access_lists_and_mutates_the_store() {
    let _guard = lock();
    let files = empty_store();
    install(&files);

    for path in [
        "user://profile/slot1.sav",
        "user://profile/slot2.sav",
        "user://profile/screens/thumb.png",
        "user://settings.cfg",
    ] {
        let mut f = FileAccess::open(path, FileMode::Write).unwrap();
        f.store_string("x").unwrap();
    }

    assert!(DirAccess::dir_exists("user://profile"));
    assert!(
        DirAccess::dir_exists("user://"),
        "the store root always exists, as the filesystem user root does"
    );
    assert!(!DirAccess::dir_exists("user://nope"));

    assert_eq!(
        DirAccess::get_files_at("user://profile").unwrap(),
        vec!["slot1.sav".to_string(), "slot2.sav".to_string()],
        "immediate children only, sorted — a nested file is not a sibling"
    );
    assert_eq!(
        DirAccess::get_directories_at("user://profile").unwrap(),
        vec!["screens".to_string()]
    );
    assert_eq!(
        DirAccess::get_files_at("user://").unwrap(),
        vec!["settings.cfg".to_string()]
    );

    // A handle on a store-only directory opens, lists, and resolves relative
    // children in `user://` space rather than against a filesystem path.
    let mut dir = DirAccess::open("user://profile").expect("a store-only directory opens");
    assert_eq!(
        dir.get_files(),
        vec!["slot1.sav".to_string(), "slot2.sav".to_string()]
    );
    dir.list_dir_begin().expect("list_dir_begin");

    dir.rename("slot2.sav", "slot3.sav")
        .expect("rename through the store");
    assert!(FileAccess::file_exists("user://profile/slot3.sav"));
    assert!(!FileAccess::file_exists("user://profile/slot2.sav"));

    dir.remove("slot1.sav").expect("remove through the store");
    assert!(!FileAccess::file_exists("user://profile/slot1.sav"));
    assert_eq!(dir.get_files(), vec!["slot3.sav".to_string()]);

    dir.change_dir("screens")
        .expect("change_dir in user:// space");
    assert_eq!(dir.get_files(), vec!["thumb.png".to_string()]);

    uninstall();
}

/// The regression guard for this change's blast radius: with no store
/// registered, `user://` still resolves under the configured user root and
/// reads and writes the real filesystem exactly as before.
#[test]
fn without_a_store_user_paths_still_use_the_filesystem() {
    let _guard = lock();
    uninstall();

    let temp =
        std::env::temp_dir().join(format!("patina-user-storage-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&temp);
    std::fs::create_dir_all(&temp).expect("temp user root");
    set_path_roots(None, Some(temp.clone()));

    {
        let mut f = FileAccess::open("user://native.cfg", FileMode::Write)
            .expect("the filesystem branch still handles user://");
        f.store_string("on disk").unwrap();
    }
    assert_eq!(
        std::fs::read_to_string(temp.join("native.cfg")).unwrap(),
        "on disk",
        "with no store registered the bytes must land under user_root()"
    );
    assert!(FileAccess::file_exists("user://native.cfg"));
    assert_eq!(
        FileAccess::read_file_string("user://native.cfg").unwrap(),
        "on disk"
    );
    assert!(DirAccess::dir_exists("user://"));
    assert_eq!(
        DirAccess::get_files_at("user://").unwrap(),
        vec!["native.cfg".to_string()]
    );

    let _ = std::fs::remove_dir_all(&temp);
    uninstall();
}
