//! Source glyph mapping, connection anchors, syntax runs, and variable highlights.
use super::*;

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
    let line = &painted.rows.get(row)?.world_code;
    Some(Point::new(
        card.source.code_gutter_width() + f32::from(line.x_for_index(word.end)),
        HEADER + 8.0 + row as f32 * LINE,
    ))
}

/// Locate the displayed word using the server's absolute UTF-16 token range.
/// Without semantic tokens, use ordinary text word selection, never code analysis.
pub(super) fn connected_word(card: &CodeCard, position: Position) -> Option<(usize, Range<usize>)> {
    if !card.source.contains_display_position(position) {
        return None;
    }
    let row = card.source.display_row(position)?;
    let lines = card.source.display_lines();
    let text = lines.get(row)?.text.as_ref();
    let first_character = lines[row].position?.character;
    if let Some(token) = card.source.tokens.iter().find(|token| {
        token.line == position.line
            && token.start <= position.character
            && token
                .start
                .checked_add(token.length)
                .is_some_and(|end| position.character < end)
    }) {
        let start = token.start.saturating_sub(first_character);
        let end = token
            .start
            .checked_add(token.length)?
            .saturating_sub(first_character);
        let start = refscape_model::utf16_byte_offset(text, start)?;
        let end = refscape_model::utf16_byte_offset(text, end)?;
        return (end > start).then_some((row, start..end));
    }
    let byte =
        refscape_model::utf16_byte_offset(text, position.character.checked_sub(first_character)?)?;
    let is_word = |ch: char| ch.is_alphanumeric() || ch == '_';
    if !text[byte..].chars().next().is_some_and(is_word) {
        return None;
    }
    let start = text[..byte]
        .char_indices()
        .rev()
        .take_while(|(_, ch)| is_word(*ch))
        .last()
        .map_or(byte, |(index, _)| index);
    let end = text[byte..]
        .char_indices()
        .take_while(|(_, ch)| is_word(*ch))
        .last()
        .map(|(index, ch)| byte + index + ch.len_utf8())?;
    Some((row, start..end))
}

pub(super) fn code_connections(
    session: &Session,
    canvas: Bounds<Pixels>,
    window: &mut Window,
) -> Vec<CodeConnection> {
    if session.viewport.zoom < 0.65 {
        return vec![];
    }
    let zoom = session.viewport.zoom;
    session
        .connections
        .iter()
        .filter_map(|connection| {
            let from = session
                .cards
                .iter()
                .find(|card| card.id == connection.from)?;
            let to = session.cards.iter().find(|card| card.id == connection.to)?;
            let (row, span) = connected_word(from, connection.source)?;
            let lines = from.source.display_lines();
            let text = lines.get(row)?.text.as_ref();
            let runs = code_runs(
                text,
                connection.source.line,
                lines[row].position?.character,
                &from.source.tokens,
                &session.theme.palette,
            );
            let line =
                window
                    .text_system()
                    .shape_line(text.to_string().into(), px(12.0), &runs, None);
            let rect = card_bounds(from, session, canvas);
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
            let target = card_bounds(to, session, canvas);
            let start = point(underline.right(), y + underline.size.height / 2.0);
            Some(CodeConnection {
                source_card: from.id.clone(),
                underline,
                start,
                exit: point(rect.right() + px(24.0 * zoom), start.y),
                end: point(target.left(), target.top() + px(HEADER * zoom * 0.5)),
            })
        })
        .collect()
}
pub(super) fn card_title(session: &Session, card: &CodeCard) -> String {
    let mut variables = Vec::new();
    for edge in session
        .connections
        .iter()
        .filter(|edge| edge.to == card.id && edge.kind == ConnectionKind::TypeDefinition)
    {
        let Some(origin) = session.cards.iter().find(|origin| origin.id == edge.from) else {
            continue;
        };
        if let Some((row, span)) = connected_word(origin, edge.source)
            && let Some(line) = origin.source.display_lines().get(row)
        {
            let name = line.text[span].to_string();
            if !variables.contains(&name) {
                variables.push(name);
            }
        }
    }
    if variables.is_empty() {
        card.source.symbol.name.clone()
    } else {
        format!("{} → {}", variables.join(", "), card.source.symbol.name)
    }
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
    let lines = card.source.display_lines();
    let Some(source) = lines.get(row) else {
        return Vec::new();
    };
    let Some(position) = source.position else {
        return Vec::new();
    };
    let text = source.text.as_ref();
    let line = position.line;
    let first_character = position.character;
    let last_character = first_character + text.encode_utf16().count() as u32;
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

pub(super) fn code_runs(
    text: &str,
    line: u32,
    first_character: u32,
    tokens: &[refscape_model::SemanticToken],
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
    let mut tokens: Vec<_> = tokens.iter().filter(|token| token.line == line).collect();
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
