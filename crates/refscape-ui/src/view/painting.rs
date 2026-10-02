//! Canvas geometry and GPUI painting.
mod code;

use super::{render::color, scene, shaping::code_connections};
use code::paint_code;
use gpui::{
    App, Bounds, PathBuilder, Pixels, ShapedLine, TextAlign, TextRun, Window, fill, point, px,
    quad, size,
};
use refscape_application::{ApplicationSnapshot, VariableInspection};
use refscape_model::{CODE_REGION_HEADER, CODE_REGION_PADDING, CodeCard, Point, Position};

pub(super) struct PaintedCard {
    pub(super) id: String,
    pub(super) bounds: Bounds<Pixels>,
    pub(super) rows: Vec<PaintedRow>,
    pub(super) origin: gpui::Point<Pixels>,
    pub(super) first_row: usize,
    pub(super) source_revision: refscape_model::SourceRevision,
    pub(super) fold_revision: refscape_model::FoldRevision,
    pub(super) viewport: refscape_model::Viewport,
}

/// A row keeps its source mapping and each painted column together.
pub(super) struct PaintedRow {
    pub(super) code: ShapedLine,
    pub(super) world_code: ShapedLine,
    pub(super) position: Option<Position>,
    pub(super) fold: Option<usize>,
    pub(super) number: Option<PaintedNumber>,
}

pub(super) struct PaintedNumber {
    pub(super) line: ShapedLine,
    pub(super) origin: gpui::Point<Pixels>,
}
fn text(
    text: String,
    origin: gpui::Point<Pixels>,
    font_size: f32,
    color: gpui::Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let run = TextRun {
        len: text.len(),
        font: gpui::font("Segoe UI"),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text.into(), px(font_size), &[run], None);
    let _ = line.paint(
        origin,
        px(font_size * 1.45),
        TextAlign::Left,
        None,
        window,
        cx,
    );
}
pub(super) fn card_height(card: &CodeCard) -> f32 {
    card.display_height()
}
pub(super) fn card_bounds(
    card: &CodeCard,
    session: &ApplicationSnapshot,
    canvas: Bounds<Pixels>,
) -> Bounds<Pixels> {
    bounds_at(card, card.position.point(), session, canvas)
}

pub(super) fn bounds_at(
    card: &CodeCard,
    position: Point,
    session: &ApplicationSnapshot,
    canvas: Bounds<Pixels>,
) -> Bounds<Pixels> {
    let position = session.viewport.world_to_screen(position);
    Bounds::new(
        point(
            canvas.left() + px(position.x),
            canvas.top() + px(position.y),
        ),
        size(
            px(card.width * session.viewport.zoom),
            px(card_height(card) * session.viewport.zoom),
        ),
    )
}

