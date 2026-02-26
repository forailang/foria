use super::semver::SemVer;
use std::path::{Path, PathBuf};
use std::process::Command;

fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg).join("forai")
    } else if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/forai")
    } else {
        PathBuf::from(".forai")
    }
}

pub fn cache_dir() -> PathBuf {
    config_dir().join("cache")
}

pub fn package_cache_path(name: &str, version: &SemVer) -> PathBuf {
    cache_dir().join(name).join(format!("v{version}"))
}

pub fn is_cached(name: &str, version: &SemVer) -> bool {
    let path = package_cache_path(name, version);
    path.join("forai.json").exists()
}

pub fn resolve_git_url(package_name: &str) -> String {
    // @user/repo -> https://github.com/user/repo.git
    let stripped = package_name.strip_prefix('@').unwrap_or(package_name);
    format!("https://github.com/{stripped}.git")
}

/// List available semver tags from a git remote URL.
pub fn list_available_versions_from_url(url: &str, label: &str) -> Result<Vec<SemVer>, String> {
    let output = Command::new("git")
        .args(["ls-remote", "--tags", url])
        .output()
        .map_err(|e| format!("failed to run git ls-remote for '{label}': {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "failed to list versions for '{label}': {stderr}"
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut versions: Vec<SemVer> = Vec::new();

    for line in stdout.lines() {
        // Format: "<sha>\trefs/tags/v1.0.0" or "<sha>\trefs/tags/v1.0.0^{}"
        if let Some(tag_ref) = line.split('\t').nth(1) {
            let tag = tag_ref
                .strip_prefix("refs/tags/")
                .unwrap_or(tag_ref)
                .trim_end_matches("^{}");
            if let Ok(ver) = SemVer::parse(tag) {
                if !versions.contains(&ver) {
                    versions.push(ver);
                }
            }
        }
    }

    versions.sort();
    versions.reverse(); // highest first
    Ok(versions)
}

/// Clone a specific version tag from a git URL into the cache.
pub fn fetch_from_url(
    url: &str,
    label: &str,
    version: &SemVer,
    cache_name: &str,
) -> Result<(PathBuf, String), String> {
    let dest = package_cache_path(cache_name, version);

    // Check cache first
    if dest.join("forai.json").exists() {
        let sha = std::fs::read_to_string(dest.join(".forai-sha"))
            .unwrap_or_else(|_| "unknown".to_string());
        return Ok((dest, sha.trim().to_string()));
    }

    let tag = format!("v{version}");

    // Ensure parent dir exists
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!(
                "failed to create cache directory {}: {e}",
                parent.display()
            )
        })?;
    }

    // Shallow clone at the specific tag
    let output = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--branch",
            &tag,
            url,
            &dest.to_string_lossy(),
        ])
        .output()
        .map_err(|e| format!("failed to run git clone for '{label}': {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_dir_all(&dest);
        return Err(format!(
            "failed to fetch '{label}' {tag}: {stderr}"
        ));
    }

    // Get the commit SHA before removing .git
    let sha_output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dest)
        .output()
        .map_err(|e| format!("failed to read git SHA: {e}"))?;
    let sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();

    // Save SHA marker
    let _ = std::fs::write(dest.join(".forai-sha"), &sha);

    // Remove .git directory to save space
    let git_dir = dest.join(".git");
    if git_dir.exists() {
        std::fs::remove_dir_all(&git_dir)
            .map_err(|e| format!("failed to clean .git from cache: {e}"))?;
    }

    // Validate the package
    if !dest.join("forai.json").exists() {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(format!(
            "'{label}' v{version} is not a valid forai package (missing forai.json)"
        ));
    }

    Ok((dest, sha))
}

/// Resolve a file: dependency path relative to the project root.
/// Returns the canonicalized path.
pub fn resolve_file_dep(path: &str, project_root: &Path) -> Result<PathBuf, String> {
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        project_root.join(path)
    };

    let canonical = resolved.canonicalize().map_err(|e| {
        format!(
            "file dependency path '{}' does not exist: {e}",
            resolved.display()
        )
    })?;

    if !canonical.join("forai.json").exists() {
        return Err(format!(
            "file dependency '{}' is not a valid forai package (missing forai.json)",
            canonical.display()
        ));
    }

    Ok(canonical)
}
