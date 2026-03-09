// Shared types from forai-core
mod ast {
    pub use forai_core::ast::*;
}
mod codec {
    pub use forai_core::codec::*;
}
mod host {
    pub use forai_core::host::*;
}
mod ir {
    pub use forai_core::ir::*;
}
mod types;

// Loader: types from core, loading logic local
mod loader;

// Compiler-only modules (not in core)
mod cli;
mod config;
mod debugger;
mod deps;
mod doc;
mod ffi_manager;
mod formatter;
mod host_native;
mod lsp;
mod mcp;
mod parser {
    pub use forai_core::parser::*;
}
mod runtime;
mod sema {
    pub use forai_core::sema::*;
}
mod stdlib_docs;
mod tester;
mod ui_gtk;
#[cfg(feature = "linux-gtk")]
mod ui_gtk_backend;
mod ui_layout;
mod ui_render;
mod typecheck {
    pub use forai_core::typecheck::*;
}
mod wasm_runner;

use crate::ast::{Flow, Statement, TopDecl};
use crate::cli::{CliCommand, parse_cli, usage};
use crate::codec::CodecRegistry;
use crate::ir::Ir;
use crate::loader::FlowRegistry;
use crate::types::TypeRegistry;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

// --- Native bundle (self-extracting executable) ---
// V2 format: [ exe bytes ][ compressed .wasm bytes ][ compressed_len: u64 LE ][ flags: u8 ][ FORAI_BUNDLE_V2\0 ]
const BUNDLE_MAGIC: &[u8; 16] = b"FORAI_BUNDLE_V2\0";
const BUNDLE_FLAGS_COMPRESSED: u8 = 0x01;
const BUNDLE_FOOTER_SIZE: usize = 16 + 1 + 8; // magic + flags + u64 length

fn extract_embedded_bundle() -> Option<Vec<u8>> {
    let exe_path = std::env::current_exe().ok()?;
    let exe_bytes = fs::read(&exe_path).ok()?;
    if exe_bytes.len() < BUNDLE_FOOTER_SIZE {
        return None;
    }
    let magic = &exe_bytes[exe_bytes.len() - 16..];
    if magic != BUNDLE_MAGIC {
        return None;
    }
    let flags = exe_bytes[exe_bytes.len() - 17];
    let len_start = exe_bytes.len() - 25;
    let len_bytes: [u8; 8] = exe_bytes[len_start..len_start + 8].try_into().ok()?;
    let payload_len = u64::from_le_bytes(len_bytes) as usize;
    let payload_start = len_start.checked_sub(payload_len)?;
    let payload = &exe_bytes[payload_start..len_start];

    if flags & BUNDLE_FLAGS_COMPRESSED != 0 {
        use flate2::read::GzDecoder;
        use std::io::Read;
        let mut decoder = GzDecoder::new(payload);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).ok()?;
        Some(decompressed)
    } else {
        Some(payload.to_vec())
    }
}

fn create_native_bundle(wasm_path: &Path, out_path: &Path) -> Result<(), String> {
    let exe_path =
        std::env::current_exe().map_err(|e| format!("cannot locate own executable: {e}"))?;
    let exe_bytes =
        fs::read(&exe_path).map_err(|e| format!("failed to read {}: {e}", exe_path.display()))?;
    let wasm_bytes =
        fs::read(wasm_path).map_err(|e| format!("failed to read {}: {e}", wasm_path.display()))?;

    // Step 1: Write exe to output
    fs::write(out_path, &exe_bytes)
        .map_err(|e| format!("failed to write {}: {e}", out_path.display()))?;

    // Step 2: Strip debug symbols (best-effort)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        let _ = fs::set_permissions(out_path, perms);
    }
    let strip_result = if cfg!(target_os = "macos") {
        std::process::Command::new("strip")
            .arg("-x")
            .arg(out_path)
            .output()
    } else {
        std::process::Command::new("strip").arg(out_path).output()
    };
    match strip_result {
        Ok(o) if o.status.success() => {
            let orig_size = exe_bytes.len();
            let stripped_size = fs::metadata(out_path).map(|m| m.len()).unwrap_or(0) as usize;
            if stripped_size < orig_size {
                eprintln!(
                    "  strip    saved {:.1} MB",
                    (orig_size - stripped_size) as f64 / 1_048_576.0
                );
            }
        }
        _ => eprintln!("  strip    skipped (not available)"),
    }

    // Step 3: Compress WASM payload with gzip
    let compressed = {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
        encoder
            .write_all(&wasm_bytes)
            .map_err(|e| format!("gzip compression failed: {e}"))?;
        encoder
            .finish()
            .map_err(|e| format!("gzip finish failed: {e}"))?
    };

    // Step 4: Append compressed WASM + footer
    let compressed_len = compressed.len() as u64;
    let flags: u8 = BUNDLE_FLAGS_COMPRESSED;
    let mut footer = Vec::with_capacity(compressed.len() + BUNDLE_FOOTER_SIZE);
    footer.extend_from_slice(&compressed);
    footer.extend_from_slice(&compressed_len.to_le_bytes());
    footer.push(flags);
    footer.extend_from_slice(BUNDLE_MAGIC);

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(out_path)
        .map_err(|e| format!("failed to open {} for append: {e}", out_path.display()))?;
    file.write_all(&footer)
        .map_err(|e| format!("failed to append bundle data: {e}"))?;

    // chmod +x on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        fs::set_permissions(out_path, perms)
            .map_err(|e| format!("failed to set permissions on {}: {e}", out_path.display()))?;
    }

    Ok(())
}

// --- Native bundle V3 (self-extracting with async runtime) ---
// V3 format: [ runner exe ][ compressed bundle JSON ][ len: u64 LE ][ flags: u8 ][ FORAI_BUNDLE_V3\0 ]
const BUNDLE_V3_MAGIC: &[u8; 16] = b"FORAI_BUNDLE_V3\0";
const BUNDLE_V3_FLAGS_COMPRESSED: u8 = 0x01;

fn create_native_v3_bundle_with_runner(
    bundle_json: &[u8],
    out_path: &Path,
    runner_path: &Path,
) -> Result<(), String> {
    let runner_bytes = fs::read(&runner_path).map_err(|e| {
        format!(
            "failed to read runner binary {}: {e}",
            runner_path.display()
        )
    })?;

    // Write runner binary to output
    fs::write(out_path, &runner_bytes)
        .map_err(|e| format!("failed to write {}: {e}", out_path.display()))?;

    // Strip debug symbols (best-effort)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        let _ = fs::set_permissions(out_path, perms);
    }
    let strip_result = if cfg!(target_os = "macos") {
        std::process::Command::new("strip")
            .arg("-x")
            .arg(out_path)
            .output()
    } else {
        std::process::Command::new("strip").arg(out_path).output()
    };
    match strip_result {
        Ok(o) if o.status.success() => {
            let orig_size = runner_bytes.len();
            let stripped_size = fs::metadata(out_path).map(|m| m.len()).unwrap_or(0) as usize;
            if stripped_size < orig_size {
                eprintln!(
                    "  strip    saved {:.1} MB",
                    (orig_size - stripped_size) as f64 / 1_048_576.0
                );
            }
        }
        _ => eprintln!("  strip    skipped (not available)"),
    }

    // Compress bundle JSON with gzip
    let compressed = {
        use flate2::Compression;
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::best());
        encoder
            .write_all(bundle_json)
            .map_err(|e| format!("gzip compression failed: {e}"))?;
        encoder
            .finish()
            .map_err(|e| format!("gzip finish failed: {e}"))?
    };

    // Append compressed bundle + V3 footer
    let compressed_len = compressed.len() as u64;
    let flags: u8 = BUNDLE_V3_FLAGS_COMPRESSED;
    let mut footer = Vec::with_capacity(compressed.len() + BUNDLE_FOOTER_SIZE);
    footer.extend_from_slice(&compressed);
    footer.extend_from_slice(&compressed_len.to_le_bytes());
    footer.push(flags);
    footer.extend_from_slice(BUNDLE_V3_MAGIC);

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(out_path)
        .map_err(|e| format!("failed to open {} for append: {e}", out_path.display()))?;
    file.write_all(&footer)
        .map_err(|e| format!("failed to append bundle data: {e}"))?;

    // chmod +x on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        fs::set_permissions(out_path, perms)
            .map_err(|e| format!("failed to set permissions on {}: {e}", out_path.display()))?;
    }

    Ok(())
}

