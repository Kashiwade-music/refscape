//! Application use cases and adapter contracts.

pub mod explorer;
pub mod ports;

pub type Result<T> = std::result::Result<T, String>;
