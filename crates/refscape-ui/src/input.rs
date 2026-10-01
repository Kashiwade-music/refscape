//! UTF-16-aware native text input, including IME composition.
use super::ExplorerView;
use gpui::{Bounds, Context, EntityInputHandler, Pixels, UTF16Selection, Window};
use refscape_application::{LanguageService, SessionRepository};
use std::ops::Range;

pub(crate) fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut count = 0;
    for (byte, ch) in text.char_indices() {
        if count >= offset {
            return byte;
        }
        count += ch.len_utf16();
    }
    text.len()
}
fn byte_to_utf16(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].encode_utf16().count()
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> ExplorerView<L, R> {
    pub(crate) fn replace_query(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        if self.closing {
            return;
        }
        let range = range
            .or_else(|| self.query_marked.clone())
            .unwrap_or_else(|| self.query_selection.clone());
        let text = text.replace(['\n', '\r'], " ");
        self.query.replace_range(range.clone(), &text);
        self.query_selection = range.start + text.len()..range.start + text.len();
        self.query_marked = None;
        cx.notify();
    }
    fn utf16_range(&self, range: Range<usize>) -> Range<usize> {
        byte_to_utf16(&self.query, range.start)..byte_to_utf16(&self.query, range.end)
    }
    fn byte_range(&self, range: Range<usize>) -> Range<usize> {
        utf16_to_byte(&self.query, range.start)..utf16_to_byte(&self.query, range.end)
    }
}

impl<L: LanguageService + 'static, R: SessionRepository + 'static> EntityInputHandler
    for ExplorerView<L, R>
{
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.byte_range(range);
        *adjusted = Some(self.utf16_range(range.clone()));
        Some(self.query[range].into())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.utf16_range(self.query_selection.clone()),
            reversed: false,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.query_marked.clone().map(|r| self.utf16_range(r))
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.query_marked = None;
        cx.notify();
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range.map(|range| self.byte_range(range));
        self.replace_query(range, text, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| self.byte_range(r))
            .or_else(|| self.query_marked.clone())
            .unwrap_or_else(|| self.query_selection.clone());
        let start = range.start;
        self.replace_query(Some(range), text, cx);
        if !text.is_empty() {
            self.query_marked = Some(start..start + text.len());
        }
        if let Some(selected) = selected {
            self.query_selection = start + utf16_to_byte(text, selected.start)
                ..start + utf16_to_byte(text, selected.end);
        }
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.byte_range(range);
        let line = self.query_line.as_ref()?;
        Some(Bounds::from_corners(
            gpui::point(
                bounds.left() + gpui::px(8.0) + line.x_for_index(range.start),
                bounds.top(),
            ),
            gpui::point(
                bounds.left() + gpui::px(8.0) + line.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }
    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.query_bounds?;
        let line = self.query_line.as_ref()?;
        Some(byte_to_utf16(
            &self.query,
            line.closest_index_for_x(point.x - bounds.left() - gpui::px(8.0))
                .min(self.query.len()),
        ))
    }
    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        self.search_focus && !self.closing
    }
    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.query_selection = self.byte_range(range);
        cx.notify();
    }
    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.query.encode_utf16().count())
    }
}

#[cfg(test)]
mod tests {
    use super::utf16_to_byte;
    #[test]
    fn source_offsets_respect_surrogate_pairs_and_multibyte_characters() {
        assert_eq!(utf16_to_byte("a😀日本", 1), 1);
        assert_eq!(utf16_to_byte("a😀日本", 3), 5);
        assert_eq!(utf16_to_byte("a😀日本", 4), 8);
        assert_eq!(utf16_to_byte("a😀日本", 100), 11);
    }
}