fn create_native_v3_bundle(bundle_json: &[u8], out_path: &Path) -> Result<(), String> {
    let runner_path = find_runner_binary()?;
    create_native_v3_bundle_with_runner(bundle_json, out_path, &runner_path)
}

fn find_runner_binary() -> Result<PathBuf, String> {
    // 1. Environment variable
    if let Ok(path) = std::env::var("FORAI_RUNNER") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    // 2. Adjacent to the current binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let adjacent = dir.join("forai-runner");
            if adjacent.exists() {
                return Ok(adjacent);
            }
        }
    }

    // 3. Workspace target directory (development builds)
    let dev_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/release/forai-runner");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    let dev_debug =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/forai-runner");
    if dev_debug.exists() {
        return Ok(dev_debug);
    }

    // 4. Auto-build the runner
    match build_runner_binary() {
        Ok(p) => Ok(p),
        Err(e) => Err(format!("forai-runner not found and auto-build failed: {e}")),
    }
}

fn build_runner_binary() -> Result<PathBuf, String> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    eprintln!("  native   building runner...");
    let output = std::process::Command::new("cargo")
        .args(["build", "-p", "forai-runner", "--release"])
        .current_dir(&workspace_root)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    if !output.status.success() {
        return Err("cargo build failed for forai-runner".to_string());
    }
    let runner_path = workspace_root.join("target/release/forai-runner");
    if runner_path.exists() {
        Ok(runner_path)
    } else {
        Err("forai-runner build succeeded but output file not found".to_string())
    }
}

fn build_runner_binary_with_features(features: &[&str]) -> Result<PathBuf, String> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    eprintln!(
        "  linux-ui building runner (features: {})...",
        features.join(",")
    );
    let mut cmd = std::process::Command::new("cargo");
    cmd.args(["build", "-p", "forai-runner", "--release", "--features"]);
    cmd.arg(features.join(","));
    let output = cmd
        .current_dir(&workspace_root)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    if !output.status.success() {
        return Err("cargo build failed for forai-runner with requested features".to_string());
    }
    let runner_path = workspace_root.join("target/release/forai-runner");
    if runner_path.exists() {
        Ok(runner_path)
    } else {
        Err("forai-runner feature build succeeded but output file not found".to_string())
    }
}

fn collect_ops(statements: &[Statement], out: &mut Vec<String>) {
    for stmt in statements {
        match stmt {
            Statement::Node(n) => out.push(n.op.clone()),
            Statement::ExprAssign(_) => {}
            Statement::Case(c) => {
                for arm in &c.arms {
                    collect_ops(&arm.body, out);
                }
                collect_ops(&c.else_body, out);
            }
            Statement::Loop(l) => collect_ops(&l.body, out),
            Statement::Sync(s) => collect_ops(&s.body, out),
            Statement::Emit(_) => {}
            Statement::SendNowait(sn) => out.push(sn.target.clone()),
            Statement::Break | Statement::Continue => {}
            Statement::BareLoop(b) => collect_ops(&b.body, out),
            Statement::SourceLoop(sl) => {
                out.push(sl.source_op.clone());
                collect_ops(&sl.body, out);
            }
            Statement::On(on_block) => {
                out.push(on_block.source_op.clone());
                collect_ops(&on_block.body, out);
            }
        }
    }
}

/// Try to resolve project dependencies from a source path.
/// Walks up to find forai.json; if found and it has dependencies, resolves them.
/// Returns empty deps if no config or no dependencies.
fn resolve_deps_for_source(source_path: &Path) -> deps::ResolvedDeps {
    let dir = if source_path.is_file() {
        source_path.parent().unwrap_or(Path::new("."))
    } else {
        source_path
    };
    if let Ok((cfg, root)) = config::find_config(dir) {
        if !cfg.dependencies.is_empty() {
            if let Ok(rd) = deps::resolve_dependencies(&cfg, &root) {
                return rd;
            }
        }
    }
    deps::ResolvedDeps::empty()
}

fn format_unknown_op(file: &Path, op: &str) -> String {
    let base = format!("{}: unknown op `{op}`", file.display());
    if let Some(hint) = sema::unknown_op_fix_hint(op) {
        format!("{base} — {hint}")
    } else {
        base
    }
}

pub(crate) fn compile_source(
    source_path: &PathBuf,
    resolved_deps: &deps::ResolvedDeps,
) -> Result<
    (
        Flow,
        Ir,
        TypeRegistry,
        FlowRegistry,
        ffi_manager::FfiRegistry,
    ),
    String,
> {
    if source_path.extension().and_then(|s| s.to_str()) != Some("fa") {
        return Err(format!(
            "Source must use the .fa extension, got `{}`",
            source_path.display()
        ));
    }

    let text = fs::read_to_string(source_path)
        .map_err(|e| format!("Failed to read {}: {e}", source_path.display()))?;

    let module = parser::parse_module_v1(&text).map_err(|e| {
        format!(
            "{}:{}:{} {}",
            source_path.display(),
            e.span.line,
            e.span.col,
            e.message
        )
    })?;

    let filename = source_path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string());
    if let Err(errors) = sema::validate_module(&module, filename.as_deref()) {
        let rendered = errors
            .into_iter()
            .map(|e| format!("{}:{e}", source_path.display()))
            .collect::<Vec<String>>()
            .join("\n");
        return Err(rendered);
    }

    let registry = types::TypeRegistry::from_module(&module).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| format!("{}:{e}", source_path.display()))
            .collect::<Vec<String>>()
            .join("\n")
    })?;

    let flow_registry = loader::build_flow_registry(source_path, &module, resolved_deps)?;

    let mut ffi_registry = ffi_manager::build_ffi_registry(&module);
    ffi_registry.merge(loader::collect_imported_ffi(
        source_path,
        &module,
        resolved_deps,
    ));

    let flow = parser::parse_runtime_func_from_module_v1(&module)
        .map_err(|e| format!("{}:{e}", source_path.display()))?;

    let codec_registry = CodecRegistry::default_registry();
    let mut known: HashSet<&str> = runtime::known_ops().iter().copied().collect();
    let codec_ops = codec_registry.known_ops();
    for cop in &codec_ops {
        known.insert(cop.as_str());
    }
    // Add FFI ops to known set
    let ffi_op_keys: Vec<String> = ffi_registry.op_keys().cloned().collect();
    for ffi_op in &ffi_op_keys {
        known.insert(ffi_op.as_str());
    }
    let mut ops = Vec::new();
    collect_ops(&flow.body, &mut ops);
    let unknown: Vec<_> = ops
        .iter()
        .filter(|op| !known.contains(op.as_str()) && !flow_registry.is_flow(op))
        .collect();
    if !unknown.is_empty() {
        let rendered = unknown
            .iter()
            .map(|op| format_unknown_op(source_path, op))
            .collect::<Vec<String>>()
            .join("\n");
        return Err(rendered);
    }

    // Transform entry flow body: wrap source calls into SourceLoop blocks
    let source_names: std::collections::HashSet<String> = flow_registry
        .iter()
        .filter(|(_, p)| p.kind == ast::DeclKind::Source)
        .map(|(name, _)| name.clone())
        .collect();
    let mut flow = flow;
    if !source_names.is_empty() {
        flow.body = loader::transform_source_steps(&flow.body, &source_names);
    }

    let ir = ir::lower_to_ir(&flow)?;
    Ok((flow, ir, registry, flow_registry, ffi_registry))
}

