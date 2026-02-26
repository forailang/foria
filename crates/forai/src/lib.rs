// lib.rs — re-exports for fuzz targets.
// Only includes modules needed for fuzzing (parser, lexer, formatter).
// The full application entry point is main.rs.

mod ast {
    pub use forai_core::ast::*;
}

pub mod lexer {
    pub use forai_core::lexer::*;
}
pub mod parser {
    pub use forai_core::parser::*;
}
pub mod formatter {
    pub use forai_core::formatter::*;
}

pub mod deps {
    pub mod semver;
    pub mod source;
}