pub(super) fn paint_canvas(
    session: &ApplicationSnapshot,
    interaction: (Option<&str>, Option<&(String, usize)>),
    inspection: Option<&VariableInspection>,
    cache: &mut scene::SceneCache,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<PaintedCard> {
    cache.begin_frame();
    cache.index(session);
    let (selected, context_hover) = interaction;
    let palette = &session.theme.palette;
    let zoom = session.viewport.zoom;
    let detail = scene::DetailLevel::from_zoom(zoom);
    let step = (32.0 * zoom).max(12.0);
    let mut y = session.viewport.offset.y.rem_euclid(step);
    while y < f32::from(bounds.size.height) {
        let mut x = session.viewport.offset.x.rem_euclid(step);
        while x < f32::from(bounds.size.width) {
            window.paint_quad(fill(
                Bounds::new(
                    point(bounds.left() + px(x), bounds.top() + px(y)),
                    size(px(1.0), px(1.0)),
                ),
                color(&palette.border),
            ));
            x += step;
        }
        y += step;
    }
    for region in session.regions.iter() {
        let is_crate = matches!(
            region.kind,
            refscape_model::RegionKind::Project | refscape_model::RegionKind::Crate
        );
        if is_crate != (detail == scene::DetailLevel::Region) {
            continue;
        }
        let rects: Vec<_> = region
            .card_ids
            .iter()
            .filter_map(|id| cache.card(id).and_then(|index| session.cards.get(index)))
            .map(|card| card_bounds(card, session, bounds))
            .collect();
        if rects.is_empty() {
            continue;
        }
        let left = rects
            .iter()
            .map(|r| f32::from(r.left()))
            .fold(f32::INFINITY, f32::min)
            - CODE_REGION_PADDING * zoom;
        let top = rects
            .iter()
            .map(|r| f32::from(r.top()))
            .fold(f32::INFINITY, f32::min)
            - CODE_REGION_HEADER * zoom;
        let right = rects
            .iter()
            .map(|r| f32::from(r.right()))
            .fold(f32::NEG_INFINITY, f32::max)
            + CODE_REGION_PADDING * zoom;
        let bottom = rects
            .iter()
            .map(|r| f32::from(r.bottom()))
            .fold(f32::NEG_INFINITY, f32::max)
            + CODE_REGION_PADDING * zoom;
        window.paint_quad(quad(
            Bounds::new(
                point(px(left), px(top)),
                size(px(right - left), px(bottom - top)),
            ),
            px(10.0),
            color(&palette.surface_alt).opacity(0.35),
            px(1.0),
            color(&palette.border),
            Default::default(),
        ));
        text(
            region.label.clone(),
            point(px(left + 12.0), px(top + 8.0)),
            (13.0 * zoom).max(10.0),
            color(&palette.muted),
            window,
            cx,
        );
    }
    let connections = code_connections(session, bounds, cache, window);
    for connection in &connections {
        let end = connection.end;
        let mut path = PathBuilder::stroke(connection.underline.size.height);
        path.move_to(connection.start);
        path.line_to(connection.exit);
        path.line_to(point(end.x - px(24.0 * zoom), end.y));
        path.line_to(end);
        if let Ok(path) = path.build() {
            window.paint_path(path, color(&palette.connection));
        }
        let mut arrow = PathBuilder::stroke(px(1.5));
        arrow.move_to(point(end.x - px(7.0), end.y - px(4.0)));
        arrow.line_to(end);
        arrow.line_to(point(end.x - px(7.0), end.y + px(4.0)));
        if let Ok(path) = arrow.build() {
            window.paint_path(path, color(&palette.connection));
        }
    }
    let mut painted = vec![];
    for card in session.cards.iter() {
        let rect = card_bounds(card, session, bounds);
        if detail == scene::DetailLevel::Region
            || rect.right() < bounds.left()
            || rect.left() > bounds.right()
            || rect.bottom() < bounds.top()
            || rect.top() > bounds.bottom()
        {
            continue;
        }
        window.paint_quad(quad(
            rect,
            px(7.0 * zoom),
            color(&palette.surface),
            px(if selected == Some(card.id.as_str()) {
                2.0
            } else {
                1.0
            }),
            color(if selected == Some(card.id.as_str()) {
                &palette.accent
            } else {
                &palette.border
            }),
            Default::default(),
        ));
        let (origin, first_row, rows) =
            window.with_content_mask(Some(gpui::ContentMask { bounds: rect }), |window| {
                text(
                    cache.title(card),
                    point(rect.left() + px(16.0 * zoom), rect.top() + px(8.0 * zoom)),
                    (13.0 * zoom).max(9.0),
                    color(&palette.text),
                    window,
                    cx,
                );
                text(
                    card.source
                        .symbol
                        .path
                        .strip_prefix(&session.project_root)
                        .unwrap_or(&card.source.symbol.path)
                        .display()
                        .to_string(),
                    point(rect.left() + px(16.0 * zoom), rect.top() + px(29.0 * zoom)),
                    (10.0 * zoom).max(8.0),
                    color(&palette.muted),
                    window,
                    cx,
                );
                text(
                    "×".into(),
                    point(rect.right() - px(22.0 * zoom), rect.top() + px(8.0 * zoom)),
                    (15.0 * zoom).max(9.0),
                    color(&palette.muted),
                    window,
                    cx,
                );
                let (origin, first_row, rows) = paint_code(
                    card,
                    session,
                    (context_hover, inspection),
                    cache,
                    (rect, bounds),
                    window,
                    cx,
                );
                for connection in connections
                    .iter()
                    .filter(|connection| connection.source_card == card.id)
                {
                    window.paint_quad(fill(connection.underline, color(&palette.connection)));
                    // Paint the part inside the card above its background; the outer
                    // route stays behind cards so unrelated source text is never crossed.
                    let mut path = PathBuilder::stroke(connection.underline.size.height);
                    path.move_to(connection.start);
                    path.line_to(connection.exit);
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color(&palette.connection));
                    }
                }
                (origin, first_row, rows)
            });
        painted.push(PaintedCard {
            id: card.id.to_string(),
            bounds: rect,
            rows,
            origin,
            first_row,
            source_revision: card.source.projection().source_revision,
            fold_revision: card.source.projection().fold_revision,
            viewport: session.viewport,
        });
    }
    if session.cards.is_empty() {
        text(
            "Your code, connected.".into(),
            point(bounds.left() + px(80.0), bounds.top() + px(100.0)),
            30.0,
            color(&palette.text),
            window,
            cx,
        );
        text(
            "Open a project, then choose a file or search for a symbol.".into(),
            point(bounds.left() + px(80.0), bounds.top() + px(150.0)),
            14.0,
            color(&palette.muted),
            window,
            cx,
        );
        text(
            "Definitions and references unfold as cards on this canvas.".into(),
            point(bounds.left() + px(80.0), bounds.top() + px(180.0)),
            14.0,
            color(&palette.muted),
            window,
            cx,
        );
    }
    painted
}

impl PaintedCard {
    pub(super) fn current(&self, snapshot: &ApplicationSnapshot) -> bool {
        self.viewport == snapshot.viewport
            && snapshot
                .cards
                .iter()
                .find(|card| card.id.as_str() == self.id)
                .is_some_and(|card| {
                    card.source.projection().source_revision == self.source_revision
                        && card.source.projection().fold_revision == self.fold_revision
                })
    }
    pub(super) fn row(&self, index: usize) -> Option<&PaintedRow> {
        self.rows.get(index.checked_sub(self.first_row)?)
    }
}
impl PaintedRow {
    pub(super) fn source_position(&self, x: Pixels) -> Option<Position> {
        let origin = self.position?;
        let byte = self.code.index_for_x(x)?;
        let units = u32::try_from(self.code.text.get(..byte)?.encode_utf16().count()).ok()?;
        Some(Position::new(
            origin.line,
            origin.character.checked_add(units)?,
        ))
    }
}
