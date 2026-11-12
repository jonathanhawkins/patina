//! pat-s3rrv.227 — installing an asset `.zip` through the served AssetLib.
//!
//! Godot's `EditorAssetInstaller` unpacks a downloaded archive into the project,
//! lets the user untick individual entries first, rescans the filesystem, and
//! any `addons/<name>/plugin.cfg` that lands becomes a toggleable plugin whose
//! enabled state is written to `project.godot [editor_plugins] enabled`.
//!
//! Patina had `/api/assetlib/list` only: the AssetLib screen's Import button
//! logged "not yet implemented", so an asset could be browsed and never
//! installed. These drive the real HTTP endpoint, so a pass means the screen is
//! wired end to end rather than that a helper exists.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// The install endpoint sandboxes against the process CWD, so these tests move
/// the CWD to their fixture. The runner gives each test its own process; this
/// lock only matters when one process runs them all.
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn connect_with_retry(port: u16) -> TcpStream {
    for attempt in 0..20 {
        match TcpStream::connect(format!("127.0.0.1:{port}")) {
            Ok(s) => return s,
            Err(_) if attempt < 19 => thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("failed to connect: {e}"),
        }
    }
    unreachable!()
}

fn http_post(port: u16, path: &str, body: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
}

/// Process-unique fixture dir — a fixed `/tmp` path collides across the
/// concurrent test processes in the verifier lane.
fn unique_dir(tag: &str) -> std::path::PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "patina_assetlib_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Standard base64, written here rather than imported so the test encodes the
/// upload independently of the decoder it exercises. Sharing one helper would
/// make a matched pair of bugs invisible.
fn b64(data: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            A[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            A[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

const PLUGIN_CFG: &str = "[plugin]\nname=\"demo\"\ndescription=\"A demo addon\"\nauthor=\"Patina\"\nversion=\"1.0\"\nscript=\"demo.gd\"\n";
const PLUGIN_GD: &str = "@tool\nextends EditorPlugin\n\nfunc _enter_tree():\n\tpass\n";

/// A minimal project so `update_project_file` has something to rewrite.
const PROJECT_GODOT: &str =
    "; Engine configuration file.\nconfig_version=5\n\n[application]\n\nconfig/name=\"AssetLibFixture\"\n";

/// Builds an archive through `ZipPacker` and returns its bytes.
///
/// The archive is written outside `dir`, so the install assertions never see the
/// upload itself sitting in the project.
fn pack(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let scratch = unique_dir("pack");
    let path = scratch.join("upload.zip");
    let mut packer = gdresource::ZipPacker::new();
    packer
        .open(&path.to_string_lossy(), gdresource::ZipAppend::Create)
        .unwrap();
    for (name, data) in entries {
        packer.start_file(name).unwrap();
        packer.write_file(data).unwrap();
        packer.close_file().unwrap();
    }
    packer.close().unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_dir_all(&scratch);
    bytes
}

/// Starts a server whose CWD (and therefore sandbox root) is `dir`.
fn serve(dir: &std::path::Path) -> u16 {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Root", "Node2D")).unwrap();

    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());

    let port = free_port();
    // `EditorServerHandle` has no `Drop`, so the listener thread outlives this
    // helper and serves for the rest of the test.
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    port
}

/// **The acceptance.** Upload a zip holding `addons/demo/plugin.cfg`; it unpacks
/// under the project, the plugin list picks `demo` up, and toggling it writes
/// the choice to `project.godot`.
#[test]
fn editor_assetlib_zip_install_served() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("accept");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    let zip = pack(&[
        ("addons/demo/plugin.cfg", PLUGIN_CFG.as_bytes()),
        ("addons/demo/demo.gd", PLUGIN_GD.as_bytes()),
        ("addons/demo/README.md", b"unticked by the user"),
    ]);

    // `skip` is the installer dialog's per-entry opt-out: the user unticked the
    // README before pressing Install.
    let body = format!(
        r#"{{"zip_base64":"{}","skip":["addons/demo/README.md"]}}"#,
        b64(&zip)
    );
    let resp = http_post(port, "/api/assetlib/install", &body);
    assert!(resp.contains("\"ok\":true"), "install failed:\n{resp}");

    // The ticked entries landed under the project...
    let cfg = dir.join("addons/demo/plugin.cfg");
    assert!(cfg.is_file(), "plugin.cfg must be unpacked at {cfg:?}");
    assert_eq!(
        std::fs::read_to_string(&cfg).unwrap(),
        PLUGIN_CFG,
        "the unpacked file must be byte-identical to the archived one"
    );
    assert!(
        dir.join("addons/demo/demo.gd").is_file(),
        "the plugin script must be unpacked too"
    );

    // ...and the unticked one did not. Guarding both directions matters: an
    // install that ignored `skip` would still pass every assertion above.
    assert!(
        !dir.join("addons/demo/README.md").exists(),
        "a skipped entry must NOT be written"
    );

    // The FileSystem rescan ran, so the new addon is already a known plugin —
    // without it `demo` stays invisible until the server restarts.
    assert!(
        resp.contains("\"demo\""),
        "the response must report `demo` as a discovered plugin, which is what \
         makes it toggleable without a reload:\n{resp}"
    );

    // Toggling it on persists to `project.godot`, the acceptance's last clause.
    let toggled = http_post(
        port,
        "/api/plugins/toggle",
        r#"{"name":"demo","enabled":true}"#,
    );
    assert!(
        toggled.contains("\"ok\":true"),
        "the freshly installed plugin must be toggleable:\n{toggled}"
    );
    assert!(
        toggled.contains("\"persisted\":true"),
        "the choice must be written to project.godot, not just held in memory:\n{toggled}"
    );
    let project = std::fs::read_to_string(dir.join("project.godot")).unwrap();
    assert!(
        project.contains("addons/demo/plugin.cfg"),
        "project.godot must record the enabled plugin by cfg path:\n{project}"
    );

    // The screen that triggers all of this must actually call the endpoint —
    // otherwise the server route is reachable only by a test.
    assert!(
        gdeditor::editor_ui::EDITOR_HTML.contains("/api/assetlib/install"),
        "the AssetLib Import button must POST to /api/assetlib/install"
    );
}

/// A zip our own `ZipPacker` did not write. Uploaded assets come from other
/// zippers, and a packer/reader round-trip would agree with itself even if both
/// sides shared a bug — so these bytes are laid out by hand from the spec.
#[test]
fn editor_assetlib_zip_install_served_accepts_a_foreign_stored_zip() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("foreign");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    let name = b"addons/hand/plugin.cfg";
    let data = PLUGIN_CFG.as_bytes();
    let crc = gdresource::crc32(data);
    let mut zip = Vec::new();

    // Local file header: STORED (method 0), sizes known up front.
    zip.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
    zip.extend_from_slice(&20u16.to_le_bytes()); // version needed
    zip.extend_from_slice(&0u16.to_le_bytes()); // flags
    zip.extend_from_slice(&0u16.to_le_bytes()); // method: STORE
    zip.extend_from_slice(&0u16.to_le_bytes()); // mod time
    zip.extend_from_slice(&0u16.to_le_bytes()); // mod date
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&(data.len() as u32).to_le_bytes()); // compressed
    zip.extend_from_slice(&(data.len() as u32).to_le_bytes()); // uncompressed
    zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // extra len
    zip.extend_from_slice(name);
    zip.extend_from_slice(data);

    let cd_offset = zip.len() as u32;
    zip.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
    zip.extend_from_slice(&20u16.to_le_bytes()); // version made by
    zip.extend_from_slice(&20u16.to_le_bytes()); // version needed
    zip.extend_from_slice(&0u16.to_le_bytes()); // flags
    zip.extend_from_slice(&0u16.to_le_bytes()); // method: STORE
    zip.extend_from_slice(&0u16.to_le_bytes()); // mod time
    zip.extend_from_slice(&0u16.to_le_bytes()); // mod date
    zip.extend_from_slice(&crc.to_le_bytes());
    zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
    zip.extend_from_slice(&(data.len() as u32).to_le_bytes());
    zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // extra len
    zip.extend_from_slice(&0u16.to_le_bytes()); // comment len
    zip.extend_from_slice(&0u16.to_le_bytes()); // disk number
    zip.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
    zip.extend_from_slice(&0u32.to_le_bytes()); // external attrs
    zip.extend_from_slice(&0u32.to_le_bytes()); // local header offset
    zip.extend_from_slice(name);
    let cd_size = zip.len() as u32 - cd_offset;

    zip.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    zip.extend_from_slice(&0u16.to_le_bytes()); // this disk
    zip.extend_from_slice(&0u16.to_le_bytes()); // cd start disk
    zip.extend_from_slice(&1u16.to_le_bytes()); // entries on this disk
    zip.extend_from_slice(&1u16.to_le_bytes()); // entries total
    zip.extend_from_slice(&cd_size.to_le_bytes());
    zip.extend_from_slice(&cd_offset.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // comment len

    let body = format!(r#"{{"zip_base64":"{}"}}"#, b64(&zip));
    let resp = http_post(port, "/api/assetlib/install", &body);
    assert!(resp.contains("\"ok\":true"), "install failed:\n{resp}");
    assert_eq!(
        std::fs::read_to_string(dir.join("addons/hand/plugin.cfg")).unwrap(),
        PLUGIN_CFG,
        "a STORED entry from a foreign zipper must unpack byte-for-byte"
    );
}

/// A hostile archive cannot write outside the project. The file list comes from
/// an untrusted download, so this is the endpoint's actual threat model.
#[test]
fn editor_assetlib_zip_install_served_rejects_path_traversal() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("evil");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    let zip = pack(&[("../escaped.txt", b"pwned")]);
    let body = format!(r#"{{"zip_base64":"{}"}}"#, b64(&zip));
    let resp = http_post(port, "/api/assetlib/install", &body);

    assert!(
        !resp.contains("\"ok\":true"),
        "a traversal entry must be refused, not installed:\n{resp}"
    );
    // The parent is the fixture's own parent (`/tmp`), so a leak is observable.
    let escaped = dir.parent().unwrap().join("escaped.txt");
    assert!(
        !escaped.exists(),
        "nothing may be written outside the project root, found {escaped:?}"
    );
}

/// Garbage in the upload field is a 4xx, not a panic or a half-written project.
#[test]
fn editor_assetlib_zip_install_served_rejects_a_non_zip_payload() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("garbage");
    std::fs::write(dir.join("project.godot"), PROJECT_GODOT).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    let resp = http_post(
        port,
        "/api/assetlib/install",
        &format!(r#"{{"zip_base64":"{}"}}"#, b64(b"this is not a zip file")),
    );
    assert!(
        !resp.contains("\"ok\":true"),
        "a non-zip upload must be refused:\n{resp}"
    );
    assert!(
        !dir.join("addons").exists(),
        "a refused upload must leave the project untouched"
    );
}
