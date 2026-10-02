use super::*;
use crate::{SourceRange, Symbol};

fn excerpt() -> SourceDocument {
    SourceDocument {
        symbol: Symbol::file(
            "file.rs".into(),
            SourceRange {
                start: Position::new(5, 0),
                end: Position::new(5, 8),
            },
        ),
        code: "fn a(){}".into(),
        tokens: vec![],
        context: vec![SourceContext {
            start_line: 0,
            code: "impl A {".into(),
        }],
        code_start: None,
        folded: vec![SourceContext {
            start_line: 1,
            code: "\n    😀();\n\n}\n".into(),
        }],
        expanded: vec![],
    }
}

#[test]
fn long_projection_uses_one_sparse_run_and_compact_rows() {
    let mut document = excerpt();
    document.context.clear();
    document.folded.clear();
    document.symbol = Symbol::file(
        "file.rs".into(),
        SourceRange {
            start: Position::new(7, 0),
            end: Position::new(10007, 0),
        },
    );
    document.code = "日本😀\r\n".repeat(10000);
    let source = CardSource::try_from(document).unwrap();
    assert_eq!(source.projection().rows.len(), 10000);
    assert_eq!(source.projection().rows.capacity(), 10000);
    assert_eq!(source.projection().line_runs.len(), 1);
    assert!(std::mem::size_of::<ProjectionRow>() <= 40);
    for row in [0, 1, 127, 9999] {
        assert_eq!(
            source.display_row(Position::new(7 + row, 0)),
            Some(row as usize)
        );
        assert_eq!(source.projection().rows[row as usize].text(), "日本😀");
    }
    assert_eq!(source.display_row(Position::new(6, 0)), None);
    assert_eq!(source.display_row(Position::new(10007, 0)), None);
}

#[test]
fn sparse_projection_run_accepts_last_representable_source_line() {
    let mut document = excerpt();
    document.context.clear();
    document.folded.clear();
    document.symbol = Symbol::file(
        "file.rs".into(),
        SourceRange {
            start: Position::new(u32::MAX, 0),
            end: Position::new(u32::MAX, 1),
        },
    );
    document.code = "a".into();
    let source = CardSource::try_from(document).unwrap();
    assert_eq!(source.display_row(Position::new(u32::MAX, 0)), Some(0));
    assert_eq!(source.display_row(Position::new(u32::MAX - 1, 0)), None);
    assert_eq!(source.projection().rows[0].text(), "a");
}

#[test]
fn folds_share_original_segments_and_round_trip_sparse_source() {
    let document = excerpt();
    let mut source = CardSource::try_from(document.clone()).unwrap();
    let original = source.snapshot().clone();
    let collapsed = source.shared_projection();
    assert_eq!(source.body_line_count(), 1);
    assert_eq!(source.display_lines()[1].text, "    ... (Show 4 Lines)");
    assert_eq!(source.display_anchor_row(Position::new(2, 4)), Some(1));
    for _ in 0..20 {
        assert_eq!(source.toggle_fold(0).unwrap(), FoldToggle::Expanded);
        assert_eq!(source.body_line_count(), 1);
        assert!(Arc::ptr_eq(&original, source.snapshot()));
        assert_eq!(source.display_lines()[2].text, "    😀();");
        assert_eq!(source.display_anchor_row(Position::new(2, 4)), Some(2));
        let exported = source.to_document();
        exported.validate().unwrap();
        assert_eq!(CardSource::try_from(exported).unwrap(), source);
        assert_eq!(source.toggle_fold(0).unwrap(), FoldToggle::Collapsed);
        assert_eq!(source.to_document(), document);
    }
    assert_eq!(source.projection().rows, collapsed.rows);
}

