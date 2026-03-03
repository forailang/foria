use crate::deps::source::DepSource;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ProjectType {
    #[serde(rename = "app")]
    App,
    #[serde(rename = "lib")]
    Lib,
}

impl Default for ProjectType {
    fn default() -> Self {
        ProjectType::App
    }
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ProjectConfig {
    pub name: String,

    #[serde(default = "default_version")]
    pub version: String,

    #[serde(default)]
    pub description: Option<String>,

    #[serde(default)]
    pub forai: Option<String>,

    #[serde(rename = "type", default)]
    pub project_type: ProjectType,

    pub main: String,

    #[serde(default)]
    pub build: BuildConfig,

    #[serde(default)]
    pub test: TestConfig,

    #[serde(default)]
    pub docs: DocsConfig,

    #[serde(default)]
    pub dependencies: std::collections::HashMap<String, String>,

    #[serde(default)]
    pub ffi: std::collections::HashMap<String, FfiLibConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[allow(dead_code)]
pub struct FfiLibConfig {
    pub lib: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildConfig {
    #[serde(default = "default_build_out")]
    pub out: String,

    #[serde(default = "default_build_targets")]
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct TestConfig {
    #[serde(default = "default_test_out")]
    pub out: String,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct DocsConfig {
    #[serde(default = "default_docs_out")]
    pub out: String,
}

fn default_version() -> String {
    "0.0.0".to_string()
}

fn default_build_out() -> String {
    "dist/".to_string()
}

fn default_build_targets() -> Vec<String> {
    vec!["wasm".to_string()]
}

fn default_test_out() -> String {
    "test-results/".to_string()
}

fn default_docs_out() -> String {
    "docs/".to_string()
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            out: default_build_out(),
            targets: default_build_targets(),
        }
    }
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            out: default_test_out(),
        }
    }
}

impl Default for DocsConfig {
    fn default() -> Self {
        Self {
            out: default_docs_out(),
        }
    }
}

pub const CONFIG_FILENAME: &str = "forai.json";
pub const COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Load and parse a forai.json from the given directory.
/// Returns the parsed config and the project root directory (the dir containing forai.json).
pub fn load_config(dir: &Path) -> Result<(ProjectConfig, PathBuf), String> {
    let config_path = dir.join(CONFIG_FILENAME);
    if !config_path.exists() {
        return Err(format!(
            "no forai.json found in {}\n       run `forai init` to create one",
            dir.display()
        ));
    }

    let text = fs::read_to_string(&config_path)
        .map_err(|e| format!("failed to read {}: {e}", config_path.display()))?;

    let config: ProjectConfig =
        serde_json::from_str(&text).map_err(|e| format!("invalid forai.json: {e}"))?;

    Ok((config, dir.to_path_buf()))
}

/// Search for forai.json starting from `start_dir` and walking up.
/// Returns the config and the project root directory.
pub fn find_config(start_dir: &Path) -> Result<(ProjectConfig, PathBuf), String> {
    let mut dir = start_dir.to_path_buf();
    loop {
        let config_path = dir.join(CONFIG_FILENAME);
        if config_path.exists() {
            let (config, _) = load_config(&dir)?;
            return Ok((config, dir));
        }
        if !dir.pop() {
            break;
        }
    }
    Err(format!(
        "no forai.json found in {} or any parent directory\n       run `forai init` to create one",
        start_dir.display()
    ))
}

/// Check that the compiler version satisfies the project's forai version constraint.
/// For now, supports simple prefix matching: ">=X.Y.Z" and exact "X.Y.Z".
pub fn check_version(config: &ProjectConfig) -> Result<(), String> {
    let constraint = match &config.forai {
        Some(c) if c != "*" => c,
        _ => return Ok(()),
    };

    let compiler = parse_semver(COMPILER_VERSION)
        .ok_or_else(|| format!("cannot parse compiler version: {COMPILER_VERSION}"))?;

    if let Some(min) = constraint.strip_prefix(">=") {
        let required = parse_semver(min.trim())
            .ok_or_else(|| format!("cannot parse version constraint: {constraint}"))?;
        if compiler < required {
            return Err(format!(
                "this project requires forai {constraint} but you have {COMPILER_VERSION}\n       \
                 update the compiler or change the \"forai\" field in forai.json"
            ));
        }
        return Ok(());
    }

    // Exact match
    let required = parse_semver(constraint.trim())
        .ok_or_else(|| format!("cannot parse version constraint: {constraint}"))?;
    if compiler != required {
        return Err(format!(
            "this project requires forai {constraint} but you have {COMPILER_VERSION}\n       \
             update the compiler or change the \"forai\" field in forai.json"
        ));
    }
    Ok(())
}

fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

pub fn validate_dependencies(config: &ProjectConfig) -> Result<(), String> {
    for (name, value) in &config.dependencies {
        let source = DepSource::parse(name, value)?;
        // GitHub shorthand requires @user/repo format
        if matches!(source, DepSource::GitHub { .. }) && (!name.starts_with('@') || name.matches('/').count() != 1) {
            return Err(format!(
                "invalid dependency '{name}': GitHub packages must use '@user/repo' format"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_minimal_config() {
        let json = r#"{"name": "hello", "main": "main.fa"}"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.name, "hello");
        assert_eq!(config.main, "main.fa");
        assert_eq!(config.version, "0.0.0");
        assert_eq!(config.build.out, "dist/");
        assert_eq!(config.build.targets, vec!["wasm"]);
        assert_eq!(config.test.out, "test-results/");
        assert_eq!(config.docs.out, "docs/");
        assert!(config.dependencies.is_empty());
    }

    #[test]
    fn parse_full_config() {
        let json = r#"{
            "name": "my-app",
            "version": "0.1.0",
            "description": "A test app",
            "forai": ">=0.1.0",
            "main": "src/main.fa",
            "build": { "out": "output/", "targets": ["wasm", "bundle"] },
            "test": { "out": "results/" },
            "docs": { "out": "api-docs/" },
            "dependencies": { "sqlite": "0.1.0" }
        }"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.name, "my-app");
        assert_eq!(config.version, "0.1.0");
        assert_eq!(config.description.as_deref(), Some("A test app"));
        assert_eq!(config.forai.as_deref(), Some(">=0.1.0"));
        assert_eq!(config.main, "src/main.fa");
        assert_eq!(config.build.out, "output/");
        assert_eq!(config.build.targets, vec!["wasm", "bundle"]);
        assert_eq!(config.test.out, "results/");
        assert_eq!(config.docs.out, "api-docs/");
        assert_eq!(config.dependencies.get("sqlite").unwrap(), "0.1.0");
    }

    #[test]
    fn missing_required_fields() {
        let json = r#"{"name": "hello"}"#;
        let result: Result<ProjectConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn load_config_missing_file() {
        let dir = std::env::temp_dir().join("forai_test_no_config");
        let _ = fs::create_dir_all(&dir);
        let result = load_config(&dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no forai.json found"));
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn load_config_from_dir() {
        let dir = std::env::temp_dir().join("forai_test_load_config");
        let _ = fs::create_dir_all(&dir);
        let config_path = dir.join("forai.json");
        fs::write(&config_path, r#"{"name": "test", "main": "main.fa"}"#).unwrap();

        let (config, _) = load_config(&dir).unwrap();
        assert_eq!(config.name, "test");
        assert_eq!(config.main, "main.fa");

        let _ = fs::remove_file(&config_path);
        let _ = fs::remove_dir(&dir);
    }

    #[test]
    fn version_check_wildcard() {
        let config = ProjectConfig {
            name: "test".into(),
            version: "0.0.0".into(),
            description: None,
            forai: None,
            project_type: ProjectType::App,
            main: "main.fa".into(),
            build: BuildConfig::default(),
            test: TestConfig::default(),
            docs: DocsConfig::default(),
            dependencies: Default::default(),
            ffi: Default::default(),
        };
        assert!(check_version(&config).is_ok());
    }

    #[test]
    fn semver_parsing() {
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("bad"), None);
        assert_eq!(parse_semver("1.2"), None);
    }

    #[test]
    fn parse_project_type_app() {
        let json = r#"{"name": "hello", "main": "main.fa", "type": "app"}"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.project_type, ProjectType::App);
    }

    #[test]
    fn parse_project_type_lib() {
        let json = r#"{"name": "mylib", "main": "lib/", "type": "lib"}"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.project_type, ProjectType::Lib);
    }

    #[test]
    fn parse_project_type_defaults_to_app() {
        let json = r#"{"name": "hello", "main": "main.fa"}"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.project_type, ProjectType::App);
    }

    #[test]
    fn validate_dependencies_github() {
        let json = r#"{"name": "test", "main": "main.fa", "dependencies": {"@user/repo": "^1.0.0"}}"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        assert!(validate_dependencies(&config).is_ok());
    }

    #[test]
    fn validate_dependencies_file() {
        let json = r#"{"name": "test", "main": "main.fa", "dependencies": {"mylib": "file:../mylib/"}}"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        assert!(validate_dependencies(&config).is_ok());
    }

    #[test]
    fn validate_dependencies_git_url() {
        let json = r#"{"name": "test", "main": "main.fa", "dependencies": {"requests": "git+https://somesite.com/repo.git#^1.0.0"}}"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        assert!(validate_dependencies(&config).is_ok());
    }

    #[test]
    fn validate_dependencies_bad_github_name() {
        let json = r#"{"name": "test", "main": "main.fa", "dependencies": {"badname": "1.0.0"}}"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        let err = validate_dependencies(&config).unwrap_err();
        assert!(err.contains("@user/repo"));
    }

    #[test]
    fn validate_dependencies_bad_version() {
        let json = r#"{"name": "test", "main": "main.fa", "dependencies": {"@user/repo": "xyz"}}"#;
        let config: ProjectConfig = serde_json::from_str(json).unwrap();
        let err = validate_dependencies(&config).unwrap_err();
        assert!(err.contains("cannot parse version requirement"));
    }
}
