use crate::{CardMetrics, FoldRevision, Position, SourceLine, SourceLineNumber, SourceRevision};
use std::{borrow::Cow, collections::BTreeMap, num::NonZeroU32, ops::Range, sync::Arc};

/// One thin shared owner per original segment, rather than a fat text pointer per row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectionText(pub(crate) Arc<str>);
impl ProjectionText {
    pub(crate) fn new(text: Arc<str>) -> Self {
        Self(text)
    }
}

/// Validated source segments are disjoint and ordered. Row lookup needs one run
/// per segment; it does not need a separate allocation for each source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineRun {
    pub first_line: SourceLineNumber,
    pub last_line: SourceLineNumber,
    pub first_row: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionRow {
    position: Option<Position>,
    text: Arc<ProjectionText>,
    bytes: Range<usize>,
    fold: Option<NonZeroU32>,
}
impl ProjectionRow {
    pub(crate) fn source(
        position: Position,
        text: Arc<ProjectionText>,
        bytes: Range<usize>,
        fold: Option<NonZeroU32>,
    ) -> Self {
        Self {
            position: Some(position),
            text,
            bytes,
            fold,
        }
    }
    pub(crate) fn gap(text: Arc<ProjectionText>, bytes: Range<usize>, fold: NonZeroU32) -> Self {
        Self {
            position: None,
            text,
            bytes,
            fold: Some(fold),
        }
    }
    pub fn text(&self) -> &str {
        &self.text.0[self.bytes.clone()]
    }
    pub fn position(&self) -> Option<Position> {
        self.position
    }
    pub fn fold(&self) -> Option<usize> {
        self.fold.map(|fold| (fold.get() - 1) as usize)
    }
}

/// A single indexed row projection shared by rendering, hit testing and metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceProjection {
    pub rows: Vec<ProjectionRow>,
    pub source_revision: SourceRevision,
    pub fold_revision: FoldRevision,
    pub metrics: CardMetrics,
    pub(crate) line_runs: Vec<LineRun>,
    pub(crate) token_indices: BTreeMap<SourceLineNumber, Vec<usize>>,
    pub(crate) gap_rows: Vec<(Range<u32>, usize)>,
}
impl SourceProjection {
    pub fn display_row(&self, position: Position) -> Option<usize> {
        let line = SourceLineNumber(position.line);
        let index = self
            .line_runs
            .partition_point(|run| run.first_line <= line)
            .checked_sub(1)?;
        let run = &self.line_runs[index];
        if line > run.last_line {
            return None;
        }
        run.first_row
            .checked_add(line.0.checked_sub(run.first_line.0)?)
            .map(|row| row as usize)
    }
    pub fn display_anchor_row(&self, position: Position) -> Option<usize> {
        self.display_row(position).or_else(|| {
            let index = self
                .gap_rows
                .partition_point(|(range, _)| range.start <= position.line)
                .checked_sub(1)?;
            let (range, row) = &self.gap_rows[index];
            range.contains(&position.line).then_some(*row)
        })
    }
    pub fn token_indices(&self, line: u32) -> &[usize] {
        self.token_indices
            .get(&SourceLineNumber(line))
            .map_or(&[], Vec::as_slice)
    }
    pub fn display_lines(&self) -> Vec<SourceLine<'_>> {
        self.rows
            .iter()
            .map(|row| SourceLine {
                position: row.position(),
                text: Cow::Borrowed(row.text()),
                fold: row.fold(),
            })
            .collect()
    }
}
