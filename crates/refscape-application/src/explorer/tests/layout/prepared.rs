use super::*;

#[test]
fn prepared_expansion_reuses_sources_and_latest_parent_coordinates() {
    let mut explorer = explorer();
    let root = explorer
        .add_symbol(symbol("root"), Point::new(100.0, 80.0))
        .unwrap();
    let edit = explorer
        .prepare_expansion(
            &root,
            Position::new(0, 4),
            ConnectionKind::Definition,
            Point::new(100.0, 60.0),
        )
        .unwrap();
    explorer.move_card(&root, Point::new(500.0, 300.0)).unwrap();
    explorer.language.fail = true;
    let outcome = explorer.commit_prepared(edit).unwrap();
    assert_eq!(outcome.added.len(), 1);
    let child = explorer
        .session
        .cards
        .iter()
        .find(|card| card.id == outcome.added[0])
        .unwrap();
    assert_eq!(child.position, Point::new(1120.0, 360.0));
    assert_eq!(explorer.session.cards[0].position, Point::new(500.0, 300.0));
    assert_nonoverlapping(&explorer.session.cards);
}

#[test]
fn outdated_layout_commit_is_rejected_without_overwriting_recent_movement() {
    let mut explorer = explorer();
    let root = explorer
        .add_symbol(symbol("root"), Point::default())
        .unwrap();
    let edit = explorer
        .prepare_expansion(
            &root,
            Position::new(0, 4),
            ConnectionKind::Definition,
            Point::new(100.0, 60.0),
        )
        .unwrap();
    let before = explorer.session.clone();
    let commit = explorer.plan_prepared(edit).unwrap();
    assert_eq!(explorer.session, before);
    explorer.move_card(&root, Point::new(400.0, 300.0)).unwrap();
    let latest = explorer.session.clone();
    assert!(explorer.apply_commit(commit).is_err());
    assert_eq!(explorer.session, latest);
}

#[test]
fn prepared_expansion_rejects_content_changes_and_session_switches() {
    for switch_session in [false, true] {
        let mut explorer = explorer();
        let root = explorer
            .add_symbol(symbol("root"), Point::default())
            .unwrap();
        let edit = explorer
            .prepare_expansion(
                &root,
                Position::new(0, 4),
                ConnectionKind::Definition,
                Point::new(100.0, 60.0),
            )
            .unwrap();
        if switch_session {
            explorer
                .open_project(
                    &std::env::current_dir().unwrap(),
                    &ProjectOptions::default(),
                )
                .unwrap();
        } else {
            explorer.remove_card(&root).unwrap();
        }
        let before = explorer.session.clone();
        assert!(explorer.commit_prepared(edit).is_err());
        assert_eq!(explorer.session, before);
    }
}

#[test]
fn prepared_layout_commit_preserves_the_latest_camera_and_theme() {
    let mut explorer = explorer();
    let root = explorer
        .add_symbol(symbol("root"), Point::default())
        .unwrap();
    let edit = explorer
        .prepare_expansion(
            &root,
            Position::new(0, 4),
            ConnectionKind::Definition,
            Point::new(100.0, 60.0),
        )
        .unwrap();
    let commit = explorer.plan_prepared(edit).unwrap();
    explorer.pan(Point::new(50.0, -100.0)).unwrap();
    explorer.zoom(1.5, Point::new(100.0, 200.0)).unwrap();
    explorer.set_theme(Theme::light()).unwrap();
    let viewport = explorer.session.viewport;
    let theme = explorer.session.theme.clone();
    explorer.apply_commit(commit).unwrap();
    assert_eq!(explorer.session.viewport, viewport);
    assert_eq!(explorer.session.theme, theme);
    assert_eq!(explorer.session.cards.len(), 2);
}
