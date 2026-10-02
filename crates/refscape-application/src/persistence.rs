use crate::state::ApplicationSnapshot;
use std::{path::PathBuf, sync::Arc};
#[derive(Clone, Debug)]
pub struct ImportedSession {
    pub snapshot: ApplicationSnapshot,
}
#[derive(Clone, Debug)]
pub struct PersistableSession {
    pub snapshot: Arc<ApplicationSnapshot>,
    pub epoch: u64,
    pub revision: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SaveDestination {
    #[default]
    Unset,
    Writable(PathBuf),
    Protected {
        path: PathBuf,
        reason: String,
    },
}
impl SaveDestination {
    pub fn path(&self) -> Option<&std::path::Path> {
        match self {
            Self::Unset => None,
            Self::Writable(path) | Self::Protected { path, .. } => Some(path),
        }
    }
    pub fn writable(&self) -> Option<&std::path::Path> {
        match self {
            Self::Writable(path) => Some(path),
            _ => None,
        }
    }
}
