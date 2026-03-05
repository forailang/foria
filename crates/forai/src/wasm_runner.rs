use serde_json::Value;
use std::path::Path;
use wasmtime::*;

use crate::host::Host;
use crate::host_native::NativeHost;

struct WasmState {
    native_host: NativeHost,
    stdin_buf: Vec<u8>,
    stdin_pos: usize,
    args: Vec<String>,
}

/// Run a .wasm artifact produced by `dataflowc build`.
/// Extracts the program bundle from the WASM custom section and provides it via stdin.
pub async fn run_wasm(wasm_path: &Path) -> Result<(), String> {
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| format!("failed to read {}: {e}", wasm_path.display()))?;

    run_wasm_from_bytes(&wasm_bytes, vec![]).await
}

/// Run a WASM module from raw bytes (used by both file-based and self-extracting bundle paths).
pub async fn run_wasm_from_bytes(wasm_bytes: &[u8], args: Vec<String>) -> Result<(), String> {
    let bundle = extract_custom_section(wasm_bytes, "forai_program")
        .ok_or_else(|| "no 'forai_program' section in WASM".to_string())?;

    let wasm_owned = wasm_bytes.to_vec();
    tokio::task::spawn_blocking(move || run_wasm_sync(&wasm_owned, bundle, args))
        .await
        .map_err(|e| format!("WASM task panicked: {e}"))?
}

fn extract_custom_section(wasm: &[u8], name: &str) -> Option<Vec<u8>> {
    if wasm.len() < 8 || &wasm[0..4] != b"\0asm" {
        return None;
    }

    let mut pos = 8; // skip magic + version
    while pos < wasm.len() {
        let section_id = wasm[pos];
        pos += 1;

        let (section_len, bytes_read) = read_leb128(&wasm[pos..])?;
        pos += bytes_read;

        if section_id == 0 {
            // Custom section: name length + name + data
            let section_start = pos;
            let (name_len, name_bytes_read) = read_leb128(&wasm[pos..])?;
            pos += name_bytes_read;

            if pos + name_len <= wasm.len() {
                let section_name = std::str::from_utf8(&wasm[pos..pos + name_len]).ok()?;
                if section_name == name {
                    let data_start = pos + name_len;
                    let data_end = section_start + section_len;
                    if data_end <= wasm.len() {
                        return Some(wasm[data_start..data_end].to_vec());
                    }
                }
            }
            pos = section_start + section_len;
        } else {
            pos += section_len;
        }
    }
    None
}

fn read_leb128(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut result: usize = 0;
    let mut shift = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        result |= ((byte & 0x7F) as usize) << shift;
        if byte & 0x80 == 0 {
            return Some((result, i + 1));
        }
        shift += 7;
        if shift >= 35 {
            return None; // overflow
        }
    }
    None
}

