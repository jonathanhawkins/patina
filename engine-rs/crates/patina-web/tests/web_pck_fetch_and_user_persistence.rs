//! The web I/O backends, driven end to end through the browser-facing seams
//! (pat-nzgdu.149).
//!
//! The three halves of the web runtime's I/O story — `res://` out of a fetched
//! `.pck`, `user://` persisted across a reload, and the `AudioWorkletProcessor`
//! that is the audio sink — each have their own suite already
//! (`web_vfs_res_and_user_paths`, `web_audio_worklet`,
//! `audio_web_backend_contract`). **Nothing asserted that they agree**, and
//! nothing drove them through the entry points the page actually calls. That is
//! what this file is for, and it is deliberately narrow about it:
//!
//! 1. the pack is mounted through [`patina_web::vfs::mount_pack_bytes`] — the
//!    exact call `WebVfs::mountPack` makes with the bytes `fetch(url)` →
//!    `arrayBuffer()` produced — and an asset is then *parsed out of it*, not
//!    just byte-compared, so the pack is proven usable as a resource source;
//! 2. the `user://` queue is drained with [`WriteQueue::take_next`] in a loop,
//!    which is what `WebVfs::takePendingWrite` serves and what the
//!    `drainPendingWrites` half of `USER_STORE_GLUE` calls. The sibling suite
//!    drains with `drain()`; a `take_next` that reordered or dropped an entry
//!    would pass there and fail here;
//! 3. the worklet source the engine hands JS is asserted to *be* `gdaudio`'s,
//!    parameterised by the engine's own processor name and channel count —
//!    parsed out of the emitted JS and compared for equality, not `contains`,
//!    and cross-checked against the stride `pull_block` actually interleaves
//!    at.
//!
//! Like its siblings this runs natively with **no path roots configured**
//! (`set_path_roots(None, None)`), which is precisely the browser's situation:
//! no filesystem under `res://` or `user://`. The browser-only halves
//! ([`patina_web::web_vfs`], [`patina_web::web_audio`]) are `cfg`-gated to
//! `wasm32` and are covered by the target check this bead also gates on.
//!
//! What this does **not** prove: that `fetch`, IndexedDB and `AudioWorklet`
//! behave as assumed. The reload below replays the mutation queue through a
//! fresh store, which is the sequence a page reload performs against
//! IndexedDB, and clause 3 pins the JS contract as text.
//!
//! ⚠️ Known divergence this file does **not** assert, recorded so the next
//! reader does not mistake it for covered: `gdplatform::export::
//! web_audio_worklet_glue` — the worklet written beside an *exported static
//! site* — registers `patina-audio`, while the live runtime here registers
//! `patina-audio-out`. They are separate delivery paths with separate hosts, so
//! making them equal is a judgement call and a change to `gdplatform`'s own
//! green suite; it is tracked separately and is out of this bead's scope.

use gdcore::file_access::{
    clear_mounted_packs, set_path_roots, set_user_storage, DirAccess, FileAccess, FileMode,
};
use gdresource::pck::PckPacker;
use gdresource::ConfigFile;
use patina_web::audio::{WebAudioEngine, PROCESSOR_NAME, RENDER_QUANTUM_FRAMES};
use patina_web::vfs::{
    mount_pack_bytes, queued_user_store, UserStoreWrite, WebUserStore, WriteQueue,
};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// The engine's mix rate — Godot's `audio/driver/mix_rate` default.
const MIX_RATE: u32 = 44_100;
/// A rate a browser might pick, deliberately different from [`MIX_RATE`].
const CONTEXT_RATE: u32 = 48_000;

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

// ---------------------------------------------------------------------------
// The acceptance, in one test
// ---------------------------------------------------------------------------

