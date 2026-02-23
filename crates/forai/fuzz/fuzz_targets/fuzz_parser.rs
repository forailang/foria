#![no_main]
use libfuzzer_sys::fuzz_target;

// Parser must never panic, regardless of input.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Module parser (first pass)
        let _ = forai::parser::parse_module_v1(s);
    }
});
