// lib.rs — public API for the forai crate.
// Exposes the async runtime, native host, and forai-core re-exports
// so that forai-runner (and fuzz targets) can depend on this crate.

// Re-exports from forai-core
pub mod ast {
    pub use forai_core::ast::*;
}
pub mod codec {
    pub use forai_core::codec::*;
}
pub mod host {
    pub use forai_core::host::*;
}
pub mod ir {
    pub use forai_core::ir::*;
}
pub mod lexer {
    pub use forai_core::lexer::*;
}
pub mod loader {
    pub use forai_core::loader::*;
}
pub mod parser {
    pub use forai_core::parser::*;
}
pub mod formatter {
    pub use forai_core::formatter::*;
}
pub mod sema {
    pub use forai_core::sema::*;
}
pub mod typecheck {
    pub use forai_core::typecheck::*;
}
pub mod types;

// Local modules needed by forai-runner
pub mod ffi_manager;
pub mod host_native;
pub mod runtime;

pub mod deps {
    pub mod semver;
    pub mod source;
}
