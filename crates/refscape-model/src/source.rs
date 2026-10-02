mod document;
pub use document::*;
pub mod index;
pub mod projection;
#[cfg(test)]
mod tests;

use crate::{
    CardMetrics, ErrorKind, FoldId, FoldRevision, RefscapeError, SnapshotId, SourceLineNumber,
    SourceRevision,
};
pub use index::{DocumentFingerprint, DocumentSnapshot, TextIndex};
pub use projection::{ProjectionRow, SourceProjection};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU32,
    ops::{Deref, Range},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapContent {
    Available(Arc<SourceContext>),
    MissingLegacy,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceGap {
    pub id: FoldId,
    pub range: Range<u32>,
    pub content: GapContent,
    text: Option<Arc<str>>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FoldState {
    expanded: BTreeSet<FoldId>,
    revision: FoldRevision,
}
impl FoldState {
    pub fn is_expanded(&self, id: FoldId) -> bool {
        self.expanded.contains(&id)
    }
    pub fn revision(&self) -> FoldRevision {
        self.revision
    }
    pub fn expanded(&self) -> impl Iterator<Item = FoldId> + '_ {
        self.expanded.iter().copied()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardSourceSnapshot {
    pub id: SnapshotId,
    pub symbol: Symbol,
    pub code: Arc<str>,
    pub context: Arc<Vec<SourceContext>>,
    pub code_start: Option<Position>,
    pub tokens: Arc<Vec<SemanticToken>>,
    pub gaps: Vec<Option<SourceGap>>,
    pub revision: SourceRevision,
    pub document_fingerprint: Option<DocumentFingerprint>,
    body_line_count: usize,
    segments: Vec<Arc<str>>,
    gutter_width: f32,
}

#[derive(Debug, Clone)]
pub struct CardSource {
    snapshot: Arc<CardSourceSnapshot>,
    folds: FoldState,
    projection: Arc<SourceProjection>,
}
impl PartialEq for CardSource {
    fn eq(&self, other: &Self) -> bool {
        (Arc::ptr_eq(&self.snapshot, &other.snapshot)
            || (self.snapshot.symbol == other.snapshot.symbol
                && self.snapshot.code == other.snapshot.code
                && self.snapshot.context == other.snapshot.context
                && self.snapshot.code_start == other.snapshot.code_start
                && self.snapshot.tokens == other.snapshot.tokens
                && self.snapshot.document_fingerprint == other.snapshot.document_fingerprint
                && self.snapshot.gaps == other.snapshot.gaps))
            && self.folds.expanded == other.folds.expanded
    }
}
impl Eq for CardSource {}
impl Deref for CardSource {
    type Target = CardSourceSnapshot;
    fn deref(&self) -> &Self::Target {
        &self.snapshot
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldToggle {
    Expanded,
    Collapsed,
    MissingLegacy { range: Range<u32> },
}
impl TryFrom<SourceDocument> for CardSource {
    type Error = RefscapeError;
    fn try_from(mut document: SourceDocument) -> Result<Self, Self::Error> {
        document
            .validate()
            .map_err(|e| RefscapeError::new(ErrorKind::InvalidData, e))?;
        let mut folds = FoldState::default();
        for (index, context) in document.context.iter_mut().enumerate() {
            let count = u32::try_from(context.code.lines().count())
                .map_err(|_| invalid("Context is too long"))?;
            let end = context
                .start_line
                .checked_add(count)
                .ok_or_else(|| invalid("Context line overflow"))?;
            if let Some(gap) = document.expanded.iter().find(|gap| {
                gap.start_line > context.start_line
                    && gap.start_line.checked_add(gap.code.lines().count() as u32) == Some(end)
            }) {
                let header_count = (gap.start_line - context.start_line) as usize;
                let boundary = context
                    .code
                    .split_inclusive('\n')
                    .take(header_count)
                    .map(str::len)
                    .sum::<usize>();
                context.code.truncate(boundary);
                if context.code.ends_with('\n') {
                    context.code.pop();
                    if context.code.ends_with('\r') {
                        context.code.pop();
                    }
                }
                document.folded.push(gap.clone());
                folds.expanded.insert(FoldId(index as u64));
            }
        }
        document.expanded.clear();
        let gaps = document
            .context
            .iter()
            .enumerate()
            .map(|(index, _)| {
                document.folded_range(index).map(|range| {
                    let content = document
                        .folded
                        .iter()
                        .find(|gap| gap.start_line == range.start)
                        .cloned();
                    let text = content.as_ref().map(|context| {
                        record_materialized(context.code.len());
                        Arc::from(context.code.as_str())
                    });
                    SourceGap {
                        id: FoldId(index as u64),
                        content: content.map_or(GapContent::MissingLegacy, |context| {
                            GapContent::Available(Arc::new(context))
                        }),
                        range,
                        text,
                    }
                })
            })
            .collect();
        let segments = document
            .context
            .iter()
            .map(|context| {
                record_materialized(context.code.len());
                Arc::from(context.code.as_str())
            })
            .collect();
        let gutter_width = document.code_gutter_width();
        let body_line_count = document.code.lines().count();
        record_materialized(document.code.len());
        let code = Arc::from(document.code);
        let tokens = Arc::new(std::mem::take(&mut document.tokens));
        let revision = next_revision()?;
        let snapshot = Arc::new(CardSourceSnapshot {
            id: SnapshotId::new(format!("source:{}", revision.0))?,
            symbol: document.symbol,
            code,
            context: Arc::new(document.context),
            code_start: document.code_start,
            tokens,
            gaps,
            revision,
            document_fingerprint: None,
            body_line_count,
            segments,
            gutter_width,
        });
        let projection = Arc::new(build_projection(&snapshot, &folds)?);
        Ok(Self {
            snapshot,
            folds,
            projection,
        })
    }
}
impl CardSource {
    pub fn with_document_fingerprint(mut self, fingerprint: DocumentFingerprint) -> Self {
        Arc::make_mut(&mut self.snapshot).document_fingerprint = Some(fingerprint);
        self
    }
    /// Body-only count used by the established summary label, independent of folds.
    pub fn body_line_count(&self) -> usize {
        self.snapshot.body_line_count
    }
    pub fn snapshot(&self) -> &Arc<CardSourceSnapshot> {
        &self.snapshot
    }
    pub fn folds(&self) -> &FoldState {
        &self.folds
    }
    pub fn projection(&self) -> &SourceProjection {
        &self.projection
    }
    pub fn metrics(&self) -> CardMetrics {
        self.projection.metrics
    }
    pub fn shared_projection(&self) -> Arc<SourceProjection> {
        self.projection.clone()
    }
    pub fn display_lines(&self) -> Vec<SourceLine<'_>> {
        self.projection.display_lines()
    }
    pub fn display_row(&self, position: Position) -> Option<usize> {
        self.projection.display_row(position)
    }
    pub fn display_anchor_row(&self, position: Position) -> Option<usize> {
        self.projection.display_anchor_row(position)
    }
    pub fn code_gutter_width(&self) -> f32 {
        self.snapshot.gutter_width
    }
    pub fn contains_display_position(&self, position: Position) -> bool {
        self.projection
            .display_row(position)
            .and_then(|row| self.projection.rows.get(row))
            .is_some_and(|row| {
                let start = row.position().unwrap_or_default();
                position.character >= start.character
                    && position
                        .character
                        .checked_sub(start.character)
                        .is_some_and(|column| {
                            crate::utf16_byte_offset(row.text(), column).is_some()
                                && column < (row.text().encode_utf16().count() as u32)
                        })
            })
    }
    pub fn variable_token(&self, position: Position) -> Option<&SemanticToken> {
        self.projection
            .token_indices(position.line)
            .iter()
            .filter_map(|index| self.tokens.get(*index))
            .find(|token| {
                token.start <= position.character
                    && token
                        .start
                        .checked_add(token.length)
                        .is_some_and(|end| position.character < end)
                    && matches!(token.kind.as_str(), "variable" | "parameter" | "property")
            })
    }
    pub fn folded_range(&self, index: usize) -> Option<Range<u32>> {
        self.snapshot
            .gaps
            .get(index)?
            .as_ref()
            .map(|gap| gap.range.clone())
            .filter(|_| !self.folds.is_expanded(FoldId(index as u64)))
    }
    pub fn expanded_context(&self, index: usize) -> Option<&SourceContext> {
        let gap = self.snapshot.gaps.get(index)?.as_ref()?;
        if !self.folds.is_expanded(gap.id) {
            return None;
        }
        match &gap.content {
            GapContent::Available(context) => Some(context),
            GapContent::MissingLegacy => None,
        }
    }
    pub fn toggle_fold(&mut self, index: usize) -> Result<FoldToggle, RefscapeError> {
        let gap = self
            .snapshot
            .gaps
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("No omitted source at this context"))?;
        if !self.folds.is_expanded(gap.id) && matches!(gap.content, GapContent::MissingLegacy) {
            return Ok(FoldToggle::MissingLegacy {
                range: gap.range.clone(),
            });
        }
        let mut folds = self.folds.clone();
        let result = if folds.expanded.remove(&gap.id) {
            FoldToggle::Collapsed
        } else {
            folds.expanded.insert(gap.id);
            FoldToggle::Expanded
        };
        folds.revision = FoldRevision(
            folds
                .revision
                .0
                .checked_add(1)
                .ok_or_else(|| invalid("Fold revision overflow"))?,
        );
        let projection = Arc::new(build_projection(&self.snapshot, &folds)?);
        self.folds = folds;
        self.projection = projection;
        Ok(result)
    }
    pub fn with_gap(
        &self,
        index: usize,
        context: SourceContext,
        tokens: Vec<SemanticToken>,
    ) -> Result<Self, RefscapeError> {
        let gap = self
            .snapshot
            .gaps
            .get(index)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("No omitted source at this context"))?;
        if context.start_line != gap.range.start
            || context.code.lines().count() != (gap.range.end - gap.range.start) as usize
        {
            return Err(invalid("Fetched gap does not match omitted range"));
        }
        if !matches!(gap.content, GapContent::MissingLegacy) {
            return Ok(self.clone());
        }
        let mut snapshot = (*self.snapshot).clone();
        let index_map =
            TextIndex::with_origin(&context.code, Position::new(context.start_line, 0))?;
        if tokens.iter().any(|token| {
            token.length == 0
                || token.start.checked_add(token.length).is_none()
                || !gap.range.contains(&token.line)
        }) {
            return Err(invalid("Fetched gap contains invalid semantic tokens"));
        }
        for token in &tokens {
            index_map.byte_offset(Position::new(token.line, token.start))?;
            index_map.byte_offset(Position::new(
                token.line,
                token
                    .start
                    .checked_add(token.length)
                    .ok_or_else(|| invalid("Semantic token column overflow"))?,
            ))?;
        }
        let updated_gap = snapshot.gaps[index]
            .as_mut()
            .ok_or_else(|| invalid("Missing gap"))?;
        updated_gap.text = Some(Arc::from(context.code.as_str()));
        record_materialized(context.code.len());
        updated_gap.content = GapContent::Available(Arc::new(context));
        Arc::make_mut(&mut snapshot.tokens).extend(tokens);
        snapshot.revision = next_revision()?;
        snapshot.id = SnapshotId::new(format!("source:{}", snapshot.revision.0))?;
        let snapshot = Arc::new(snapshot);
        let projection = Arc::new(build_projection(&snapshot, &self.folds)?);
        Ok(Self {
            snapshot,
            folds: self.folds.clone(),
            projection,
        })
    }
    pub fn export_context(&self) -> Vec<SourceContext> {
        self.context
            .iter()
            .enumerate()
            .map(|(index, header)| {
                let Some(gap) = self.expanded_context(index) else {
                    record_exported(header.code.len());
                    return header.clone();
                };
                let separator = if header.code.ends_with('\n') {
                    ""
                } else {
                    "\n"
                };
                record_exported(header.code.len() + separator.len() + gap.code.len());
                SourceContext {
                    start_line: header.start_line,
                    code: format!("{}{separator}{}", header.code, gap.code),
                }
            })
            .collect()
    }
    pub fn export_folded(&self) -> Vec<&SourceContext> {
        self.snapshot
            .gaps
            .iter()
            .flatten()
            .filter(|gap| !self.folds.is_expanded(gap.id))
            .filter_map(|gap| match &gap.content {
                GapContent::Available(context) => Some(context.as_ref()),
                _ => None,
            })
            .collect()
    }
    pub fn export_expanded(&self) -> Vec<&SourceContext> {
        self.snapshot
            .gaps
            .iter()
            .flatten()
            .filter(|gap| self.folds.is_expanded(gap.id))
            .filter_map(|gap| match &gap.content {
                GapContent::Available(context) => Some(context.as_ref()),
                _ => None,
            })
            .collect()
    }
    pub fn to_document(&self) -> SourceDocument {
        record_exported(
            self.code.len()
                + self
                    .export_folded()
                    .iter()
                    .chain(self.export_expanded().iter())
                    .map(|context| context.code.len())
                    .sum::<usize>(),
        );
        SourceDocument {
            symbol: self.symbol.clone(),
            code: self.code.to_string(),
            tokens: (*self.tokens).clone(),
            context: self.export_context(),
            code_start: self.code_start,
            folded: self.export_folded().into_iter().cloned().collect(),
            expanded: self.export_expanded().into_iter().cloned().collect(),
        }
    }
    /// Construction and gap hydration validate source once; immutable snapshots
    /// need no further traversal on geometry or camera commits.
    pub fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
fn invalid(message: &str) -> RefscapeError {
    RefscapeError::new(ErrorKind::InvalidData, message)
}
fn next_revision() -> Result<SourceRevision, RefscapeError> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        value.checked_add(1)
    })
    .map(SourceRevision)
    .map_err(|_| invalid("Source revision overflow"))
}
fn append_rows(
    rows: &mut Vec<ProjectionRow>,
    line_runs: &mut Vec<projection::LineRun>,
    text: Arc<str>,
    start: Position,
    fold: Option<usize>,
) -> Result<(), RefscapeError> {
    let first_row =
        u32::try_from(rows.len()).map_err(|_| invalid("Projection has too many rows"))?;
    let fold = fold
        .map(|index| {
            u32::try_from(index)
                .ok()
                .and_then(|index| index.checked_add(1))
                .and_then(NonZeroU32::new)
                .ok_or_else(|| invalid("Projection has too many fold controls"))
        })
        .transpose()?;
    let text = Arc::new(projection::ProjectionText::new(text));
    let mut offset = 0;
    let mut last_line = None;
    for (index, part) in text.0.split_inclusive('\n').enumerate() {
        let content = part.strip_suffix('\n').unwrap_or(part);
        let content = if part.ends_with('\n') {
            content.strip_suffix('\r').unwrap_or(content)
        } else {
            content
        };
        let line = start
            .line
            .checked_add(u32::try_from(index).map_err(|_| invalid("Source has too many lines"))?)
            .ok_or_else(|| invalid("Source line overflow"))?;
        u32::try_from(rows.len()).map_err(|_| invalid("Projection has too many rows"))?;
        last_line = Some(SourceLineNumber(line));
        rows.push(ProjectionRow::source(
            Position::new(line, if index == 0 { start.character } else { 0 }),
            text.clone(),
            offset..offset + content.len(),
            if index == 0 { fold } else { None },
        ));
        offset += part.len();
    }
    if let Some(last_line) = last_line {
        line_runs.push(projection::LineRun {
            first_line: SourceLineNumber(start.line),
            last_line,
            first_row,
        });
    }
    Ok(())
}
fn build_projection(
    snapshot: &CardSourceSnapshot,
    folds: &FoldState,
) -> Result<SourceProjection, RefscapeError> {
    PROJECTION_BUILDS.fetch_add(1, Ordering::Relaxed);
    let mut row_count = snapshot.body_line_count;
    for (index, context) in snapshot.context.iter().enumerate() {
        row_count = row_count
            .checked_add(context.code.lines().count())
            .ok_or_else(|| invalid("Projection row count overflow"))?;
        if let Some(gap) = &snapshot.gaps[index] {
            let count = if folds.is_expanded(gap.id) {
                (gap.range.end - gap.range.start) as usize
            } else {
                1
            };
            row_count = row_count
                .checked_add(count)
                .ok_or_else(|| invalid("Projection row count overflow"))?;
        }
    }
    let mut rows = Vec::with_capacity(row_count);
    let mut line_runs = Vec::new();
    let mut gap_rows = Vec::new();
    for (index, context) in snapshot.context.iter().enumerate() {
        append_rows(
            &mut rows,
            &mut line_runs,
            snapshot.segments[index].clone(),
            Position::new(context.start_line, 0),
            None,
        )?;
        if let Some(gap) = &snapshot.gaps[index] {
            if folds.is_expanded(gap.id) {
                if let GapContent::Available(context) = &gap.content {
                    append_rows(
                        &mut rows,
                        &mut line_runs,
                        gap.text
                            .as_ref()
                            .ok_or_else(|| invalid("Available gap has no segment"))?
                            .clone(),
                        Position::new(context.start_line, 0),
                        Some(index),
                    )?;
                } else {
                    return Err(invalid("Cannot expand source absent from legacy snapshot"));
                }
            } else {
                let fold = u32::try_from(index)
                    .ok()
                    .and_then(|index| index.checked_add(1))
                    .and_then(NonZeroU32::new)
                    .ok_or_else(|| invalid("Projection has too many fold controls"))?;
                let label: Arc<str> = Arc::from(format!(
                    "    ... (Show {} Lines)",
                    gap.range.end - gap.range.start
                ));
                let length = label.len();
                gap_rows.push((gap.range.clone(), rows.len()));
                rows.push(ProjectionRow::gap(
                    Arc::new(projection::ProjectionText::new(label)),
                    0..length,
                    fold,
                ));
            }
        }
    }
    append_rows(
        &mut rows,
        &mut line_runs,
        snapshot.code.clone(),
        snapshot.code_start.unwrap_or(snapshot.symbol.range.start),
        None,
    )?;
    let mut token_indices = BTreeMap::<_, Vec<_>>::new();
    for (index, token) in snapshot.tokens.iter().enumerate() {
        token_indices
            .entry(SourceLineNumber(token.line))
            .or_default()
            .push(index);
    }
    let gutter_width = snapshot.gutter_width;
    let longest = rows
        .iter()
        .map(|row| {
            row.text()
                .chars()
                .map(|character| {
                    if character == '\t' {
                        4
                    } else if character.is_ascii() {
                        1
                    } else {
                        2
                    }
                })
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0);
    let metrics = CardMetrics {
        width: (longest as f32 * 8.0 + gutter_width + 20.0).max(520.0),
        height: (crate::CODE_CARD_HEADER
            + rows.len().max(1) as f32 * crate::CODE_LINE_HEIGHT
            + 24.0)
            .max(128.0),
        gutter_width,
    };
    Ok(SourceProjection {
        rows,
        line_runs,
        token_indices,
        gap_rows,
        source_revision: snapshot.revision,
        fold_revision: folds.revision,
        metrics,
    })
}

static TEXT_MATERIALIZED: AtomicU64 = AtomicU64::new(0);
static TEXT_EXPORTED: AtomicU64 = AtomicU64::new(0);
static PROJECTION_BUILDS: AtomicU64 = AtomicU64::new(0);
fn record_materialized(bytes: usize) {
    TEXT_MATERIALIZED.fetch_add(bytes as u64, Ordering::Relaxed);
}
fn record_exported(bytes: usize) {
    TEXT_EXPORTED.fetch_add(bytes as u64, Ordering::Relaxed);
}

/// Actual source-layer allocation work. Counters are cumulative and monotonic;
/// measurements compare before/after on an otherwise idle benchmark process.
/// They count materialized code/context/gap bytes, explicit v1 export bytes and
/// projection construction, rather than claiming allocator-wide statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceWorkCounters {
    pub text_bytes_materialized: u64,
    pub text_bytes_exported: u64,
    pub projection_builds: u64,
}
pub fn source_work_counters() -> SourceWorkCounters {
    SourceWorkCounters {
        text_bytes_materialized: TEXT_MATERIALIZED.load(Ordering::Relaxed),
        text_bytes_exported: TEXT_EXPORTED.load(Ordering::Relaxed),
        projection_builds: PROJECTION_BUILDS.load(Ordering::Relaxed),
    }
}
