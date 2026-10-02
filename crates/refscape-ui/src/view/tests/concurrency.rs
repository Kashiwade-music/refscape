//! Completion ordering regressions traverse the public controller/effect boundary.
use super::layout::connected_explorer;
use super::*;
use refscape_application::{Completion, Effect, Transition};
use refscape_model::ErrorKind;

fn finish(fixture: &mut FixtureDriver, transition: Transition) {
    let mut pending: std::collections::VecDeque<_> = transition.effects.into();
    while let Some(effect) = pending.pop_front() {
        let completion = fixture.driver.executor.execute(effect);
        let next = fixture.driver.controller.complete(completion);
        pending.extend(next.effects);
    }
}

#[test]
fn a_cancelled_worker_with_an_old_snapshot_cannot_restore_new_geometry() {
    let mut fixture = connected_explorer();
    let old = fixture.snapshot().clone();
    let old_basis = fixture.driver.controller.basis();
    let transition = fixture
        .driver
        .controller
        .dispatch(Command::Arrange { selected: None });
    let effect = transition
        .effects
        .into_iter()
        .find(|effect| matches!(effect, Effect::PlanCanvas { .. }))
        .unwrap();
    let (context, basis, edit, interaction) = match &effect {
        Effect::PlanCanvas {
            context,
            basis,
            edit,
            interaction,
            ..
        } => (context.clone(), *basis, edit.clone(), *interaction),
        _ => unreachable!(),
    };
    let executor = fixture.driver.executor.clone();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        executor.execute(effect)
    });
    started_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    // Cancellation frees the owner to accept newer intent while a worker still owns its old input.
    context.cancel.cancel();
    let cancelled = fixture
        .driver
        .controller
        .complete(Completion::CanvasPlanned {
            context: context.clone(),
            basis,
            edit,
            interaction,
            result: Err(refscape_model::RefscapeError::new(
                ErrorKind::Cancelled,
                "Layout calculation cancelled",
            )),
        });
    finish(&mut fixture, cancelled);
    fixture
        .move_card(&old.cards[1].id, Point::new(6000.0, 50.0))
        .unwrap();
    fixture.pan(Point::new(40.0, 20.0)).unwrap();
    let confirmed = fixture.snapshot().clone();
    let basis = fixture.driver.controller.basis();
    assert_ne!(confirmed, old);
    assert_ne!(basis, old_basis);
    release_tx.send(()).unwrap();
    let completion = worker.join().unwrap();
    assert!(
        matches!(&completion,Completion::CanvasPlanned{result:Err(error),..} if error.kind==ErrorKind::Cancelled)
    );
    let duplicate = fixture.driver.controller.complete(completion);
    finish(&mut fixture, duplicate);
    assert_eq!(fixture.snapshot(), &confirmed);
    assert_eq!(fixture.driver.controller.basis(), basis);
}

#[test]
fn prepared_placement_commits_from_its_owned_input_while_hover_answer_is_pending() {
    let (mut fixture, _) = fixture();
    let before = fixture.snapshot().cards.clone();
    let hover = fixture.driver.controller.dispatch(Command::Hover {
        card: before[0].id.to_string(),
        position: Position::new(12, 9),
    });
    assert_eq!(hover.effects.len(), 1);
    let range = before[0].source.symbol.range;
    let query = fixture.driver.controller.dispatch(Command::AddSymbol {
        symbol: Symbol::file("target.rs".into(), range),
        position: Point::new(800.0, 50.0),
        toggle: false,
    });
    let source_completion = fixture
        .driver
        .executor
        .execute(query.effects.into_iter().next().unwrap());
    let plan = fixture.driver.controller.complete(source_completion);
    assert_eq!(fixture.snapshot().cards, before);
    assert!(fixture.driver.controller.busy());
    finish(&mut fixture, plan);
    assert_eq!(fixture.snapshot().cards.len(), 2);
    assert_eq!(fixture.snapshot().cards[0], before[0]);
    assert_eq!(
        fixture.snapshot().cards[1].position,
        Point::new(800.0, 50.0)
    );
    assert!(!fixture.driver.controller.busy());
    assert!(
        !fixture.driver.controller.error(),
        "{}",
        fixture.driver.controller.status()
    );
    let confirmed = fixture.snapshot().clone();
    finish(&mut fixture, hover);
    assert_eq!(
        fixture.snapshot(),
        &confirmed,
        "a read-only completion cannot overwrite a canvas commit"
    );
}

#[test]
fn manual_tree_arrangement_keeps_camera_and_fixed_cards_with_hover_answer_pending() {
    let mut fixture = connected_explorer();
    let before = fixture.snapshot().cards.clone();
    let hover = fixture.driver.controller.dispatch(Command::Hover {
        card: before[0].id.to_string(),
        position: Position::new(12, 9),
    });
    assert_eq!(hover.effects.len(), 1);
    let arrange = fixture
        .driver
        .controller
        .dispatch(Command::Arrange { selected: None });
    assert!(
        !arrange.effects.is_empty(),
        "connected fixture must require tree arrangement"
    );
    assert_eq!(
        fixture.snapshot().cards,
        before,
        "planning does not mutate the owner"
    );
    fixture.pan(Point::new(40.0, 20.0)).unwrap();
    let viewport = fixture.snapshot().viewport;
    finish(&mut fixture, arrange);
    assert_ne!(fixture.snapshot().cards, before);
    assert_eq!(fixture.snapshot().cards[0], before[0]);
    assert_eq!(fixture.snapshot().cards[3], before[3]);
    assert_eq!(fixture.snapshot().viewport, viewport);
    assert!(fixture.driver.controller.can_undo_layout());
    assert!(!fixture.driver.controller.busy());
    assert!(!fixture.driver.controller.error());
    let confirmed = fixture.snapshot().clone();
    finish(&mut fixture, hover);
    assert_eq!(fixture.snapshot(), &confirmed);
}
