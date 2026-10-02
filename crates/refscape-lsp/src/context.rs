//! Verified official-symbol intervals and pure excerpts over an immutable text index.
use refscape_model::{
    DocumentSnapshot, ErrorKind, Position, RefscapeError, SourceContext, SourceRange, Symbol,
};
use std::{cmp::Reverse, sync::Arc};
type Result<T> = std::result::Result<T, RefscapeError>;
fn invalid(message: &str) -> RefscapeError {
    RefscapeError::new(ErrorKind::Protocol, message)
}
struct Entry {
    range: SourceRange,
    selection: SourceRange,
    parent: Option<usize>,
    child_index: usize,
    file: bool,
    preorder: usize,
}
struct Interval {
    entry: usize,
    left: Option<usize>,
    right: Option<usize>,
    max_end: Position,
}
pub(crate) struct SymbolIndex {
    symbols: Arc<Vec<Symbol>>,
    entries: Vec<Entry>,
    intervals: Vec<Interval>,
    root: Option<usize>,
}
impl SymbolIndex {
    pub fn new(snapshot: &DocumentSnapshot, symbols: Arc<Vec<Symbol>>) -> Result<Self> {
        let mut entries = vec![];
        let mut stack = symbols
            .iter()
            .enumerate()
            .rev()
            .map(|(child_index, symbol)| (symbol, None, child_index))
            .collect::<Vec<_>>();
        while let Some((symbol, parent, child_index)) = stack.pop() {
            snapshot
                .index
                .range_bytes(symbol.range)
                .map_err(|error| invalid(&error.to_string()))?;
            snapshot
                .index
                .range_bytes(symbol.selection_range)
                .map_err(|error| invalid(&error.to_string()))?;
            if symbol.selection_range.start < symbol.range.start
                || symbol.selection_range.end > symbol.range.end
            {
                return Err(invalid("symbol selection is outside its range"));
            }
            let index = entries.len();
            entries.push(Entry {
                range: symbol.range,
                selection: symbol.selection_range,
                parent,
                child_index,
                file: symbol.kind == "file",
                preorder: index,
            });
            stack.extend(
                symbol
                    .children
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(child_index, child)| (child, Some(index), child_index)),
            );
        }
        let mut order = (0..entries.len()).collect::<Vec<_>>();
        order.sort_by_key(|index| (entries[*index].range.start, entries[*index].preorder));
        fn build(order: &[usize], entries: &[Entry], nodes: &mut Vec<Interval>) -> Option<usize> {
            if order.is_empty() {
                return None;
            }
            let middle = order.len() / 2;
            let left = build(&order[..middle], entries, nodes);
            let right = build(&order[middle + 1..], entries, nodes);
            let entry = order[middle];
            let mut max_end = entries[entry].range.end;
            for node in [left, right].into_iter().flatten() {
                max_end = max_end.max(nodes[node].max_end);
            }
            let index = nodes.len();
            nodes.push(Interval {
                entry,
                left,
                right,
                max_end,
            });
            Some(index)
        }
        let mut intervals = vec![];
        let root = build(&order, &entries, &mut intervals);
        Ok(Self {
            symbols,
            entries,
            intervals,
            root,
        })
    }
    pub fn symbols(&self) -> &Arc<Vec<Symbol>> {
        &self.symbols
    }
    pub fn enclosing(&self, position: Position) -> Option<&Symbol> {
        let mut best: Option<Vec<usize>> = None;
        let mut stack = self.root.into_iter().collect::<Vec<_>>();
        while let Some(index) = stack.pop() {
            let node = &self.intervals[index];
            if node.max_end <= position {
                continue;
            }
            let entry = &self.entries[node.entry];
            if let Some(left) = node.left {
                stack.push(left);
            }
            if entry.range.start > position {
                continue;
            }
            if let Some(right) = node.right {
                stack.push(right);
            }
            if entry.range.end <= position {
                continue;
            }
            let mut path = vec![entry.child_index];
            let mut parent = entry.parent;
            let mut reachable = true;
            while let Some(index) = parent {
                let ancestor = &self.entries[index];
                if ancestor.range.start > position || ancestor.range.end <= position {
                    reachable = false;
                    break;
                }
                path.push(ancestor.child_index);
                parent = ancestor.parent;
            }
            if reachable {
                path.reverse();
                path.push(usize::MAX);
                if best.as_ref().is_none_or(|current| path < *current) {
                    best = Some(path);
                }
            }
        }
        let path = best?;
        let mut symbol = &self.symbols[path[0]];
        for &index in &path[1..path.len() - 1] {
            symbol = &symbol.children[index];
        }
        Some(symbol)
    }
    fn ancestors(&self, target: SourceRange) -> Vec<&Entry> {
        let mut found = vec![];
        let mut stack = self.root.into_iter().collect::<Vec<_>>();
        while let Some(index) = stack.pop() {
            let node = &self.intervals[index];
            if node.max_end < target.end {
                continue;
            }
            let entry = &self.entries[node.entry];
            if let Some(left) = node.left {
                stack.push(left);
            }
            if entry.range.start <= target.start {
                if let Some(right) = node.right {
                    stack.push(right);
                }
                if entry.range.end >= target.end
                    && entry.range != target
                    && !entry.file
                    && entry.range.start.line < target.start.line
                {
                    let mut parent = entry.parent;
                    let mut reachable = true;
                    while let Some(index) = parent {
                        let entry = &self.entries[index];
                        if entry.range.start > target.start
                            || entry.range.end < target.end
                            || entry.range == target
                        {
                            reachable = false;
                            break;
                        }
                        parent = entry.parent;
                    }
                    if reachable {
                        found.push(entry);
                    }
                }
            }
        }
        found.sort_by_key(|entry| (entry.range.start, Reverse(entry.range.end), entry.preorder));
        found
    }
}
pub(crate) struct ExcerptBuilder<'a> {
    snapshot: &'a DocumentSnapshot,
    index: &'a SymbolIndex,
}
impl<'a> ExcerptBuilder<'a> {
    pub fn new(snapshot: &'a DocumentSnapshot, index: &'a SymbolIndex) -> Self {
        Self { snapshot, index }
    }
    pub fn range(&self, mut range: SourceRange) -> Result<SourceRange> {
        self.snapshot.index.range_bytes(range)?;
        let line = self.snapshot.index.line_range(range.start.line)?;
        let offset = self.snapshot.index.byte_offset(range.start)?;
        if self
            .snapshot
            .text
            .get(line.start..offset)
            .ok_or_else(|| invalid("invalid declaration prefix"))?
            .chars()
            .all(char::is_whitespace)
        {
            range.start.character = 0;
        }
        Ok(range)
    }
    fn lines(&self, start: u32, end: u32, trailing_newline: bool) -> Result<String> {
        let mut code = String::new();
        for line in start..end {
            if line != start {
                code.push('\n');
            }
            let range = self.snapshot.index.line_range(line)?;
            code.push_str(
                self.snapshot
                    .text
                    .get(range)
                    .ok_or_else(|| invalid("source index mismatch"))?,
            );
        }
        if trailing_newline {
            code.push('\n');
        }
        Ok(code)
    }
    pub fn context(&self, target: &Symbol) -> Result<Vec<SourceContext>> {
        let mut context = vec![];
        let mut next_line = 0;
        for ancestor in self.index.ancestors(target.range) {
            let start = ancestor.range.start.line.max(next_line);
            let end = ancestor.selection.end.line.min(
                target
                    .range
                    .start
                    .line
                    .checked_sub(1)
                    .ok_or_else(|| invalid("ancestor at source origin"))?,
            );
            if start > end {
                continue;
            }
            next_line = end
                .checked_add(1)
                .ok_or_else(|| invalid("declaration line overflow"))?;
            context.push(SourceContext {
                start_line: start,
                code: self.lines(start, next_line, false)?,
            });
        }
        Ok(context)
    }
    pub fn gaps(
        &self,
        context: &[SourceContext],
        range: SourceRange,
    ) -> Result<Vec<SourceContext>> {
        let mut gaps = vec![];
        for (index, header) in context.iter().enumerate() {
            let count = u32::try_from(header.code.lines().count())
                .map_err(|_| invalid("context line overflow"))?;
            let start = header
                .start_line
                .checked_add(count)
                .ok_or_else(|| invalid("context line overflow"))?;
            let end = context
                .get(index + 1)
                .map_or(range.start.line, |next| next.start_line);
            if start < end {
                gaps.push(SourceContext {
                    start_line: start,
                    code: self.lines(start, end, true)?,
                });
            }
        }
        Ok(gaps)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn symbol(name: &str, start: u32, end: u32, children: Vec<Symbol>) -> Symbol {
        Symbol {
            id: name.into(),
            name: name.into(),
            kind: "namespace".into(),
            path: "source.rs".into(),
            range: SourceRange {
                start: Position::new(start, 4),
                end: Position::new(end, if start == end { 8 } else { 1 }),
            },
            selection_range: SourceRange {
                start: Position::new(start, 4),
                end: Position::new(start, 8),
            },
            children,
        }
    }
    #[test]
    fn indexed_enclosing_matches_official_order_with_overlaps_and_equal_ranges() {
        let text = "    outer name\n    first name\n    child name\n    final name\n}";
        let first = symbol("first", 1, 3, vec![symbol("child", 2, 2, vec![])]);
        let outer = symbol(
            "outer",
            0,
            4,
            vec![first.clone(), symbol("overlap", 1, 3, vec![])],
        );
        let snapshot = DocumentSnapshot::new("source.rs".into(), 1, Arc::from(text)).unwrap();
        let symbols = Arc::new(vec![outer, first]);
        let index = SymbolIndex::new(&snapshot, symbols.clone()).unwrap();
        for line in 0..5 {
            for character in 0..14 {
                let position = Position::new(line, character);
                assert_eq!(
                    index.enclosing(position),
                    crate::conversion::enclosing(&symbols, position)
                );
            }
        }
    }
    #[test]
    fn malformed_official_symbol_ranges_are_rejected_before_indexing() {
        let snapshot = DocumentSnapshot::new("source.rs".into(), 1, Arc::from("sample")).unwrap();
        let mut bad = symbol("bad", 0, 0, vec![]);
        bad.selection_range.end = Position::new(9, 0);
        assert!(
            matches!(SymbolIndex::new(&snapshot, Arc::new(vec![bad])), Err(error) if error.kind == ErrorKind::Protocol)
        );
    }
    #[test]
    fn nested_ancestors_exclude_sibling_implementations_and_preserve_indentation() {
        let text = "mod outer {\n    impl Other {}\n    impl CppProject {\n        fn first() {}\n\n        pub(crate) fn options() {}\n    }\n}";
        let target = symbol("options", 5, 5, vec![]);
        let implementation = symbol("CppProject", 2, 6, vec![target.clone()]);
        let mut module = symbol(
            "outer",
            0,
            7,
            vec![symbol("Other", 1, 1, vec![]), implementation],
        );
        module.range.start.character = 0;
        let snapshot = DocumentSnapshot::new("source.rs".into(), 1, Arc::from(text)).unwrap();
        let index = SymbolIndex::new(&snapshot, Arc::new(vec![module])).unwrap();
        let builder = ExcerptBuilder::new(&snapshot, &index);
        assert_eq!(
            builder.context(&target).unwrap(),
            vec![
                SourceContext {
                    start_line: 0,
                    code: "mod outer {".into()
                },
                SourceContext {
                    start_line: 2,
                    code: "    impl CppProject {".into()
                }
            ]
        );
        assert_eq!(
            builder.range(target.range).unwrap().start,
            Position::new(5, 0)
        );
        let snapshot =
            DocumentSnapshot::new("source.rs".into(), 1, Arc::from("let x = call();")).unwrap();
        let index = SymbolIndex::new(&snapshot, Arc::new(vec![])).unwrap();
        assert_eq!(
            ExcerptBuilder::new(&snapshot, &index)
                .range(SourceRange {
                    start: Position::new(0, 8),
                    end: Position::new(0, 12)
                })
                .unwrap()
                .start
                .character,
            8
        );
    }
}
