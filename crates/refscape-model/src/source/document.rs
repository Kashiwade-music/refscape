use crate::TextIndex;
use std::{borrow::Cow, collections::HashSet, path::PathBuf};

/// Zero-based source coordinates. `character` counts UTF-16 code units (LSP).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// Half-open source range, in document coordinates.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    pub start: Position,
    pub end: Position,
}

impl SourceRange {
    pub fn contains(self, position: Position) -> bool {
        self.start <= position && position < self.end
    }

    pub fn validate(self) -> Result<(), String> {
        if self.start > self.end {
            return Err("Source range starts after its end".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub path: PathBuf,
    pub range: SourceRange,
    pub selection_range: SourceRange,
    pub children: Vec<Symbol>,
}

impl Symbol {
    pub fn file(path: PathBuf, range: SourceRange) -> Self {
        Self {
            id: format!("{}:file", path.to_string_lossy()),
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            kind: "file".into(),
            path,
            range,
            selection_range: SourceRange {
                start: range.start,
                end: range.start,
            },
            children: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.range.validate()?;
        self.selection_range.validate()?;
        if self.id.is_empty() || self.path.as_os_str().is_empty() {
            return Err("Symbol ID and path must be nonempty".into());
        }
        if self.selection_range.start < self.range.start
            || self.selection_range.end > self.range.end
        {
            return Err("Symbol selection must lie inside its source range".into());
        }
        for child in &self.children {
            child.validate()?;
        }
        Ok(())
    }
}

/// Semantic tokens retain absolute document coordinates, even on excerpt cards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticToken {
    pub line: u32,
    pub start: u32,
    pub length: u32,
    pub kind: String,
    pub modifiers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDocument {
    pub symbol: Symbol,
    pub code: String,
    pub tokens: Vec<SemanticToken>,
    /// Ancestor declaration excerpts supplied by the official language backend.
    pub context: Vec<SourceContext>,
    /// Includes the first line's indentation without changing the symbol's identity.
    pub code_start: Option<Position>,
    /// Source snapshots of the gaps between ancestor declarations and the symbol body.
    pub folded: Vec<SourceContext>,
    /// Revealed gap snapshots retained so each section can be folded again.
    pub expanded: Vec<SourceContext>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceContext {
    pub start_line: u32,
    pub code: String,
}

pub struct SourceLine<'a> {
    pub position: Option<Position>,
    pub text: Cow<'a, str>,
    pub fold: Option<usize>,
}

impl SourceDocument {
    /// Reserve separate control and number columns using the entire excerpt's line range.
    /// Revealing hidden rows cannot change the code or line-number column's position.
    pub(crate) fn code_gutter_width(&self) -> f32 {
        let start = self.code_start.unwrap_or(self.symbol.range.start).line;
        let last = (u64::from(start) + self.code.lines().count() as u64)
            .max(u64::from(self.symbol.range.end.line) + 1);
        let digits = last.to_string().len().max(3);
        24.0 + digits as f32 * 8.0 + 12.0
    }

    pub(crate) fn expanded_context(&self, index: usize) -> Option<&SourceContext> {
        let context = self.context.get(index)?;
        let end = context
            .start_line
            .checked_add(context.code.lines().count() as u32)?;
        self.expanded.iter().find(|gap| {
            gap.start_line > context.start_line
                && gap.start_line.checked_add(gap.code.lines().count() as u32) == Some(end)
        })
    }

    pub(crate) fn folded_range(&self, index: usize) -> Option<std::ops::Range<u32>> {
        let context = self.context.get(index)?;
        let start = context
            .start_line
            .checked_add(context.code.lines().count() as u32)?;
        let end = self.context.get(index + 1).map_or(
            self.code_start.unwrap_or(self.symbol.range.start).line,
            |next| next.start_line,
        );
        (start < end).then_some(start..end)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.symbol.validate()?;
        let start = self.code_start.unwrap_or(self.symbol.range.start);
        TextIndex::with_origin(&self.code, start).map_err(|error| error.to_string())?;
        if start.line != self.symbol.range.start.line
            || start.character > self.symbol.range.start.character
        {
            return Err("Source excerpt must start on the symbol's first line".into());
        }
        let mut previous_end = None;
        for context in &self.context {
            TextIndex::with_origin(&context.code, Position::new(context.start_line, 0))
                .map_err(|error| error.to_string())?;
            let count = u32::try_from(context.code.lines().count())
                .map_err(|_| "Source context is too long")?;
            let end = context
                .start_line
                .checked_add(count)
                .ok_or("Source context line overflow")?;
            if count == 0
                || end > start.line
                || previous_end.is_some_and(|previous| previous > context.start_line)
            {
                return Err("Source context must precede the excerpt in source order".into());
            }
            previous_end = Some(end);
        }
        let mut seen = HashSet::new();
        for folded in &self.folded {
            if !seen.insert(folded.start_line)
                || !self.context.iter().enumerate().any(|(index, _)| {
                    self.folded_range(index).is_some_and(|range| {
                        range.start == folded.start_line
                            && folded.code.lines().count() == (range.end - range.start) as usize
                    })
                })
            {
                return Err("Folded source must match an omitted context range".into());
            }
        }
        for expanded in &self.expanded {
            if expanded.code.lines().count() == 0
                || !seen.insert(expanded.start_line)
                || !self.context.iter().enumerate().any(|(index, context)| {
                    self.expanded_context(index).is_some_and(|gap| {
                        gap == expanded
                            && context
                                .code
                                .lines()
                                .skip((gap.start_line - context.start_line) as usize)
                                .eq(gap.code.lines())
                    })
                })
            {
                return Err("Expanded source must match the end of its context section".into());
            }
        }
        if self
            .tokens
            .iter()
            .any(|token| token.length == 0 || token.start.checked_add(token.length).is_none())
        {
            return Err("Semantic tokens must have a valid nonempty span".into());
        }
        let body_index =
            TextIndex::with_origin(&self.code, start).map_err(|error| error.to_string())?;
        let mut indices = vec![(start.line, self.code.lines().count(), body_index)];
        for segment in self.context.iter().chain(&self.folded) {
            indices.push((
                segment.start_line,
                segment.code.lines().count(),
                TextIndex::with_origin(&segment.code, Position::new(segment.start_line, 0))
                    .map_err(|error| error.to_string())?,
            ));
        }
        for token in &self.tokens {
            let end = token
                .start
                .checked_add(token.length)
                .ok_or("Semantic token column overflow")?;
            if let Some((_, _, index)) = indices.iter().find(|(line, count, _)| {
                token
                    .line
                    .checked_sub(*line)
                    .is_some_and(|row| (row as usize) < *count)
            }) {
                // The first excerpt line can begin in the middle of a token.
                // Its absent prefix is unknown, but every recorded endpoint
                // must still be a real UTF-16 boundary.
                if token.line == start.line && token.start < start.character {
                    if end > start.character {
                        index
                            .byte_offset(Position::new(token.line, end))
                            .map_err(|error| error.to_string())?;
                    }
                } else {
                    index
                        .byte_offset(Position::new(token.line, token.start))
                        .map_err(|error| error.to_string())?;
                    index
                        .byte_offset(Position::new(token.line, end))
                        .map_err(|error| error.to_string())?;
                }
            } else if !self.context.iter().enumerate().any(|(index, _)| {
                self.folded_range(index)
                    .is_some_and(|range| range.contains(&token.line))
            }) {
                return Err("Semantic token is outside the recorded source".into());
            }
        }
        Ok(())
    }
}

/// Translate a UTF-16 column to a UTF-8 byte boundary, rejecting split surrogates.
pub fn utf16_byte_offset(text: &str, column: u32) -> Option<usize> {
    let mut units = 0_u32;
    for (offset, ch) in text.char_indices() {
        if units == column {
            return Some(offset);
        }
        units = units.checked_add(ch.len_utf16() as u32)?;
        if units > column {
            return None;
        }
    }
    (units == column).then_some(text.len())
}
