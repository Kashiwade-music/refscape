//! Language-neutral, read-only LSP sessions and protocol conversion.
mod conversion;
mod session;
pub mod transport;
pub use conversion::{byte_offset, full_range};
pub use session::{LspSession, ServerConfiguration};