/// Embed a program bundle into a pre-built WASM runtime binary.
/// Appends the bundle as a custom section named "forai_program".
fn embed_bundle_in_wasm(bundle_json: &[u8], out_path: &Path) -> Result<(), String> {
    let runtime_wasm = find_runtime_wasm()?;

    let mut wasm = fs::read(&runtime_wasm).map_err(|e| {
        format!(
            "failed to read runtime WASM {}: {e}",
            runtime_wasm.display()
        )
    })?;

    // Validate WASM magic number
    if wasm.len() < 8 || &wasm[0..4] != b"\0asm" {
        return Err(format!(
            "{} is not a valid WASM module",
            runtime_wasm.display()
        ));
    }

    // Append a custom section (id=0) with name "forai_program"
    let name = b"forai_program";
    let content_size = leb128_size(name.len() as u64) + name.len() + bundle_json.len();

    wasm.push(0x00); // custom section id
    leb128_encode(&mut wasm, content_size as u64);
    leb128_encode(&mut wasm, name.len() as u64);
    wasm.extend_from_slice(name);
    wasm.extend_from_slice(bundle_json);

    fs::write(out_path, &wasm)
        .map_err(|e| format!("failed to write WASM to {}: {e}", out_path.display()))?;

    Ok(())
}

fn leb128_encode(buf: &mut Vec<u8>, mut value: u64) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn leb128_size(mut value: u64) -> usize {
    let mut size = 0;
    loop {
        value >>= 7;
        size += 1;
        if value == 0 {
            break;
        }
    }
    size
}

fn find_runtime_wasm() -> Result<PathBuf, String> {
    // 1. Environment variable
    if let Ok(path) = std::env::var("DATAFLOW_WASM_RUNTIME") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    // 2. Adjacent to the current binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let adjacent = dir.join("forai-wasm.wasm");
            if adjacent.exists() {
                return Ok(adjacent);
            }
        }
    }

    // 3. In a workspace/dev context, build runtime to ensure op table stays in sync
    if let Ok(p) = build_wasm_runtime() {
        return Ok(p);
    }

    // 4. Fallback to any existing workspace target artifact
    let dev_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-wasip1/release/forai-wasm.wasm");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    let dev_debug = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/wasm32-wasip1/debug/forai-wasm.wasm");
    if dev_debug.exists() {
        return Ok(dev_debug);
    }

    Err("WASM runtime not found (and runtime rebuild failed)".to_string())
}

fn build_wasm_runtime() -> Result<PathBuf, String> {
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    eprintln!("  wasm     building runtime...");
    let output = std::process::Command::new("cargo")
        .args([
            "build",
            "-p",
            "forai-wasm",
            "--target",
            "wasm32-wasip1",
            "--release",
        ])
        .current_dir(&workspace_root)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    if !output.status.success() {
        return Err("cargo build failed for forai-wasm".to_string());
    }
    let wasm_path = workspace_root.join("target/wasm32-wasip1/release/forai-wasm.wasm");
    if wasm_path.exists() {
        Ok(wasm_path)
    } else {
        Err("WASM runtime build succeeded but output file not found".to_string())
    }
}

fn find_browser_js() -> Result<PathBuf, String> {
    // 1. Environment variable
    if let Ok(path) = std::env::var("DATAFLOW_BROWSER_JS") {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    // 2. Adjacent to current binary
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let adjacent = dir.join("forai-browser.js");
            if adjacent.exists() {
                return Ok(adjacent);
            }
        }
    }

    // 3. Workspace browser/dist directory (development builds)
    let dev_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../browser/dist/forai-browser.js");
    if dev_path.exists() {
        return Ok(dev_path);
    }

    // 4. Auto-build the browser JS runtime
    match build_browser_js() {
        Ok(p) => Ok(p),
        Err(e) => Err(format!(
            "Browser JS runtime not found and auto-build failed: {e}"
        )),
    }
}

fn build_browser_js() -> Result<PathBuf, String> {
    let browser_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../browser");
    if !browser_dir.join("package.json").exists() {
        return Err(
            "Browser JS runtime not found and browser/ directory not available".to_string(),
        );
    }
    eprintln!("  browser  building JS runtime...");
    let output = std::process::Command::new("npm")
        .args(["run", "build"])
        .current_dir(&browser_dir)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("failed to run npm: {e}"))?;
    if !output.status.success() {
        return Err("npm run build failed for browser JS".to_string());
    }
    let js_path = browser_dir.join("dist/forai-browser.js");
    if js_path.exists() {
        Ok(js_path)
    } else {
        Err("Browser JS build succeeded but output file not found".to_string())
    }
}