/// The bead's named acceptance: a fetched pack serves `res://`, a `user://`
/// write survives a reload when drained the way the host drains it, and the
/// worklet the engine hands JS is the sink both halves agree on.
///
/// The focused tests below cover the same three clauses one at a time so a
/// failure localises; this one exists because the clauses have to hold
/// *together*, in one process, in the order a page performs them.
#[test]
fn web_pck_fetch_and_user_persistence() {
    let _guard = lock();
    reset();

    // --- boot: the page fetched the pack and mounted the bytes ---
    mount_pack_bytes("game", packed_fixture(), 0).expect("the fetched bytes are a valid pck");
    let settings = load_packed_config("res://default_settings.cfg");
    assert!(
        settings.has_section_key("audio", "mix_rate"),
        "the packed settings resource must parse out of the fetched pack, got sections {:?}",
        settings.get_sections()
    );

    // --- first session: write user:// and let the host persist it ---
    let (store, queue) = queued_user_store();
    WebUserStore::install(Arc::clone(&store));
    write_user_file("user://save.cfg", "level=7");
    write_user_file("user://profile/name.txt", "ada");

    let persisted = drain_the_way_the_host_does(&queue);
    let persisted_paths: Vec<&str> = persisted.keys().map(String::as_str).collect();
    assert_eq!(
        persisted_paths,
        vec!["user://profile/name.txt", "user://save.cfg"],
        "both writes must reach the host's object store"
    );

    // --- reload: the engine-side store is gone, IndexedDB is not ---
    drop(store);
    set_user_storage(None);
    assert!(
        FileAccess::read_file_string("user://save.cfg").is_err(),
        "with no store installed the data must be unreachable — otherwise this \
         test would pass on leftover state instead of on rehydration"
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
        "a reload must not re-queue what it just loaded, got {:?}",
        fresh_queue.drain()
    );
    // The pack is still mounted across all of that: `user://` and `res://` are
    // independent backends, and installing one must not disturb the other.
    assert!(
        FileAccess::file_exists("res://default_settings.cfg"),
        "mounting a user:// store must not unmount res://"
    );

    // --- the audio sink the same page stands up ---
    assert_worklet_halves_agree();

    reset();
}

// ---------------------------------------------------------------------------
// Clause 1 — res:// out of the bytes `fetch` produced
// ---------------------------------------------------------------------------

/// A `res://` asset is *loaded* out of the fetched pack, not merely readable
/// from it: the bytes go through a resource parser, which is what a scene load
/// would do.
#[test]
fn a_packed_asset_parses_out_of_the_fetched_bytes() {
    let _guard = lock();
    reset();

    mount_pack_bytes("game", packed_fixture(), 0).expect("mount the fetched pack");

    let settings = load_packed_config("res://default_settings.cfg");
    assert_eq!(
        settings.get_sections(),
        vec!["audio"],
        "the parsed resource must carry the packed section"
    );
    assert!(
        settings.has_section_key("audio", "mix_rate"),
        "the packed key must survive the pack -> read -> parse round trip"
    );
    // A second, binary asset: a text-only path would hide a reader that
    // mangles non-UTF-8 bytes.
    assert_eq!(
        FileAccess::read_file_bytes("res://art/icon.png").expect("read the packed texture"),
        PNG_BYTES.to_vec(),
        "binary asset bytes must come back out of the pack unchanged"
    );
    assert!(
        !FileAccess::file_exists("res://absent.tscn"),
        "a path in no pack must not report as existing"
    );

    reset();
}

// ---------------------------------------------------------------------------
// Clause 2 — the host's own drain order
// ---------------------------------------------------------------------------

/// `take_next` — the call `WebVfs::takePendingWrite` serves — must hand the
/// host the engine's write order, oldest first.
///
/// ⚠️ This is the clause the sibling suite cannot see: it drains with
/// `drain()`, which collects the whole queue at once, so a `take_next` that
/// popped from the wrong end would stay green there. Replayed newest-first the
/// scratch file below is deleted before it is written, and survives the reload.
#[test]
fn the_host_drain_replays_mutations_in_the_engines_order() {
    let _guard = lock();
    reset();

    let (store, queue) = queued_user_store();
    WebUserStore::install(Arc::clone(&store));

    write_user_file("user://save.cfg", "first");
    write_user_file("user://save.cfg", "second");
    write_user_file("user://scratch.tmp", "junk");
    remove_user_file("scratch.tmp");

    let mut taken = Vec::new();
    while let Some(write) = queue.take_next() {
        taken.push(write);
    }
    assert_eq!(
        taken,
        vec![
            UserStoreWrite::Put {
                path: "user://save.cfg".to_string(),
                bytes: b"first".to_vec(),
            },
            UserStoreWrite::Put {
                path: "user://save.cfg".to_string(),
                bytes: b"second".to_vec(),
            },
            UserStoreWrite::Put {
                path: "user://scratch.tmp".to_string(),
                bytes: b"junk".to_vec(),
            },
            UserStoreWrite::Remove {
                path: "user://scratch.tmp".to_string(),
            },
        ],
        "take_next must serve the host oldest-first, so a replay reproduces the \
         engine's history"
    );

    reset();
}

