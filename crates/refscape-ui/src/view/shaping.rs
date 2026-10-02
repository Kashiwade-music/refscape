//! Source glyph mapping, connection anchors, syntax runs, and variable highlights.
use super::{
    HEADER, LINE, input,
    painting::{PaintedCard, card_bounds},
    render::color,
    scene,
};
use gpui::{Bounds, Pixels, TextRun, Window, point, px, size};
use refscape_application::{ApplicationSnapshot, VariableInspection};
use refscape_model::{CodeCard, Palette, Point, Position};
use std::ops::Range;

pub(super) struct CodeConnection {
    pub(super) source_card: String,
    pub(super) underline: Bounds<Pixels>,
    pub(super) start: gpui::Point<Pixels>,
    pub(super) exit: gpui::Point<Pixels>,
    pub(super) end: gpui::Point<Pixels>,
}

/// The measured word edge at zoom one is independent of camera and clicked glyph.
pub(super) fn symbol_anchor_offset(
    card: &CodeCard,
    painted: &PaintedCard,
    position: Position,
) -> Option<Point> {
    let (row, word) = connected_word(card, position)?;
    let line = &painted.row(row)?.world_code;
    Some(Point::new(
        card.source.code_gutter_width() + f32::from(line.x_for_index(word.end)),
        HEADER + 8.0 + row as f32 * LINE,
    ))
}

/// Locate the displayed word using the server's absolute UTF-16 token range.
/// Without semantic tokens, use ordinary text word selection, never code analysis.
pub(super) fn connected_word(card: &CodeCard, position: Position) -> Option<(usize, Range<usize>)> {
    refscape_application::navigation::source_word(&card.source, position)
}
pub(super) fn code_connections(
    session: &ApplicationSnapshot,
    canvas: Bounds<Pixels>,
    cache: &mut scene::SceneCache,
    window: &mut Window,
) -> Vec<CodeConnection> {
    if session.viewport.zoom < 0.65 {
        return vec![];
    }
    let zoom = session.viewport.zoom;
    cache.index(session);
    session
        .connections
        .iter()
        .filter_map(|connection| {
            let from = session.cards.get(cache.card(&connection.from)?)?;
            let to = session.cards.get(cache.card(&connection.to)?)?;
            let (row, span) = connected_word(from, connection.source)?;
            let rect = card_bounds(from, session, canvas);
            let target = card_bounds(to, session, canvas);
            // Conservatively bound the entire route before glyph work. Endpoints
            // may both be outside the viewport while their route crosses it.
            let source_y = rect.top() + px((HEADER + 8.0 + row as f32 * LINE) * zoom);
            let target_y = target.top() + px(HEADER * zoom * 0.5);
            let left = rect.left().min(target.left() - px(24.0 * zoom));
            let right = (rect.right() + px(24.0 * zoom)).max(target.left());
            let top = source_y.min(target_y) - px(LINE * zoom);
            let bottom = source_y.max(target_y) + px(LINE * zoom);
            if right < canvas.left()
                || left > canvas.right()
                || bottom < canvas.top()
                || top > canvas.bottom()
            {
                return None;
            }
            let source = from.source.projection().rows.get(row)?;
            cache.frame_work.edge_rows += 1;
            let text = source.text();
            let runs = code_runs(
                text,
                connection.source.line,
                source.position()?.character,
                from.source
                    .projection()
                    .token_indices(connection.source.line)
                    .iter()
                    .filter_map(|index| from.source.tokens.get(*index)),
                &session.theme.palette,
            );
            let (line, _) = cache.shape(&from.source, row, &runs, zoom, window);
            let x = rect.left() + px(from.source.code_gutter_width() * zoom);
            let y = rect.top()
                + px((HEADER + 8.0 + row as f32 * LINE) * zoom)
                + gpui::underline_y_offset(px(LINE), line.ascent, line.descent) * zoom;
            let underline = Bounds::new(
                point(x + line.x_for_index(span.start) * zoom, y),
                size(
                    (line.x_for_index(span.end) - line.x_for_index(span.start)) * zoom,
                    px((1.5 * zoom).max(1.0)),
                ),
            );
            let start = point(underline.right(), y + underline.size.height / 2.0);
            cache.frame_work.painted_edges += 1;
            Some(CodeConnection {
                source_card: from.id.to_string(),
                underline,
                start,
                exit: point(rect.right() + px(24.0 * zoom), start.y),
                end: point(target.left(), target.top() + px(HEADER * zoom * 0.5)),
            })
        })
        .collect()
}
#[cfg(test)]
pub(super) fn card_title(session: &ApplicationSnapshot, card: &CodeCard) -> String {
    let mut cache = scene::SceneCache::default();
    cache.index(session);
    cache.title(card)
}
/// Intersect absolute UTF-16 ranges with an excerpt's displayed line.
pub(super) fn variable_highlight_spans(
    card: &CodeCard,
    row: usize,
    inspection: Option<&VariableInspection>,
) -> Vec<Range<usize>> {
    let Some(inspection) =
        inspection.filter(|inspection| inspection.path == card.source.symbol.path)
    else {
        return Vec::new();
    };
    let Some(source) = card.source.projection().rows.get(row) else {
        return Vec::new();
    };
    let Some(position) = source.position() else {
        return Vec::new();
    };
    let text = source.text();
    let line = position.line;
    let first_character = position.character;
    let Some(last_character) = u32::try_from(text.encode_utf16().count())
        .ok()
        .and_then(|length| first_character.checked_add(length))
    else {
        return Vec::new();
    };
    inspection
        .highlights
        .iter()
        .filter_map(|range| {
            if line < range.start.line || line > range.end.line {
                return None;
            }
            let start = if line == range.start.line {
                range.start.character.max(first_character)
            } else {
                first_character
            };
            let end = if line == range.end.line {
                range.end.character.min(last_character)
            } else {
                last_character
            };
            if start >= end {
                return None;
            }
            Some(
                refscape_model::utf16_byte_offset(text, start - first_character)?
                    ..refscape_model::utf16_byte_offset(text, end - first_character)?,
            )
        })
        .collect()
}

