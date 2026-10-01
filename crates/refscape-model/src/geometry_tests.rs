use super::*;

fn card() -> CodeCard {
    let range = SourceRange {
        start: Position::default(),
        end: Position::new(1, 0),
    };
    CodeCard {
        id: "card".into(),
        source: SourceDocument {
            symbol: Symbol::file("project/main.rs".into(), range),
            code: "fn main() {}".into(),
            tokens: vec![],
            context: vec![],
            code_start: None,
            folded: vec![],
            expanded: vec![],
        },
        position: Point::new(-500.0, -500.0),
        width: 500.0,
        height: 128.0,
    }
}

#[test]
fn geometry_rejects_invalid_dimensions_and_unrepresentable_edges() {
    let original = card();
    original.validate_geometry().unwrap();
    let mut session = Session::new("project".into());
    session.cards.push(original.clone());
    session.validate().unwrap();
    for (position, width, height) in [
        (Point::new(f32::NAN, 0.0), 500.0, 128.0),
        (Point::new(0.0, f32::INFINITY), 500.0, 128.0),
        (Point::default(), f32::INFINITY, 128.0),
        (Point::default(), 500.0, f32::NAN),
        (Point::default(), 0.0, 128.0),
        (Point::default(), 500.0, -1.0),
        (Point::new(f32::MAX, 0.0), f32::MAX, 128.0),
        (Point::new(0.0, f32::MAX), 500.0, f32::MAX),
        (Point::new(f32::MAX, 0.0), 500.0, 128.0),
        (Point::new(0.0, -f32::MAX), 500.0, 128.0),
    ] {
        let mut invalid = original.clone();
        invalid.position = position;
        invalid.width = width;
        invalid.height = height;
        // Validate a late invalid card as well as an isolated card.
        invalid.id = "late".into();
        session.cards.push(invalid);
        assert!(session.validate().is_err());
        session.cards.pop();
        assert_eq!(session.cards[0], original);
    }
}

#[test]
fn session_validation_keeps_overlap_repair_in_the_canvas_layer() {
    let mut session = Session::new("project".into());
    session.cards = vec![card(), card()];
    session.cards[1].id = "other".into();
    session.validate().unwrap();
}