#[test]
fn multiple_fold_sections_keep_independent_state_and_original_body_bytes() {
    let mut document = excerpt();
    document.symbol.range = SourceRange {
        start: Position::new(6, 0),
        end: Position::new(7, 0),
    };
    document.symbol.selection_range = SourceRange {
        start: Position::new(6, 0),
        end: Position::new(6, 2),
    };
    document.code = "fn a(){}\r\n".into();
    document.context.push(SourceContext {
        start_line: 3,
        code: "    fn parent() {".into(),
    });
    document.folded = vec![
        SourceContext {
            start_line: 1,
            code: "    first();\n\n".into(),
        },
        SourceContext {
            start_line: 4,
            code: "    second();\n\n".into(),
        },
    ];
    let mut source = CardSource::try_from(document.clone()).unwrap();
    let body = source.code.clone();
    source.toggle_fold(0).unwrap();
    assert_eq!(source.display_anchor_row(Position::new(5, 0)), Some(4));
    source.toggle_fold(1).unwrap();
    assert_eq!(source.display_row(Position::new(5, 0)), Some(5));
    let reimported = CardSource::try_from(source.to_document()).unwrap();
    assert_eq!(reimported, source);
    source.toggle_fold(0).unwrap();
    assert!(source.expanded_context(1).is_some());
    assert!(source.expanded_context(0).is_none());
    source.toggle_fold(1).unwrap();
    assert_eq!(source.to_document(), document);
    assert!(Arc::ptr_eq(&body, &source.code));
    assert_eq!(&*source.code, "fn a(){}\r\n");
}

#[test]
fn missing_legacy_gap_is_hydrated_only_when_requested_and_preserves_body() {
    let mut document = excerpt();
    document.folded.clear();
    let mut source = CardSource::try_from(document.clone()).unwrap();
    assert_eq!(
        source.toggle_fold(0).unwrap(),
        FoldToggle::MissingLegacy { range: 1..5 }
    );
    assert_eq!(source.to_document(), document);
    let acquired = source
        .with_gap(0, excerpt().folded[0].clone(), vec![])
        .unwrap();
    assert!(Arc::ptr_eq(
        &source.snapshot.context,
        &acquired.snapshot.context
    ));
    assert!(Arc::ptr_eq(&source.snapshot.code, &acquired.snapshot.code));
    assert_eq!(source.code, acquired.code);
    assert_eq!(source.to_document(), document);
    let mut acquired = acquired;
    assert_eq!(acquired.toggle_fold(0).unwrap(), FoldToggle::Expanded);
    assert!(acquired.display_row(Position::new(2, 4)).is_some());
}

#[test]
fn old_context_without_expanded_metadata_remains_opaque() {
    let mut document = excerpt();
    document.context[0].code = "impl A {\n\n    old();\n\n}\n".into();
    document.folded.clear();
    let source = CardSource::try_from(document.clone()).unwrap();
    assert!(source.snapshot.gaps[0].is_none());
    assert!(source.display_lines().iter().all(|row| row.fold.is_none()));
    assert_eq!(source.to_document(), document);
}

#[test]
fn source_line_overflow_is_rejected_before_projection() {
    let mut document = excerpt();
    document.context.clear();
    document.folded.clear();
    document.symbol.range = SourceRange {
        start: Position::new(u32::MAX, 0),
        end: Position::new(u32::MAX, 0),
    };
    document.symbol.selection_range = document.symbol.range;
    document.code = "a\nb".into();
    assert!(document.validate().is_err());
    assert!(CardSource::try_from(document).is_err());
}

#[test]
fn invalid_token_endpoints_are_rejected_before_projection() {
    let mut document = excerpt();
    document.code = "a😀猫".into();
    document.tokens = vec![SemanticToken {
        line: 5,
        start: 1,
        length: 1,
        kind: "variable".into(),
        modifiers: vec![],
    }];
    assert!(document.validate().is_err());
    document.tokens[0].length = 2;
    assert!(document.validate().is_ok());
    document.tokens[0].line = 9;
    assert!(document.validate().is_err());
}

#[test]
fn text_index_handles_crlf_surrogates_terminal_empty_line_and_partial_origin() {
    let text = "a😀猫\r\n\n";
    let index = TextIndex::new(text).unwrap();
    assert_eq!(index.line_count(), 3);
    assert_eq!(index.byte_offset(Position::new(0, 3)).unwrap(), 5);
    assert!(index.byte_offset(Position::new(0, 2)).is_err());
    assert_eq!(&text[index.line_range(0).unwrap()], "a😀猫");
    assert_eq!(index.byte_offset(Position::new(2, 0)).unwrap(), text.len());
    let index = TextIndex::with_origin("😀\nnext", Position::new(70, 4)).unwrap();
    assert_eq!(index.byte_offset(Position::new(70, 6)).unwrap(), 4);
    assert!(index.byte_offset(Position::new(70, 5)).is_err());
    assert_eq!(index.byte_offset(Position::new(71, 0)).unwrap(), 5);
}
