//! Integration tests for the remote debugging protocol over TCP.
//!
//! Tests the full debug protocol: command/event serialization roundtrips,
//! TCP client-server communication, and multi-step debugging sessions.

use std::thread;
use std::time::Duration;

use gdcore::debug_protocol::*;
use gdcore::debugger::*;

// ---------------------------------------------------------------------------
// Protocol serialization
// ---------------------------------------------------------------------------

#[test]
fn command_roundtrip_all() {
    let commands = vec![
        DebugCommand::SetBreakpoint {
            script: "res://player.gd".into(),
            line: 42,
        },
        DebugCommand::RemoveBreakpoint {
            script: "res://player.gd".into(),
            line: 42,
        },
        DebugCommand::ClearBreakpoints,
        DebugCommand::Continue,
        DebugCommand::StepIn,
        DebugCommand::StepOver,
        DebugCommand::StepOut,
        DebugCommand::Pause,
        DebugCommand::GetCallStack,
        DebugCommand::GetLocals { frame: 2 },
        DebugCommand::GetGlobals,
        DebugCommand::Evaluate {
            expression: "health * 2".into(),
        },
        DebugCommand::Disconnect,
    ];

    for cmd in &commands {
        let json = cmd.to_json();
        let parsed = DebugCommand::from_json(&json).expect(&format!("failed to parse: {json}"));
        assert_eq!(*cmd, parsed);
    }
}

#[test]
fn event_roundtrip_simple() {
    let events = vec![
        DebugEvent::Paused {
            script: "res://enemy.gd".into(),
            line: 100,
            reason: "breakpoint".into(),
        },
        DebugEvent::Resumed,
        DebugEvent::BreakpointSet {
            script: "res://x.gd".into(),
            line: 1,
        },
        DebugEvent::BreakpointRemoved {
            script: "res://x.gd".into(),
            line: 1,
        },
        DebugEvent::Error {
            message: "bad command".into(),
        },
        DebugEvent::Exiting,
    ];

    for ev in &events {
        let json = ev.to_json();
        let parsed = DebugEvent::from_json(&json).expect(&format!("failed to parse: {json}"));
        assert_eq!(*ev, parsed);
    }
}

#[test]
fn special_characters_in_strings() {
    let cmd = DebugCommand::Evaluate {
        expression: "\"hello\\nworld\"\ttab".into(),
    };
    let json = cmd.to_json();
    // Ensure no raw newlines in wire format.
    assert!(!json.contains('\n'));
    assert!(!json.contains('\t'));
    let parsed = DebugCommand::from_json(&json).unwrap();
    assert_eq!(cmd, parsed);
}

