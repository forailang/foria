mod sync_runtime;
mod wasi_host;

use forai_core::codec::CodecRegistry;
use std::collections::HashMap;

/// Program bundle embedded as a WASM custom section named "forai_program".
/// At build time, `dataflowc build` appends this section to the WASM binary.
/// At runtime, the loader extracts it before starting execution.
///
/// For testing, the bundle can also be read from stdin.
fn main() {
    let bundle_bytes = read_bundle();
    let bundle: forai_core::loader::ProgramBundle = serde_json::from_slice(&bundle_bytes)
        .unwrap_or_else(|e| {
            eprintln!("Failed to deserialize program bundle: {e}");
            std::process::exit(1);
        });

    let codecs = CodecRegistry::default_registry();
    let host = wasi_host::WasiHost::new();

    let inputs: HashMap<String, serde_json::Value> = HashMap::new();

    let result = sync_runtime::execute_flow(
        &bundle.entry_flow,
        bundle.entry_ir,
        inputs,
        &bundle.type_registry,
        Some(&bundle.flow_registry),
        &codecs,
        &host,
    );

    match result {
        Ok(run_result) => {
            let json = serde_json::to_string_pretty(&run_result.outputs)
                .unwrap_or_else(|_| "{}".to_string());
            println!("{json}");
        }
        Err(e) => {
            eprintln!("Runtime error: {e}");
            std::process::exit(1);
        }
    }
}

fn read_bundle() -> Vec<u8> {
    // Try to read from a WASM custom section first (embedded by forai build).
    // For now, read from stdin for testing.
    use std::io::Read;
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).unwrap_or_else(|e| {
        eprintln!("Failed to read program bundle from stdin: {e}");
        std::process::exit(1);
    });
    buf
}
