use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemVer {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl SemVer {
    pub fn parse(s: &str) -> Result<SemVer, String> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(format!("invalid semver: '{s}' (expected X.Y.Z)"));
        }
        let major = parts[0]
            .parse::<u64>()
            .map_err(|_| format!("invalid major version: '{}'", parts[0]))?;
        let minor = parts[1]
            .parse::<u64>()
            .map_err(|_| format!("invalid minor version: '{}'", parts[1]))?;
        let patch = parts[2]
            .parse::<u64>()
            .map_err(|_| format!("invalid patch version: '{}'", parts[2]))?;
        Ok(SemVer {
            major,
            minor,
            patch,
        })
    }
}

impl Ord for SemVer {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }
}

impl PartialOrd for SemVer {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionReq {
    /// Exact match: "1.2.3"
    Exact(SemVer),
    /// Caret: "^1.2.3" — compatible with version
    Caret(SemVer),
    /// Tilde: "~1.2.3" — approximately equivalent
    Tilde(SemVer),
    /// Greater than or equal: ">=1.2.3"
    Gte(SemVer),
    /// Greater than: ">1.2.3"
    Gt(SemVer),
    /// Less than or equal: "<=1.2.3"
    Lte(SemVer),
    /// Less than: "<1.2.3"
    Lt(SemVer),
}

impl VersionReq {
    pub fn parse(s: &str) -> Result<VersionReq, String> {
        let s = s.trim();
        if s.starts_with("^") {
            let ver = SemVer::parse(&s[1..])?;
            Ok(VersionReq::Caret(ver))
        } else if s.starts_with("~") {
            let ver = SemVer::parse(&s[1..])?;
            Ok(VersionReq::Tilde(ver))
        } else if s.starts_with(">=") {
            let ver = SemVer::parse(&s[2..])?;
            Ok(VersionReq::Gte(ver))
        } else if s.starts_with(">") {
            let ver = SemVer::parse(&s[1..])?;
            Ok(VersionReq::Gt(ver))
        } else if s.starts_with("<=") {
            let ver = SemVer::parse(&s[2..])?;
            Ok(VersionReq::Lte(ver))
        } else if s.starts_with("<") {
            let ver = SemVer::parse(&s[1..])?;
            Ok(VersionReq::Lt(ver))
        } else {
            let ver = SemVer::parse(s)?;
            Ok(VersionReq::Exact(ver))
        }
    }

    pub fn matches(&self, version: &SemVer) -> bool {
        match self {
            VersionReq::Exact(req) => version == req,
            VersionReq::Caret(req) => {
                if version < req {
                    return false;
                }
                if req.major > 0 {
                    // ^1.2.3 := >=1.2.3, <2.0.0
                    version.major == req.major
                } else if req.minor > 0 {
                    // ^0.2.3 := >=0.2.3, <0.3.0
                    version.major == 0 && version.minor == req.minor
                } else {
                    // ^0.0.3 := >=0.0.3, <0.0.4
                    version.major == 0 && version.minor == 0 && version.patch == req.patch
                }
            }
            VersionReq::Tilde(req) => {
                // ~1.2.3 := >=1.2.3, <1.3.0
                version >= req && version.major == req.major && version.minor == req.minor
            }
            VersionReq::Gte(req) => version >= req,
            VersionReq::Gt(req) => version > req,
            VersionReq::Lte(req) => version <= req,
            VersionReq::Lt(req) => version < req,
        }
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionReq::Exact(v) => write!(f, "{v}"),
            VersionReq::Caret(v) => write!(f, "^{v}"),
            VersionReq::Tilde(v) => write!(f, "~{v}"),
            VersionReq::Gte(v) => write!(f, ">={v}"),
            VersionReq::Gt(v) => write!(f, ">{v}"),
            VersionReq::Lte(v) => write!(f, "<={v}"),
            VersionReq::Lt(v) => write!(f, "<{v}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_semver_basic() {
        let v = SemVer::parse("1.2.3").unwrap();
        assert_eq!(v, SemVer { major: 1, minor: 2, patch: 3 });
    }

    #[test]
    fn parse_semver_with_v_prefix() {
        let v = SemVer::parse("v1.2.3").unwrap();
        assert_eq!(v, SemVer { major: 1, minor: 2, patch: 3 });
    }

    #[test]
    fn parse_semver_invalid() {
        assert!(SemVer::parse("bad").is_err());
        assert!(SemVer::parse("1.2").is_err());
        assert!(SemVer::parse("1.2.x").is_err());
    }

    #[test]
    fn semver_ordering() {
        let v1 = SemVer::parse("1.0.0").unwrap();
        let v2 = SemVer::parse("1.0.1").unwrap();
        let v3 = SemVer::parse("1.1.0").unwrap();
        let v4 = SemVer::parse("2.0.0").unwrap();
        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v3 < v4);
    }

