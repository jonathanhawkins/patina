//! pat-s3rrv.227 (second half) — the optional configured catalog URL.
//!
//! Godot's AssetLib queries godotengine.org over HTTPS, renders the returned
//! assets, and installs a chosen asset's zip. Patina bakes in no host, so the
//! catalog is opt-in per project through `editor/assetlib/index_url` — the key
//! `api_get_assetlib_list` had already named while returning an empty catalog.
//!
//! ⚠️ These serve the catalog over plaintext `http://` on loopback because
//! `HttpClient::perform` refuses TLS before it connects: no TLS backend is wired
//! yet. That is a real product limitation, not a test shortcut, and one test
//! below pins the error message a user gets if they configure an `https://` URL.

use gdeditor::editor_server::{EditorServerHandle, EditorState};
use gdscene::node::Node;
use gdscene::SceneTree;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

/// The install path sandboxes against the process CWD, so these tests move the
/// CWD to their fixture. The runner gives each test its own process; this lock
/// only matters when one process runs them all.
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

fn http_get(port: u16, path: &str) -> String {
    let mut stream = connect_with_retry(port);
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let req = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).unwrap();
    let mut resp = Vec::new();
    let _ = stream.read_to_end(&mut resp);
    String::from_utf8_lossy(&resp).to_string()
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
        "patina_assetcat_{}_{}_{}",
        tag,
        std::process::id(),
        n
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A one-shot origin server standing in for the asset catalog / download host.
///
/// It answers every request with the same body, which is all these tests need:
/// the point under test is our client and mapper, not content negotiation.
fn serve_bytes(content_type: &str, body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let ct = content_type.to_string();
    thread::spawn(move || {
        // Two connections: one test may both list and download from this host.
        for _ in 0..2 {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            // Read the request line so the client is not left writing into a
            // socket nobody drained.
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(head.as_bytes());
            let _ = sock.write_all(&body);
            let _ = sock.flush();
        }
    });
    format!("http://127.0.0.1:{port}/catalog.json")
}

const PLUGIN_CFG: &str = "[plugin]\nname=\"demo\"\ndescription=\"A demo addon\"\nauthor=\"Patina\"\nversion=\"1.0\"\nscript=\"demo.gd\"\n";

/// A project whose `[editor]` section names the catalog, as the setting is
/// spelled when Godot splits `editor/assetlib/index_url` into section + key.
fn project_godot(index_url: Option<&str>) -> String {
    let mut s = String::from(
        "; Engine configuration file.\nconfig_version=5\n\n[application]\n\nconfig/name=\"CatalogFixture\"\n",
    );
    if let Some(u) = index_url {
        s.push_str(&format!("\n[editor]\n\nassetlib/index_url=\"{u}\"\n"));
    }
    s
}

fn pack(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let scratch = unique_dir("pack");
    let path = scratch.join("asset.zip");
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

fn serve(dir: &std::path::Path) -> u16 {
    let mut tree = SceneTree::new();
    let root = tree.root_id();
    tree.add_child(root, Node::new("Root", "Node2D")).unwrap();

    let mut state = EditorState::new(tree);
    state.project_root = Some(dir.to_string_lossy().to_string());

    let port = free_port();
    let _handle = EditorServerHandle::start(port, state);
    thread::sleep(Duration::from_millis(100));
    port
}

/// **The acceptance for this half.** A configured catalog URL is fetched
/// server-side and mapped into the shape the AssetLib grid already renders.
#[test]
fn editor_assetlib_catalog_url_served() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("catalog");

    // Godot's own envelope and field names, including `cost` for the licence —
    // the mapper has to rename that to `license` or the grid renders a blank.
    let catalog = br#"{"result":[
        {"title":"Dialogue System","author":"someone","category":"Scripts",
         "cost":"MIT","version_string":"2.1","download_url":"http://example.invalid/d.zip"},
        {"title":"Pixel Kit","author":"artist","category":"2D Tools",
         "cost":"CC0","version_string":"1.0","download_url":"http://example.invalid/p.zip"}
    ]}"#
    .to_vec();
    let url = serve_bytes("application/json", catalog);

    std::fs::write(dir.join("project.godot"), project_godot(Some(&url))).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    let resp = http_get(port, "/api/assetlib/list");
    assert!(
        resp.contains("\"configured\":true"),
        "a project that names a catalog must report it as configured:\n{resp}"
    );
    assert!(
        resp.contains("Dialogue System") && resp.contains("Pixel Kit"),
        "both catalog entries must reach the client:\n{resp}"
    );
    // `cost` -> `license` is the rename the grid depends on.
    assert!(
        resp.contains("\"license\":\"MIT\""),
        "upstream's `cost` must be mapped to the grid's `license` field:\n{resp}"
    );
    assert!(
        resp.contains("\"download_url\":\"http://example.invalid/d.zip\""),
        "each asset must carry its download_url, which is what makes the \
         per-asset Download button possible:\n{resp}"
    );

    // The button that consumes it must exist in the served UI.
    assert!(
        gdeditor::editor_ui::EDITOR_HTML.contains("/api/assetlib/download"),
        "the AssetLib Download button must POST to /api/assetlib/download"
    );
}

