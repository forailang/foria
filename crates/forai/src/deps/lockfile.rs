use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use super::semver::{SemVer, VersionReq};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    pub lockfile_version: u32,
    pub packages: HashMap<String, LockedPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPackage {
    pub version: String,
    pub resolved: String,
    pub sha: String,
    pub dependencies: HashMap<String, String>,
}

impl Lockfile {
    pub fn new() -> Self {
        Lockfile {
            lockfile_version: 1,
            packages: HashMap::new(),
        }
    }

    pub fn load(project_root: &Path) -> Result<Option<Lockfile>, String> {
        let path = project_root.join("forai.lock");
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read forai.lock: {e}"))?;
        let lockfile: Lockfile =
            serde_json::from_str(&text).map_err(|e| format!("invalid forai.lock: {e}"))?;
        Ok(Some(lockfile))
    }

    pub fn save(&self, project_root: &Path) -> Result<(), String> {
        let path = project_root.join("forai.lock");
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize forai.lock: {e}"))?;
        std::fs::write(&path, format!("{text}\n"))
            .map_err(|e| format!("failed to write forai.lock: {e}"))?;
        Ok(())
    }

    pub fn get_locked(&self, name: &str, req: &VersionReq) -> Option<&LockedPackage> {
        let locked = self.packages.get(name)?;
        let ver = SemVer::parse(&locked.version).ok()?;
        if req.matches(&ver) {
            Some(locked)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfile_roundtrip() {
        let mut lockfile = Lockfile::new();
        lockfile.packages.insert(
            "@user/repo".to_string(),
            LockedPackage {
                version: "1.2.3".to_string(),
                resolved: "https://github.com/user/repo.git".to_string(),
                sha: "abc123".to_string(),
                dependencies: HashMap::new(),
            },
        );

        let json = serde_json::to_string_pretty(&lockfile).unwrap();
        let parsed: Lockfile = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.lockfile_version, 1);
        assert!(parsed.packages.contains_key("@user/repo"));
        let pkg = &parsed.packages["@user/repo"];
        assert_eq!(pkg.version, "1.2.3");
        assert_eq!(pkg.sha, "abc123");
    }

    #[test]
    fn get_locked_matching() {
        let mut lockfile = Lockfile::new();
        lockfile.packages.insert(
            "@user/repo".to_string(),
            LockedPackage {
                version: "1.2.3".to_string(),
                resolved: "https://github.com/user/repo.git".to_string(),
                sha: "abc123".to_string(),
                dependencies: HashMap::new(),
            },
        );

        let req = VersionReq::parse("^1.0.0").unwrap();
        assert!(lockfile.get_locked("@user/repo", &req).is_some());

        let req2 = VersionReq::parse("^2.0.0").unwrap();
        assert!(lockfile.get_locked("@user/repo", &req2).is_none());
    }

    #[test]
    fn load_missing_returns_none() {
        let dir = std::env::temp_dir().join("forai_test_lockfile_missing");
        let _ = std::fs::create_dir_all(&dir);
        let result = Lockfile::load(&dir).unwrap();
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
