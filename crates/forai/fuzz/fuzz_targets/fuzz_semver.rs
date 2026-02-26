#![no_main]
use libfuzzer_sys::fuzz_target;

// SemVer::parse and VersionReq::parse must never panic on arbitrary input.
// If parsing succeeds, matches() must never panic either.
fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Fuzz SemVer parsing
        let _ = forai::deps::semver::SemVer::parse(s);

        // Fuzz VersionReq parsing, and if it succeeds, exercise matches()
        if let Ok(req) = forai::deps::semver::VersionReq::parse(s) {
            // Match against a few fixed versions
            let versions = [
                forai::deps::semver::SemVer { major: 0, minor: 0, patch: 0 },
                forai::deps::semver::SemVer { major: 0, minor: 0, patch: 1 },
                forai::deps::semver::SemVer { major: 0, minor: 1, patch: 0 },
                forai::deps::semver::SemVer { major: 1, minor: 0, patch: 0 },
                forai::deps::semver::SemVer { major: 1, minor: 2, patch: 3 },
                forai::deps::semver::SemVer { major: 99, minor: 99, patch: 99 },
            ];
            for v in &versions {
                let _ = req.matches(v);
            }
        }
    }
});
