//! Language-neutral, read-only LSP project sessions and protocol conversion.
mod context;
mod conversion;
pub mod runtime;
mod session;
pub mod transport;
pub use conversion::{byte_offset, full_range};
pub use refscape_analysis::{NavigationLocation, NavigationTarget};
pub use session::{DocumentStatistics, LspProjectSession, ServerConfiguration};
