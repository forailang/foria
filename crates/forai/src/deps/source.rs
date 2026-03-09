use super::semver::VersionReq;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    /// GitHub shorthand: key is "@user/repo", value is a semver range
    GitHub { version_req: VersionReq },
    /// Local filesystem path: value is "file:../some/path"
    File { path: String },
    /// Arbitrary git URL: value is "git+https://host/repo.git#^1.0.0"
    Git {
        url: String,
        version_req: VersionReq,
    },
}

impl DepSource {
    pub fn parse(dep_name: &str, dep_value: &str) -> Result<DepSource, String> {
        let value = dep_value.trim();

        if value.starts_with("file:") {
            let path = value.strip_prefix("file:").unwrap().to_string();
            if path.is_empty() {
                return Err(format!("empty path in file dependency for '{dep_name}'"));
            }
            return Ok(DepSource::File { path });
        }

        if value.starts_with("git+") {
            let rest = value.strip_prefix("git+").unwrap();
            // Format: git+https://host/repo.git#^1.0.0
            let (url, version_part) = rest.rsplit_once('#').ok_or_else(|| {
                format!(
                    "git dependency '{dep_name}' must include a version after '#', \
                     e.g. \"git+https://host/repo.git#^1.0.0\""
                )
            })?;
            if url.is_empty() {
                return Err(format!("empty URL in git dependency for '{dep_name}'"));
            }
            let version_req = VersionReq::parse(version_part).map_err(|e| {
                format!("cannot parse version requirement '{version_part}' for '{dep_name}': {e}")
            })?;
            return Ok(DepSource::Git {
                url: url.to_string(),
                version_req,
            });
        }

        // Default: semver range (for @user/repo GitHub shorthand)
        let version_req = VersionReq::parse(value).map_err(|e| {
            format!("cannot parse version requirement '{value}' for '{dep_name}': {e}")
        })?;
        Ok(DepSource::GitHub { version_req })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::semver::SemVer;

    #[test]
    fn parse_github_shorthand() {
        let src = DepSource::parse("@user/repo", "^1.0.0").unwrap();
        assert!(matches!(src, DepSource::GitHub { .. }));
        if let DepSource::GitHub { version_req } = src {
            assert!(version_req.matches(&SemVer::parse("1.2.3").unwrap()));
        }
    }

    #[test]
    fn parse_exact_version() {
        let src = DepSource::parse("@user/repo", "v1.0.0").unwrap();
        assert!(matches!(src, DepSource::GitHub { .. }));
    }

    #[test]
    fn parse_file_source() {
        let src = DepSource::parse("mylib", "file:../somelibrary/").unwrap();
        assert_eq!(
            src,
            DepSource::File {
                path: "../somelibrary/".to_string()
            }
        );
    }

    #[test]
    fn parse_file_absolute() {
        let src = DepSource::parse("mylib", "file:/home/user/libs/mylib").unwrap();
        assert_eq!(
            src,
            DepSource::File {
                path: "/home/user/libs/mylib".to_string()
            }
        );
    }

    #[test]
    fn parse_file_empty_path_error() {
        let err = DepSource::parse("mylib", "file:").unwrap_err();
        assert!(err.contains("empty path"));
    }

    #[test]
    fn parse_git_url() {
        let src = DepSource::parse("requests", "git+https://somesite.com/repo.git#^1.0.0").unwrap();
        if let DepSource::Git { url, version_req } = &src {
            assert_eq!(url, "https://somesite.com/repo.git");
            assert!(version_req.matches(&SemVer::parse("1.2.0").unwrap()));
        } else {
            panic!("expected Git variant");
        }
    }

    #[test]
    fn parse_git_url_exact_version() {
        let src = DepSource::parse("lib", "git+https://git.corp.com/team/lib.git#v2.0.0").unwrap();
        if let DepSource::Git { url, version_req } = &src {
            assert_eq!(url, "https://git.corp.com/team/lib.git");
            assert!(version_req.matches(&SemVer::parse("2.0.0").unwrap()));
        } else {
            panic!("expected Git variant");
        }
    }

    #[test]
    fn parse_git_missing_hash_error() {
        let err = DepSource::parse("lib", "git+https://somesite.com/repo.git").unwrap_err();
        assert!(err.contains("must include a version after '#'"));
    }

    #[test]
    fn parse_git_empty_url_error() {
        let err = DepSource::parse("lib", "git+#^1.0.0").unwrap_err();
        assert!(err.contains("empty URL"));
    }

    #[test]
    fn parse_git_bad_version_error() {
        let err = DepSource::parse("lib", "git+https://x.com/r.git#badversion").unwrap_err();
        assert!(err.contains("cannot parse version requirement"));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;

    use proptest::prelude::*;

    proptest! {
        #[test]
        fn parse_never_panics(name in "\\PC{0,30}", value in "\\PC{0,50}") {
            let _ = DepSource::parse(&name, &value);
        }

        #[test]
        fn file_prefix_always_yields_file_or_error(path in "[a-zA-Z0-9_./-]{1,30}") {
            // Use non-whitespace paths so outer trim() doesn't alter the value
            let value = format!("file:{path}");
            match DepSource::parse("dep", &value) {
                Ok(DepSource::File { path: p }) => prop_assert_eq!(p, path),
                Err(e) => {
                    let has_empty = e.contains("empty path");
                    prop_assert!(has_empty);
                }
                _ => prop_assert!(false),
            }
        }

        #[test]
        fn git_prefix_requires_hash(url_part in "[a-z]{1,15}") {
            // No '#' → must error
            let value = format!("git+{url_part}");
            let result = DepSource::parse("dep", &value);
            prop_assert!(result.is_err());
        }

        #[test]
        fn valid_semver_yields_github(
            major in 0u64..100,
            minor in 0u64..100,
            patch in 0u64..100,
        ) {
            let value = format!("^{major}.{minor}.{patch}");
            let result = DepSource::parse("@user/repo", &value).unwrap();
            let is_github = matches!(result, DepSource::GitHub { .. });
            prop_assert!(is_github);
        }

        #[test]
        fn valid_git_url_parses(
            host in "[a-z]{3,8}",
            major in 0u64..100,
            minor in 0u64..100,
            patch in 0u64..100,
        ) {
            let value = format!("git+https://{host}.com/repo.git#^{major}.{minor}.{patch}");
            let result = DepSource::parse("lib", &value).unwrap();
            match result {
                DepSource::Git { url, version_req } => {
                    let has_host = url.contains(&host);
                    prop_assert!(has_host);
                    let ver = crate::deps::semver::SemVer { major, minor, patch };
                    let matches_ver = version_req.matches(&ver);
                    prop_assert!(matches_ver);
                }
                _ => prop_assert!(false),
            }
        }
    }
}
