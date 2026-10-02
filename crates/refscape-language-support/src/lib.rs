//! Shared project catalogs, launch resolution and opened analysis runtime.
pub mod catalog;
pub mod environment;
pub mod process;
pub mod resolver;
pub mod runtime;
pub use environment::EnvironmentSnapshot;
pub use runtime::{LspAnalysisSession, Metadata, MetadataProvider, SearchMergePolicy};