/// Replaying what the host persisted into a fresh store is the reload, and the
/// deleted file must not come back.
#[test]
fn a_deleted_user_file_does_not_return_after_a_reload() {
    let _guard = lock();
    reset();

    let (store, queue) = queued_user_store();
    WebUserStore::install(Arc::clone(&store));
    write_user_file("user://save.cfg", "level=7");
    write_user_file("user://scratch.tmp", "junk");
    remove_user_file("scratch.tmp");

    let persisted = drain_the_way_the_host_does(&queue);
    drop(store);
    set_user_storage(None);

    let (fresh, _fresh_queue) = queued_user_store();
    for (path, bytes) in &persisted {
        fresh.hydrate(path, bytes.clone());
    }
    WebUserStore::install(Arc::clone(&fresh));

    assert_eq!(
        FileAccess::read_file_string("user://save.cfg").expect("the save survives"),
        "level=7"
    );
    assert!(
        !FileAccess::file_exists("user://scratch.tmp"),
        "a file deleted before the reload must not come back"
    );

    reset();
}

// ---------------------------------------------------------------------------
// Clause 3 — the two worklet halves agree
// ---------------------------------------------------------------------------

/// The worklet source `patina-web` hands the page must *be* `gdaudio`'s,
/// parameterised by this engine's own processor name and channel count.
///
/// ⚠️ Written as an equality against `gdaudio::audio_worklet_glue` rather than
/// a bag of `contains` checks because the failure this guards is a **fork**:
/// the day `patina-web` grows its own copy of the JS, the two halves start
/// drifting silently and both existing suites stay green — each asserts its own
/// copy's expectations. That is not hypothetical; it is exactly how
/// `gdplatform`'s exported-site glue came to register a different processor
/// name (see the module header).
#[test]
fn the_engines_worklet_glue_is_gdaudios_with_the_engines_own_values() {
    assert_worklet_halves_agree();
}

/// The glue's constants really are parameterised, which is what makes the
/// equality above meaningful — and what makes the parsing helpers below
/// trustworthy.
///
/// ⚠️ Without this, a template that hardcoded the name and ignored its
/// argument would satisfy every other assertion in this file.
#[test]
fn the_worklet_glue_constants_follow_their_arguments() {
    let glue = gdaudio::audio_worklet_glue("probe-processor", 7);
    assert_eq!(
        glue_string_const(&glue, "PATINA_PROCESSOR"),
        "probe-processor",
        "the processor name must come from the argument, not a literal"
    );
    assert_eq!(
        glue_u16_const(&glue, "PATINA_CHANNELS"),
        7,
        "the channel count must come from the argument, not a literal"
    );
    assert!(
        !glue.contains(PROCESSOR_NAME),
        "a glue built for `probe-processor` must not mention `{PROCESSOR_NAME}` \
         anywhere — if it does, the runtime name is baked in somewhere the \
         argument does not reach"
    );
}

