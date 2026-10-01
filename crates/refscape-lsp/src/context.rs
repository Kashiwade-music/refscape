//! Declaration context derived from server-owned symbol ranges, never indentation parsing.
use refscape_model::{SourceContext, SourceRange, Symbol};

pub(crate) fn excerpt_range(text: &str, range: SourceRange) -> SourceRange {
    let mut range = range;
    if let Some(line) = text.lines().nth(range.start.line as usize)
        && let Some(end) = refscape_model::utf16_byte_offset(line, range.start.character)
        && line[..end].chars().all(char::is_whitespace)
    {
        range.start.character = 0;
    }
    range
}

pub(crate) fn source_context(
    text: &str,
    symbols: &[Symbol],
    target: &Symbol,
) -> Vec<SourceContext> {
    fn collect<'a>(symbols: &'a [Symbol], target: &Symbol, ancestors: &mut Vec<&'a Symbol>) {
        for symbol in symbols {
            if symbol.range.start <= target.range.start
                && symbol.range.end >= target.range.end
                && symbol.range != target.range
            {
                if symbol.kind != "file" && symbol.range.start.line < target.range.start.line {
                    ancestors.push(symbol);
                }
                collect(&symbol.children, target, ancestors);
            }
        }
    }
    let mut ancestors = Vec::new();
    collect(symbols, target, &mut ancestors);
    ancestors.sort_by_key(|symbol| (symbol.range.start, std::cmp::Reverse(symbol.range.end)));
    let lines: Vec<_> = text.lines().collect();
    let mut context = Vec::new();
    let mut next_line = 0;
    for ancestor in ancestors {
        let start = ancestor.range.start.line.max(next_line);
        let end = ancestor
            .selection_range
            .end
            .line
            .min(target.range.start.line - 1);
        if start > end {
            continue;
        }
        if let Some(header) = lines.get(start as usize..=end as usize) {
            context.push(SourceContext {
                start_line: start,
                code: header.join("\n"),
            });
            next_line = end + 1;
        }
    }
    context
}

#[cfg(test)]
mod tests {
    use super::*;
    use refscape_model::Position;

    fn symbol(name: &str, start: u32, end: u32, children: Vec<Symbol>) -> Symbol {
        Symbol {
            id: name.into(),
            name: name.into(),
            kind: "namespace".into(),
            path: "source.rs".into(),
            range: SourceRange {
                start: Position::new(start, 4),
                end: Position::new(end, 1),
            },
            selection_range: SourceRange {
                start: Position::new(start, 4),
                end: Position::new(start, 8),
            },
            children,
        }
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
        let context = source_context(text, &[module], &target);
        assert_eq!(
            context,
            vec![
                SourceContext {
                    start_line: 0,
                    code: "mod outer {".into()
                },
                SourceContext {
                    start_line: 2,
                    code: "    impl CppProject {".into()
                },
            ]
        );
        assert_eq!(excerpt_range(text, target.range).start, Position::new(5, 0));
        assert_eq!(
            excerpt_range(
                "let x = call();",
                SourceRange {
                    start: Position::new(0, 8),
                    end: Position::new(0, 12)
                }
            )
            .start
            .character,
            8
        );
    }
}