fn run_wasm_sync(wasm_bytes: &[u8], stdin_data: Vec<u8>, args: Vec<String>) -> Result<(), String> {
    let mut config = Config::new();
    config.async_support(false);
    let engine =
        Engine::new(&config).map_err(|e| format!("failed to create wasmtime engine: {e}"))?;
    let module = Module::from_binary(&engine, wasm_bytes)
        .map_err(|e| format!("failed to load WASM module: {e}"))?;

    let mut linker = Linker::new(&engine);

    // Add WASI stubs
    wasmtime_wasi_stubs(&mut linker)?;

    // Add host_call import — uses Caller to access WasmState in the Store
    linker
        .func_wrap(
            "env",
            "host_call",
            |mut caller: Caller<'_, WasmState>,
             op_ptr: u32,
             op_len: u32,
             args_ptr: u32,
             args_len: u32,
             result_ptr: u32,
             result_cap: u32|
             -> i32 {
                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => {
                        return write_error(
                            &mut caller,
                            result_ptr,
                            result_cap,
                            "no memory export",
                        );
                    }
                };

                let data = memory.data(&caller);

                let op = match read_wasm_str(data, op_ptr, op_len) {
                    Ok(s) => s,
                    Err(e) => return write_error(&mut caller, result_ptr, result_cap, &e),
                };

                let args_str = match read_wasm_str(data, args_ptr, args_len) {
                    Ok(s) => s,
                    Err(e) => return write_error(&mut caller, result_ptr, result_cap, &e),
                };

                let args: Vec<Value> = match serde_json::from_str(&args_str) {
                    Ok(a) => a,
                    Err(e) => {
                        return write_error(
                            &mut caller,
                            result_ptr,
                            result_cap,
                            &format!("invalid args JSON: {e}"),
                        );
                    }
                };

                // Execute I/O op synchronously using a mini tokio runtime
                let result = {
                    let host = &caller.data().native_host;
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    rt.block_on(host.execute_io_op(&op, &args))
                };

                match result {
                    Ok(val) => {
                        let json = serde_json::to_string(&val).unwrap_or_default();
                        let bytes = json.as_bytes();
                        if bytes.len() > result_cap as usize {
                            return write_error(
                                &mut caller,
                                result_ptr,
                                result_cap,
                                "result too large for buffer",
                            );
                        }
                        let memory = caller.get_export("memory").unwrap();
                        if let Extern::Memory(m) = memory {
                            let data = m.data_mut(&mut caller);
                            let start = result_ptr as usize;
                            data[start..start + bytes.len()].copy_from_slice(bytes);
                        }
                        bytes.len() as i32
                    }
                    Err(e) => write_error(&mut caller, result_ptr, result_cap, &e),
                }
            },
        )
        .map_err(|e| format!("failed to define host_call: {e}"))?;

    let mut store = Store::new(
        &engine,
        WasmState {
            native_host: NativeHost::new(),
            stdin_buf: stdin_data,
            stdin_pos: 0,
            args,
        },
    );

    let instance = linker
        .instantiate(&mut store, &module)
        .map_err(|e| format!("failed to instantiate WASM module: {e}"))?;

    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .map_err(|e| format!("no _start export: {e}"))?;

    match start.call(&mut store, ()) {
        Ok(()) => Ok(()),
        Err(e) => {
            // proc_exit(0) shows up as a trap — that's normal termination
            let msg = e.to_string();
            if msg.contains("proc_exit") || msg.contains("exit with code 0") {
                Ok(())
            } else {
                Err(format!("WASM execution failed: {e}"))
            }
        }
    }
}

fn read_wasm_str(data: &[u8], ptr: u32, len: u32) -> Result<String, String> {
    let start = ptr as usize;
    let end = start + len as usize;
    if end > data.len() {
        return Err("memory access out of bounds".to_string());
    }
    std::str::from_utf8(&data[start..end])
        .map(|s| s.to_string())
        .map_err(|e| format!("invalid UTF-8: {e}"))
}

fn write_error(
    caller: &mut Caller<'_, WasmState>,
    result_ptr: u32,
    result_cap: u32,
    msg: &str,
) -> i32 {
    let bytes = msg.as_bytes();
    let len = bytes.len().min(result_cap as usize);
    if let Some(Extern::Memory(m)) = caller.get_export("memory") {
        let data = m.data_mut(caller);
        let start = result_ptr as usize;
        if start + len <= data.len() {
            data[start..start + len].copy_from_slice(&bytes[..len]);
        }
    }
    -(len as i32)
}