/// Both clause-3 call sites, so the end-to-end test and the focused test assert
/// exactly the same thing.
fn assert_worklet_halves_agree() {
    let mut engine = WebAudioEngine::new(CONTEXT_RATE, MIX_RATE);
    let glue = engine.worklet_glue();

    assert_eq!(
        glue,
        gdaudio::audio_worklet_glue(PROCESSOR_NAME, engine.channels()),
        "patina-web must hand the page gdaudio's worklet source parameterised by \
         its own values, not a second copy of the JS"
    );

    // Parsed equality, not `contains`: `contains(PROCESSOR_NAME)` is satisfied
    // by `patina-audio-out-v2`, which is a different processor and throws
    // `InvalidStateError` on `new AudioWorkletNode`.
    assert_eq!(
        glue_string_const(&glue, "PATINA_PROCESSOR"),
        PROCESSOR_NAME,
        "the name the glue registers must be exactly the one patina-web exports \
         to JS as the node to instantiate"
    );
    assert_eq!(
        glue_u16_const(&glue, "PATINA_CHANNELS"),
        engine.channels(),
        "the node's channel count must be the engine's"
    );

    // The constants have to be what the two JS calls actually use; declaring
    // them and then registering a literal would pass the parses above.
    assert!(
        glue.contains(r#"registerProcessor("${PATINA_PROCESSOR}""#),
        "the worklet must register under the declared constant"
    );
    assert!(
        glue.contains("new AudioWorkletNode(context, PATINA_PROCESSOR,"),
        "the node must be instantiated under the same declared constant"
    );
    assert!(
        glue.contains("outputChannelCount: [PATINA_CHANNELS]"),
        "the node must be built with the declared channel count"
    );

    // ⚠️ Declared vs. observed: the channel count in the JS is the stride the
    // ring is interleaved at. A mismatch does not throw — it de-interleaves
    // against the wrong stride and the stereo image comes apart — so the
    // declaration is checked against what `pull_block` really produces.
    let declared = usize::from(glue_u16_const(&glue, "PATINA_CHANNELS"));
    let block = engine.pull_block(RENDER_QUANTUM_FRAMES);
    assert_eq!(
        block.len(),
        RENDER_QUANTUM_FRAMES * declared,
        "the glue declares {declared} channels, but a render quantum carries {} \
         samples — the worklet would de-interleave against the wrong stride",
        block.len()
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Stand-in for a packed texture. Not a real PNG — the point is that the bytes
/// are not valid UTF-8, so a reader that round-trips through `String` fails.
const PNG_BYTES: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0xFF, 0x00, 0xFE,
];

/// A `.pck` holding a parseable settings resource and a binary asset, as
/// `fetch().arrayBuffer()` would deliver it.
fn packed_fixture() -> Vec<u8> {
    let mut packer = PckPacker::new();
    packer.add_file(
        "res://default_settings.cfg",
        b"[audio]\nmix_rate=44100\n".to_vec(),
    );
    packer.add_file("res://art/icon.png", PNG_BYTES.to_vec());
    packer.pack().expect("pack the fixture")
}

/// Reads `path` out of the mounted pack and parses it as a `ConfigFile`, the
/// way a resource load does — bytes out of the archive, through a parser, into
/// a resource.
fn load_packed_config(path: &str) -> ConfigFile {
    let text = FileAccess::read_file_string(path)
        .unwrap_or_else(|err| panic!("{path} must be readable out of the mounted pack: {err:?}"));
    ConfigFile::load_from_string(&text)
        .unwrap_or_else(|err| panic!("{path} must parse as a ConfigFile: {err}"))
}

/// Writes `text` to `path` the way a script does, and closes the handle.
fn write_user_file(path: &str, text: &str) {
    let mut file = FileAccess::open(path, FileMode::Write).expect("write open creates the file");
    file.store_string(text).expect("store_string");
    // Dropped here: close() is what flushes the buffer into the store.
}

/// Deletes `name` from `user://`, which is a `DirAccess` operation — the store
/// underneath is a flat key-value map with no file-level unlink of its own.
fn remove_user_file(name: &str) {
    DirAccess::open("user://")
        .expect("user:// opens through the store")
        .remove(name)
        .unwrap_or_else(|err| panic!("remove user://{name}: {err:?}"));
}

/// Applies everything queued to a map the way the host's IndexedDB would,
/// draining with [`WriteQueue::take_next`] — the call
/// `drainPendingWrites(db, vfs)` makes through `vfs.takePendingWrite()`.
fn drain_the_way_the_host_does(queue: &Arc<WriteQueue>) -> BTreeMap<String, Vec<u8>> {
    let mut persisted: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    while let Some(write) = queue.take_next() {
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

/// The value of `const <name> = "…";` in an emitted glue module.
fn glue_string_const(glue: &str, name: &str) -> String {
    let needle = format!("const {name} = \"");
    let start = glue
        .find(&needle)
        .unwrap_or_else(|| panic!("the glue declares no string `const {name}`"))
        + needle.len();
    let rest = &glue[start..];
    let end = rest
        .find('"')
        .unwrap_or_else(|| panic!("`const {name}` is unterminated"));
    rest[..end].to_string()
}

/// The value of `const <name> = <number>;` in an emitted glue module.
fn glue_u16_const(glue: &str, name: &str) -> u16 {
    let needle = format!("const {name} = ");
    let start = glue
        .find(&needle)
        .unwrap_or_else(|| panic!("the glue declares no `const {name}`"))
        + needle.len();
    let rest = &glue[start..];
    let end = rest
        .find(';')
        .unwrap_or_else(|| panic!("`const {name}` is unterminated"));
    rest[..end]
        .trim()
        .parse()
        .unwrap_or_else(|err| panic!("`const {name}` is not a u16: {err}"))
}
