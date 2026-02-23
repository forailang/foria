#![no_main]
use libfuzzer_sys::fuzz_target;

// Lexer must never panic, regardless of input.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = forai::lexer::lex(s);
    }
});
