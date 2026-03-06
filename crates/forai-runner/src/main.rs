use forai::codec::CodecRegistry;
use forai::ffi_manager::FfiRegistry;
use forai::host_native::NativeHost;
use forai::runtime;
use forai_core::loader::ProgramBundle;
use std::collections::HashMap;
use std::rc::Rc;

// V3 format: [ runner exe ][ compressed bundle JSON ][ len: u64 LE ][ flags: u8 ][ FORAI_BUNDLE_V3\0 ]
const BUNDLE_MAGIC: &[u8; 16] = b"FORAI_BUNDLE_V3\0";
const BUNDLE_FLAGS_COMPRESSED: u8 = 0x01;
const BUNDLE_FOOTER_SIZE: usize = 16 + 1 + 8; // magic + flags + u64 length

fn main() {
    let bundle_bytes = match extract_embedded_bundle() {
        Some(bytes) => bytes,
        None => {
            // Fallback: read from stdin (for testing)
            use std::io::Read;
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf).unwrap_or_else(|e| {
                eprintln!("No embedded bundle found and failed to read stdin: {e}");
                std::process::exit(1);
            });
            buf
        }
    };

    let bundle: ProgramBundle = serde_json::from_slice(&bundle_bytes).unwrap_or_else(|e| {
        eprintln!("Failed to deserialize program bundle: {e}");
        std::process::exit(1);
    });

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| {
            eprintln!("Failed to create async runtime: {e}");
            std::process::exit(1);
        });

    let local = tokio::task::LocalSet::new();
    let result = local.block_on(&rt, async {
        let codecs = CodecRegistry::default_registry();
        let mut native_host = NativeHost::new();
        if let Some(ffi_json) = &bundle.ffi_registry {
            if let Ok(ffi_reg) = serde_json::from_value::<FfiRegistry>(ffi_json.clone()) {
                native_host = native_host.with_ffi_registry(ffi_reg);
            }
        }
        let host: Rc<dyn forai::host::Host> = Rc::new(native_host);

        let inputs: HashMap<String, serde_json::Value> = load_inputs_from_args(&bundle);

        runtime::execute_flow(
            &bundle.entry_flow,
            bundle.entry_ir,
            inputs,
            &bundle.type_registry,
            Some(&bundle.flow_registry),
            &codecs,
            Some(host),
        )
        .await
    });

    match result {
        Ok(_report) => std::process::exit(0),
        Err(e) => {
            eprintln!("Runtime error: {e}");
            std::process::exit(1);
        }
    }
}

/// Map CLI arguments to flow input ports.
/// If the flow takes inputs, positional args are mapped in order.
/// If no inputs are declared, args are ignored.
fn load_inputs_from_args(bundle: &ProgramBundle) -> HashMap<String, serde_json::Value> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut inputs = HashMap::new();

    let take_ports: Vec<_> = bundle
        .entry_flow
        .inputs
        .iter()
        .map(|p| p.name.clone())
        .collect();

    for (i, port_name) in take_ports.iter().enumerate() {
        if let Some(arg) = args.get(i) {
            // Try to parse as JSON first, fall back to string
            let value = serde_json::from_str(arg)
                .unwrap_or_else(|_| serde_json::Value::String(arg.clone()));
            inputs.insert(port_name.clone(), value);
        }
    }

    inputs
}

fn extract_embedded_bundle() -> Option<Vec<u8>> {
    let exe_path = std::env::current_exe().ok()?;
    let exe_bytes = std::fs::read(&exe_path).ok()?;
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
