//! Word identity shared by presentation anchors and command classification.
use refscape_model::{CardSource, Position, utf16_byte_offset};
use std::ops::Range;

pub fn source_word(source: &CardSource, position: Position) -> Option<(usize, Range<usize>)> {
    if !source.contains_display_position(position) {
        return None;
    }
    let row = source.display_row(position)?;
    let projected = source.projection().rows.get(row)?;
    let text = projected.text();
    let first = projected.position()?.character;
    if let Some(token) = source
        .projection()
        .token_indices(position.line)
        .iter()
        .filter_map(|index| source.tokens.get(*index))
        .find(|token| {
            token.start <= position.character
                && token
                    .start
                    .checked_add(token.length)
                    .is_some_and(|end| position.character < end)
        })
    {
        let start = utf16_byte_offset(text, token.start.saturating_sub(first))?;
        let end = utf16_byte_offset(
            text,
            token.start.checked_add(token.length)?.saturating_sub(first),
        )?;
        return (end > start).then_some((row, start..end));
    }
    let byte = utf16_byte_offset(text, position.character.checked_sub(first)?)?;
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
