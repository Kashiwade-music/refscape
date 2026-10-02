use super::*;
#[test]
fn refreshed_metadata_adopts_one_generation_without_copying_sources() {
    let mut h = Harness::new();
    h.add("a", Point::default());
    let before = h.driver.controller.shared_snapshot();
    let basis = h.driver.controller.basis();
    let old_files = h.pending(Command::Files).pop().unwrap();
    let options = ResolvedProjectOptions::try_from(ProjectOpenOptions {
        language: ProjectLanguage::Cpp,
        compilation_database: Some(h.root.join("build")),
    })
    .unwrap();
    h.state.lock().unwrap().metadata = Some(AnalysisMetadata {
        options: options.clone(),
        crates: vec![ProjectCrate {
            id: "crate".into(),
            name: "renamed".into(),
            root: PathBuf::from("/project"),
        }],
        catalog_revision: 2,
        files: vec![h.root.join("new.cpp")],
    });
    let completion = h.execute(old_files);
    let events = h.finish(completion).events;
    assert!(events.iter().any(
        |event| matches!(event, ViewEvent::Files(files) if files == &vec![h.root.join("new.cpp")])
    ));
    assert_eq!(h.snapshot().project_options, options.to_open_options());
    assert_eq!(
        h.driver.controller.project_state(),
        &ProjectState::Open {
            epoch: basis.project,
            options: options.clone()
        }
    );
    assert_eq!(h.driver.controller.basis(), basis);
    assert!(Arc::ptr_eq(&before.cards, &h.snapshot().cards));
    assert!(Arc::ptr_eq(
        before.cards[0].source.snapshot(),
        h.snapshot().cards[0].source.snapshot()
    ));
    assert!(
        h.snapshot()
            .regions
            .iter()
            .any(|region| region.label == "renamed")
    );
    h.state
        .lock()
        .unwrap()
        .metadata
        .as_mut()
        .unwrap()
        .catalog_revision = 1;
    h.state.lock().unwrap().metadata.as_mut().unwrap().crates[0].name = "stale".into();
    h.run(Command::Search("a".into()));
    assert!(
        h.snapshot()
            .regions
            .iter()
            .any(|region| region.label == "renamed")
    );
    assert!(
        !h.snapshot()
            .regions
            .iter()
            .any(|region| region.label == "stale")
    );
}