    #[test]
    fn semver_display() {
        let v = SemVer { major: 1, minor: 2, patch: 3 };
        assert_eq!(format!("{v}"), "1.2.3");
    }

    #[test]
    fn parse_version_req_exact() {
        let req = VersionReq::parse("1.2.3").unwrap();
        assert_eq!(req, VersionReq::Exact(SemVer { major: 1, minor: 2, patch: 3 }));
    }

    #[test]
    fn parse_version_req_with_v_prefix() {
        let req = VersionReq::parse("v1.2.3").unwrap();
        assert_eq!(req, VersionReq::Exact(SemVer { major: 1, minor: 2, patch: 3 }));
    }

    #[test]
    fn parse_version_req_caret() {
        let req = VersionReq::parse("^1.2.3").unwrap();
        assert_eq!(req, VersionReq::Caret(SemVer { major: 1, minor: 2, patch: 3 }));
    }

    #[test]
    fn parse_version_req_tilde() {
        let req = VersionReq::parse("~1.2.3").unwrap();
        assert_eq!(req, VersionReq::Tilde(SemVer { major: 1, minor: 2, patch: 3 }));
    }

    #[test]
    fn parse_version_req_gte() {
        let req = VersionReq::parse(">=1.0.0").unwrap();
        assert_eq!(req, VersionReq::Gte(SemVer { major: 1, minor: 0, patch: 0 }));
    }

    #[test]
    fn parse_version_req_gt() {
        let req = VersionReq::parse(">1.0.0").unwrap();
        assert_eq!(req, VersionReq::Gt(SemVer { major: 1, minor: 0, patch: 0 }));
    }

    #[test]
    fn parse_version_req_lte() {
        let req = VersionReq::parse("<=2.0.0").unwrap();
        assert_eq!(req, VersionReq::Lte(SemVer { major: 2, minor: 0, patch: 0 }));
    }

    #[test]
    fn parse_version_req_lt() {
        let req = VersionReq::parse("<2.0.0").unwrap();
        assert_eq!(req, VersionReq::Lt(SemVer { major: 2, minor: 0, patch: 0 }));
    }

    // --- Matching tests ---

    #[test]
    fn exact_matches() {
        let req = VersionReq::parse("1.2.3").unwrap();
        assert!(req.matches(&SemVer::parse("1.2.3").unwrap()));
        assert!(!req.matches(&SemVer::parse("1.2.4").unwrap()));
        assert!(!req.matches(&SemVer::parse("1.3.0").unwrap()));
    }

    #[test]
    fn caret_major_nonzero() {
        // ^1.2.3 := >=1.2.3, <2.0.0
        let req = VersionReq::parse("^1.2.3").unwrap();
        assert!(req.matches(&SemVer::parse("1.2.3").unwrap()));
        assert!(req.matches(&SemVer::parse("1.2.4").unwrap()));
        assert!(req.matches(&SemVer::parse("1.9.9").unwrap()));
        assert!(!req.matches(&SemVer::parse("2.0.0").unwrap()));
        assert!(!req.matches(&SemVer::parse("1.2.2").unwrap()));
    }

    #[test]
    fn caret_major_zero_minor_nonzero() {
        // ^0.2.3 := >=0.2.3, <0.3.0
        let req = VersionReq::parse("^0.2.3").unwrap();
        assert!(req.matches(&SemVer::parse("0.2.3").unwrap()));
        assert!(req.matches(&SemVer::parse("0.2.9").unwrap()));
        assert!(!req.matches(&SemVer::parse("0.3.0").unwrap()));
        assert!(!req.matches(&SemVer::parse("0.2.2").unwrap()));
    }

