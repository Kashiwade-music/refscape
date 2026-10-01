//! Validated, versioned exploration session persistence.

use std::path::{Path, PathBuf};

use refscape_application::ports::SessionRepository;
use refscape_model::{SESSION_VERSION, Session};

use crate::document::{read_versioned_json, write_json};

/// Stateless repository: callers choose paths, so named sessions can be shared.
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonSessionRepository;

impl SessionRepository for JsonSessionRepository {
    fn save(&self, path: &Path, session: &Session) -> Result<(), String> {
        session.validate()?;
        write_json(path, session)
    }

    fn load(&self, path: &Path) -> Result<Session, String> {
        // Check the format version before deserializing a potentially newer schema.
        let document = read_versioned_json(path, SESSION_VERSION, "session")?;
        let session: Session = serde_json::from_value(document)
            .map_err(|error| format!("invalid session {}: {error}", path.display()))?;
        session.validate()?;
        Ok(session)
    }
}

pub fn default_session_path(project_root: &Path) -> PathBuf {
    project_root.join(".refscape").join("session.json")
}