/// The honest empty state survives. Removing it would be a regression: a
/// fabricated catalog invites a user to install something that does not exist.
#[test]
fn editor_assetlib_catalog_url_served_unconfigured_stays_empty_and_honest() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("unconfigured");
    std::fs::write(dir.join("project.godot"), project_godot(None)).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    let resp = http_get(port, "/api/assetlib/list");
    assert!(
        resp.contains("\"configured\":false"),
        "an unset catalog must report configured:false:\n{resp}"
    );
    assert!(
        resp.contains("\"assets\":[]"),
        "no catalog means no assets — never fabricated ones:\n{resp}"
    );
    assert!(
        resp.contains("editor/assetlib/index_url"),
        "the empty state must name the setting the user has to set:\n{resp}"
    );
}

/// The Download button's endpoint: fetch the asset zip server-side and install
/// it, so a catalog asset lands as a usable plugin.
#[test]
fn editor_assetlib_catalog_url_served_download_installs_the_asset() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("download");
    std::fs::write(dir.join("project.godot"), project_godot(None)).unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    let zip = pack(&[
        ("addons/demo/plugin.cfg", PLUGIN_CFG.as_bytes()),
        ("addons/demo/demo.gd", b"@tool\nextends EditorPlugin\n"),
    ]);
    let url = serve_bytes("application/zip", zip);

    let resp = http_post(
        port,
        "/api/assetlib/download",
        &format!(r#"{{"url":"{url}"}}"#),
    );
    assert!(resp.contains("\"ok\":true"), "download failed:\n{resp}");
    assert_eq!(
        std::fs::read_to_string(dir.join("addons/demo/plugin.cfg")).unwrap(),
        PLUGIN_CFG,
        "the downloaded archive must unpack byte-for-byte"
    );
    assert!(
        resp.contains("\"demo\""),
        "the downloaded addon must be picked up as a plugin:\n{resp}"
    );
}

/// An `https://` catalog fails with an explanation, not a bare socket error.
///
/// `HttpClient::perform` refuses TLS before connecting, so without this the user
/// configures the obvious URL and gets a connection failure that reads like
/// their catalog is down.
#[test]
fn editor_assetlib_catalog_url_served_https_reports_the_tls_gap() {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = unique_dir("https");
    std::fs::write(
        dir.join("project.godot"),
        project_godot(Some("https://godotengine.org/asset-library/api/asset")),
    )
    .unwrap();
    std::env::set_current_dir(&dir).unwrap();
    let port = serve(&dir);

    let resp = http_get(port, "/api/assetlib/list");
    // Configured-but-unreachable must NOT collapse into "unconfigured", or the
    // user goes hunting for a setting they have already set.
    assert!(
        resp.contains("\"configured\":true"),
        "the catalog IS configured; the fetch is what failed:\n{resp}"
    );
    assert!(
        resp.contains("TLS"),
        "the reason must name the missing TLS backend rather than surfacing a \
         bare connection error:\n{resp}"
    );
}