#[test]
fn invalid_json_returns_none() {
    assert!(DebugCommand::from_json("").is_none());
    assert!(DebugCommand::from_json("null").is_none());
    assert!(DebugCommand::from_json("[]").is_none());
    assert!(DebugCommand::from_json("{garbage}").is_none());
    assert!(DebugCommand::from_json(r#"{"type":"nonexistent"}"#).is_none());
}

// ---------------------------------------------------------------------------
// TCP transport
// ---------------------------------------------------------------------------

#[test]
fn tcp_single_command_response() {
    let server = DebugServer::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();

    let server_thread = thread::spawn(move || {
        let mut session = server.accept().unwrap();
        let cmd = session.recv_command().unwrap();
        assert_eq!(
            cmd,
            DebugCommand::SetBreakpoint {
                script: "res://main.gd".into(),
                line: 5,
            }
        );
        session
            .send_event(&DebugEvent::BreakpointSet {
                script: "res://main.gd".into(),
                line: 5,
            })
            .unwrap();
    });

    let mut client = DebugClient::connect(&addr.to_string()).unwrap();
    let event = client
        .request(&DebugCommand::SetBreakpoint {
            script: "res://main.gd".into(),
            line: 5,
        })
        .unwrap();

    assert_eq!(
        event,
        DebugEvent::BreakpointSet {
            script: "res://main.gd".into(),
            line: 5,
        }
    );

    server_thread.join().unwrap();
}

#[test]
fn tcp_full_debug_session() {
    let server = DebugServer::bind("127.0.0.1:0").unwrap();
    let addr = server.local_addr().unwrap();

    // Game-side thread: simulate a debug session with the Debugger state machine.
    let server_thread = thread::spawn(move || {
        let mut session = server.accept().unwrap();
        let mut debugger = Debugger::new();
        debugger.attach();
        debugger.push_frame(StackFrame::new("res://player.gd", "_ready", 1));

        loop {
            let cmd = session.recv_command().unwrap();
            match cmd {
                DebugCommand::SetBreakpoint { ref script, line } => {
                    debugger.add_breakpoint(BreakpointLocation::new(script.clone(), line));
                    session
                        .send_event(&DebugEvent::BreakpointSet {
                            script: script.clone(),
                            line,
                        })
                        .unwrap();
                }
                DebugCommand::Pause => {
                    debugger.pause();
                    let frame = debugger.current_frame().unwrap();
                    session
                        .send_event(&DebugEvent::Paused {
                            script: frame.script.clone(),
                            line: frame.line,
                            reason: "pause".into(),
                        })
                        .unwrap();
                }
                DebugCommand::Continue => {
                    debugger.continue_running();
                    session.send_event(&DebugEvent::Resumed).unwrap();
                }
                DebugCommand::StepIn => {
                    debugger.step_in();
                    session.send_event(&DebugEvent::Resumed).unwrap();
                }
                DebugCommand::Disconnect => {
                    debugger.detach();
                    session.send_event(&DebugEvent::Exiting).unwrap();
                    break;
                }
                _ => {
                    session
                        .send_event(&DebugEvent::Error {
                            message: "unhandled command".into(),
                        })
                        .unwrap();
                }
            }
        }
    });

    // Editor-side: drive the debugging session.
    let mut client = DebugClient::connect(&addr.to_string()).unwrap();

    // Set a breakpoint
    let ev = client
        .request(&DebugCommand::SetBreakpoint {
            script: "res://player.gd".into(),
            line: 15,
        })
        .unwrap();
    assert!(matches!(ev, DebugEvent::BreakpointSet { line: 15, .. }));

    // Pause the game
    let ev = client.request(&DebugCommand::Pause).unwrap();
    assert!(matches!(ev, DebugEvent::Paused { .. }));

    // Continue
    let ev = client.request(&DebugCommand::Continue).unwrap();
    assert_eq!(ev, DebugEvent::Resumed);

    // Disconnect
    let ev = client.request(&DebugCommand::Disconnect).unwrap();
    assert_eq!(ev, DebugEvent::Exiting);

    server_thread.join().unwrap();
}

#[test]
fn tcp_connect_timeout() {
    // Try connecting to a port where nothing is listening.
    let addr: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
    let result = DebugClient::connect_timeout(addr, Duration::from_millis(100));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Event serialization for complex types
// ---------------------------------------------------------------------------

#[test]
fn event_call_stack_json() {
    let event = DebugEvent::CallStack {
        frames: vec![
            StackFrameInfo {
                script: "res://main.gd".into(),
                function: "_ready".into(),
                line: 10,
            },
            StackFrameInfo {
                script: "res://util.gd".into(),
                function: "helper".into(),
                line: 5,
            },
        ],
    };
    let json = event.to_json();
    assert!(json.contains("call_stack"));
    assert!(json.contains("_ready"));
    assert!(json.contains("helper"));
    // Verify it's valid single-line JSON (no newlines).
    assert!(!json.contains('\n'));
}

#[test]
fn event_locals_json() {
    let event = DebugEvent::Locals {
        frame: 0,
        variables: vec![
            ("health".into(), "100".into()),
            ("name".into(), "\"Player\"".into()),
        ],
    };
    let json = event.to_json();
    assert!(json.contains("locals"));
    assert!(json.contains("health"));
}

#[test]
fn event_eval_result_json() {
    let event = DebugEvent::EvalResult {
        expression: "x + 1".into(),
        value: "42".into(),
        error: None,
    };
    let json = event.to_json();
    assert!(json.contains("eval_result"));
    assert!(json.contains("null")); // error is null

    let event_err = DebugEvent::EvalResult {
        expression: "bad()".into(),
        value: "".into(),
        error: Some("undefined function".into()),
    };
    let json_err = event_err.to_json();
    assert!(json_err.contains("undefined function"));
}

// ---------------------------------------------------------------------------
// Default port constant
// ---------------------------------------------------------------------------

#[test]
fn default_port_is_6007() {
    assert_eq!(DEFAULT_DEBUG_PORT, 6007);
}
