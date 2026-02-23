#![no_main]
use libfuzzer_sys::fuzz_target;

// Formatter must never panic, and must be idempotent.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let once = forai::formatter::format_source(s);
        let twice = forai::formatter::format_source(&once);
        assert_eq!(once, twice, "formatter is not idempotent");
    }
});
