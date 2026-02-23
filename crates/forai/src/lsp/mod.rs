mod completion;
mod diagnostics;
mod document;
mod goto_def;
mod hover;
mod line_index;
mod server;
mod symbols;

pub fn run_lsp() {
    server::main_loop();
}
