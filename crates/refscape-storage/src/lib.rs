//! Versioned JSON persistence for sessions, user settings, and themes.

mod document;
pub mod session;
pub mod settings;
pub mod theme;

#[cfg(test)]
mod tests;
