use super::*;

fn card() -> CodeCard {
    let range = SourceRange {
        start: Position::default(),
        end: Position::new(1, 0),
    };
    CodeCard {
        id: "card".into(),
        source: CardSource::try_from(SourceDocument {
            symbol: Symbol::file("project/main.rs".into(), range),
            code: "fn main() {}".into(),
            tokens: vec![],
            context: vec![],
            code_start: None,
            folded: vec![],
            expanded: vec![],
        })
        .unwrap(),
        position: WorldPoint::new(-500.0, -500.0).unwrap(),
        width: 500.0,
        height: 128.0,
    }
}

#[test]
fn geometry_rejects_invalid_dimensions_and_unrepresentable_edges() {
    let original = card();
    original.validate_geometry().unwrap();

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
        let Ok(position) = WorldPoint::try_from(position) else {
            continue;
        };
        invalid.position = position;
        invalid.width = width;
        invalid.height = height;
        // Validate a late invalid card as well as an isolated card.
        invalid.id = "late".into();
        assert!(invalid.validate_geometry().is_err());
    }
}

#[test]
fn typed_coordinates_reject_nonfinite_and_conversion_overflow() {
    assert!(WorldPoint::new(f32::NAN, 0.0).is_err());
    assert!(ScreenPoint::new(0.0, f32::INFINITY).is_err());
    assert!(WorldSize::new(0.0, 128.0).is_err());
    let viewport = Viewport {
        offset: ScreenPoint::default(),
        zoom: 3.0,
    };
    assert!(
        viewport
            .project_world(WorldPoint::new(f32::MAX, 0.0).unwrap())
            .is_err()
    );
    let world = WorldPoint::new(-500.0, 123.0).unwrap();
    assert_eq!(
        viewport
            .unproject_screen(viewport.project_world(world).unwrap())
            .unwrap(),
        world
    );
}
