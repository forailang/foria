pub mod fetch;
pub mod lockfile;
pub mod resolve;
pub mod semver;
pub mod source;

pub use resolve::{ResolvedDeps, resolve_dependencies};