pub(super) fn code_runs<'a>(
    text: &str,
    line: u32,
    first_character: u32,
    tokens: impl Iterator<Item = &'a refscape_model::SemanticToken>,
    palette: &Palette,
) -> Vec<TextRun> {
    let mut runs = vec![];
    let mut cursor = 0;
    let font = gpui::font("Cascadia Code");
    let make = |len, value: &str| TextRun {
        len,
        font: font.clone(),
        color: color(value),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let mut tokens: Vec<_> = tokens.filter(|token| token.line == line).collect();
    tokens.sort_by_key(|token| token.start);
    for token in tokens {
        let start =
            input::utf16_to_byte(text, token.start.saturating_sub(first_character) as usize);
        let end = input::utf16_to_byte(
            text,
            token
                .start
                .saturating_add(token.length)
                .saturating_sub(first_character) as usize,
        );
        if start < cursor || end <= start {
            continue;
        }
        if start > cursor {
            runs.push(make(start - cursor, &palette.text));
        }
        let value = match token.kind.as_str() {
            "keyword" | "modifier" => &palette.syntax_keyword,
            "string" | "number" | "regexp" => &palette.syntax_string,
            "type" | "struct" | "class" | "enum" | "interface" | "typeParameter" | "namespace" => {
                &palette.syntax_type
            }
            "function" | "method" | "macro" => &palette.syntax_function,
            "comment" => &palette.muted,
            _ => &palette.text,
        };
        runs.push(make(end - start, value));
        cursor = end;
    }
    if cursor < text.len() {
        runs.push(make(text.len() - cursor, &palette.text));
    }
    if runs.is_empty() {
        runs.push(make(text.len(), &palette.text));
    }
    runs
}
