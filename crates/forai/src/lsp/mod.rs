mod server;
mod document;
mod diagnostics;
mod completion;
mod hover;
mod goto_def;
mod symbols;
mod line_index;

pub fn run_lsp() {
    server::main_loop();
}