fn wasmtime_wasi_stubs(linker: &mut Linker<WasmState>) -> Result<(), String> {
    // fd_write: stdout/stderr output
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_write",
            |mut caller: Caller<'_, WasmState>,
             fd: i32,
             iovs_ptr: i32,
             iovs_len: i32,
             nwritten_ptr: i32|
             -> i32 {
                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return 8,
                };

                let data = memory.data(&caller);
                let mut total = 0u32;

                for i in 0..iovs_len {
                    let iov_offset = (iovs_ptr as usize) + (i as usize) * 8;
                    if iov_offset + 8 > data.len() {
                        return 21;
                    }
                    let buf_ptr =
                        u32::from_le_bytes(data[iov_offset..iov_offset + 4].try_into().unwrap());
                    let buf_len = u32::from_le_bytes(
                        data[iov_offset + 4..iov_offset + 8].try_into().unwrap(),
                    );

                    let start = buf_ptr as usize;
                    let end = start + buf_len as usize;
                    if end > data.len() {
                        return 21;
                    }

                    let bytes = &data[start..end];
                    if fd == 1 {
                        use std::io::Write;
                        let _ = std::io::stdout().write_all(bytes);
                    } else if fd == 2 {
                        use std::io::Write;
                        let _ = std::io::stderr().write_all(bytes);
                    }
                    total += buf_len;
                }

                let memory = caller.get_export("memory").unwrap();
                if let Extern::Memory(m) = memory {
                    let data = m.data_mut(&mut caller);
                    let np = nwritten_ptr as usize;
                    if np + 4 <= data.len() {
                        data[np..np + 4].copy_from_slice(&total.to_le_bytes());
                    }
                }

                0
            },
        )
        .map_err(|e| format!("failed to define fd_write: {e}"))?;

    // proc_exit — exit the process
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "proc_exit",
            |_caller: Caller<'_, WasmState>, code: i32| -> () {
                if code != 0 {
                    eprintln!("WASM process exited with code {code}");
                }
                std::process::exit(code);
            },
        )
        .map_err(|e| format!("failed to define proc_exit: {e}"))?;

    // fd_close, fd_prestat_get, fd_prestat_dir_name — stubs returning EBADF
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_close",
            |_: Caller<'_, WasmState>, _: i32| -> i32 { 8 },
        )
        .map_err(|e| format!("failed to define fd_close: {e}"))?;

    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_prestat_get",
            |_: Caller<'_, WasmState>, _: i32, _: i32| -> i32 { 8 },
        )
        .map_err(|e| format!("failed to define fd_prestat_get: {e}"))?;

    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_prestat_dir_name",
            |_: Caller<'_, WasmState>, _: i32, _: i32, _: i32| -> i32 { 8 },
        )
        .map_err(|e| format!("failed to define fd_prestat_dir_name: {e}"))?;

    // fd_read: serve stdin from the embedded bundle
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_read",
            |mut caller: Caller<'_, WasmState>,
             fd: i32,
             iovs_ptr: i32,
             iovs_len: i32,
             nread_ptr: i32|
             -> i32 {
                if fd != 0 {
                    return 8; // EBADF for non-stdin
                }

                let memory = match caller.get_export("memory") {
                    Some(Extern::Memory(m)) => m,
                    _ => return 8,
                };

                // Read iov structures first
                let data = memory.data(&caller);
                let mut iovs = Vec::new();
                for i in 0..iovs_len {
                    let iov_offset = (iovs_ptr as usize) + (i as usize) * 8;
                    if iov_offset + 8 > data.len() {
                        return 21;
                    }
                    let buf_ptr =
                        u32::from_le_bytes(data[iov_offset..iov_offset + 4].try_into().unwrap());
                    let buf_len = u32::from_le_bytes(
                        data[iov_offset + 4..iov_offset + 8].try_into().unwrap(),
                    );
                    iovs.push((buf_ptr as usize, buf_len as usize));
                }

                let state = caller.data_mut();
                let remaining = &state.stdin_buf[state.stdin_pos..];
                let mut total_read = 0usize;
                let mut src_offset = 0usize;

                let write_ops: Vec<(usize, Vec<u8>)> = iovs
                    .iter()
                    .map(|&(buf_ptr, buf_len)| {
                        let available = remaining.len() - src_offset;
                        let to_copy = buf_len.min(available);
                        let chunk = remaining[src_offset..src_offset + to_copy].to_vec();
                        src_offset += to_copy;
                        total_read += to_copy;
                        (buf_ptr, chunk)
                    })
                    .collect();

                state.stdin_pos += total_read;

                let memory = caller.get_export("memory").unwrap();
                if let Extern::Memory(m) = memory {
                    let data = m.data_mut(&mut caller);
                    for (buf_ptr, chunk) in &write_ops {
                        data[*buf_ptr..*buf_ptr + chunk.len()].copy_from_slice(chunk);
                    }
                    let np = nread_ptr as usize;
                    if np + 4 <= data.len() {
                        data[np..np + 4].copy_from_slice(&(total_read as u32).to_le_bytes());
                    }
                }

                0
            },
        )
        .map_err(|e| format!("failed to define fd_read: {e}"))?;

    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "fd_seek",
            |_: Caller<'_, WasmState>, _: i32, _: i64, _: i32, _: i32| -> i32 { 8 },
        )
        .map_err(|e| format!("failed to define fd_seek: {e}"))?;

    // environ_sizes_get, environ_get
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "environ_sizes_get",
            |mut caller: Caller<'_, WasmState>, count_ptr: i32, size_ptr: i32| -> i32 {
                if let Some(Extern::Memory(m)) = caller.get_export("memory") {
                    let data = m.data_mut(&mut caller);
                    let cp = count_ptr as usize;
                    let sp = size_ptr as usize;
                    if cp + 4 <= data.len() {
                        data[cp..cp + 4].copy_from_slice(&0u32.to_le_bytes());
                    }
                    if sp + 4 <= data.len() {
                        data[sp..sp + 4].copy_from_slice(&0u32.to_le_bytes());
                    }
                }
                0
            },
        )
        .map_err(|e| format!("failed to define environ_sizes_get: {e}"))?;

    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "environ_get",
            |_: Caller<'_, WasmState>, _: i32, _: i32| -> i32 { 0 },
        )
        .map_err(|e| format!("failed to define environ_get: {e}"))?;

    // args_sizes_get, args_get — pass through real CLI args
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "args_sizes_get",
            |mut caller: Caller<'_, WasmState>, argc_ptr: i32, argv_buf_size_ptr: i32| -> i32 {
                let argc = caller.data().args.len() as u32;
                let buf_size: u32 = caller
                    .data()
                    .args
                    .iter()
                    .map(|a| a.len() as u32 + 1) // +1 for null terminator
                    .sum();
                if let Some(Extern::Memory(m)) = caller.get_export("memory") {
                    let data = m.data_mut(&mut caller);
                    let ap = argc_ptr as usize;
                    let bp = argv_buf_size_ptr as usize;
                    if ap + 4 <= data.len() {
                        data[ap..ap + 4].copy_from_slice(&argc.to_le_bytes());
                    }
                    if bp + 4 <= data.len() {
                        data[bp..bp + 4].copy_from_slice(&buf_size.to_le_bytes());
                    }
                }
                0
            },
        )
        .map_err(|e| format!("failed to define args_sizes_get: {e}"))?;

    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "args_get",
            |mut caller: Caller<'_, WasmState>, argv_ptr: i32, argv_buf_ptr: i32| -> i32 {
                let args: Vec<String> = caller.data().args.clone();
                if let Some(Extern::Memory(m)) = caller.get_export("memory") {
                    let data = m.data_mut(&mut caller);
                    let mut buf_offset = argv_buf_ptr as usize;
                    for (i, arg) in args.iter().enumerate() {
                        // Write pointer to this arg in the argv array
                        let ptr_offset = argv_ptr as usize + i * 4;
                        if ptr_offset + 4 <= data.len() {
                            data[ptr_offset..ptr_offset + 4]
                                .copy_from_slice(&(buf_offset as u32).to_le_bytes());
                        }
                        // Write null-terminated arg string into buffer
                        let arg_bytes = arg.as_bytes();
                        if buf_offset + arg_bytes.len() + 1 <= data.len() {
                            data[buf_offset..buf_offset + arg_bytes.len()]
                                .copy_from_slice(arg_bytes);
                            data[buf_offset + arg_bytes.len()] = 0;
                        }
                        buf_offset += arg_bytes.len() + 1;
                    }
                }
                0
            },
        )
        .map_err(|e| format!("failed to define args_get: {e}"))?;

    // clock_time_get
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "clock_time_get",
            |mut caller: Caller<'_, WasmState>,
             _clock_id: i32,
             _precision: i64,
             time_ptr: i32|
             -> i32 {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let ns = now.as_nanos() as u64;
                if let Some(Extern::Memory(m)) = caller.get_export("memory") {
                    let data = m.data_mut(&mut caller);
                    let tp = time_ptr as usize;
                    if tp + 8 <= data.len() {
                        data[tp..tp + 8].copy_from_slice(&ns.to_le_bytes());
                    }
                }
                0
            },
        )
        .map_err(|e| format!("failed to define clock_time_get: {e}"))?;

    // random_get
    linker
        .func_wrap(
            "wasi_snapshot_preview1",
            "random_get",
            |mut caller: Caller<'_, WasmState>, buf_ptr: i32, buf_len: i32| -> i32 {
                if let Some(Extern::Memory(m)) = caller.get_export("memory") {
                    let data = m.data_mut(&mut caller);
                    let start = buf_ptr as usize;
                    let end = start + buf_len as usize;
                    if end <= data.len() {
                        use rand::RngExt;
                        let mut rng = rand::rng();
                        for byte in &mut data[start..end] {
                            *byte = rng.random();
                        }
                    }
                }
                0
            },
        )
        .map_err(|e| format!("failed to define random_get: {e}"))?;

    Ok(())
}
