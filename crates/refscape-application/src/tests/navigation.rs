use super::*;
#[test]
fn navigation_retains_link_ranges_until_commit_and_keeps_symbol_excerpt_policy() {
    let mut h = Harness::new();
    let root = h.add("root", Point::default());
    let target = symbol("target");
    let location = NavigationLocation {
        document: target.path.clone(),
        target_range: SourceRange {
            start: Position::default(),
            end: Position::new(10, 0),
        },
        selection_range: target.selection_range,
        origin_range: Some(SourceRange {
            start: Position::new(0, 3),
            end: Position::new(0, 7),
        }),
    };
    let mut other_origin = location.clone();
    other_origin.origin_range = Some(SourceRange {
        start: Position::new(0, 7),
        end: Position::new(0, 9),
    });
    h.state.lock().unwrap().navigation_targets = Some(vec![
        NavigationTarget {
            symbol: target.clone(),
            location: location.clone(),
        },
        NavigationTarget {
            symbol: target.clone(),
            location: other_origin,
        },
    ]);
    let effect = h
        .pending(Command::Navigate {
            card: root,
            position: Position::new(0, 3),
            kind: ConnectionKind::Definition,
            anchor: Point::new(500.0, 60.0),
            toggle: false,
        })
        .pop()
        .unwrap();
    let completion = h.execute(effect);
    let Completion::AnalysisQueried {
        result:
            Ok(crate::effect::AnalysisReply::Edit {
                edit: crate::editing::PreparedEdit::Expand { sources, .. },
                ..
            }),
        ..
    } = &completion
    else {
        panic!("expected prepared navigation")
    };
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].location, location);
    assert_eq!(sources[0].source.symbol.range, target.range);
    let transition = h.finish(completion);
    h.drain(transition);
    assert_eq!(h.card("target").source.symbol.range, target.range);
    assert_eq!(h.state.lock().unwrap().source_calls, 2);
    assert_eq!(h.snapshot().connections.len(), 1);
}
#[test]
fn expansion_deduplicates_cards_and_edges_and_removal_cleans_regions() {
    let mut h = Harness::new();
    let root = h.add("origin", Point::default());
    h.state.lock().unwrap().targets = vec![symbol("target"), symbol("target")];
    for _ in 0..2 {
        h.run(Command::Navigate {
            card: root.clone(),
            position: Position::new(0, 4),
            kind: ConnectionKind::Definition,
            anchor: Point::new(520.0, 60.0),
            toggle: false,
        });
    }
    assert_eq!(h.snapshot().cards.len(), 2);
    assert_eq!(h.snapshot().connections.len(), 1);
    assert_eq!(h.snapshot().regions.len(), 3);
    let target = h.card("target").id.to_string();
    h.run(Command::CloseCard { id: target });
    assert!(h.snapshot().connections.is_empty());
    assert_eq!(h.snapshot().regions.len(), 2);
    h.snapshot().validate().unwrap();
}
#[test]
fn toggling_visible_symbols_cleans_links_and_reopens_the_card() {
    let mut h = Harness::new();
    let root = h.add("origin", Point::default());
    let target = h.expand(&root, 0, "target");
    h.run(Command::Navigate {
        card: root.clone(),
        position: Position::new(0, 4),
        kind: ConnectionKind::Definition,
        anchor: Point::new(520.0, 60.0),
        toggle: true,
    });
    assert_eq!(h.snapshot().cards.len(), 1);
    assert!(h.snapshot().connections.is_empty());
    assert_eq!(h.expand(&root, 0, "target"), target);
    let mut selected = symbol("target");
    selected.id = "search-result-id".into();
    h.run(Command::AddSymbol {
        symbol: selected.clone(),
        position: Point::default(),
        toggle: true,
    });
    assert_eq!(h.snapshot().cards.len(), 1);
    assert!(
        h.snapshot()
            .regions
            .iter()
            .all(|region| !region.card_ids.iter().any(|id| id == &target))
    );
    h.run(Command::AddSymbol {
        symbol: selected,
        position: Point::default(),
        toggle: true,
    });
    assert_eq!(h.snapshot().cards.len(), 2);
}
#[test]
fn source_acquisition_is_deduplicated_before_backend_io() {
    let mut h = Harness::new();
    let root = h.add("origin", Point::default());
    let before = h.state.lock().unwrap().source_calls;
    h.state.lock().unwrap().targets = vec![symbol("target"), symbol("target"), symbol("target")];
    h.run(Command::Navigate {
        card: root.clone(),
        position: Position::new(0, 4),
        kind: ConnectionKind::Definition,
        anchor: Point::new(520.0, 60.0),
        toggle: false,
    });
    assert_eq!(h.state.lock().unwrap().source_calls - before, 1);
    h.expand(&root, 0, "target");
    assert_eq!(h.state.lock().unwrap().source_calls - before, 1);
}
#[test]
fn closing_a_child_removes_its_descendants_for_every_close_action() {
    for toggle in [false, true] {
        let mut h = Harness::new();
        let root = h.add("root", Point::default());
        let child = h.expand(&root, 0, "child");
        h.expand(&child, 0, "grandchild");
        if toggle {
            h.run(Command::AddSymbol {
                symbol: symbol("child"),
                position: Point::default(),
                toggle: true,
            });
        } else {
            h.run(Command::CloseCard { id: child });
        }
        assert_eq!(h.snapshot().cards.len(), 1);
        assert_eq!(h.card("root").id, root);
        assert!(h.snapshot().connections.is_empty());
    }
}
#[test]
fn closing_a_branch_preserves_shared_descendants_and_handles_cycles() {
    let mut h = Harness::new();
    let root = h.add("root", Point::default());
    let child = h.expand(&root, 0, "child");
    let shared = h.expand(&child, 0, "shared");
    let other = h.add("other", Point::new(-1000.0, 0.0));
    h.expand(&other, 0, "shared");
    h.expand(&shared, 0, "child");
    let before = h.card("shared").position;
    h.run(Command::CloseCard { id: child });
    assert!(h.snapshot().cards.iter().any(|card| card.id == shared));
    assert_eq!(h.card("shared").position, before);
    assert!(
        h.snapshot()
            .cards
            .iter()
            .all(|card| card.id != "card:child")
    );
}
#[test]
fn toggling_a_cyclic_branch_preserves_the_clicked_source() {
    let mut h = Harness::new();
    let root = h.add("root", Point::default());
    let child = h.expand(&root, 0, "child");
    h.expand(&child, 0, "root");
    h.run(Command::Navigate {
        card: root.clone(),
        position: Position::new(0, 4),
        kind: ConnectionKind::Definition,
        anchor: Point::new(520.0, 60.0),
        toggle: true,
    });
    assert_eq!(h.snapshot().cards.len(), 1);
    assert_eq!(h.card("root").id, root);
    assert!(h.snapshot().connections.is_empty());
}
#[test]
fn self_references_and_multiple_parents_reuse_cards_in_place() {
    let mut h = Harness::new();
    let root = h.add("root", Point::default());
    let root_pos = h.card("root").position;
    h.expand(&root, 0, "root");
    assert_eq!(h.snapshot().cards.len(), 1);
    let child = h.expand(&root, 0, "child");
    let child_pos = h.card("child").position;
    let other = h.add("other", Point::new(-1000.0, 0.0));
    assert_eq!(h.expand(&other, 0, "child"), child);
    assert_eq!(h.card("child").position, child_pos);
    assert_eq!(h.card("root").position, root_pos);
}
#[test]
fn ambiguous_symbol_indexes_choose_saved_first_card() {
    let mut h = Harness::new();
    h.add("first", Point::default());
    let mut second = symbol("second");
    second.path = symbol("first").path;
    second.range.start.line = 1;
    second.range.end.line = 1;
    second.selection_range.start.line = 1;
    second.selection_range.end.line = 1;
    h.run(Command::AddSymbol {
        symbol: second,
        position: Point::new(1000.0, 0.0),
        toggle: false,
    });
    let mut ambiguous = symbol("second");
    ambiguous.path = symbol("first").path;
    ambiguous.kind = symbol("first").kind;
    ambiguous.range = symbol("first").range;
    let calls = h.state.lock().unwrap().source_calls;
    let events = h.run(Command::AddSymbol {
        symbol: ambiguous,
        position: Point::default(),
        toggle: false,
    });
    let target = events
        .into_iter()
        .find_map(|event| match event {
            ViewEvent::Canvas(outcome) => outcome.targets.first().cloned(),
            _ => None,
        })
        .unwrap();
    assert_eq!(target, "card:first");
    assert_eq!(h.state.lock().unwrap().source_calls, calls);
}
#[test]
fn reference_toggle_keeps_source_and_definition_toggle_hides_every_target() {
    let mut h = Harness::new();
    let root = h.add("root", Point::default());
    h.state.lock().unwrap().targets = vec![symbol("a"), symbol("b")];
    for kind in [ConnectionKind::Definition, ConnectionKind::Reference] {
        h.run(Command::Navigate {
            card: root.clone(),
            position: Position::new(0, 4),
            kind,
            anchor: Point::new(520.0, 60.0),
            toggle: true,
        });
    }
    assert_eq!(h.snapshot().connections.len(), 4);
    h.run(Command::Navigate {
        card: root.clone(),
        position: Position::new(0, 4),
        kind: ConnectionKind::Reference,
        anchor: Point::new(520.0, 60.0),
        toggle: true,
    });
    assert_eq!(h.snapshot().cards.len(), 1);
    assert_eq!(h.card("root").id, root);
}
#[test]
fn normal_variable_click_uses_token_start_and_other_glyph_closes_same_link() {
    let mut h = Harness::new();
    let s = symbol("root");
    let mut source = document(s.clone(), "let value = other;");
    source.tokens = vec![SemanticToken {
        line: 0,
        start: 4,
        length: 5,
        kind: "variable".into(),
        modifiers: vec![],
    }];
    h.state
        .lock()
        .unwrap()
        .sources
        .insert("root".into(), source);
    h.run(Command::AddSymbol {
        symbol: s,
        position: Point::default(),
        toggle: false,
    });
    let id = h.card("root").id.to_string();
    h.run(Command::Click {
        card: id.clone(),
        position: Position::new(0, 6),
        anchor: Point::new(520.0, 60.0),
        mode: NavigationMode::Normal,
    });
    assert_eq!(h.snapshot().connections[0].source, Position::new(0, 4));
    assert_eq!(
        h.snapshot().connections[0].kind,
        ConnectionKind::TypeDefinition
    );
    h.run(Command::Click {
        card: id,
        position: Position::new(0, 8),
        anchor: Point::new(520.0, 60.0),
        mode: NavigationMode::Normal,
    });
    assert_eq!(h.snapshot().cards.len(), 1);
    assert!(h.snapshot().connections.is_empty());
}
#[test]
fn linked_plain_word_reuses_saved_glyph_position() {
    let mut h = Harness::new();
    let id = h.add("root", Point::default());
    h.run(Command::Click {
        card: id.clone(),
        position: Position::new(0, 4),
        anchor: Point::new(520.0, 60.0),
        mode: NavigationMode::Definition,
    });
    assert_eq!(h.snapshot().connections[0].source, Position::new(0, 4));
    h.run(Command::Click {
        card: id,
        position: Position::new(0, 8),
        anchor: Point::new(520.0, 60.0),
        mode: NavigationMode::Definition,
    });
    assert_eq!(h.snapshot().cards.len(), 1);
}
