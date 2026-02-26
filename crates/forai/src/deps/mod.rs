pub mod semver;
pub mod source;
pub mod fetch;
pub mod lockfile;
pub mod resolve;

pub use resolve::{ResolvedDeps, resolve_dependencies};
