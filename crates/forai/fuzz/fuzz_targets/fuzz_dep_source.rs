#![no_main]
use libfuzzer_sys::fuzz_target;

// DepSource::parse must never panic on arbitrary name/value pairs.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Split input into name and value at the first newline
        let (name, value) = s.split_once('\n').unwrap_or((s, ""));
        let _ = forai::deps::source::DepSource::parse(name, value);
    }
});
