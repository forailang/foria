mod protocol;
mod server;
mod tools;

pub use tools::{collect_fa_files, expand_with_imports};

pub async fn run_mcp() {
    server::main_loop().await;
}
