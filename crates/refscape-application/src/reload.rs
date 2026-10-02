//! Disk comparison and fresh official analysis run only on the effect worker.
use crate::{Result, editing::RefreshedCard, executor::acquire};
use refscape_analysis::AnalysisSession;
use refscape_model::{
    CardSource, CodeCard, DocumentFingerprint, ErrorKind, OperationContext, Position,
    RefscapeError, SourceRange, Symbol, TextIndex,
};
use std::{collections::BTreeMap, fs, io::Read, path::Path, time::Duration};

fn io(error: std::io::Error, path: &Path) -> RefscapeError {
    RefscapeError::new(
        ErrorKind::Io,
        format!("Cannot reload {}: {error}", path.display()),
    )
    .with_path(path)
}
fn read_document(path: &Path, context: &OperationContext) -> Result<Option<String>> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io(error, path)),
    };
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        context.check()?;
        let count = file.read(&mut buffer).map_err(|error| io(error, path))?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    String::from_utf8(bytes).map(Some).map_err(|error| {
        io(
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
            path,
        )
    })
}
fn settle(context: &OperationContext) -> Result<()> {
    // Coalesce editor replacement/partial-write windows without blocking the owner.
    for _ in 0..5 {
        context.check()?;
        std::thread::park_timeout(Duration::from_millis(10));
    }
    context.check()
}
fn matches_saved(source: &CardSource, text: &str, index: &TextIndex) -> bool {
    if source.symbol.kind == "file" {
        return source.code.as_ref() == text;
    }
    let matches = |position, code: &str| {
        index
            .byte_offset(position)
            .ok()
            .and_then(|start| text.get(start..))
            .is_some_and(|remaining| remaining.starts_with(code))
    };
    matches(
        source.code_start.unwrap_or(source.symbol.range.start),
        &source.code,
    ) && source
        .context
        .iter()
        .all(|context| matches(Position::new(context.start_line, 0), &context.code))
        && source.gaps.iter().flatten().all(|gap| match &gap.content {
            refscape_model::GapContent::Available(context) => {
                matches(Position::new(context.start_line, 0), &context.code)
            }
            refscape_model::GapContent::MissingLegacy => true,
        })
}
fn replacement<'a>(old: &Symbol, symbols: &'a [Symbol]) -> Option<&'a Symbol> {
    let mut pending: Vec<_> = symbols.iter().collect();
    let mut candidates = Vec::new();
    let mut same_position = Vec::new();
    while let Some(symbol) = pending.pop() {
        if symbol.kind == old.kind && symbol.range.start == old.range.start {
            same_position.push(symbol);
        }
        if symbol.name == old.name && symbol.kind == old.kind {
            if symbol.id == old.id {
                return Some(symbol);
            }
            candidates.push(symbol);
        }
        pending.extend(&symbol.children);
    }
    // Do not silently retarget a card to an ambiguous same-named declaration.
    if candidates.len() == 1 {
        Some(candidates[0])
    } else if candidates.is_empty() && same_position.len() == 1 {
        Some(same_position[0])
    } else {
        None
    }
}
fn same_excerpt(a: &CardSource, b: &CardSource) -> bool {
    let relevant =
        |source: &CardSource, line: u32| {
            let line = u64::from(line);
            let start = u64::from(source.code_start.unwrap_or(source.symbol.range.start).line);
            (start..start + source.body_line_count() as u64).contains(&line)
                || source.context.iter().any(|context| {
                    let start = u64::from(context.start_line);
                    (start..start + context.code.lines().count() as u64).contains(&line)
                })
                || source.gaps.iter().flatten().any(|gap| {
                    (u64::from(gap.range.start)..u64::from(gap.range.end)).contains(&line)
                })
        };
    a.symbol == b.symbol
        && a.code == b.code
        && a.code_start == b.code_start
        && a.context == b.context
        && a.gaps == b.gaps
        && a.tokens
            .iter()
            .filter(|token| relevant(a, token.line))
            .eq(b.tokens.iter().filter(|token| relevant(b, token.line)))
}
pub(crate) fn refresh_sources(
    session: &mut dyn AnalysisSession,
    cards: &[CodeCard],
    context: &OperationContext,
) -> Result<Vec<RefreshedCard>> {
    context.check()?;
    if !session.supports_source_reload() {
        return Ok(Vec::new());
    }
    let mut files = BTreeMap::<_, Vec<_>>::new();
    for card in cards {
        files
            .entry(&card.source.symbol.path)
            .or_default()
            .push(card);
    }
    let mut refreshed = Vec::new();
    for (path, cards) in files {
        context.check()?;
        let text = match read_document(path, context)? {
            Some(text) => text,
            None => {
                settle(context)?;
                if read_document(path, context)?.is_some() {
                    return Err(RefscapeError::new(
                        ErrorKind::Stale,
                        "File replaced during reload",
                    ));
                }
                refreshed.extend(cards.into_iter().map(|card| RefreshedCard {
                    id: card.id.clone(),
                    source: None,
                    content_changed: true,
                }));
                continue;
            }
        };
        context.check()?;
        let fingerprint = DocumentFingerprint::of(text.as_bytes());
        if cards
            .iter()
            .all(|card| card.source.document_fingerprint == Some(fingerprint))
        {
            continue;
        }
        let index = if cards
            .iter()
            .any(|card| card.source.document_fingerprint.is_none())
        {
            Some(TextIndex::new(&text)?)
        } else {
            None
        };
        let changed = cards.iter().any(|card| {
            card.source.document_fingerprint.is_some()
                || !matches_saved(&card.source, &text, index.as_ref().unwrap())
        });
        if changed {
            settle(context)?;
            if read_document(path, context)?
                .as_ref()
                .map(|text| DocumentFingerprint::of(text.as_bytes()))
                != Some(fingerprint)
            {
                return Err(RefscapeError::new(
                    ErrorKind::Stale,
                    "File changed during reload",
                ));
            }
        }
        let mut symbols = None;
        for card in cards {
            context.check()?;
            if card.source.document_fingerprint == Some(fingerprint) {
                continue;
            }
            if card.source.document_fingerprint.is_none()
                && matches_saved(&card.source, &text, index.as_ref().unwrap())
            {
                refreshed.push(RefreshedCard {
                    id: card.id.clone(),
                    source: Some(card.source.clone().with_document_fingerprint(fingerprint)),
                    content_changed: false,
                });
                continue;
            }
            let old = &card.source.symbol;
            let symbol = if old.kind == "file" {
                let line = u32::try_from(text.bytes().filter(|byte| *byte == b'\n').count())
                    .map_err(|_| "Document has too many lines")?;
                let tail = text
                    .rsplit('\n')
                    .next()
                    .unwrap_or_default()
                    .trim_end_matches('\r');
                let column = u32::try_from(tail.encode_utf16().count())
                    .map_err(|_| "Document line is too long")?;
                Some(Symbol::file(
                    path.clone(),
                    SourceRange {
                        start: Position::default(),
                        end: Position::new(line, column),
                    },
                ))
            } else {
                if symbols.is_none() {
                    symbols = Some(session.symbols(path, context)?);
                }
                replacement(old, symbols.as_ref().unwrap()).cloned()
            };
            let source = symbol
                .map(|symbol| acquire(session, &symbol, context))
                .transpose()?;
            let source = match source {
                Some(source) => {
                    if source
                        .document_fingerprint
                        .is_some_and(|current| current != fingerprint)
                    {
                        return Err(RefscapeError::new(
                            ErrorKind::Stale,
                            "File changed during reload",
                        ));
                    }
                    Some(source.with_document_fingerprint(fingerprint))
                }
                None => None,
            };
            let content_changed = source
                .as_ref()
                .is_none_or(|source| !same_excerpt(&card.source, source));
            let source = if content_changed {
                source
            } else {
                Some(card.source.clone().with_document_fingerprint(fingerprint))
            };
            refreshed.push(RefreshedCard {
                id: card.id.clone(),
                source,
                content_changed,
            });
        }
    }
    context.check()?;
    Ok(refreshed)
}
