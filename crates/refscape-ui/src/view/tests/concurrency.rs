use super::layout::connected_explorer;
use super::*;
use crate::view::layout::Placement;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn a_cancelled_worker_waiting_for_the_explorer_cannot_restore_old_geometry() {
    let shared = Arc::new(Mutex::new(connected_explorer()));
    let cancel = Arc::new(AtomicBool::new(false));
    let mut owner = shared.lock().unwrap();
    let old = owner.session().clone();
    let old_generation = owner.generation();
    let worker_explorer = shared.clone();
    let worker_cancel = cancel.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        crate::view::layout::capture_layout_snapshot(&worker_explorer, &worker_cancel)
    });
    started_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    // A newer confirmed operation finishes before the waiting worker gets the lock.
    owner
        .move_card(&old.cards[1].id, Point::new(6000.0, 50.0))
        .unwrap();
    owner.pan(Point::new(40.0, 20.0)).unwrap();
    let confirmed = owner.session().clone();
    let generation = owner.generation();
    assert_ne!(confirmed, old);
    assert_ne!(generation, old_generation);
    cancel.store(true, Ordering::Relaxed);
    drop(owner);
    let result = worker.join().unwrap();
    assert!(matches!(result, Err(error) if error.contains("cancelled")));
    let explorer = shared.lock().unwrap();
    assert_eq!(explorer.session(), &confirmed);
    assert_eq!(explorer.generation(), generation);
}

#[gpui::test]
fn prepared_placement_waits_for_a_read_only_explorer_lock(cx: &mut TestAppContext) {
    let (explorer, _) = fixture();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            "session.json".into(),
            vec![],
            None,
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let (shared, prepared, commit, before) = view.read_with(cx, |view, _| {
        let mut explorer = view.explorer.lock().unwrap();
        let range = explorer.session().cards[0].source.symbol.range;
        let prepared = explorer
            .prepare_add_symbol(
                Symbol::file("target.rs".into(), range),
                Point::new(800.0, 50.0),
            )
            .unwrap();
        let commit = explorer.plan_prepared(prepared.clone()).unwrap();
        (
            view.explorer.clone(),
            prepared,
            commit,
            view.session.cards.clone(),
        )
    });
    // Read-only hover holds the same exclusive guard while the backend answers.
    let reader = shared.lock().unwrap();
    view.update(cx, |view, cx| {
        view.requests.busy = true;
        view.layout.pending_output = Some(Output {
            prepared: Some(prepared),
            ..Default::default()
        });
        view.complete_placement(
            Ok(commit),
            Placement::Edit(Box::new(
                view.layout
                    .pending_output
                    .as_ref()
                    .unwrap()
                    .prepared
                    .clone()
                    .unwrap(),
            )),
            cx,
        );
        assert_eq!(view.session.cards, before);
        assert!(view.layout.pending_output.is_some());
        assert!(view.layout.planning);
        assert!(!view.requests.error);
    });
    cx.run_until_parked();
    drop(reader);
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(10));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_eq!(view.session.cards.len(), 2);
        assert_eq!(view.session.cards[0], before[0]);
        assert_eq!(view.session.cards[1].position, Point::new(800.0, 50.0));
        assert!(!view.requests.busy);
        assert!(!view.requests.error, "{}", view.requests.status);
        assert!(view.layout.pending_output.is_none());
        assert_eq!(
            view.session.cards,
            view.explorer.lock().unwrap().session().cards
        );
    });
}

#[gpui::test]
fn manual_tree_arrangement_waits_for_a_read_only_explorer_lock(cx: &mut TestAppContext) {
    let explorer = connected_explorer();
    let (view, cx) = cx.add_window_view(|window, cx| {
        ExplorerView::new(
            explorer,
            "session.json".into(),
            vec![],
            None,
            ProjectOptions::default(),
            window,
            cx,
        )
    });
    cx.run_until_parked();
    let (shared, commit, before) = view.read_with(cx, |view, _| {
        let explorer = view.explorer.lock().unwrap();
        let commit = explorer.plan_layout(None).unwrap();
        assert!(
            commit.changed,
            "the connected fixture must require tree arrangement"
        );
        (view.explorer.clone(), commit, view.session.cards.clone())
    });
    let reader = shared.lock().unwrap();
    view.update(cx, |view, cx| {
        view.requests.busy = true;
        view.layout.planning = true;
        view.complete_layout(
            Ok(commit),
            view.layout.session_epoch,
            view.layout.revision,
            cx,
        );
        assert_eq!(view.session.cards, before);
        assert!(view.requests.busy);
        assert!(view.layout.planning);
        assert!(!view.requests.error);
    });
    cx.run_until_parked();
    drop(reader);
    cx.background_executor
        .advance_clock(std::time::Duration::from_millis(10));
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        assert_ne!(view.session.cards, before);
        assert_eq!(view.session.cards[0], before[0]);
        assert_eq!(view.session.cards[3], before[3]);
        assert!(!view.requests.busy);
        assert!(view.layout.can_undo);
        assert!(!view.requests.error);
        assert_eq!(
            view.session.cards,
            view.explorer.lock().unwrap().session().cards
        );
    });
}
