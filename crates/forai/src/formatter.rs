// Re-export pure formatting functions from forai-core
pub use forai_core::formatter::*;

use std::fs;
use std::path::{Path, PathBuf};

/// Format all `.fa` files under a path (file or directory).
/// Returns (formatted_files, total_files).
pub fn fmt_path(path: &Path, check_only: bool) -> Result<(Vec<PathBuf>, usize), String> {
    let files = collect_fa_files(path)?;
    let total = files.len();
    let mut changed = Vec::new();

    for file in &files {
        let source = fs::read_to_string(file)
            .map_err(|e| format!("failed to read {}: {e}", file.display()))?;
        let formatted = format_source(&source);
        if formatted != source {
            changed.push(file.clone());
            if !check_only {
                fs::write(file, &formatted)
                    .map_err(|e| format!("failed to write {}: {e}", file.display()))?;
            }
        }
    }

    Ok((changed, total))
}

fn collect_fa_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        if path.extension().and_then(|s| s.to_str()) == Some("fa") {
            return Ok(vec![path.to_path_buf()]);
        }
        return Err(format!("{} is not a .fa file", path.display()));
    }
    if !path.is_dir() {
        return Err(format!("{} does not exist", path.display()));
    }
    let mut files = Vec::new();
    collect_fa_recursive(path, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_fa_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir)
        .map_err(|e| format!("failed to read directory {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("directory entry error: {e}"))?;
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "dist" || name == "node_modules" || name == "docs" {
                continue;
            }
            collect_fa_recursive(&path, files)?;
        } else if path.extension().and_then(|s| s.to_str()) == Some("fa") {
            files.push(path);
        }
    }
    Ok(())
}
