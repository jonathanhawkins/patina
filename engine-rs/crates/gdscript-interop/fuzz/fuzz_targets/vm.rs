#![no_main]

use libfuzzer_sys::fuzz_target;

use gdscript_interop::interpreter::Interpreter;

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 {
        return;
    }

    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };

    // Skip potentially unbounded constructs that could spin the interpreter
    // past the libfuzzer per-input timeout. The parser target already exercises
    // the full grammar, so filtering here does not reduce lexer/parser coverage.
    if source.contains("while") || source.contains("for ") {
        return;
    }

    let mut interp = Interpreter::new();
    let _ = interp.run(source);
});
