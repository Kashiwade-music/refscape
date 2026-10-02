//! Code rows have independent control, right-aligned line-number, and source columns.
use super::{PaintedNumber, PaintedRow, text};
use crate::view::shaping::{code_runs, variable_highlight_spans};
use crate::view::{HEADER, LINE, render::color, scene};
use gpui::{App, Bounds, Pixels, TextAlign, TextRun, Window, fill, point, px, size};
use refscape_application::{ApplicationSnapshot, VariableInspection};
use refscape_model::CodeCard;

pub(super) fn paint_code(
    card: &CodeCard,
    session: &ApplicationSnapshot,
    interaction: (Option<&(String, usize)>, Option<&VariableInspection>),
    cache: &mut scene::SceneCache,
    areas: (Bounds<Pixels>, Bounds<Pixels>),
    window: &mut Window,
    cx: &mut App,
) -> (gpui::Point<Pixels>, usize, Vec<PaintedRow>) {
    let (context_hover, inspection) = interaction;
    let zoom = session.viewport.zoom;
    let (rect, canvas) = areas;
    let palette = &session.theme.palette;
    let origin = point(
        rect.left() + px(card.source.code_gutter_width() * zoom),
        rect.top() + px((HEADER + 8.0) * zoom),
    );
    if scene::DetailLevel::from_zoom(zoom) != scene::DetailLevel::Code {
        cache.frame_work.summary_metric_reads += 1;
        text(
            format!(
                "{} · {} lines",
                card.source.symbol.kind,
                card.source.body_line_count()
            ),
            origin,
            11.0,
            color(&palette.muted),
            window,
            cx,
        );
        return (origin, 0, vec![]);
    }
    let mut rows = Vec::new();
    let projection = card.source.projection();
    let first_row = ((f32::from(canvas.top() - origin.y) / (LINE * zoom))
        .floor()
        .max(0.0) as usize)
        .min(projection.rows.len());
    let last_row = ((f32::from(canvas.bottom() - origin.y) / (LINE * zoom))
        .ceil()
        .max(0.0) as usize)
        .min(projection.rows.len());
    for index in first_row..last_row {
        cache.frame_work.visible_rows += 1;
        let source = &projection.rows[index];
        let y = origin.y + px(index as f32 * LINE * zoom);
        let mut runs = code_runs(
            source.text(),
            source.position().map_or(u32::MAX, |position| position.line),
            source.position().map_or(0, |position| position.character),
            projection
                .token_indices(source.position().map_or(u32::MAX, |p| p.line))
                .iter()
                .filter_map(|index| card.source.tokens.get(*index)),
            palette,
        );
        if source.position().is_none() {
            for run in &mut runs {
                run.color = color(&palette.muted);
            }
        }
        let (world_code, code) = cache.shape(&card.source, index, &runs, zoom, window);
        let number = source.position().map(|position| {
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
            position: source.position(),
            fold: source.fold(),
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
    (origin, first_row, rows)
}
