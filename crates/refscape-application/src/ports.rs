use crate::{Completion, Effect, ImportedSession, PersistableSession, Result};
use std::path::Path;
pub trait SessionRepository: Send + Sync {
    fn save(&self, path: &Path, session: &PersistableSession) -> Result<()>;
    fn load(&self, path: &Path) -> Result<ImportedSession>;
}
/// Executed by the host on a worker; the controller never crosses this boundary.
pub trait EffectExecutor: Send + Sync {
    fn execute(&self, effect: Effect) -> Completion;
}
