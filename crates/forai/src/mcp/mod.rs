mod protocol;
mod server;
mod tools;

pub async fn run_mcp() {
    server::main_loop().await;
}