    #[test]
    fn caret_all_zero_except_patch() {
        // ^0.0.3 := >=0.0.3, <0.0.4 (exact)
        let req = VersionReq::parse("^0.0.3").unwrap();
        assert!(req.matches(&SemVer::parse("0.0.3").unwrap()));
        assert!(!req.matches(&SemVer::parse("0.0.4").unwrap()));
        assert!(!req.matches(&SemVer::parse("0.0.2").unwrap()));
    }

    #[test]
    fn tilde_matches() {
        // ~1.2.3 := >=1.2.3, <1.3.0
        let req = VersionReq::parse("~1.2.3").unwrap();
        assert!(req.matches(&SemVer::parse("1.2.3").unwrap()));
        assert!(req.matches(&SemVer::parse("1.2.9").unwrap()));
        assert!(!req.matches(&SemVer::parse("1.3.0").unwrap()));
        assert!(!req.matches(&SemVer::parse("1.2.2").unwrap()));
    }

    #[test]
    fn gte_matches() {
        let req = VersionReq::parse(">=1.0.0").unwrap();
        assert!(req.matches(&SemVer::parse("1.0.0").unwrap()));
        assert!(req.matches(&SemVer::parse("2.0.0").unwrap()));
        assert!(!req.matches(&SemVer::parse("0.9.9").unwrap()));
    }

    #[test]
    fn gt_matches() {
        let req = VersionReq::parse(">1.0.0").unwrap();
        assert!(!req.matches(&SemVer::parse("1.0.0").unwrap()));
        assert!(req.matches(&SemVer::parse("1.0.1").unwrap()));
    }

    #[test]
    fn lte_matches() {
        let req = VersionReq::parse("<=2.0.0").unwrap();
        assert!(req.matches(&SemVer::parse("2.0.0").unwrap()));
        assert!(req.matches(&SemVer::parse("1.0.0").unwrap()));
        assert!(!req.matches(&SemVer::parse("2.0.1").unwrap()));
    }

    #[test]
    fn lt_matches() {
        let req = VersionReq::parse("<2.0.0").unwrap();
        assert!(!req.matches(&SemVer::parse("2.0.0").unwrap()));
        assert!(req.matches(&SemVer::parse("1.9.9").unwrap()));
    }

