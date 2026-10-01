//! Pure canvas geometry, graph policies, and source grouping.

pub mod graph;
pub mod layout;
pub mod regions;

pub type Result<T> = std::result::Result<T, String>;

#[cfg(test)]
mod tests;
