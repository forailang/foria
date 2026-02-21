use serde_json::Value;

/// Synchronous host trait for I/O ops in the WASM runtime.
pub trait SyncHost {
    fn execute_io_op(&self, op: &str, args: &[Value]) -> Result<Value, String>;
    fn cleanup(&self);
}