fn create_browser_target(
    wasm_path: &Path,
    browser_dir: &Path,
    name: &str,
    project_root: &Path,
) -> Result<(), String> {
    // Create browser output directory
    fs::create_dir_all(browser_dir)
        .map_err(|e| format!("failed to create {}: {e}", browser_dir.display()))?;

    // Copy WASM directly (no Asyncify transform — Worker+SAB architecture handles async ops)
    let dest_wasm = browser_dir.join(format!("{name}.wasm"));
    fs::copy(wasm_path, &dest_wasm).map_err(|e| format!("failed to copy WASM: {e}"))?;
    let wasm_size = fs::metadata(&dest_wasm).map(|m| m.len()).unwrap_or(0);
    eprintln!("  wasm ok ({:.1} MB)", wasm_size as f64 / 1_048_576.0);

    // Copy the pre-built browser JS runtime
    let browser_js = find_browser_js()?;
    let dest_js = browser_dir.join("forai-browser.js");
    fs::copy(&browser_js, &dest_js).map_err(|e| format!("failed to copy browser JS: {e}"))?;

    // Also copy sourcemap if it exists
    let sourcemap = browser_js.with_extension("js.map");
    if sourcemap.exists() {
        let dest_map = browser_dir.join("forai-browser.js.map");
        let _ = fs::copy(&sourcemap, &dest_map);
    }

    // Use public/index.html if it exists, otherwise generate a default one
    let html_path = browser_dir.join("index.html");
    let custom_html = project_root.join("public/index.html");
    if custom_html.exists() {
        fs::copy(&custom_html, &html_path)
            .map_err(|e| format!("failed to copy public/index.html: {e}"))?;
    } else {
        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>{name}</title>
  <style>
    body {{ font-family: monospace; background: #1a1a2e; color: #e0e0e0; padding: 2rem; }}
    #output {{ white-space: pre-wrap; line-height: 1.6; }}
    .error {{ color: #ff6b6b; }}
  </style>
</head>
<body>
  <div id="output"></div>
  <script type="module">
    import {{ run }} from "./forai-browser.js";
    const output = document.getElementById("output");
    function print(text) {{
      output.textContent += text + "\n";
    }}
    if (typeof crossOriginIsolated !== "undefined" && !crossOriginIsolated) {{
      print("Error: This page requires cross-origin isolation for SharedArrayBuffer.");
      print("Serve with headers:");
      print("  Cross-Origin-Opener-Policy: same-origin");
      print("  Cross-Origin-Embedder-Policy: require-corp");
    }} else {{
      try {{
        await run({{
          wasmUrl: "./{name}.wasm",
          onPrint: print,
          onStdout: (text) => print(text.trimEnd()),
          onStderr: (text) => {{ const span = document.createElement("span"); span.className = "error"; span.textContent = text; output.appendChild(span); }},
        }});
      }} catch (e) {{
        print("Error: " + e.message);
      }}
    }}
  </script>
</body>
</html>
"#
        );
        fs::write(&html_path, html).map_err(|e| format!("failed to write index.html: {e}"))?;
    }

    Ok(())
}

fn create_linux_ui_target(
    out_dir: &Path,
    project_name: &str,
    bundle_json: &[u8],
) -> Result<(), String> {
    let linux_dir = out_dir.join("linux-ui");
    fs::create_dir_all(&linux_dir)
        .map_err(|e| format!("failed to create {}: {e}", linux_dir.display()))?;

    let runner_with_gtk = build_runner_binary_with_features(&["linux-gtk"])?;
    let linux_bin = linux_dir.join(project_name);
    create_native_v3_bundle_with_runner(bundle_json, &linux_bin, &runner_with_gtk)?;

    let run_sh = linux_dir.join("run-linux-ui.sh");
    let script = format!(
        "#!/usr/bin/env bash\nset -euo pipefail\n\nDIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\nFORAI_UI_BACKEND=\"${{FORAI_UI_BACKEND:-gtk}}\" exec \"$DIR/{}\" \"$@\"\n",
        project_name
    );
    fs::write(&run_sh, script).map_err(|e| format!("failed to write {}: {e}", run_sh.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&run_sh)
            .map_err(|e| format!("failed to read {} metadata: {e}", run_sh.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&run_sh, perms)
            .map_err(|e| format!("failed to set executable bit on {}: {e}", run_sh.display()))?;
    }

    let readme = linux_dir.join("README.md");
    let text = format!(
        "# Linux UI Target\n\nGenerated Linux-native executable and launcher.\n\n## Run\n\n```bash\n{0}/{1}\n```\n\nOr use the wrapper script:\n\n```bash\n{0}/run-linux-ui.sh\n```\n\nThe wrapper sets `FORAI_UI_BACKEND=gtk` by default and executes the bundled binary.\n",
        linux_dir.display(),
        project_name
    );
    fs::write(&readme, text).map_err(|e| format!("failed to write {}: {e}", readme.display()))?;

    Ok(())
}

fn generate_docs_for_source(source_path: &Path, project_root: Option<&Path>) {
    let discovered;
    let root = match project_root {
        Some(r) => r,
        None => {
            let start = source_path.parent().unwrap_or(Path::new("."));
            discovered = match config::find_config(start) {
                Ok((_cfg, root)) => root,
                Err(_) => source_path
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| Path::new(".").to_path_buf()),
            };
            &discovered
        }
    };
    let text = match fs::read_to_string(source_path) {
        Ok(t) => t,
        Err(_) => return,
    };
    let module = match parser::parse_module_v1(&text) {
        Ok(m) => m,
        Err(_) => return,
    };
    if let Err(e) = doc::generate_docs_folder(root, source_path, &module) {
        eprintln!("warning: docs generation failed: {e}");
    }
}

async fn run() -> Result<(), String> {
    match parse_cli()? {
        CliCommand::Version => {
            println!("forai {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        CliCommand::Help => {
            println!("{}", usage());
            Ok(())
        }
        CliCommand::Build { dir, debug: _ } => {
            let dir = if dir.is_relative() {
                std::env::current_dir()
                    .map_err(|e| format!("cannot resolve working directory: {e}"))?
                    .join(&dir)
            } else {
                dir
            };

            let (config, project_root) = config::load_config(&dir)?;
            config::check_version(&config)?;

            let source_path = project_root.join(&config.main);
            if !source_path.exists() {
                return Err(format!(
                    "main entry point not found: {} (from forai.json \"main\": {:?})",
                    source_path.display(),
                    config.main
                ));
            }

            eprintln!("building {} v{}\n", config.name, config.version);

            // Step 0: Format
            let src_dir = source_path.parent().unwrap_or(&project_root);
            let (formatted, fmt_total) = formatter::fmt_path(src_dir, false)?;
            if !formatted.is_empty() {
                eprintln!(
                    "  fmt      {} reformatted ({} files)",
                    formatted.len(),
                    fmt_total
                );
            } else {
                eprintln!("  fmt      ok ({} files)", fmt_total);
            }

            // Step 0.5: Resolve dependencies
            let resolved_deps = if config.dependencies.is_empty() {
                deps::ResolvedDeps::empty()
            } else {
                config::validate_dependencies(&config)?;
                eprintln!("  deps     resolving...");
                let rd = deps::resolve_dependencies(&config, &project_root)?;
                eprintln!("  deps     ok ({} packages)", rd.packages.len());
                rd
            };

            // Step 1: Compile and generate docs
            let (flow, ir, registry, flow_registry, ffi_registry) =
                compile_source(&source_path, &resolved_deps)?;
            generate_docs_for_source(&source_path, Some(&project_root));
            eprintln!("  docs     ok ({} files)", fmt_total);

            // Reject WASM targets when FFI extern blocks are present
            if !ffi_registry.is_empty() {
                let has_wasm_target = config
                    .build
                    .targets
                    .iter()
                    .any(|t| t == "wasm" || t == "bundle" || t == "browser");
                if has_wasm_target {
                    return Err(
                        "project uses `extern` FFI blocks which are incompatible with WASM targets.\n\
                         Remove WASM from build.targets in forai.json, or guard FFI calls with ffi.available().".to_string()
                    );
                }
            }

            // Step 2: Write IR to output directory
            let out_dir = project_root.join(&config.build.out);
            fs::create_dir_all(&out_dir)
                .map_err(|e| format!("failed to create output dir {}: {e}", out_dir.display()))?;

            let ir_path = out_dir.join(format!("{}.ir.json", config.name));
            let rendered = serde_json::to_string_pretty(&ir).map_err(|e| e.to_string())?;
            fs::write(&ir_path, format!("{rendered}\n"))
                .map_err(|e| format!("failed to write {}: {e}", ir_path.display()))?;

            // Step 3: Run tests (quiet: suppress per-test output, collect warnings)
            let summary = tester::run_tests_at_path_build(&project_root).await?;
            if summary.total > 0 {
                if summary.failed > 0 {
                    eprintln!(
                        "  test     FAILED ({} passed, {} failed)",
                        summary.passed, summary.failed
                    );
                    for f in &summary.failures {
                        eprintln!("      FAIL  {}", f.name);
                        eprintln!("            > {}", f.error);
                    }
                    return Err(format!("{} test(s) failed", summary.failed));
                } else {
                    eprintln!("  test     ok ({} tests)", summary.passed);
                }
            }
            if !summary.warnings.is_empty() {
                eprintln!();
                for w in &summary.warnings {
                    eprintln!("WARN  {w}");
                }
            }

            // Step 4: Build targets
            let targets = &config.build.targets;
            let needs_wasm = targets
                .iter()
                .any(|t| t == "wasm" || t == "bundle" || t == "browser");
            let mut target_failures: Vec<String> = Vec::new();

            let ffi_json = if ffi_registry.is_empty() {
                None
            } else {
                Some(
                    serde_json::to_value(&ffi_registry)
                        .map_err(|e| format!("failed to serialize ffi_registry: {e}"))?,
                )
            };
            let bundle = forai_core::loader::ProgramBundle {
                entry_flow: flow,
                entry_ir: ir,
                type_registry: registry,
                flow_registry,
                ffi_registry: ffi_json,
            };
            let bundle_json = serde_json::to_vec(&bundle)
                .map_err(|e| format!("failed to serialize program bundle: {e}"))?;

            let wasm_path = out_dir.join(format!("{}.wasm", config.name));
            let wasm_ok = if needs_wasm {
                match embed_bundle_in_wasm(&bundle_json, &wasm_path) {
                    Ok(()) => {
                        let size = fs::metadata(&wasm_path).map(|m| m.len()).unwrap_or(0);
                        eprintln!("  wasm     ok ({:.1} MB)", size as f64 / 1_048_576.0);
                        true
                    }
                    Err(e) => {
                        eprintln!("  wasm     FAILED: {e}");
                        target_failures.push("wasm".to_string());
                        false
                    }
                }
            } else {
                false
            };

            if targets.contains(&"bundle".to_string()) {
                if wasm_ok {
                    let bundle_path = out_dir.join(&config.name);
                    match create_native_bundle(&wasm_path, &bundle_path) {
                        Ok(()) => {
                            let size = fs::metadata(&bundle_path).map(|m| m.len()).unwrap_or(0);
                            eprintln!("  bundle   ok ({:.1} MB)", size as f64 / 1_048_576.0);
                        }
                        Err(e) => {
                            eprintln!("  bundle   FAILED: {e}");
                            target_failures.push("bundle".to_string());
                        }
                    }
                } else {
                    eprintln!("  bundle   skipped (requires wasm)");
                    target_failures.push("bundle".to_string());
                }
            }

            if targets.contains(&"native".to_string()) {
                let native_path = out_dir.join(&config.name);
                match create_native_v3_bundle(&bundle_json, &native_path) {
                    Ok(()) => {
                        let size = fs::metadata(&native_path).map(|m| m.len()).unwrap_or(0);
                        eprintln!("  native   ok ({:.1} MB)", size as f64 / 1_048_576.0);
                    }
                    Err(e) => {
                        eprintln!("  native   FAILED: {e}");
                        target_failures.push("native".to_string());
                    }
                }
            }

            if targets.contains(&"browser".to_string()) {
                if wasm_ok {
                    let browser_dir = out_dir.join("browser");
                    match create_browser_target(
                        &wasm_path,
                        &browser_dir,
                        &config.name,
                        &project_root,
                    ) {
                        Ok(()) => {
                            eprintln!("  browser  ok");
                        }
                        Err(e) => {
                            eprintln!("  browser  FAILED: {e}");
                            target_failures.push("browser".to_string());
                        }
                    }
                } else {
                    eprintln!("  browser  skipped (requires wasm)");
                    target_failures.push("browser".to_string());
                }
            }

            if targets.contains(&"linux-ui".to_string()) {
                match create_linux_ui_target(&out_dir, &config.name, &bundle_json) {
                    Ok(()) => eprintln!("  linux-ui ok"),
                    Err(e) => {
                        eprintln!("  linux-ui FAILED: {e}");
                        target_failures.push("linux-ui".to_string());
                    }
                }
            }

            let docs_dir = project_root.join("docs");
            if target_failures.is_empty() {
                eprintln!("\n  build    ok");
                eprintln!("  output   {}", out_dir.display());
                eprintln!("  docs     {}", docs_dir.display());
                Ok(())
            } else {
                eprintln!("\n  output   {}", out_dir.display());
                eprintln!("  docs     {}", docs_dir.display());
                Err(format!(
                    "build failed: {} target(s) failed ({})",
                    target_failures.len(),
                    target_failures.join(", ")
                ))
            }
        }
        CliCommand::Check { path } => {
            let path = if path.is_relative() {
                std::env::current_dir()
                    .map_err(|e| format!("cannot resolve working directory: {e}"))?
                    .join(&path)
            } else {
                path
            };
            // When path is ".", walk up to find project root
            let path = {
                let cwd = std::env::current_dir().unwrap_or_else(|_| path.clone());
                if path == cwd {
                    match config::find_config(&path) {
                        Ok((_, root)) => root,
                        Err(_) => path,
                    }
                } else {
                    path
                }
            };

            let mut files = mcp::collect_fa_files(&path);
            if path.is_file() {
                mcp::expand_with_imports(&mut files);
            }
            if files.is_empty() {
                return Err(format!("no .fa files found at {}", path.display()));
            }

            // Format first, then check
            let fmt_changed = match formatter::fmt_path(&path, false) {
                Ok((changed, _)) => changed,
                Err(e) => {
                    eprintln!("fmt: {e}");
                    vec![]
                }
            };
            if !fmt_changed.is_empty() {
                for f in &fmt_changed {
                    eprintln!("formatted {}", f.display());
                }
                // Re-collect files after formatting
                files = mcp::collect_fa_files(&path);
                if path.is_file() {
                    mcp::expand_with_imports(&mut files);
                }
            }

            let mut errors: Vec<String> = Vec::new();
            let mut warnings: Vec<String> = Vec::new();
            let mut ok_count = 0;

            for file in &files {
                let text = match fs::read_to_string(file) {
                    Ok(t) => t,
                    Err(e) => {
                        errors.push(format!("{}:0:0 failed to read: {e}", file.display()));
                        continue;
                    }
                };

                let module = match parser::parse_module_v1(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        errors.push(format!(
                            "{}:{}:{} {}",
                            file.display(),
                            e.span.line,
                            e.span.col,
                            e.message
                        ));
                        continue;
                    }
                };

                let filename = file
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string());
                if let Err(errs) = sema::validate_module(&module, filename.as_deref()) {
                    for e in errs {
                        errors.push(format!("{}:{e}", file.display()));
                    }
                    continue;
                }
                for w in sema::test_call_warnings(&module) {
                    warnings.push(format!("{}:{w}", file.display()));
                }

                let type_registry = match types::TypeRegistry::from_module(&module) {
                    Ok(r) => r,
                    Err(errs) => {
                        for e in errs {
                            errors.push(format!("{}:{e}", file.display()));
                        }
                        continue;
                    }
                };

                let mut file_ok = true;
                for decl in &module.decls {
                    match decl {
                        TopDecl::Func(f) | TopDecl::Sink(f) | TopDecl::Source(f) => {
                            match parser::parse_runtime_func_decl_v1(f) {
                                Err(e) => {
                                    errors.push(format!(
                                        "{}: func `{}` parse error: {e}",
                                        file.display(),
                                        f.name
                                    ));
                                    file_ok = false;
                                }
                                Ok(flow) => {
                                    if let Err(e) = typecheck::typecheck_func(
                                        &f.name,
                                        &f.takes,
                                        &flow.body,
                                        &f.emits,
                                        &f.fails,
                                        &type_registry,
                                    ) {
                                        errors.push(format!("{}:{e}", file.display()));
                                        file_ok = false;
                                    }
                                    if let Err(e) = ir::lower_to_ir(&flow) {
                                        errors.push(format!(
                                            "{}: func `{}` IR error: {e}",
                                            file.display(),
                                            f.name
                                        ));
                                        file_ok = false;
                                    }
                                }
                            }
                        }
                        TopDecl::Flow(f) => match parser::parse_flow_graph_decl_v1(f) {
                            Err(e) => {
                                errors.push(format!(
                                    "{}: flow `{}` parse error: {e}",
                                    file.display(),
                                    f.name
                                ));
                                file_ok = false;
                            }
                            Ok(graph) => match parser::lower_flow_graph_to_flow(&graph) {
                                Err(e) => {
                                    errors.push(format!(
                                        "{}: flow `{}` lower error: {e}",
                                        file.display(),
                                        f.name
                                    ));
                                    file_ok = false;
                                }
                                Ok(flow) => {
                                    if let Err(e) = ir::lower_to_ir(&flow) {
                                        errors.push(format!(
                                            "{}: flow `{}` IR error: {e}",
                                            file.display(),
                                            f.name
                                        ));
                                        file_ok = false;
                                    }
                                }
                            },
                        },
                        _ => {}
                    }
                }

                if file_ok {
                    ok_count += 1;
                }
            }

            for w in &warnings {
                eprintln!("warn: {w}");
            }

            if errors.is_empty() {
                println!("ok — {} file(s) checked", ok_count);
                Ok(())
            } else {
                for e in &errors {
                    eprintln!("{e}");
                }
                Err(format!("{} error(s)", errors.len()))
            }
        }
        CliCommand::Stdlib { query } => {
            let all_docs = stdlib_docs::all_stdlib_docs();
            let filtered: Vec<_> = match &query {
                None => all_docs,
                Some(q) => {
                    let q = q.to_lowercase();
                    all_docs
                        .into_iter()
                        .filter(|ns| {
                            ns.namespace.to_lowercase().contains(&q)
                                || ns
                                    .ops
                                    .iter()
                                    .any(|op| op.full_name.to_lowercase().contains(&q))
                        })
                        .collect()
                }
            };
            if filtered.is_empty() {
                return Err(format!(
                    "no stdlib entries matching \"{}\"",
                    query.as_deref().unwrap_or("")
                ));
            }
            let rendered = serde_json::to_string_pretty(&filtered).map_err(|e| e.to_string())?;
            println!("{rendered}");
            Ok(())
        }
        CliCommand::Compile {
            source,
            out,
            compact,
        } => {
            let (_flow, ir, _registry, _flow_registry, _ffi_registry) =
                compile_source(&source, &resolve_deps_for_source(&source))?;
            generate_docs_for_source(&source, None);
            let rendered = if compact {
                serde_json::to_string(&ir).map_err(|e| e.to_string())?
            } else {
                serde_json::to_string_pretty(&ir).map_err(|e| e.to_string())?
            };

            if let Some(out_path) = out {
                fs::write(&out_path, format!("{rendered}\n"))
                    .map_err(|e| format!("Failed to write {}: {e}", out_path.display()))?;
            } else {
                println!("{rendered}");
            }
            Ok(())
        }
        CliCommand::Run {
            source,
            input,
            report: report_path,
            args,
            debug,
            debug_port,
        } => {
            let source = match source {
                Some(p) => p,
                None => {
                    let cwd = std::env::current_dir()
                        .map_err(|e| format!("cannot resolve working directory: {e}"))?;
                    let (config, project_root) = config::find_config(&cwd)?;
                    project_root.join(&config.main)
                }
            };
            let (flow, ir, registry, flow_registry, ffi_registry) =
                compile_source(&source, &resolve_deps_for_source(&source))?;
            generate_docs_for_source(&source, None);

            if debug {
                let input_path = match input {
                    Some(p) => Some(p),
                    None => {
                        let stem = source.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                        let auto = source.with_file_name(format!("{stem}.input.json"));
                        if auto.exists() {
                            eprintln!("Auto-detected input: {}", auto.display());
                            Some(auto)
                        } else {
                            None
                        }
                    }
                };
                let inputs = runtime::load_inputs(&flow, input_path.as_ref())?;
                let source_text = fs::read_to_string(&source)
                    .map_err(|e| format!("Failed to read {}: {e}", source.display()))?;

                let module = parser::parse_module_v1(&source_text)
                    .map_err(|e| format!("{}:{e}", source.display()))?;
                let mut docs = HashMap::new();
                for decl in &module.decls {
                    if let TopDecl::Docs(d) = decl {
                        docs.insert(d.name.clone(), d.markdown.clone());
                    }
                }

                debugger::serve_dev_server(
                    debug_port,
                    flow,
                    ir,
                    inputs,
                    registry,
                    flow_registry,
                    source_text,
                    docs,
                )
            } else {
                let inputs = if !args.is_empty() {
                    runtime::load_inputs_from_args(&flow, &args)?
                } else {
                    runtime::load_inputs(&flow, input.as_ref())?
                };
                let codecs = CodecRegistry::default_registry();
                let native_host = host_native::NativeHost::new().with_ffi_registry(ffi_registry);
                let host: std::rc::Rc<dyn host::Host> = std::rc::Rc::new(native_host);
                let report = runtime::execute_flow(
                    &flow,
                    ir,
                    inputs,
                    &registry,
                    Some(&flow_registry),
                    &codecs,
                    Some(host),
                )
                .await?;

                if let Some(rp) = report_path {
                    let full_json =
                        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
                    fs::write(&rp, format!("{full_json}\n"))
                        .map_err(|e| format!("Failed to write report to {}: {e}", rp.display()))?;
                }

                Ok(())
            }
        }
        CliCommand::RunWasm { wasm_path } => wasm_runner::run_wasm(&wasm_path).await,
        CliCommand::Test { path } => {
            // When no explicit path given (defaults to "."), discover the project root
            let test_path = if path == PathBuf::from(".") {
                let cwd = std::env::current_dir().unwrap_or_else(|_| path.clone());
                match config::find_config(&cwd) {
                    Ok((_, root)) => root,
                    Err(_) => path,
                }
            } else {
                path
            };
            let summary = tester::run_tests_at_path_async(&test_path).await?;
            println!(
                "Tests: {} total, {} passed, {} failed",
                summary.total, summary.passed, summary.failed
            );
            if summary.failed > 0 {
                Err(format!("{} test(s) failed", summary.failed))
            } else {
                Ok(())
            }
        }
        CliCommand::Doc { path, out, query } => {
            let base_dir = if path.is_file() {
                path.parent().unwrap_or(Path::new(".")).to_path_buf()
            } else {
                path.clone()
            };
            if path.is_file() {
                generate_docs_for_source(&path, None);
            }
            let artifact = doc::generate_docs_at_path(&path, &base_dir)?;

            let rendered = if let Some(q) = &query {
                let q = q.to_lowercase();
                let filtered = serde_json::json!({
                    "dataflow_doc": artifact.dataflow_doc,
                    "modules": artifact.modules.iter()
                        .filter(|m| {
                            serde_json::to_string(m)
                                .unwrap_or_default()
                                .to_lowercase()
                                .contains(&q)
                        })
                        .collect::<Vec<_>>()
                });
                serde_json::to_string_pretty(&filtered).map_err(|e| e.to_string())?
            } else {
                serde_json::to_string_pretty(&artifact).map_err(|e| e.to_string())?
            };

            if let Some(out_path) = out {
                fs::write(&out_path, format!("{rendered}\n"))
                    .map_err(|e| format!("Failed to write {}: {e}", out_path.display()))?;
            } else {
                println!("{rendered}");
            }
            Ok(())
        }
        CliCommand::Fmt { path, check } => {
            let path = if path.is_relative() {
                std::env::current_dir()
                    .map_err(|e| format!("cannot resolve working directory: {e}"))?
                    .join(&path)
            } else {
                path
            };

            let (changed, total) = formatter::fmt_path(&path, check)?;

            if check {
                if changed.is_empty() {
                    eprintln!("all {} file(s) formatted", total);
                    Ok(())
                } else {
                    for f in &changed {
                        eprintln!("  needs formatting: {}", f.display());
                    }
                    Err(format!("{} file(s) need formatting", changed.len()))
                }
            } else {
                for f in &changed {
                    eprintln!("  formatted {}", f.display());
                }
                if changed.is_empty() {
                    eprintln!("all {} file(s) already formatted", total);
                } else {
                    eprintln!(
                        "{} file(s) formatted, {} already ok",
                        changed.len(),
                        total - changed.len()
                    );
                }
                Ok(())
            }
        }
        CliCommand::Lsp => {
            lsp::run_lsp();
            Ok(())
        }
        CliCommand::Mcp => {
            mcp::run_mcp().await;
            Ok(())
        }
        CliCommand::New { name } => scaffold_new_project(&name),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async {
            // Self-extracting bundle: if this binary has an embedded WASM payload, run it directly
            if let Some(wasm_bytes) = extract_embedded_bundle() {
                let args: Vec<String> = std::env::args().collect();
                if let Err(err) = wasm_runner::run_wasm_from_bytes(&wasm_bytes, args).await {
                    eprintln!("error: {err}");
                    std::process::exit(1);
                }
                return;
            }

            if let Err(err) = run().await {
                eprintln!("error: {err}");
                std::process::exit(1);
            }
        })
        .await;
}

// --- forai new: project scaffolding ---

const SCAFFOLD_LANGUAGE_MD: &str = include_str!("../../../LANGUAGE.md");

fn scaffold_new_project(name: &str) -> Result<(), String> {
    let root = PathBuf::from(name);
    if root.exists() {
        return Err(format!("directory `{name}` already exists"));
    }

    fs::create_dir_all(root.join("lib")).map_err(|e| format!("failed to create lib: {e}"))?;
    fs::create_dir_all(root.join("sinks")).map_err(|e| format!("failed to create sinks: {e}"))?;
    fs::create_dir_all(root.join("sources"))
        .map_err(|e| format!("failed to create sources: {e}"))?;

    // forai.json
    let forai_json = format!(
        r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "description": "A forai app",
  "main": "main.fa",
  "build": {{
    "targets": ["wasm", "bundle"]
  }}
}}
"#
    );
    write_scaffold(&root, "forai.json", &forai_json)?;

    // README.md
    let readme = format!(
        "# {name}\n\nA forai app.\n\n## Quick Start\n\n```bash\nforai run main.fa         # run the app\nforai test                # run all tests (scans entire project)\nforai build               # build WASM artifact\nforai dev main.fa         # interactive debugger\n```\n\nSee [LANGUAGE.md](LANGUAGE.md) for the full language reference.\n"
    );
    write_scaffold(&root, "README.md", &readme)?;

    // CLAUDE.md
    let claude_md = format!(
        "# CLAUDE.md\n\n## DO THIS FIRST\n\nforai is a new language with its own rules — read [LANGUAGE.md](LANGUAGE.md) before writing any code so you know what you're working with. It takes 2 minutes and will save you from common mistakes.\n\n---\n\nThis file provides guidance to Claude Code when working with this repository.\n\n## What This Is\n\n`{name}` is a forai app. forai is a dataflow programming language where you wire sources, funcs, and sinks together using flows.\n\n## Commands\n\n```bash\nforai run main.fa         # run the app\nforai test                # run ALL tests (scans entire project recursively)\nforai build               # build WASM artifact (also runs all tests)\nforai dev main.fa         # interactive debugger\n```\n\n## Project Structure\n\n```\nmain.fa          # flow main — entry point (the top-level flowchart)\nlib/             # funcs — computation happens here\nsinks/           # sinks — output and side effects (print, respond, write)\nsources/         # sources — event producers (HTTP, polling, user input)\n```\n\n## Language Reference\n\nSee [LANGUAGE.md](LANGUAGE.md) for the full language reference including syntax, built-in ops, control flow, and patterns.\n\n## Key Rules\n\n- No bare expressions — every line is `var = ...`, `emit`, `fail`, or control flow\n- `emit`/`fail`/`return` take variables, not literals — `ok = true` then `emit ok`\n- Loop collection must be a variable — assign it before `loop`\n- `exec.run` needs separate command and args list — not a combined string\n- One callable per file, name must match filename\n- `docs` blocks are required for every func, flow, sink, and test\n- `use ... from \"...\"` paths are relative to the importing file's directory\n- Variable names, wire names, and ports cannot be forai keywords (`done`, `step`, `body`, `emit`, `fail`, `return`, `loop`, `on`, etc.) — use `ok`, `result`, `finished` instead\n"
    );
    write_scaffold(&root, "CLAUDE.md", &claude_md)?;

    // AGENTS.md
    let agents_md = format!(
        "# AGENTS.md\n\n## DO THIS FIRST\n\nforai is a new language with its own rules — read [LANGUAGE.md](LANGUAGE.md) before writing any code so you know what you're working with. It takes 2 minutes and will save you from common mistakes.\n\n---\n\nAgent guidance for `{name}`. See [LANGUAGE.md](LANGUAGE.md) for the full language reference and syntax.\n\n## Commands\n\n```bash\nforai run main.fa         # run the app\nforai test                # run ALL tests (scans entire project recursively)\nforai build               # build WASM artifact (also runs all tests)\nforai fmt .               # format all source files\n```\n\n## Project Structure\n\n```\nmain.fa          # flow main — entry point (the top-level flowchart)\nlib/             # funcs — computation happens here\nsinks/           # sinks — output and side effects (print, respond, write)\nsources/         # sources — event producers (HTTP, polling, user input)\n```\n\n## Validation\n\nEach command validates a different surface area. Use them in order:\n\n| Command | What it validates |\n|---------|------------------|\n| `forai check [path]` | Syntax, semantic rules, type constraints, IR lowering, typecheck — follows imports. Fast per-file check. |\n| `forai test` | Everything in `check` + executes all test blocks across the entire project recursively |\n| `forai build` | Definitive end-to-end: fmt + compile + test + artifact. Run as final verification |\n\n**`forai check` with a file** follows `use` imports and checks all transitively-imported modules too.\n\n**`forai test`** always scans the whole project from its root — no need to pass a path.\n\n**`forai build`** is the final authority. Always run before considering a task complete.\n\n## Reference Tools\n\n```bash\nforai stdlib [query]          # search built-in op reference  (e.g. forai stdlib http)\nforai doc <path> [query]      # search project docs           (e.g. forai doc . Greet)\n```\n\nUse `forai stdlib` to look up available ops before writing code. Use `forai doc` to understand what functions exist in the project.\n\n## Key Gotchas\n\n1. No bare expressions — every line must be `var = ...`, `emit`, `fail`, or control flow\n2. `emit`/`fail`/`return` take variables, not literals — `ok = true` then `emit ok`\n3. Loop collection must be a variable — `loop list.range(0,5) as i` fails; assign the range first\n4. `exec.run` needs separate command and args — `exec.run(\"ls\", [\"-la\"])` not `exec.run(\"ls -la\")`\n5. `use ... from \"...\"` paths are relative to the importing file, not the project root\n6. One callable per file — name must match filename (`func Foo` lives in `Foo.fa`)\n7. `docs` are mandatory — compiler rejects missing `docs` blocks\n8. Flows don't compute — no `+`, no function calls except `step` invocations\n9. `_` discards a return value — use `_ = op(...)` when you don't need the result\n10. All data structures are immutable — `obj.set` and `list.append` return new copies\n11. Wire names, locals, and ports cannot be forai keywords — `done`, `step`, `body`, `emit`, `fail`, `return`, `loop`, `on`, `case`, `when`, `else`, `sync`, `branch`, `take`, `from`, `use`, `func`, `flow`, `sink`, `source`, `type`, `data`, `enum`, `test`, `docs`, `mock`, `trap`, `must` cannot be variable or wire names. Use `ok`, `result`, `finished`, `out` instead.\n"
    );
    write_scaffold(&root, "AGENTS.md", &agents_md)?;

    // LANGUAGE.md (embedded at compile time from workspace LANGUAGE.md)
    write_scaffold(&root, "LANGUAGE.md", SCAFFOLD_LANGUAGE_MD)?;

    // .mcp.json
    let mcp_json = "{\n  \"mcpServers\": {\n    \"forai\": {\n      \"command\": \"forai\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}\n";
    write_scaffold(&root, ".mcp.json", mcp_json)?;

    // main.fa
    let main_fa = "use lib from \"./lib\"\nuse sinks from \"./sinks\"\n\ndocs main\n    Hello world.\n    Greets the world and prints the message.\ndone\n\nflow main\nbody\n    step lib.Greet(\"World\" to :name) then\n        next :result to msg\n    done\n    step sinks.Print(msg to :line) done\ndone\n\ntest main\n    mock lib.Greet => \"Hello, World!\"\n    mock sinks.Print => true\n    _ = main()\ndone\n";
    write_scaffold(&root, "main.fa", main_fa)?;

    // lib/Greet.fa
    let greet_fa = "docs Greet\n    Builds a greeting message for the given name.\n\n    docs name\n        The name to greet.\n    done\ndone\n\nfunc Greet\n    take name as text\n    emit result as text\n    fail error as text\nbody\n    msg = \"Hello, #{name}!\"\n    emit msg\ndone\n\ntest Greet\n    r = Greet(\"World\")\n    must r == \"Hello, World!\"\ndone\n";
    write_scaffold(&root, "lib/Greet.fa", greet_fa)?;

    // sinks/Print.fa
    let print_fa = "docs Print\n    Prints a line of text to the terminal.\n\n    docs line\n        The text line to print.\n    done\ndone\n\nsink Print\n    take line as text\nbody\n    term.print(line)\ndone\n\ntest Print\n    it \"works\"\n        Print(\"hello\")\n        must true\n    done\ndone\n";
    write_scaffold(&root, "sinks/Print.fa", print_fa)?;

    // sources/Events.fa
    let events_fa = "docs Events\n    Reads user input from the terminal and emits trimmed text events.\ndone\n\nsource Events\n    emit event as text\n    fail error as text\nbody\n    on :input from term.prompt(\"> \") to raw\n        trimmed = str.trim(raw)\n        emit trimmed\n    done\ndone\n\ntest Events\n    # Source: reads from terminal — stub test, integration tested separately\n    ok = true\n    must ok == true\ndone\n";
    write_scaffold(&root, "sources/Events.fa", events_fa)?;

    println!("Created project `{name}`\n");
    println!("  cd {name}");
    println!("  forai run main.fa         run the app");
    println!("  forai test                run all tests");
    println!("  forai build               build WASM artifact\n");
    println!("MCP server configured in .mcp.json — open in Claude Code to use forai tools.");
    println!("See LANGUAGE.md for the full language reference.");

    Ok(())
}

