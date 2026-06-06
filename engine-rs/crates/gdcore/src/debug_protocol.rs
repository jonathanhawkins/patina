//! Remote debugging protocol over TCP.
//!
//! Defines message types for editor ↔ running-game communication and provides
//! a line-based JSON wire format using only `std::net` (no async, no serde).
//!
//! ## Wire format
//!
//! Each message is a single line of JSON terminated by `\n`. The connection
//! uses a simple request/response pattern:
//!
//! - **Editor → Game**: [`DebugCommand`] (e.g. set breakpoint, step, inspect)
//! - **Game → Editor**: [`DebugEvent`] (e.g. paused, stack info, variable values)
//!
//! ## Usage
//!
//! ```text
//! // Game side:
//! let server = DebugServer::bind("127.0.0.1:6007")?;
//! let mut session = server.accept()?;       // blocks until editor connects
//! // ... game loop ...
//! if let Some(cmd) = session.try_recv()? {  // non-blocking poll
//!     handle_command(cmd, &mut debugger);
//! }
//! session.send_event(DebugEvent::Paused { ... })?;
//!
//! // Editor side:
//! let mut client = DebugClient::connect("127.0.0.1:6007")?;
//! client.send_command(DebugCommand::SetBreakpoint { ... })?;
//! let event = client.recv_event()?;         // blocking read
//! ```

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, BufWriter, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

// ---------------------------------------------------------------------------
// DebugCommand (Editor → Game)
// ---------------------------------------------------------------------------

/// A command sent from the editor to the running game.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum DebugCommand {
    /// Set a breakpoint at script:line.
    SetBreakpoint { script: String, line: usize },
    /// Remove a breakpoint at script:line.
    RemoveBreakpoint { script: String, line: usize },
    /// Clear all breakpoints.
    ClearBreakpoints,
    /// Continue execution.
    Continue,
    /// Step into next statement.
    StepIn,
    /// Step over next statement.
    StepOver,
    /// Step out of current function.
    StepOut,
    /// Pause execution.
    Pause,
    /// Request the current call stack.
    GetCallStack,
    /// Request locals for a given stack frame (0 = top).
    GetLocals { frame: usize },
    /// Request globals.
    GetGlobals,
    /// Evaluate an expression in the current frame.
    Evaluate { expression: String },
    /// Disconnect the debugger.
    Disconnect,
}

// ---------------------------------------------------------------------------
// DebugEvent (Game → Editor)
// ---------------------------------------------------------------------------

/// An event sent from the running game to the editor.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum DebugEvent {
    /// The game has paused (breakpoint hit or step completed).
    Paused {
        script: String,
        line: usize,
        reason: String,
    },
    /// The game has resumed running.
    Resumed,
    /// Call stack response.
    CallStack { frames: Vec<StackFrameInfo> },
    /// Local variables response.
    Locals {
        frame: usize,
        variables: Vec<(String, String)>,
    },
    /// Global variables response.
    Globals { variables: Vec<(String, String)> },
    /// Expression evaluation result.
    EvalResult {
        expression: String,
        value: String,
        error: Option<String>,
    },
    /// Breakpoint confirmed set.
    BreakpointSet { script: String, line: usize },
    /// Breakpoint confirmed removed.
    BreakpointRemoved { script: String, line: usize },
    /// An error occurred processing a command.
    Error { message: String },
    /// Game is exiting.
    Exiting,
}

/// Lightweight stack frame info for the protocol.
#[derive(Debug, Clone, PartialEq)]
pub struct StackFrameInfo {
    /// Script resource path.
    pub script: String,
    /// Function name.
    pub function: String,
    /// Line number.
    pub line: usize,
}

// ---------------------------------------------------------------------------
// JSON serialization (manual, no serde)
// ---------------------------------------------------------------------------

fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl DebugCommand {
    /// Serializes to a JSON string (single line, no trailing newline).
    pub fn to_json(&self) -> String {
        match self {
            DebugCommand::SetBreakpoint { script, line } => {
                format!(
                    r#"{{"type":"set_breakpoint","script":{},"line":{}}}"#,
                    escape_json_string(script),
                    line
                )
            }
            DebugCommand::RemoveBreakpoint { script, line } => {
                format!(
                    r#"{{"type":"remove_breakpoint","script":{},"line":{}}}"#,
                    escape_json_string(script),
                    line
                )
            }
            DebugCommand::ClearBreakpoints => r#"{"type":"clear_breakpoints"}"#.to_string(),
            DebugCommand::Continue => r#"{"type":"continue"}"#.to_string(),
            DebugCommand::StepIn => r#"{"type":"step_in"}"#.to_string(),
            DebugCommand::StepOver => r#"{"type":"step_over"}"#.to_string(),
            DebugCommand::StepOut => r#"{"type":"step_out"}"#.to_string(),
            DebugCommand::Pause => r#"{"type":"pause"}"#.to_string(),
            DebugCommand::GetCallStack => r#"{"type":"get_call_stack"}"#.to_string(),
            DebugCommand::GetLocals { frame } => {
                format!(r#"{{"type":"get_locals","frame":{}}}"#, frame)
            }
            DebugCommand::GetGlobals => r#"{"type":"get_globals"}"#.to_string(),
            DebugCommand::Evaluate { expression } => {
                format!(
                    r#"{{"type":"evaluate","expression":{}}}"#,
                    escape_json_string(expression)
                )
            }
            DebugCommand::Disconnect => r#"{"type":"disconnect"}"#.to_string(),
        }
    }

    /// Parses from a JSON string. Returns `None` if parsing fails.
    pub fn from_json(s: &str) -> Option<Self> {
        let map = parse_json_object(s)?;
        let ty = map.get("type")?.as_str()?;
        match ty {
            "set_breakpoint" => Some(DebugCommand::SetBreakpoint {
                script: map.get("script")?.as_str()?.to_owned(),
                line: map.get("line")?.as_usize()?,
            }),
            "remove_breakpoint" => Some(DebugCommand::RemoveBreakpoint {
                script: map.get("script")?.as_str()?.to_owned(),
                line: map.get("line")?.as_usize()?,
            }),
            "clear_breakpoints" => Some(DebugCommand::ClearBreakpoints),
            "continue" => Some(DebugCommand::Continue),
            "step_in" => Some(DebugCommand::StepIn),
            "step_over" => Some(DebugCommand::StepOver),
            "step_out" => Some(DebugCommand::StepOut),
            "pause" => Some(DebugCommand::Pause),
            "get_call_stack" => Some(DebugCommand::GetCallStack),
            "get_locals" => Some(DebugCommand::GetLocals {
                frame: map.get("frame")?.as_usize()?,
            }),
            "get_globals" => Some(DebugCommand::GetGlobals),
            "evaluate" => Some(DebugCommand::Evaluate {
                expression: map.get("expression")?.as_str()?.to_owned(),
            }),
            "disconnect" => Some(DebugCommand::Disconnect),
            _ => None,
        }
    }
}

impl DebugEvent {
    /// Serializes to a JSON string (single line, no trailing newline).
    pub fn to_json(&self) -> String {
        match self {
            DebugEvent::Paused {
                script,
                line,
                reason,
            } => format!(
                r#"{{"type":"paused","script":{},"line":{},"reason":{}}}"#,
                escape_json_string(script),
                line,
                escape_json_string(reason)
            ),
            DebugEvent::Resumed => r#"{"type":"resumed"}"#.to_string(),
            DebugEvent::CallStack { frames } => {
                let frames_json: Vec<String> = frames
                    .iter()
                    .map(|f| {
                        format!(
                            r#"{{"script":{},"function":{},"line":{}}}"#,
                            escape_json_string(&f.script),
                            escape_json_string(&f.function),
                            f.line
                        )
                    })
                    .collect();
                format!(
                    r#"{{"type":"call_stack","frames":[{}]}}"#,
                    frames_json.join(",")
                )
            }
            DebugEvent::Locals { frame, variables } => {
                let vars: Vec<String> = variables
                    .iter()
                    .map(|(k, v)| format!("[{},{}]", escape_json_string(k), escape_json_string(v)))
                    .collect();
                format!(
                    r#"{{"type":"locals","frame":{},"variables":[{}]}}"#,
                    frame,
                    vars.join(",")
                )
            }
            DebugEvent::Globals { variables } => {
                let vars: Vec<String> = variables
                    .iter()
                    .map(|(k, v)| format!("[{},{}]", escape_json_string(k), escape_json_string(v)))
                    .collect();
                format!(r#"{{"type":"globals","variables":[{}]}}"#, vars.join(","))
            }
            DebugEvent::EvalResult {
                expression,
                value,
                error,
            } => {
                let err = match error {
                    Some(e) => escape_json_string(e),
                    None => "null".to_string(),
                };
                format!(
                    r#"{{"type":"eval_result","expression":{},"value":{},"error":{}}}"#,
                    escape_json_string(expression),
                    escape_json_string(value),
                    err
                )
            }
            DebugEvent::BreakpointSet { script, line } => format!(
                r#"{{"type":"breakpoint_set","script":{},"line":{}}}"#,
                escape_json_string(script),
                line
            ),
            DebugEvent::BreakpointRemoved { script, line } => format!(
                r#"{{"type":"breakpoint_removed","script":{},"line":{}}}"#,
                escape_json_string(script),
                line
            ),
            DebugEvent::Error { message } => format!(
                r#"{{"type":"error","message":{}}}"#,
                escape_json_string(message)
            ),
            DebugEvent::Exiting => r#"{"type":"exiting"}"#.to_string(),
        }
    }

    /// Parses from a JSON string. Returns `None` if parsing fails.
    pub fn from_json(s: &str) -> Option<Self> {
        let map = parse_json_object(s)?;
        let ty = map.get("type")?.as_str()?;
        match ty {
            "paused" => Some(DebugEvent::Paused {
                script: map.get("script")?.as_str()?.to_owned(),
                line: map.get("line")?.as_usize()?,
                reason: map.get("reason")?.as_str()?.to_owned(),
            }),
            "resumed" => Some(DebugEvent::Resumed),
            "breakpoint_set" => Some(DebugEvent::BreakpointSet {
                script: map.get("script")?.as_str()?.to_owned(),
                line: map.get("line")?.as_usize()?,
            }),
            "breakpoint_removed" => Some(DebugEvent::BreakpointRemoved {
                script: map.get("script")?.as_str()?.to_owned(),
                line: map.get("line")?.as_usize()?,
            }),
            "error" => Some(DebugEvent::Error {
                message: map.get("message")?.as_str()?.to_owned(),
            }),
            "exiting" => Some(DebugEvent::Exiting),
            // CallStack, Locals, Globals, EvalResult require array parsing —
            // for now, these are handled by the transport layer.
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Minimal JSON parser (object-level only)
// ---------------------------------------------------------------------------

/// A parsed JSON value (minimal subset for the debug protocol).
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    /// A string value.
    Str(String),
    /// A number value.
    Num(f64),
    /// Null.
    Null,
}

impl JsonValue {
    /// Returns the string value if this is a `Str`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the value as a usize if this is a `Num`.
    pub fn as_usize(&self) -> Option<usize> {
        match self {
            JsonValue::Num(n) => {
                let i = *n as usize;
                if (i as f64 - n).abs() < 0.5 {
                    Some(i)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Parses a flat JSON object `{"key": value, ...}` into a HashMap.
/// Only supports string and number values (sufficient for the protocol).
fn parse_json_object(s: &str) -> Option<HashMap<String, JsonValue>> {
    let s = s.trim();
    if !s.starts_with('{') || !s.ends_with('}') {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    let mut map = HashMap::new();
    let mut chars = inner.chars().peekable();

    loop {
        skip_whitespace(&mut chars);
        if chars.peek().is_none() {
            break;
        }

        // Parse key
        let key = parse_json_string_chars(&mut chars)?;
        skip_whitespace(&mut chars);
        if chars.next()? != ':' {
            return None;
        }
        skip_whitespace(&mut chars);

        // Parse value
        let value = match chars.peek()? {
            '"' => JsonValue::Str(parse_json_string_chars(&mut chars)?),
            'n' => {
                // null
                for expected in ['n', 'u', 'l', 'l'] {
                    if chars.next()? != expected {
                        return None;
                    }
                }
                JsonValue::Null
            }
            c if c.is_ascii_digit() || *c == '-' => {
                let mut num_str = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit()
                        || c == '.'
                        || c == '-'
                        || c == 'e'
                        || c == 'E'
                        || c == '+'
                    {
                        num_str.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                JsonValue::Num(num_str.parse().ok()?)
            }
            '[' | '{' => {
                // Skip nested structures (not needed for top-level parsing)
                let open = chars.next()?;
                let close = if open == '[' { ']' } else { '}' };
                let mut depth = 1;
                let mut buf = String::new();
                buf.push(open);
                while depth > 0 {
                    let c = chars.next()?;
                    buf.push(c);
                    if c == open {
                        depth += 1;
                    } else if c == close {
                        depth -= 1;
                    } else if c == '"' {
                        // Skip string contents
                        loop {
                            let sc = chars.next()?;
                            buf.push(sc);
                            if sc == '\\' {
                                let esc = chars.next()?;
                                buf.push(esc);
                            } else if sc == '"' {
                                break;
                            }
                        }
                    }
                }
                JsonValue::Str(buf) // Store nested as raw JSON string
            }
            _ => return None,
        };

        map.insert(key, value);

        skip_whitespace(&mut chars);
        match chars.peek() {
            Some(',') => {
                chars.next();
            }
            _ => {}
        }
    }

    Some(map)
}

fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
}

fn parse_json_string_chars(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<String> {
    if chars.next()? != '"' {
        return None;
    }
    let mut s = String::new();
    loop {
        let c = chars.next()?;
        match c {
            '"' => return Some(s),
            '\\' => {
                let esc = chars.next()?;
                match esc {
                    '"' => s.push('"'),
                    '\\' => s.push('\\'),
                    'n' => s.push('\n'),
                    'r' => s.push('\r'),
                    't' => s.push('\t'),
                    '/' => s.push('/'),
                    'u' => {
                        let mut hex = String::with_capacity(4);
                        for _ in 0..4 {
                            hex.push(chars.next()?);
                        }
                        let cp = u32::from_str_radix(&hex, 16).ok()?;
                        s.push(char::from_u32(cp)?);
                    }
                    _ => {
                        s.push('\\');
                        s.push(esc);
                    }
                }
            }
            _ => s.push(c),
        }
    }
}

// ---------------------------------------------------------------------------
// DebugServer (Game side)
// ---------------------------------------------------------------------------

/// TCP server that listens for editor debug connections.
pub struct DebugServer {
    listener: TcpListener,
}

impl DebugServer {
    /// Binds to the given address (e.g. `"127.0.0.1:6007"`).
    pub fn bind(addr: &str) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        Ok(Self { listener })
    }

    /// Returns the local address the server is bound to.
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }

    /// Waits for an editor to connect. Returns a [`DebugSession`].
    pub fn accept(&self) -> io::Result<DebugSession> {
        let (stream, _addr) = self.listener.accept()?;
        DebugSession::from_stream(stream)
    }

    /// Sets the listener to non-blocking mode for polling accepts.
    pub fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        self.listener.set_nonblocking(nonblocking)
    }
}

// ---------------------------------------------------------------------------
// DebugSession (shared connection wrapper)
// ---------------------------------------------------------------------------

/// A bidirectional debug connection (used by both server and client).
pub struct DebugSession {
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

impl DebugSession {
    fn from_stream(stream: TcpStream) -> io::Result<Self> {
        let reader_stream = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(reader_stream),
            writer: BufWriter::new(stream),
        })
    }

    /// Sets the read timeout for this session.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.reader.get_ref().set_read_timeout(timeout)
    }

    /// Sends a command over the wire.
    pub fn send_command(&mut self, cmd: &DebugCommand) -> io::Result<()> {
        let json = cmd.to_json();
        writeln!(self.writer, "{}", json)?;
        self.writer.flush()
    }

    /// Sends an event over the wire.
    pub fn send_event(&mut self, event: &DebugEvent) -> io::Result<()> {
        let json = event.to_json();
        writeln!(self.writer, "{}", json)?;
        self.writer.flush()
    }

    /// Reads a command from the wire (blocking).
    pub fn recv_command(&mut self) -> io::Result<DebugCommand> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        DebugCommand::from_json(line.trim())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid debug command"))
    }

    /// Reads an event from the wire (blocking).
    pub fn recv_event(&mut self) -> io::Result<DebugEvent> {
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        DebugEvent::from_json(line.trim())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid debug event"))
    }

    /// Non-blocking poll for a command. Returns `None` if no data available.
    pub fn try_recv_command(&mut self) -> io::Result<Option<DebugCommand>> {
        self.set_read_timeout(Some(Duration::from_millis(1)))?;
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            Ok(0) => Ok(None),
            Ok(_) => {
                let cmd = DebugCommand::from_json(line.trim()).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid debug command")
                })?;
                Ok(Some(cmd))
            }
            Err(ref e)
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}

// ---------------------------------------------------------------------------
// DebugClient (Editor side)
// ---------------------------------------------------------------------------

/// TCP client that connects to a running game's debug server.
pub struct DebugClient {
    session: DebugSession,
}

impl DebugClient {
    /// Connects to a debug server at the given address.
    pub fn connect(addr: &str) -> io::Result<Self> {
        let stream = TcpStream::connect(addr)?;
        let session = DebugSession::from_stream(stream)?;
        Ok(Self { session })
    }

    /// Connects with a timeout.
    pub fn connect_timeout(addr: std::net::SocketAddr, timeout: Duration) -> io::Result<Self> {
        let stream = TcpStream::connect_timeout(&addr, timeout)?;
        let session = DebugSession::from_stream(stream)?;
        Ok(Self { session })
    }

    /// Sends a debug command to the game.
    pub fn send_command(&mut self, cmd: &DebugCommand) -> io::Result<()> {
        self.session.send_command(cmd)
    }

    /// Receives a debug event from the game (blocking).
    pub fn recv_event(&mut self) -> io::Result<DebugEvent> {
        self.session.recv_event()
    }

    /// Sends a command and waits for the next event (convenience).
    pub fn request(&mut self, cmd: &DebugCommand) -> io::Result<DebugEvent> {
        self.send_command(cmd)?;
        self.recv_event()
    }
}

/// Default port for the debug server.
pub const DEFAULT_DEBUG_PORT: u16 = 6007;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_json_roundtrip_set_breakpoint() {
        let cmd = DebugCommand::SetBreakpoint {
            script: "res://player.gd".into(),
            line: 42,
        };
        let json = cmd.to_json();
        let parsed = DebugCommand::from_json(&json).unwrap();
        assert_eq!(cmd, parsed);
    }

    #[test]
    fn command_json_roundtrip_all_types() {
        let commands = vec![
            DebugCommand::SetBreakpoint {
                script: "res://test.gd".into(),
                line: 10,
            },
            DebugCommand::RemoveBreakpoint {
                script: "res://test.gd".into(),
                line: 10,
            },
            DebugCommand::ClearBreakpoints,
            DebugCommand::Continue,
            DebugCommand::StepIn,
            DebugCommand::StepOver,
            DebugCommand::StepOut,
            DebugCommand::Pause,
            DebugCommand::GetCallStack,
            DebugCommand::GetLocals { frame: 0 },
            DebugCommand::GetGlobals,
            DebugCommand::Evaluate {
                expression: "x + 1".into(),
            },
            DebugCommand::Disconnect,
        ];
        for cmd in commands {
            let json = cmd.to_json();
            let parsed = DebugCommand::from_json(&json).unwrap();
            assert_eq!(cmd, parsed, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn event_json_roundtrip_paused() {
        let event = DebugEvent::Paused {
            script: "res://player.gd".into(),
            line: 15,
            reason: "breakpoint".into(),
        };
        let json = event.to_json();
        let parsed = DebugEvent::from_json(&json).unwrap();
        assert_eq!(event, parsed);
    }

    #[test]
    fn event_json_roundtrip_simple_types() {
        let events = vec![
            DebugEvent::Resumed,
            DebugEvent::BreakpointSet {
                script: "res://a.gd".into(),
                line: 5,
            },
            DebugEvent::BreakpointRemoved {
                script: "res://a.gd".into(),
                line: 5,
            },
            DebugEvent::Error {
                message: "something went wrong".into(),
            },
            DebugEvent::Exiting,
        ];
        for event in events {
            let json = event.to_json();
            let parsed = DebugEvent::from_json(&json).unwrap();
            assert_eq!(event, parsed, "roundtrip failed for {json}");
        }
    }

    #[test]
    fn escape_special_characters() {
        let cmd = DebugCommand::Evaluate {
            expression: "a \"quoted\" string\nwith newline".into(),
        };
        let json = cmd.to_json();
        assert!(!json.contains('\n')); // newline must be escaped
        let parsed = DebugCommand::from_json(&json).unwrap();
        assert_eq!(cmd, parsed);
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(DebugCommand::from_json("not json").is_none());
        assert!(DebugCommand::from_json("{}").is_none());
        assert!(DebugCommand::from_json(r#"{"type":"unknown"}"#).is_none());
    }

    #[test]
    fn tcp_server_client_roundtrip() {
        // Bind to port 0 for OS-assigned ephemeral port.
        let server = DebugServer::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut session = server.accept().unwrap();
            let cmd = session.recv_command().unwrap();
            assert_eq!(
                cmd,
                DebugCommand::SetBreakpoint {
                    script: "res://test.gd".into(),
                    line: 10
                }
            );
            session
                .send_event(&DebugEvent::BreakpointSet {
                    script: "res://test.gd".into(),
                    line: 10,
                })
                .unwrap();
        });

        let mut client = DebugClient::connect(&addr.to_string()).unwrap();
        client
            .send_command(&DebugCommand::SetBreakpoint {
                script: "res://test.gd".into(),
                line: 10,
            })
            .unwrap();
        let event = client.recv_event().unwrap();
        assert_eq!(
            event,
            DebugEvent::BreakpointSet {
                script: "res://test.gd".into(),
                line: 10
            }
        );

        handle.join().unwrap();
    }

    #[test]
    fn tcp_multiple_commands() {
        let server = DebugServer::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();

        let handle = std::thread::spawn(move || {
            let mut session = server.accept().unwrap();
            // Receive 3 commands, echo back events.
            for _ in 0..3 {
                let cmd = session.recv_command().unwrap();
                match cmd {
                    DebugCommand::Continue => {
                        session.send_event(&DebugEvent::Resumed).unwrap();
                    }
                    DebugCommand::Pause => {
                        session
                            .send_event(&DebugEvent::Paused {
                                script: "res://main.gd".into(),
                                line: 1,
                                reason: "pause".into(),
                            })
                            .unwrap();
                    }
                    DebugCommand::Disconnect => {
                        session.send_event(&DebugEvent::Exiting).unwrap();
                    }
                    _ => {}
                }
            }
        });

        let mut client = DebugClient::connect(&addr.to_string()).unwrap();

        let ev = client.request(&DebugCommand::Continue).unwrap();
        assert_eq!(ev, DebugEvent::Resumed);

        let ev = client.request(&DebugCommand::Pause).unwrap();
        assert!(matches!(ev, DebugEvent::Paused { .. }));

        let ev = client.request(&DebugCommand::Disconnect).unwrap();
        assert_eq!(ev, DebugEvent::Exiting);

        handle.join().unwrap();
    }
}
