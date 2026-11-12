#![no_main]

use libfuzzer_sys::fuzz_target;

use gdscript_interop::parser::Parser;
use gdscript_interop::tokenizer::tokenize;

fuzz_target!(|data: &[u8]| {
    // Cap inputs to keep libfuzzer focused on grammar coverage rather than
    // pathological deep-nesting stack overflow. The parser has a depth guard
    // but ASan-inflated frames can still blow the stack if enough nested
    // openers slip through per-parse-expr call.
    if data.len() > 1024 {
        return;
    }

    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };

    let Ok(tokens) = tokenize(source) else {
        return;
    };

    let mut parser = Parser::new(tokens, source);
    let _ = parser.parse_script();
});