fn write_scaffold(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    let path = root.join(rel);
    fs::write(&path, content).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::compile_source;
    use crate::codec::CodecRegistry;
    use crate::deps::ResolvedDeps;
    use crate::runtime;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn all_examples_compile() {
        let examples_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let entries = fs::read_dir(&examples_dir).expect("examples dir should exist");

        for entry in entries {
            let entry = entry.expect("read_dir entry");
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let main_fa = path.join("main.fa");
            if !main_fa.exists() {
                continue;
            }
            compile_source(&main_fa, &ResolvedDeps::empty())
                .unwrap_or_else(|e| panic!("compile failed for {}: {e}", main_fa.display()));
        }
    }

    #[test]
    fn compile_errors_include_file_line_col() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be monotonic enough")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("forai_test_{stamp}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("Broken.fa");

        let src = r#"
docs TestRequest
  A test request.

  docs path
    The request path.
  done
done

type TestRequest
  path text
done

docs TestResponse
  A test response.

  docs status
    The status code.
  done
done

type TestResponse
  status long
done

docs TestError
  A test error.

  docs status
    The error status code.
  done
done

type TestError
  status long
done

docs Broken
  A broken flow for testing error output.
done

func Broken
  take request as TestRequest
  emit response as TestResponse
  fail error as TestError
body
  not valid syntax
done

test Broken
  r = Broken(request)
done
"#;
        fs::write(&path, src).expect("write temp source");

        let err = match compile_source(&path, &ResolvedDeps::empty()) {
            Ok(_) => panic!("compile should fail"),
            Err(err) => err,
        };
        assert!(err.contains(&path.display().to_string()));
        assert!(err.contains(":1:") || err.contains(":2:") || err.contains(":3:"));

        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    #[tokio::test]
    async fn classify_func_runs() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../examples/read-docs/src/app/router/Classify.fa");
                let (flow, ir, registry, flow_registry, _ffi_registry) =
                    compile_source(&path, &ResolvedDeps::empty())
                        .unwrap_or_else(|e| panic!("compile failed: {e}"));

                let mut inputs = std::collections::HashMap::new();
                inputs.insert("cmd".to_string(), serde_json::json!("help"));

                let codecs = CodecRegistry::default_registry();
                let report = runtime::execute_flow(
                    &flow,
                    ir,
                    inputs,
                    &registry,
                    Some(&flow_registry),
                    &codecs,
                    None,
                )
                .await
                .unwrap_or_else(|e| panic!("runtime failed: {e}"));

                let outputs = report
                    .outputs
                    .as_object()
                    .expect("outputs should be object");
                let result = outputs.get("result").expect("should have result output");
                assert_eq!(result.as_str(), Some("help"));
            })
            .await;
    }
}
