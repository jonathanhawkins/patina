//! A server must claim its port BEFORE `start` returns.
//!
//! `frame_server::start` used to bind on its spawned thread, so `start`
//! returned with the port still closed and every caller guessed a sleep. The
//! 150 ms guess in `space_shooter_live_test` lost at load average 30 and the
//! full workspace run went red with `ConnectionRefused`. The editor server had
//! already moved its bind out of the thread for a related race.
//!
//! This lint keeps the pattern from coming back: no `TcpListener::bind` /
//! `UdpSocket::bind` may sit inside a `thread::spawn(...)` call in engine
//! source. Bind first, then move the listener into the thread.

use std::path::{Path, PathBuf};

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            // Tests may bind wherever they like; build output is not source.
            if name == "tests" || name == "target" || name.starts_with("target-") {
                continue;
            }
            rust_sources(&p, out);
        } else if name.ends_with(".rs") {
            out.push(p);
        }
    }
}

/// The text of each `thread::spawn(` call, up to its matching `)`.
fn spawn_bodies(src: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = src[from..].find("thread::spawn(") {
        let start = from + i + "thread::spawn".len();
        let mut depth = 0i32;
        let mut end = src.len();
        for (j, ch) in src[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + j;
                        break;
                    }
                }
                _ => {}
            }
        }
        let line = src[..start].matches('\n').count() + 1;
        out.push((line, &src[start..end]));
        from = start;
    }
    out
}

fn binds(text: &str) -> bool {
    text.contains("TcpListener::bind") || text.contains("UdpSocket::bind")
}

/// The body of `fn name(` in `src`, up to its matching `}`.
fn fn_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let at = src.find(&format!("fn {name}("))?;
    let open = at + src[at..].find('{')?;
    let mut depth = 0i32;
    for (j, ch) in src[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[open..open + j]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Identifiers called in `body` (`name(`), for one level of same-file calls:
/// the original bug bound inside `run_server`, which the closure called.
fn called_fns(body: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = body.as_bytes();
    for (i, _) in body.match_indices('(') {
        let mut s = i;
        while s > 0 && (b[s - 1].is_ascii_alphanumeric() || b[s - 1] == b'_') {
            s -= 1;
        }
        if s < i {
            out.push(&body[s..i]);
        }
    }
    out
}

fn binds_inside_spawn(src: &str) -> Vec<usize> {
    spawn_bodies(src)
        .into_iter()
        .filter(|(_, body)| {
            binds(body)
                || called_fns(body)
                    .into_iter()
                    .any(|f| fn_body(src, f).is_some_and(binds))
        })
        .map(|(line, _)| line)
        .collect()
}

#[test]
fn the_lint_flags_a_bind_inside_a_spawned_thread_and_passes_a_bind_before_it() {
    let bad = "fn start(p: u16) { let t = std::thread::spawn(move || {\n let l = TcpListener::bind((\"0.0.0.0\", p)).unwrap(); serve(l) }); }";
    assert_eq!(binds_inside_spawn(bad), vec![1]);
    let good = "fn start(p: u16) { let l = TcpListener::bind((\"0.0.0.0\", p)).unwrap();\n let t = std::thread::spawn(move || serve(l)); }";
    assert!(binds_inside_spawn(good).is_empty());
    // The shape of the real bug: the closure calls a function that binds.
    let indirect = "fn start(p: u16) {\n let t = thread::spawn(move || {\n run_server(s, p);\n }); }\n\
                    fn run_server(s: S, p: u16) { let l = match TcpListener::bind(p) { Ok(l) => l, Err(_) => return }; }";
    assert_eq!(binds_inside_spawn(indirect), vec![2]);
}

#[test]
fn no_engine_server_binds_its_port_inside_a_spawned_thread() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_sources(&root.join("crates"), &mut files);
    rust_sources(&root.join("examples"), &mut files);
    rust_sources(&root.join("src"), &mut files);
    assert!(
        files.len() > 100,
        "found only {} sources under {}",
        files.len(),
        root.display()
    );
    let mut hits = Vec::new();
    for f in &files {
        let Ok(src) = std::fs::read_to_string(f) else {
            continue;
        };
        for line in binds_inside_spawn(&src) {
            hits.push(format!(
                "{}:{line}",
                f.strip_prefix(root).unwrap_or(f).display()
            ));
        }
    }
    assert!(
        hits.is_empty(),
        "these bind a port inside thread::spawn, so `start` returns before the port \
         listens — bind first and move the listener into the thread:\n{}",
        hits.join("\n")
    );
}
