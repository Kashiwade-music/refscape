use super::*;

fn disk_symbol(path: PathBuf, name: &str, code: &str, file: bool) -> Symbol {
    let end = Position::new(
        code.bytes().filter(|byte| *byte == b'\n').count() as u32,
        code.rsplit('\n').next().unwrap().encode_utf16().count() as u32,
    );
    let mut symbol = Symbol::file(
        path,
        SourceRange {
            start: Position::default(),
            end,
        },
    );
    if !file {
        symbol.id = format!("{name}:0");
        symbol.name = name.into();
        symbol.kind = "function".into();
    }
    symbol
}
fn publish(h: &Harness, symbol: &Symbol, code: &str) {
    std::fs::write(&symbol.path, code).unwrap();
    let mut state = h.state.lock().unwrap();
    state.local_sources = true;
    state.fingerprints.insert(
        symbol.path.clone(),
        DocumentFingerprint::of(code.as_bytes()),
    );
    state
        .sources
        .insert(symbol.id.clone(), document(symbol.clone(), code));
    state.targets = vec![symbol.clone()];
}
fn watch(h: &mut Harness, name: &str, code: &str, file: bool, position: Point) -> String {
    let symbol = disk_symbol(h.root.join(format!("{name}.rs")), name, code, file);
    publish(h, &symbol, code);
    h.run(Command::AddSymbol {
        symbol,
        position,
        toggle: false,
    })
    .into_iter()
    .find_map(|event| {
        if let ViewEvent::Canvas(outcome) = event {
            outcome.targets.into_iter().next()
        } else {
            None
        }
    })
    .unwrap()
}
fn find<'a>(h: &'a Harness, id: &str) -> &'a CodeCard {
    h.snapshot()
        .cards
        .iter()
        .find(|card| card.id == id)
        .unwrap()
}
#[test]
fn unchanged_content_and_mtime_only_changes_make_no_source_request_or_snapshot_commit() {
    let mut h = Harness::new();
    let id = watch(
        &mut h,
        "stable",
        "fn stable() {}",
        true,
        Point::new(140.0, 210.0),
    );
    let before = h.driver.controller.shared_snapshot();
    let calls = h.state.lock().unwrap().source_calls;
    let symbol = find(&h, &id).source.symbol.clone();
    std::fs::write(&symbol.path, "fn stable() {}").unwrap();
    assert!(h.run(Command::RefreshSources).is_empty());
    assert!(Arc::ptr_eq(&before, &h.driver.controller.shared_snapshot()));
    assert_eq!(h.state.lock().unwrap().source_calls, calls);
}
#[test]
fn changed_card_reloads_while_other_sources_positions_and_viewport_survive() {
    let mut h = Harness::new();
    let id = watch(
        &mut h,
        "edit",
        "fn edit() {}",
        true,
        Point::new(140.0, 210.0),
    );
    let other = watch(
        &mut h,
        "other",
        "fn other() {}",
        true,
        Point::new(3000.0, 1200.0),
    );
    h.run(Command::Zoom {
        factor: 1.7,
        anchor: Point::new(400.0, 300.0),
    });
    h.run(Command::Pan(Point::new(99.0, -83.0)));
    let before = h.snapshot().clone();
    let updated = "// 日本語😀\nfn edit() {\n    println!(\"new\");\n}";
    let symbol = disk_symbol(
        find(&h, &id).source.symbol.path.clone(),
        "edit",
        updated,
        true,
    );
    publish(&h, &symbol, updated);
    let events = h.run(Command::RefreshSources);
    assert_eq!(find(&h, &id).source.code.as_ref(), updated);
    assert_eq!(find(&h, &id).id, before.cards[0].id);
    assert_eq!(find(&h, &id).position, before.cards[0].position);
    assert!(find(&h, &id).height > before.cards[0].height);
    assert!(Arc::ptr_eq(
        find(&h, &other).source.snapshot(),
        before.cards[1].source.snapshot()
    ));
    assert_eq!(find(&h, &other).position, before.cards[1].position);
    assert_eq!(h.snapshot().viewport, before.viewport);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ViewEvent::SourceReloaded))
    );
    assert!(h.run(Command::RefreshSources).is_empty());
}
#[test]
fn reopening_refreshes_saved_cards_instead_of_replaying_old_code() {
    let mut h = Harness::new();
    let id = watch(
        &mut h,
        "saved",
        "fn saved() {}",
        true,
        Point::new(480.0, 650.0),
    );
    h.run(Command::Zoom {
        factor: 1.5,
        anchor: Point::new(180.0, 120.0),
    });
    let saved = h.snapshot().clone();
    let updated = "fn saved() { changed(); }";
    let symbol = disk_symbol(
        find(&h, &id).source.symbol.path.clone(),
        "saved",
        updated,
        true,
    );
    publish(&h, &symbol, updated);
    h.import(saved.clone());
    assert_eq!(find(&h, &id).source.code.as_ref(), updated);
    assert_eq!(find(&h, &id).position, saved.cards[0].position);
    assert_eq!(h.snapshot().viewport, saved.viewport);
    assert!(h.driver.controller.dirty());
}
#[test]
fn legacy_sources_gain_identity_without_discarding_unchanged_text() {
    let mut h = Harness::new();
    let id = watch(&mut h, "legacy", "fn legacy() {}", true, Point::default());
    let mut saved = h.snapshot().clone();
    let card = &mut Arc::make_mut(&mut saved.cards)[0];
    card.source =
        CardSource::try_from(document(card.source.symbol.clone(), "fn legacy() {}")).unwrap();
    let source = card.source.snapshot().clone();
    card.width += 150.0;
    let width = card.width;
    h.import(saved);
    assert_eq!(find(&h, &id).source.code.as_ref(), "fn legacy() {}");
    assert_eq!(find(&h, &id).width, width);
    assert_eq!(find(&h, &id).source.snapshot().revision, source.revision);
    assert!(find(&h, &id).source.document_fingerprint.is_some());
    let calls = h.state.lock().unwrap().source_calls;
    h.run(Command::RefreshSources);
    assert_eq!(h.state.lock().unwrap().source_calls, calls);
}
#[test]
fn reopening_legacy_file_card_includes_newly_appended_code() {
    let mut h = Harness::new();
    let id = watch(
        &mut h,
        "appended",
        "fn first() {}",
        true,
        Point::new(70.0, 90.0),
    );
    let mut saved = h.snapshot().clone();
    let card = &mut Arc::make_mut(&mut saved.cards)[0];
    card.source = card.source.to_document().try_into().unwrap();
    let position = card.position;
    let updated = "fn first() {}\nfn second() {}";
    let symbol = disk_symbol(card.source.symbol.path.clone(), "appended", updated, true);
    publish(&h, &symbol, updated);
    h.import(saved);
    assert_eq!(find(&h, &id).source.code.as_ref(), updated);
    assert_eq!(find(&h, &id).position, position);
}
#[test]
fn moved_symbol_keeps_card_identity_and_new_symbol_index_prevents_duplicates() {
    let mut h = Harness::new();
    let id = watch(
        &mut h,
        "moved",
        "fn moved() {}",
        false,
        Point::new(120.0, 250.0),
    );
    let mut updated = find(&h, &id).source.symbol.clone();
    updated.id = "moved:2".into();
    updated.range.start = Position::new(2, 0);
    updated.range.end = Position::new(2, 13);
    updated.selection_range.start = Position::new(2, 0);
    updated.selection_range.end = Position::new(2, 0);
    let full_text = "// header\n\nfn moved() {}";
    std::fs::write(&updated.path, full_text).unwrap();
    {
        let mut state = h.state.lock().unwrap();
        state.targets = vec![updated.clone()];
        state.sources.insert(
            updated.id.clone(),
            document(updated.clone(), "fn moved() {}"),
        );
        state.fingerprints.insert(
            updated.path.clone(),
            DocumentFingerprint::of(full_text.as_bytes()),
        );
    }
    h.run(Command::RefreshSources);
    assert_eq!(find(&h, &id).source.symbol.id, updated.id);
    assert_eq!(find(&h, &id).source.symbol.range.start.line, 2);
    h.run(Command::AddSymbol {
        symbol: updated,
        position: Point::new(1000.0, 2000.0),
        toggle: false,
    });
    assert_eq!(h.snapshot().cards.len(), 1);
}
#[test]
fn deleted_file_removes_its_cards_and_failed_analysis_leaves_old_canvas_intact() {
    let mut h = Harness::new();
    let id = watch(&mut h, "deleted", "fn deleted() {}", true, Point::default());
    let updated = "fn deleted() { changed(); }";
    let symbol = disk_symbol(
        find(&h, &id).source.symbol.path.clone(),
        "deleted",
        updated,
        true,
    );
    publish(&h, &symbol, updated);
    h.state.lock().unwrap().fail_source = true;
    let before = h.snapshot().clone();
    h.run(Command::RefreshSources);
    assert_eq!(h.snapshot(), &before);
    assert!(h.driver.controller.error());
    h.state.lock().unwrap().fail_source = false;
    std::fs::remove_file(symbol.path).unwrap();
    h.run(Command::RefreshSources);
    assert!(h.snapshot().cards.is_empty());
    assert!(h.snapshot().connections.is_empty());
}
#[test]
fn a_reload_in_flight_cannot_resurrect_a_closed_card() {
    let mut h = Harness::new();
    let id = watch(&mut h, "late", "fn late() {}", true, Point::default());
    let updated = "fn late() { changed(); }";
    let symbol = disk_symbol(
        find(&h, &id).source.symbol.path.clone(),
        "late",
        updated,
        true,
    );
    publish(&h, &symbol, updated);
    let effect = h.pending(Command::RefreshSources).remove(0);
    let completion = h.execute(effect);
    h.run(Command::CloseCard { id });
    let transition = h.finish(completion);
    h.drain(transition);
    assert!(h.snapshot().cards.is_empty());
}
#[test]
fn changes_reset_fold_state_and_drop_outgoing_edges_but_keep_other_cards() {
    let mut h = Harness::new();
    let path = h.root.join("folded.rs");
    let body = "    fn run() {}";
    let old_gap = "    first();\n    second();";
    let old_text = format!("impl Owner {{\n{old_gap}\n{body}\n}}");
    let mut symbol = disk_symbol(path.clone(), "run", body, false);
    symbol.range.start = Position::new(3, 4);
    symbol.range.end = Position::new(3, body.len() as u32);
    symbol.selection_range = SourceRange {
        start: Position::new(3, 7),
        end: Position::new(3, 10),
    };
    let mut source = document(symbol.clone(), body);
    source.code_start = Some(Position::new(3, 0));
    source.context.push(SourceContext {
        start_line: 0,
        code: "impl Owner {".into(),
    });
    source.folded.push(SourceContext {
        start_line: 1,
        code: old_gap.into(),
    });
    std::fs::write(&path, &old_text).unwrap();
    {
        let mut state = h.state.lock().unwrap();
        state.sources.insert(symbol.id.clone(), source.clone());
        state.targets = vec![symbol.clone()];
        state
            .fingerprints
            .insert(path.clone(), DocumentFingerprint::of(old_text.as_bytes()));
    }
    let id = h
        .run(Command::AddSymbol {
            symbol: symbol.clone(),
            position: Point::new(50.0, 50.0),
            toggle: false,
        })
        .into_iter()
        .find_map(|event| match event {
            ViewEvent::Canvas(outcome) => outcome.targets.into_iter().next(),
            _ => None,
        })
        .unwrap();
    h.run(Command::ToggleFold {
        card: id.clone(),
        index: 0,
        expand: true,
    });
    assert_eq!(find(&h, &id).source.folds().expanded().count(), 1);
    let other = watch(
        &mut h,
        "linked",
        "fn linked() {}",
        true,
        Point::new(3000.0, 1200.0),
    );
    let mut saved = h.snapshot().clone();
    Arc::make_mut(&mut saved.connections).push(Connection {
        id: "reload-edge".into(),
        from: id.clone().into(),
        to: other.clone().into(),
        kind: ConnectionKind::Definition,
        source: Position::new(1, 4),
    });
    h.import(saved);
    let position = find(&h, &id).position;
    let new_gap = "    changed();\n    second();";
    source.folded[0].code = new_gap.into();
    let updated = format!("impl Owner {{\n{new_gap}\n{body}\n}}");
    std::fs::write(&path, &updated).unwrap();
    {
        let mut state = h.state.lock().unwrap();
        state.sources.insert(symbol.id.clone(), source);
        state.targets = vec![symbol];
        state
            .fingerprints
            .insert(path, DocumentFingerprint::of(updated.as_bytes()));
    }
    h.run(Command::RefreshSources);
    assert_eq!(find(&h, &id).source.folds().expanded().count(), 0);
    assert_eq!(find(&h, &id).position, position);
    assert_eq!(h.snapshot().cards.len(), 2);
    assert!(h.snapshot().connections.is_empty());
}
#[test]
fn renamed_declaration_at_the_same_position_keeps_the_existing_card() {
    let mut h = Harness::new();
    let id = watch(
        &mut h,
        "old",
        "fn old() {}",
        false,
        Point::new(200.0, 400.0),
    );
    let before = find(&h, &id).position;
    let updated = disk_symbol(
        find(&h, &id).source.symbol.path.clone(),
        "new",
        "fn new() {}",
        false,
    );
    publish(&h, &updated, "fn new() {}");
    h.run(Command::RefreshSources);
    assert_eq!(h.snapshot().cards.len(), 1);
    assert_eq!(find(&h, &id).position, before);
    assert_eq!(find(&h, &id).source.symbol.name, "new");
    assert_eq!(find(&h, &id).source.code.as_ref(), "fn new() {}");
}
#[test]
fn obsolete_reload_plan_is_not_deferred_behind_a_newer_drag() {
    let mut h = Harness::new();
    let id = watch(
        &mut h,
        "obsolete",
        "fn obsolete() {}",
        true,
        Point::default(),
    );
    let updated = "fn obsolete() { changed(); }";
    let symbol = disk_symbol(
        find(&h, &id).source.symbol.path.clone(),
        "obsolete",
        updated,
        true,
    );
    publish(&h, &symbol, updated);
    let query = h.pending(Command::RefreshSources).remove(0);
    let completion = h.execute(query);
    let mut transition = h.finish(completion);
    let plan = transition.effects.remove(0);
    let planned = h.execute(plan);
    h.run(Command::CloseCard { id });
    h.run(Command::Interaction {
        selected: None,
        dragging: true,
    });
    let transition = h.finish(planned);
    assert!(transition.effects.is_empty());
    assert!(transition.events.is_empty());
    assert!(!h.driver.controller.busy());
    h.run(Command::Interaction {
        selected: None,
        dragging: false,
    });
    assert!(h.snapshot().cards.is_empty());
}
#[test]
fn deleted_legacy_file_is_not_restored_as_old_source() {
    let mut h = Harness::new();
    let id = watch(
        &mut h,
        "old_deleted",
        "fn old_deleted() {}",
        true,
        Point::default(),
    );
    let mut saved = h.snapshot().clone();
    let card = &mut Arc::make_mut(&mut saved.cards)[0];
    let path = card.source.symbol.path.clone();
    card.source = card.source.to_document().try_into().unwrap();
    assert!(card.source.document_fingerprint.is_none());
    std::fs::remove_file(path).unwrap();
    h.import(saved);
    assert!(!h.snapshot().cards.iter().any(|card| card.id == id));
}
#[test]
fn an_unchanged_excerpt_in_the_edited_file_reuses_its_projection_and_fold_state() {
    let mut h = Harness::new();
    let path = h.root.join("two.rs");
    let mut alpha = disk_symbol(path.clone(), "alpha", "fn alpha() {}", false);
    let mut beta = disk_symbol(path.clone(), "beta", "fn beta() {}", false);
    beta.range.start.line = 2;
    beta.range.end.line = 2;
    beta.selection_range.start.line = 2;
    beta.selection_range.end.line = 2;
    let old = "fn alpha() {}\n\nfn beta() {}";
    std::fs::write(&path, old).unwrap();
    {
        let mut state = h.state.lock().unwrap();
        state.local_sources = true;
        state
            .fingerprints
            .insert(path.clone(), DocumentFingerprint::of(old.as_bytes()));
        state.targets = vec![alpha.clone(), beta.clone()];
        state
            .sources
            .insert(alpha.id.clone(), document(alpha.clone(), "fn alpha() {}"));
        let mut source = document(beta.clone(), "fn beta() {}");
        source.tokens.push(SemanticToken {
            line: 2,
            start: 0,
            length: 2,
            kind: "keyword".into(),
            modifiers: vec![],
        });
        state.sources.insert(beta.id.clone(), source);
    }
    h.run(Command::AddSymbol {
        symbol: alpha.clone(),
        position: Point::new(50.0, 50.0),
        toggle: false,
    });
    h.run(Command::AddSymbol {
        symbol: beta.clone(),
        position: Point::new(3000.0, 1200.0),
        toggle: false,
    });
    let before = h.snapshot().cards[1].source.clone();
    let updated = "fn alpha() { changed(); }\n\nfn beta() {}";
    alpha.range.end.character = 24;
    std::fs::write(&path, updated).unwrap();
    {
        let mut state = h.state.lock().unwrap();
        state
            .fingerprints
            .insert(path, DocumentFingerprint::of(updated.as_bytes()));
        state.targets = vec![alpha.clone(), beta.clone()];
        state.sources.insert(
            alpha.id.clone(),
            document(alpha, "fn alpha() { changed(); }"),
        );
    }
    h.run(Command::RefreshSources);
    assert!(h.snapshot().cards[0].source.code.contains("changed"));
    let unchanged = &h.snapshot().cards[1].source;
    assert_eq!(unchanged.snapshot().revision, before.snapshot().revision);
    assert_eq!(unchanged.folds(), before.folds());
    assert!(Arc::ptr_eq(
        &unchanged.shared_projection(),
        &before.shared_projection()
    ));
    let calls = h.state.lock().unwrap().source_calls;
    h.run(Command::RefreshSources);
    assert_eq!(h.state.lock().unwrap().source_calls, calls);
}
