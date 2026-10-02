//! UI-independent validated values, immutable source snapshots and operation lifetimes.

pub mod canvas;
pub mod error;
pub mod geometry;
pub mod id;
pub mod operation;
pub mod project;
pub mod source;
pub mod theme;
pub use canvas::*;
pub use error::*;
pub use geometry::*;
pub use id::*;
pub use operation::*;
pub use project::*;
pub use source::*;
pub use theme::*;
pub const MIN_ZOOM: f32 = 0.15;
pub const MAX_ZOOM: f32 = 3.0;

#[cfg(test)]
mod geometry_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folded_display_retains_source_coordinates_and_counts_context_in_card_height() {
        let range = SourceRange {
            start: Position::new(74, 4),
            end: Position::new(74, 20),
        };
        let mut source = SourceDocument {
            expanded: Vec::new(),
            folded: Vec::new(),
            symbol: Symbol::file("project.rs".into(), range),
            code: "    fn options() {}".into(),
            code_start: Some(Position::new(74, 0)),
            tokens: vec![],
            context: vec![SourceContext {
                start_line: 51,
                code: "impl CppProject {".into(),
            }],
        };
        source.validate().unwrap();
        let card_source = CardSource::try_from(source.clone()).unwrap();
        let lines = card_source.display_lines();
        assert_eq!(lines[0].position, Some(Position::new(51, 0)));
        assert_eq!(lines[1].position, None);
        assert_eq!(lines[1].text, "    ... (Show 22 Lines)");
        assert_eq!(lines[2].position, Some(Position::new(74, 0)));
        assert_eq!(card_source.display_row(Position::new(74, 7)), Some(2));
        assert_eq!(
            CodeCard::source_height(&CardSource::try_from(source.clone()).unwrap()),
            136.0
        );
        source.context[0].start_line = 74;
        assert!(source.validate().is_err());
    }

    #[test]
    fn gutter_reserves_full_source_line_numbers_even_while_context_is_folded() {
        let range = SourceRange {
            start: Position::new(10_000, 4),
            end: Position::new(10_000, 20),
        };
        let mut source = SourceDocument {
            symbol: Symbol::file("project.rs".into(), range),
            code: "    fn options() {}".into(),
            tokens: vec![],
            code_start: None,
            context: vec![SourceContext {
                start_line: 9_998,
                code: "impl Project {".into(),
            }],
            folded: vec![],
            expanded: vec![],
        };
        source.validate().unwrap();
        assert_eq!(source.code_gutter_width(), 76.0);
        assert_eq!(
            CardSource::try_from(source.clone())
                .unwrap()
                .display_lines()[1]
                .text,
            "    ... (Show 1 Lines)"
        );
        source.context[0].code.push_str("\n    fn first() {}\n");
        source.validate().unwrap();
        assert_eq!(source.code_gutter_width(), 76.0);
    }

    #[test]
    fn utf16_offsets_preserve_unicode_boundaries() {
        assert_eq!(utf16_byte_offset("a😀猫", 1), Some(1));
        assert_eq!(utf16_byte_offset("a😀猫", 2), None);
        assert_eq!(utf16_byte_offset("a😀猫", 3), Some(5));
        assert_eq!(utf16_byte_offset("a😀猫", 4), Some(8));
        assert_eq!(utf16_byte_offset("a😀猫", 5), None);
    }
}
