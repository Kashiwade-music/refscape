use crate::{ErrorKind, Position, RefscapeError, SourceRange, Utf16Column};
use std::{ops::Range, path::PathBuf, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedLine {
    start: usize,
    end: usize,
    boundaries: Vec<(Utf16Column, usize)>,
}

/// UTF-8 bytes and UTF-16 columns refer to the original bytes. CRLF terminators
/// are excluded from the line text; a terminal newline adds an empty LSP line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextIndex {
    lines: Vec<IndexedLine>,
    origin: Position,
    byte_len: usize,
}
impl TextIndex {
    pub fn new(text: &str) -> Result<Self, RefscapeError> {
        Self::with_origin(text, Position::default())
    }
    pub fn with_origin(text: &str, origin: Position) -> Result<Self, RefscapeError> {
        let mut lines = Vec::new();
        let mut start = 0;
        for part in text.split_inclusive('\n') {
            let content = part.strip_suffix('\n').unwrap_or(part);
            let content = if part.ends_with('\n') {
                content.strip_suffix('\r').unwrap_or(content)
            } else {
                content
            };
            lines.push(Self::line(content, start)?);
            start += part.len();
        }
        if text.is_empty() || text.ends_with('\n') {
            lines.push(Self::line("", start)?);
        }
        let last =
            u32::try_from(lines.len() - 1).map_err(|_| invalid("Source has too many lines"))?;
        origin
            .line
            .checked_add(last)
            .ok_or_else(|| invalid("Source line overflow"))?;
        if let Some(first) = lines.first() {
            origin
                .character
                .checked_add(first.boundaries.last().map_or(0, |v| v.0.0))
                .ok_or_else(|| invalid("Source column overflow"))?;
        }
        Ok(Self {
            lines,
            origin,
            byte_len: text.len(),
        })
    }
    fn line(text: &str, start: usize) -> Result<IndexedLine, RefscapeError> {
        let mut units = 0u32;
        let mut boundaries = Vec::with_capacity(text.len().min(4096));
        for (byte, ch) in text.char_indices() {
            boundaries.push((Utf16Column(units), start + byte));
            units = units
                .checked_add(ch.len_utf16() as u32)
                .ok_or_else(|| invalid("Source column overflow"))?;
        }
        boundaries.push((Utf16Column(units), start + text.len()));
        Ok(IndexedLine {
            start,
            end: start + text.len(),
            boundaries,
        })
    }
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
    pub fn line_range(&self, line: u32) -> Result<Range<usize>, RefscapeError> {
        let index = line
            .checked_sub(self.origin.line)
            .ok_or_else(|| invalid("Source line precedes excerpt"))?;
        self.lines
            .get(index as usize)
            .map(|v| v.start..v.end)
            .ok_or_else(|| invalid("Source line is outside document"))
    }
    pub fn byte_offset(&self, position: Position) -> Result<usize, RefscapeError> {
        let index = position
            .line
            .checked_sub(self.origin.line)
            .ok_or_else(|| invalid("Source line precedes excerpt"))?;
        let column = if index == 0 {
            position
                .character
                .checked_sub(self.origin.character)
                .ok_or_else(|| invalid("Source column precedes excerpt"))?
        } else {
            position.character
        };
        let line = self
            .lines
            .get(index as usize)
            .ok_or_else(|| invalid("Source line is outside document"))?;
        let boundary = line
            .boundaries
            .binary_search_by_key(&Utf16Column(column), |v| v.0)
            .map_err(|_| invalid("Invalid UTF-16 column or split surrogate"))?;
        Ok(line.boundaries[boundary].1)
    }
    pub fn range_bytes(&self, range: SourceRange) -> Result<Range<usize>, RefscapeError> {
        if range.start > range.end {
            return Err(invalid("Source range starts after its end"));
        }
        Ok(self.byte_offset(range.start)?..self.byte_offset(range.end)?)
    }
    pub fn slice<'a>(&self, text: &'a str, range: SourceRange) -> Result<&'a str, RefscapeError> {
        text.get(self.range_bytes(range)?)
            .ok_or_else(|| invalid("Text does not match index"))
    }
    pub fn byte_len(&self) -> usize {
        self.byte_len
    }
}
fn invalid(message: &str) -> RefscapeError {
    RefscapeError::new(ErrorKind::InvalidData, message)
}

#[derive(Debug, Clone)]
pub struct DocumentSnapshot {
    pub path: PathBuf,
    pub version: u64,
    pub text: Arc<str>,
    pub index: TextIndex,
}
impl DocumentSnapshot {
    pub fn new(path: PathBuf, version: u64, text: Arc<str>) -> Result<Self, RefscapeError> {
        if path.as_os_str().is_empty() {
            return Err(invalid("Document path must be nonempty"));
        }
        let index = TextIndex::new(&text)?;
        Ok(Self {
            path,
            version,
            text,
            index,
        })
    }
}
