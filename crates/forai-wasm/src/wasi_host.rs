use forai_core::sync_host::SyncHost;
use serde_json::Value;

unsafe extern "C" {
    fn host_call(
        op_ptr: *const u8,
        op_len: u32,
        args_ptr: *const u8,
        args_len: u32,
        result_ptr: *mut u8,
        result_cap: u32,
    ) -> i32;
}

pub struct WasiHost;

impl WasiHost {
    pub fn new() -> Self {
        WasiHost
    }
}

impl SyncHost for WasiHost {
    fn execute_io_op(&self, op: &str, args: &[Value]) -> Result<Value, String> {
        let op_bytes = op.as_bytes();
        let args_json =
            serde_json::to_string(args).map_err(|e| format!("failed to serialize args: {e}"))?;
        let args_bytes = args_json.as_bytes();

        // Allocate a buffer for the result (1MB max)
        let cap: u32 = 1024 * 1024;
        let mut result_buf = vec![0u8; cap as usize];

        let result_len = unsafe {
            host_call(
                op_bytes.as_ptr(),
                op_bytes.len() as u32,
                args_bytes.as_ptr(),
                args_bytes.len() as u32,
                result_buf.as_mut_ptr(),
                cap,
            )
        };

        if result_len < 0 {
            let err_len = (-result_len) as usize;
            let err_msg = std::str::from_utf8(&result_buf[..err_len])
                .unwrap_or("unknown host error")
                .to_string();
            return Err(err_msg);
        }

        let result_str = std::str::from_utf8(&result_buf[..result_len as usize])
            .map_err(|e| format!("invalid UTF-8 from host: {e}"))?;

        serde_json::from_str(result_str).map_err(|e| format!("failed to parse host result: {e}"))
    }

    fn cleanup(&self) {
        // No cleanup needed for WASI host
    }
}
