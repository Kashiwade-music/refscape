//! Code rows have independent control, right-aligned line-number, and source columns.
use super::*;
use crate::view::shaping::{code_runs, variable_highlight_spans};

pub(super) fn paint_code(
    card: &CodeCard,
    session: &Session,
    context_hover: Option<&(String, usize)>,
    inspection: Option<&VariableInspection>,
    areas: (Bounds<Pixels>, Bounds<Pixels>),
    window: &mut Window,
    cx: &mut App,
) -> (gpui::Point<Pixels>, Vec<PaintedRow>) {
    let zoom = session.viewport.zoom;
    let (rect, canvas) = areas;
    let palette = &session.theme.palette;
    let origin = point(
        rect.left() + px(card.source.code_gutter_width() * zoom),
        rect.top() + px((HEADER + 8.0) * zoom),
    );
    if zoom < 0.65 {
        text(
            format!(
                "{} · {} lines",
                card.source.symbol.kind,
                card.source.code.lines().count()
            ),
            origin,
            11.0,
            color(&palette.muted),
            window,
            cx,
        );
        return (origin, vec![]);
    }
    let mut rows = Vec::new();
    for (index, source) in card.source.display_lines().into_iter().enumerate() {
        let y = origin.y + px(index as f32 * LINE * zoom);
        let mut runs = code_runs(
            &source.text,
            source.position.map_or(u32::MAX, |position| position.line),
            source.position.map_or(0, |position| position.character),
            &card.source.tokens,
            palette,
        );
        if source.position.is_none() {
            for run in &mut runs {
                run.color = color(&palette.muted);
            }
        }
        let world_code =
            window
                .text_system()
                .shape_line(source.text.to_string().into(), px(12.0), &runs, None);
        let code = window.text_system().shape_line(
            source.text.into_owned().into(),
            px(12.0 * zoom),
            &runs,
            None,
        );
        let number = source.position.map(|position| {
            let label = (u64::from(position.line) + 1).to_string();
            let run = TextRun {
                len: label.len(),
                font: gpui::font("Consolas"),
                color: color(&palette.muted),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window.text_system().shape_line(
                label.clone().into(),
                px(10.0 * zoom),
                &[run],
                None,
            );
            let right = origin.x - px(12.0 * zoom);
            let number_origin = point(right - line.x_for_index(label.len()), y);
            PaintedNumber {
                line,
                origin: number_origin,
            }
        });
        let row = PaintedRow {
            code,
            world_code,
            position: source.position,
            fold: source.fold,
            number,
        };
        if y + px(LINE * zoom) < canvas.top() || y >= canvas.bottom() {
            rows.push(row);
            continue;
        }
        if row.position.is_none() {
            window.paint_quad(fill(
                Bounds::new(
                    point(rect.left(), y),
                    size(rect.size.width, px(LINE * zoom)),
                ),
                color(&palette.surface_alt).opacity(0.35),
            ));
        }
        if let Some(section) = row.fold {
            let hovered =
                context_hover.is_some_and(|(id, index)| id == &card.id && *index == section);
            if hovered {
                window.paint_quad(fill(
                    Bounds::new(
                        point(rect.left() + px(4.0 * zoom), y),
                        size(px(18.0 * zoom), px(LINE * zoom)),
                    ),
                    color(&palette.border).opacity(0.65),
                ));
            }
            text(
                if row.position.is_some() { "⌃" } else { "↕" }.into(),
                point(rect.left() + px(8.0 * zoom), y),
                12.0 * zoom,
                color(if hovered {
                    &palette.text
                } else {
                    &palette.muted
                }),
                window,
                cx,
            );
        }
        for span in variable_highlight_spans(card, index, inspection) {
            window.paint_quad(fill(
                Bounds::new(
                    point(origin.x + row.code.x_for_index(span.start), y),
                    size(
                        row.code.x_for_index(span.end) - row.code.x_for_index(span.start),
                        px(LINE * zoom),
                    ),
                ),
                color(&palette.accent).opacity(0.22),
            ));
        }
        if let Some(number) = &row.number {
            let _ = number.line.paint(
                number.origin,
                px(LINE * zoom),
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
        let _ = row.code.paint(
            point(origin.x, y),
            px(LINE * zoom),
            TextAlign::Left,
            None,
            window,
            cx,
        );
        rows.push(row);
    }
    (origin, rows)
}
