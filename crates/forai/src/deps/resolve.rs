use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::fetch;
use super::lockfile::{LockedPackage, Lockfile};
use super::semver::{SemVer, VersionReq};
use super::source::DepSource;
use crate::config::{self, ProjectType};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ResolvedDep {
    pub name: String,
    pub version: SemVer,
    pub path: PathBuf,
    pub sha: String,
}

#[derive(Debug, Clone)]
pub struct ResolvedDeps {
    pub packages: Vec<ResolvedDep>,
    index: HashMap<String, usize>,
}

impl ResolvedDeps {
    pub fn empty() -> Self {
        ResolvedDeps {
            packages: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&ResolvedDep> {
        self.index.get(name).map(|&i| &self.packages[i])
    }
}

/// Resolve a single dependency from a git remote (GitHub or explicit URL).
fn resolve_git_dep(
    name: &str,
    url: &str,
    req: &VersionReq,
    lockfile: &Option<Lockfile>,
) -> Result<(SemVer, PathBuf, String), String> {
    // Check lockfile first
    if let Some(locked) = lockfile.as_ref().and_then(|lf| lf.get_locked(name, req)) {
        let ver = SemVer::parse(&locked.version)
            .map_err(|e| format!("invalid version in lockfile for '{name}': {e}"))?;
        if fetch::is_cached(name, &ver) {
            let path = fetch::package_cache_path(name, &ver);
            return Ok((ver, path, locked.sha.clone()));
        }
        // Locked but not cached — re-fetch
        let (path, sha) = fetch::fetch_from_url(url, name, &ver, name)?;
        return Ok((ver, path, sha));
    }

    // Not locked or lock doesn't satisfy — resolve from remote
    let available = fetch::list_available_versions_from_url(url, name)?;
    if available.is_empty() {
        return Err(format!(
            "no versions found for '{name}' (check that the repository exists)"
        ));
    }
    let matching = available.iter().find(|v| req.matches(v));
    let version = matching.ok_or_else(|| {
        let available_str = available
            .iter()
            .take(10)
            .map(|v| format!("{v}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("no version of '{name}' satisfies {req}. Available: {available_str}")
    })?;
    let (path, sha) = fetch::fetch_from_url(url, name, version, name)?;
    Ok((*version, path, sha))
}

pub fn resolve_dependencies(
    project_config: &config::ProjectConfig,
    project_root: &Path,
) -> Result<ResolvedDeps, String> {
    if project_config.dependencies.is_empty() {
        return Ok(ResolvedDeps::empty());
    }

    // Validate dependency names and version ranges
    config::validate_dependencies(project_config)?;

    // Load existing lockfile
    let lockfile = Lockfile::load(project_root)?;

    // Track resolved packages: name -> (version, path, sha, transitive_deps)
    let mut resolved: HashMap<String, (SemVer, PathBuf, String, HashMap<String, String>)> =
        HashMap::new();

    // Worklist: (dep_name, dep_value_string, required_by)
    let mut worklist: Vec<(String, String, String)> = project_config
        .dependencies
        .iter()
        .map(|(name, ver)| (name.clone(), ver.clone(), project_config.name.clone()))
        .collect();

    // Track visited to detect circular deps
    let mut in_progress: Vec<String> = Vec::new();

    while let Some((name, value_str, required_by)) = worklist.pop() {
        // Skip if already resolved
        if resolved.contains_key(&name) {
            // For git-based deps, verify version compatibility
            let source = DepSource::parse(&name, &value_str)?;
            match &source {
                DepSource::GitHub { version_req } | DepSource::Git { version_req, .. } => {
                    let (existing_ver, _, _, _) = &resolved[&name];
                    if !version_req.matches(existing_ver) {
                        return Err(format!(
                            "dependency conflict for '{name}':\n  \
                             '{required_by}' requires {value_str}\n  \
                             but already resolved to {existing_ver}"
                        ));
                    }
                }
                DepSource::File { .. } => {
                    // File deps don't have version conflicts
                }
            }
            continue;
        }

        // Circular dependency check
        if in_progress.contains(&name) {
            return Err(format!(
                "circular dependency detected: {}",
                in_progress.join(" -> ")
            ));
        }
        in_progress.push(name.clone());

        let source = DepSource::parse(&name, &value_str)?;

        let (version, path, sha) = match &source {
            DepSource::GitHub { version_req } => {
                let url = fetch::resolve_git_url(&name);
                resolve_git_dep(&name, &url, version_req, &lockfile)?
            }
            DepSource::Git { url, version_req } => {
                resolve_git_dep(&name, url, version_req, &lockfile)?
            }
            DepSource::File { path: file_path } => {
                let resolved_path = fetch::resolve_file_dep(file_path, project_root)?;
                // Read version from the local package's forai.json
                let (dep_config, _) = config::load_config(&resolved_path)?;
                let version = SemVer::parse(&dep_config.version).unwrap_or(SemVer {
                    major: 0,
                    minor: 0,
                    patch: 0,
                });
                (version, resolved_path, "local".to_string())
            }
        };

        // Validate it's a library
        let (dep_config, _) = config::load_config(&path)?;
        if dep_config.project_type != ProjectType::Lib {
            return Err(format!(
                "'{name}' v{version} has type 'app', not 'lib'. Only library packages can be imported"
            ));
        }

        // Collect transitive deps for lockfile
        let transitive_deps = dep_config.dependencies.clone();

        // Add transitive deps to worklist
        for (dep_name, dep_ver) in &dep_config.dependencies {
            worklist.push((dep_name.clone(), dep_ver.clone(), name.clone()));
        }

        resolved.insert(name.clone(), (version, path, sha, transitive_deps));
        in_progress.retain(|n| n != &name);
    }

    // Build the lockfile
    let mut new_lockfile = Lockfile::new();
    for (name, (version, path, sha, transitive_deps)) in &resolved {
        let resolved_url = match DepSource::parse(name, project_config.dependencies.get(name).map(|s| s.as_str()).unwrap_or("")) {
            Ok(DepSource::GitHub { .. }) => fetch::resolve_git_url(name),
            Ok(DepSource::Git { url, .. }) => url.clone(),
            Ok(DepSource::File { path: p }) => format!("file:{p}"),
            Err(_) => {
                // Transitive dep — check if it looks like a GitHub package
                if name.starts_with('@') {
                    fetch::resolve_git_url(name)
                } else {
                    path.display().to_string()
                }
            }
        };
        new_lockfile.packages.insert(
            name.clone(),
            LockedPackage {
                version: format!("{version}"),
                resolved: resolved_url,
                sha: sha.clone(),
                dependencies: transitive_deps.clone(),
            },
        );
    }
    new_lockfile.save(project_root)?;

    // Build ResolvedDeps
    let mut result = ResolvedDeps::empty();
    for (name, (version, path, sha, _)) in resolved {
        let idx = result.packages.len();
        result.packages.push(ResolvedDep {
            name: name.clone(),
            version,
            path,
            sha,
        });
        result.index.insert(name, idx);
    }

    Ok(result)
}