    #[test]
    fn version_req_display() {
        assert_eq!(format!("{}", VersionReq::parse("^1.2.3").unwrap()), "^1.2.3");
        assert_eq!(format!("{}", VersionReq::parse("~0.1.0").unwrap()), "~0.1.0");
        assert_eq!(format!("{}", VersionReq::parse(">=1.0.0").unwrap()), ">=1.0.0");
        assert_eq!(format!("{}", VersionReq::parse("1.0.0").unwrap()), "1.0.0");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_semver() -> impl Strategy<Value = SemVer> {
        (0u64..1000, 0u64..1000, 0u64..1000)
            .prop_map(|(major, minor, patch)| SemVer { major, minor, patch })
    }

    proptest! {
        #[test]
        fn parse_display_roundtrip(v in arb_semver()) {
            let s = format!("{v}");
            let parsed = SemVer::parse(&s).unwrap();
            prop_assert_eq!(parsed, v);
        }

        #[test]
        fn parse_with_v_prefix_roundtrip(v in arb_semver()) {
            let s = format!("v{v}");
            let parsed = SemVer::parse(&s).unwrap();
            prop_assert_eq!(parsed, v);
        }

        #[test]
        fn parse_never_panics(s in "\\PC{0,30}") {
            let _ = SemVer::parse(&s);
        }

        #[test]
        fn ordering_is_total(a in arb_semver(), b in arb_semver()) {
            // Exactly one of: a < b, a == b, a > b
            let lt = a < b;
            let eq = a == b;
            let gt = a > b;
            prop_assert_eq!(lt as u8 + eq as u8 + gt as u8, 1);
        }

        #[test]
        fn ordering_matches_tuple(a in arb_semver(), b in arb_semver()) {
            let ta = (a.major, a.minor, a.patch);
            let tb = (b.major, b.minor, b.patch);
            prop_assert_eq!(a.cmp(&b), ta.cmp(&tb));
        }
    }

    // --- VersionReq proptests ---

    proptest! {
        #[test]
        fn version_req_parse_never_panics(s in "\\PC{0,30}") {
            let _ = VersionReq::parse(&s);
        }

        #[test]
        fn version_req_display_roundtrip(v in arb_semver()) {
            // For each operator, display then re-parse should yield equivalent matching
            let test_version = SemVer { major: v.major, minor: v.minor, patch: v.patch };

            let reqs = vec![
                VersionReq::Exact(v),
                VersionReq::Caret(v),
                VersionReq::Tilde(v),
                VersionReq::Gte(v),
                VersionReq::Gt(v),
                VersionReq::Lte(v),
                VersionReq::Lt(v),
            ];
            for req in &reqs {
                let displayed = format!("{req}");
                let reparsed = VersionReq::parse(&displayed).unwrap();
                // Matching the base version should produce the same result
                let orig = req.matches(&test_version);
                let re = reparsed.matches(&test_version);
                prop_assert_eq!(orig, re);
            }
        }

        #[test]
        fn exact_only_matches_itself(a in arb_semver(), b in arb_semver()) {
            let req = VersionReq::Exact(a);
            prop_assert_eq!(req.matches(&b), a == b);
        }

        #[test]
        fn caret_always_matches_base(v in arb_semver()) {
            let req = VersionReq::Caret(v);
            prop_assert!(req.matches(&v));
        }

        #[test]
        fn tilde_always_matches_base(v in arb_semver()) {
            let req = VersionReq::Tilde(v);
            prop_assert!(req.matches(&v));
        }

        #[test]
        fn gte_always_matches_base(v in arb_semver()) {
            let req = VersionReq::Gte(v);
            prop_assert!(req.matches(&v));
        }

        #[test]
        fn gt_never_matches_base(v in arb_semver()) {
            let req = VersionReq::Gt(v);
            prop_assert!(!req.matches(&v));
        }

        #[test]
        fn lte_always_matches_base(v in arb_semver()) {
            let req = VersionReq::Lte(v);
            prop_assert!(req.matches(&v));
        }

        #[test]
        fn lt_never_matches_base(v in arb_semver()) {
            let req = VersionReq::Lt(v);
            prop_assert!(!req.matches(&v));
        }

        #[test]
        fn caret_nonzero_major_stays_in_major(
            base in arb_semver().prop_filter("major > 0", |v| v.major > 0),
            bump_minor in 0u64..100,
            bump_patch in 0u64..100,
        ) {
            let req = VersionReq::Caret(base);
            let candidate = SemVer {
                major: base.major,
                minor: base.minor.saturating_add(bump_minor),
                patch: base.patch.saturating_add(bump_patch),
            };
            prop_assert!(req.matches(&candidate));

            // Next major should NOT match
            let next_major = SemVer { major: base.major + 1, minor: 0, patch: 0 };
            prop_assert!(!req.matches(&next_major));
        }

        #[test]
        fn tilde_stays_in_minor(
            base in arb_semver(),
            bump_patch in 0u64..100,
        ) {
            let req = VersionReq::Tilde(base);
            let candidate = SemVer {
                major: base.major,
                minor: base.minor,
                patch: base.patch.saturating_add(bump_patch),
            };
            prop_assert!(req.matches(&candidate));

            // Next minor should NOT match
            let next_minor = SemVer {
                major: base.major,
                minor: base.minor + 1,
                patch: 0,
            };
            prop_assert!(!req.matches(&next_minor));
        }

        #[test]
        fn gte_monotonic(a in arb_semver(), b in arb_semver()) {
            let req = VersionReq::Gte(a);
            if b >= a {
                prop_assert!(req.matches(&b));
            } else {
                prop_assert!(!req.matches(&b));
            }
        }

        #[test]
        fn lt_and_gte_are_complementary(req_ver in arb_semver(), test_ver in arb_semver()) {
            let lt = VersionReq::Lt(req_ver);
            let gte = VersionReq::Gte(req_ver);
            prop_assert_ne!(lt.matches(&test_ver), gte.matches(&test_ver));
        }

        #[test]
        fn gt_and_lte_are_complementary(req_ver in arb_semver(), test_ver in arb_semver()) {
            let gt = VersionReq::Gt(req_ver);
            let lte = VersionReq::Lte(req_ver);
            prop_assert_ne!(gt.matches(&test_ver), lte.matches(&test_ver));
        }
    }
}
