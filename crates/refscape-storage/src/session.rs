//! v1 session codec and stateless persistence adapter.
mod conversion;
pub(crate) mod streaming;
pub mod v1;
use crate::document::{decode_versioned_json, write_json};
use refscape_application::{
    ApplicationSnapshot, ImportedSession, PersistableSession, SessionRepository,
};
use refscape_model::{
    CardId, CardSource, CodeCard, Connection, EdgeId, ErrorKind, RefscapeError, Region,
    SourceDocument,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
pub const SESSION_VERSION: u32 = 1;
#[derive(Debug, Default, Clone, Copy)]
pub struct JsonSessionRepository;
impl SessionRepository for JsonSessionRepository {
    fn save(&self, path: &Path, session: &PersistableSession) -> Result<(), RefscapeError> {
        session
            .snapshot
            .validate()
            .map_err(|error| invalid(error, path))?;
        write_json(path, &streaming::SessionView(&session.snapshot))
    }
    fn load(&self, path: &Path) -> Result<ImportedSession, RefscapeError> {
        let bytes = std::fs::read(path).map_err(|error| {
            RefscapeError::new(
                ErrorKind::Io,
                format!("cannot read {}: {error}", path.display()),
            )
            .with_path(path)
        })?;
        decode_v1(&bytes).map_err(|error| error.with_path(path))
    }
}
pub fn decode_v1(bytes: &[u8]) -> Result<ImportedSession, RefscapeError> {
    let document = decode_versioned_json(bytes, SESSION_VERSION, "session")?;
    import(document)
}
fn import(document: v1::SessionDocument) -> Result<ImportedSession, RefscapeError> {
    let cards = document
        .cards
        .into_iter()
        .map(|card| {
            let fingerprint = card.source.document_fingerprint;
            let source: SourceDocument = card.source.into();
            let source = CardSource::try_from(source)?;
            let source = match fingerprint {
                Some(fingerprint) => source.with_document_fingerprint(fingerprint.into()),
                None => source,
            };
            Ok(CodeCard {
                id: CardId::new(card.id)?,
                source,
                position: refscape_model::Point::from(card.position).try_into()?,
                width: card.width,
                height: card.height,
            })
        })
        .collect::<Result<Vec<_>, RefscapeError>>()?;
    let connections = document
        .connections
        .into_iter()
        .map(|edge| {
            Ok(Connection {
                id: EdgeId::new(edge.id)?,
                from: CardId::new(edge.from)?,
                to: CardId::new(edge.to)?,
                kind: edge.kind.into(),
                source: edge.source.into(),
            })
        })
        .collect::<Result<Vec<_>, RefscapeError>>()?;
    let regions = document
        .regions
        .into_iter()
        .map(|region| {
            let kind = if region.id.starts_with("module:") {
                refscape_model::RegionKind::Module
            } else {
                refscape_model::RegionKind::Crate
            };
            Ok(Region {
                kind,
                id: region.id,
                label: region.label,
                path: region.path,
                card_ids: region
                    .card_ids
                    .into_iter()
                    .map(CardId::new)
                    .collect::<Result<Vec<_>, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, RefscapeError>>()?;
    let snapshot = ApplicationSnapshot {
        project_root: document.project_root,
        project_options: document.project_options.into(),
        cards: Arc::new(cards),
        connections: Arc::new(connections),
        regions: Arc::new(regions),
        viewport: document.viewport.try_into()?,
        theme: document.theme.into(),
    };
    snapshot
        .validate()
        .map_err(|error| RefscapeError::new(ErrorKind::InvalidData, error))?;
    Ok(ImportedSession { snapshot })
}
fn invalid(error: String, path: &Path) -> RefscapeError {
    RefscapeError::new(ErrorKind::InvalidData, error).with_path(path)
}
pub fn default_session_path(project_root: &Path) -> PathBuf {
    project_root.join(".refscape").join("session.json")
}
