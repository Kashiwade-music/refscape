//! Pure canvas geometry, graph policies, and source grouping.

pub mod graph;
pub mod instrumentation;
pub mod layout;
pub mod metrics;
pub mod regions;

pub type Result<T> = std::result::Result<T, refscape_model::RefscapeError>;

pub(crate) fn invalid(message: &str) -> refscape_model::RefscapeError {
    refscape_model::RefscapeError::new(refscape_model::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
