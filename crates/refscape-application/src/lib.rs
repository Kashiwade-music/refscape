//! Single-owner application state, explicit effects, and validated commits.
pub mod command;
pub mod controller;
pub mod editing;
pub mod effect;
pub mod executor;
pub mod jobs;
pub mod navigation;
pub mod persistence;
pub mod ports;
pub mod state;
pub use command::{Command, NavigationMode};
pub use controller::{ApplicationController, Transition, ViewEvent};
pub use editing::CanvasEditOutcome;
pub use effect::{Completion, Effect};
pub use executor::{HeadlessDriver, WorkerExecutor};
pub use persistence::{ImportedSession, PersistableSession, SaveDestination};
pub use ports::{EffectExecutor, SessionRepository};
pub use state::{ApplicationSnapshot, ProjectState, VariableInspection};
pub type Result<T> = std::result::Result<T, refscape_model::RefscapeError>;

#[cfg(feature = "test-support")]
pub mod test_support {
    pub use refscape_analysis::{
        AnalysisCapabilities, AnalysisFactory, AnalysisMetadata, AnalysisResult, AnalysisSession,
        CatalogOutcome, NavigationLocation, NavigationTarget, PreparedProject,
    };
}

#[cfg(test)]
mod tests;