#[test]
fn operation_capture_cleanup_runs_after_backend_failure() {
    let mut h = Harness::new();
    let before = h.state.lock().unwrap().finished_operations;
    h.state.lock().unwrap().fail_source = true;
    h.run(Command::AddSymbol {
        symbol: symbol("failing"),
        position: Point::default(),
        toggle: false,
    });
    assert_eq!(h.state.lock().unwrap().finished_operations, before + 1);
    assert!(h.snapshot().cards.is_empty());
}
#[test]
fn pending_plan_preserves_latest_camera_and_theme() {
    let mut h = Harness::new();
    let effects = h.pending(Command::AddSymbol {
        symbol: symbol("a"),
        position: Point::default(),
        toggle: false,
    });
    let completion = h.execute(effects.into_iter().next().unwrap());
    let transition = h.finish(completion);
    let plan = transition.effects.into_iter().next().unwrap();
    let completion = h.execute(plan);
    h.run(Command::Pan(Point::new(30.0, 50.0)));
    h.run(Command::SetTheme(Theme::light()));
    let viewport = h.snapshot().viewport;
    h.finish(completion);
    assert_eq!(h.snapshot().viewport, viewport);
    assert_eq!(h.snapshot().theme, Theme::light());
    assert_eq!(h.snapshot().cards.len(), 1);
}
#[test]
fn cancelled_hover_never_publishes_popup_or_status() {
    let mut h = Harness::new();
    let card = h.add("a", Point::default());
    let mut effects = h.pending(Command::Hover {
        card,
        position: Position::new(0, 4),
    });
    let completion = h.execute(effects.remove(0));
    h.run(Command::CancelHover);
    let transition = h.finish(completion);
    assert!(transition.events.is_empty());
    assert!(transition.effects.is_empty());
}
#[test]
fn latest_search_request_wins_out_of_order_completions() {
    let mut h = Harness::new();
    let first = h.pending(Command::Search("a".into())).remove(0);
    let completion = h.execute(first);
    let second = h
        .pending(Command::Search("b".into()))
        .into_iter()
        .find(|effect| matches!(effect, Effect::QueryAnalysis { .. }))
        .unwrap();
    let latest = h.execute(second);
    assert!(h.finish(completion).events.is_empty());
    assert!(
        h.finish(latest)
            .events
            .iter()
            .any(|event| matches!(event, ViewEvent::Symbols(_)))
    );
}
#[test]
fn duplicate_completion_never_commits_twice() {
    let mut h = Harness::new();
    let effects = h.pending(Command::AddSymbol {
        symbol: symbol("a"),
        position: Point::default(),
        toggle: false,
    });
    let completion = h.execute(effects.into_iter().next().unwrap());
    let transition = h.finish(completion);
    let effect = transition.effects.into_iter().next().unwrap();
    let (context, basis, edit, interaction) = match &effect {
        Effect::PlanCanvas {
            context,
            basis,
            edit,
            interaction,
            ..
        } => (context.clone(), *basis, edit.clone(), *interaction),
        _ => panic!(),
    };
    let completion = h.execute(effect);
    h.finish(completion);
    let before = h.snapshot().clone();
    let duplicate = Completion::CanvasPlanned {
        context,
        basis,
        edit,
        interaction,
        result: Err("duplicate".into()),
    };
    assert!(h.finish(duplicate).events.is_empty());
    assert_eq!(h.snapshot(), &before);
}
#[test]
fn stale_arrange_replans_latest_selected_leaf_and_viewport() {
    let mut h = connected();
    let effect = h.pending(Command::Arrange { selected: None }).remove(0);
    let completion = h.execute(effect);
    let leaf = h.card("grandchild").id.to_string();
    h.run(Command::Interaction {
        selected: Some(leaf),
        dragging: false,
    });
    h.run(Command::Pan(Point::new(10.0, 20.0)));
    let before = h.snapshot().clone();
    let transition = h.finish(completion);
    assert!(!transition.effects.is_empty());
    h.drain(transition);
    assert_eq!(h.snapshot(), &before);
}
#[test]
fn sidebar_selection_during_arrange_changes_root_without_hiding() {
    let mut h = connected();
    let effect = h.pending(Command::Arrange { selected: None }).remove(0);
    let completion = h.execute(effect);
    let before = h.snapshot().clone();
    h.run(Command::AddSymbol {
        symbol: symbol("later"),
        position: Point::default(),
        toggle: true,
    });
    assert_eq!(h.snapshot(), &before);
    let transition = h.finish(completion);
    h.drain(transition);
    assert_eq!(
        h.card("later").position,
        before
            .cards
            .iter()
            .find(|card| card.source.symbol.name == "later")
            .unwrap()
            .position
    );
    assert_eq!(h.snapshot().cards.len(), before.cards.len());
}
#[test]
fn backend_completion_during_drag_waits_and_replans_before_commit() {
    let mut h = Harness::new();
    let effects = h.pending(Command::AddSymbol {
        symbol: symbol("a"),
        position: Point::default(),
        toggle: false,
    });
    h.run(Command::Interaction {
        selected: None,
        dragging: true,
    });
    let completion = h.execute(effects.into_iter().next().unwrap());
    let transition = h.finish(completion);
    assert!(transition.effects.is_empty());
    assert!(h.snapshot().cards.is_empty());
    assert!(h.driver.controller.busy());
    h.run(Command::Interaction {
        selected: None,
        dragging: false,
    });
    assert_eq!(h.snapshot().cards.len(), 1);
    assert!(!h.driver.controller.busy());
}
#[test]
fn drop_during_search_waits_then_commits_once() {
    let mut h = Harness::new();
    let id = h.add("a", Point::default());
    let effect = h.pending(Command::Search("a".into())).remove(0);
    h.run(Command::Interaction {
        selected: Some(id.clone()),
        dragging: true,
    });
    h.run(Command::MoveCard {
        id,
        position: Point::new(1000.0, 500.0),
    });
    assert_eq!(h.card("a").position, Point::default());
    let completion = h.execute(effect);
    let transition = h.finish(completion);
    h.drain(transition);
    assert_eq!(h.card("a").position, Point::new(1000.0, 500.0));
    assert!(!h.driver.controller.busy());
}
#[test]
fn stale_save_cannot_mark_latest_camera_saved() {
    let mut h = Harness::new();
    h.add("a", Point::default());
    let effect = h.pending(Command::Save).remove(0);
    h.run(Command::Pan(Point::new(10.0, 20.0)));
    let completion = h.execute(effect);
    h.finish(completion);
    assert!(h.driver.controller.dirty());
}
#[test]
fn pan_shares_all_source_card_and_graph_storage() {
    let mut h = Harness::new();
    h.add("a", Point::default());
    let before = h.driver.controller.shared_snapshot();
    h.run(Command::Pan(Point::new(10.0, 20.0)));
    let after = h.driver.controller.shared_snapshot();
    assert!(Arc::ptr_eq(&before.cards, &after.cards));
    assert!(Arc::ptr_eq(&before.connections, &after.connections));
    assert!(Arc::ptr_eq(
        before.cards[0].source.snapshot(),
        after.cards[0].source.snapshot()
    ));
}
#[test]
fn close_is_blocked_while_mutation_runs() {
    let mut h = Harness::new();
    let effects = h.pending(Command::AddSymbol {
        symbol: symbol("a"),
        position: Point::default(),
        toggle: false,
    });
    let close = h.pending(Command::RequestClose);
    assert!(close.is_empty());
    assert!(!h.driver.controller.closing());
    assert!(
        h.driver
            .controller
            .status()
            .contains("A request is running")
    );
    let completion = h.execute(effects.into_iter().next().unwrap());
    let transition = h.finish(completion);
    h.drain(transition);
}
#[test]
fn invalid_move_and_backend_failure_are_atomic() {
    let mut h = Harness::new();
    let id = h.add("a", Point::default());
    let before = h.snapshot().clone();
    h.run(Command::MoveCard {
        id: id.clone(),
        position: Point::new(f32::INFINITY, 0.0),
    });
    assert_eq!(h.snapshot(), &before);
    h.state.lock().unwrap().fail_source = true;
    h.run(Command::Navigate {
        card: id,
        position: Position::new(0, 4),
        kind: ConnectionKind::Definition,
        anchor: Point::new(520.0, 60.0),
        toggle: false,
    });
    assert_eq!(h.snapshot(), &before);
    assert!(!h.driver.controller.busy());
}
#[test]
fn budget_accounts_for_catalog_work_once_and_cancellation_is_immediate() {
    let policy = crate::jobs::OperationBudgetPolicy::new(
        std::time::Duration::from_secs(1),
        3,
        std::time::Duration::from_secs(2),
        std::time::Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(
        policy.for_requests(4, true).unwrap(),
        std::time::Duration::from_secs(15)
    );
    assert_eq!(
        policy.for_requests(201, true).unwrap(),
        std::time::Duration::from_secs(606)
    );
    assert!(policy.for_requests(usize::MAX, false).is_err());
    let mut controller = ApplicationController::with_budget_policy(policy);
    let transition = controller.dispatch(Command::OpenProject {
        root: std::env::current_dir().unwrap(),
        options: ProjectOpenOptions::default(),
        destination: "session.json".into(),
    });
    let context = match &transition.effects[0] {
        Effect::PrepareProject { context, .. } => context.clone(),
        _ => panic!(),
    };
    let deadline = context.deadline;
    controller.dispatch(Command::CancelHover);
    assert_eq!(context.deadline, deadline);
    context.cancel.cancel();
    assert_eq!(context.check().unwrap_err().kind, ErrorKind::Cancelled);
}
#[test]
fn duplicate_project_completion_cannot_dispose_current_analysis() {
    let mut h = Harness::new();
    let effect = h
        .pending(Command::OpenLoaded {
            loaded: ImportedSession {
                snapshot: h.snapshot().clone(),
            },
            destination: h.root.join("session.json"),
            expected_root: None,
            overrides: ProjectOpenOptions::default(),
        })
        .remove(0);
    let completion = h.execute(effect);
    let (context, request) = match &completion {
        Completion::ProjectPrepared {
            context, request, ..
        } => (context.clone(), request.clone()),
        _ => panic!(),
    };
    let transition = h.finish(completion);
    h.drain(transition);
    let duplicate = Completion::ProjectPrepared {
        context,
        request,
        result: Box::new(Ok(crate::effect::PreparedApplicationProject {
            options: ResolvedProjectOptions::try_from(h.snapshot().project_options.clone())
                .unwrap(),
            snapshot: h.snapshot().clone(),
            destination: h.root.join("session.json"),
            crates: vec![],
            files: vec![],
            protection: None,
            listing_failed: false,
            refreshed: false,
        })),
    };
    let transition = h.finish(duplicate);
    assert!(transition.effects.is_empty());
    assert!(transition.events.is_empty());
    h.add("still_alive", Point::default());
}
#[test]
fn resolved_expansion_waits_for_drop_then_uses_latest_parent_geometry() {
    let mut h = Harness::new();
    let id = h.add("root", Point::default());
    let effect = h
        .pending(Command::Navigate {
            card: id.clone(),
            position: Position::new(0, 4),
            kind: ConnectionKind::Definition,
            anchor: Point::new(520.0, 60.0),
            toggle: false,
        })
        .remove(0);
    h.run(Command::Interaction {
        selected: Some(id.clone()),
        dragging: true,
    });
    let completion = h.execute(effect);
    let transition = h.finish(completion);
    assert!(transition.effects.is_empty());
    h.run(Command::MoveCard {
        id: id.clone(),
        position: Point::new(2000.0, 500.0),
    });
    assert_eq!(h.card("root").position, Point::new(2000.0, 500.0));
    assert!(
        h.card("target").position.x >= h.card("root").position.x + h.card("root").width + 100.0
    );
    assert_eq!(h.snapshot().connections.len(), 1);
}
