use std::path::PathBuf;

use refscape_model::{
    CodeCard, Connection, ConnectionKind, Point, Position, ProjectCrate, SourceDocument,
    SourceRange, Symbol,
};

use crate::{
    graph::descendant_cards,
    layout::{arrange_cards, compact_cards},
    regions::build_regions,
};

fn card(id: &str, position: Point) -> CodeCard {
    CodeCard {
        id: id.into(),
        source: SourceDocument {
            symbol: Symbol::file(
                PathBuf::from(format!("/project/{id}.rs")),
                SourceRange::default(),
            ),
            code: "fn example() {}".into(),
            tokens: Vec::new(),
        },
        position,
        width: 520.0,
        height: 120.0,
    }
}

fn edge(from: &str, to: &str) -> Connection {
    Connection {
        id: format!("{from}:{to}"),
        from: from.into(),
        to: to.into(),
        kind: ConnectionKind::Definition,
        source: Position::default(),
    }
}

#[test]
fn invalid_later_card_does_not_apply_earlier_reflow() {
    let mut cards = vec![
        card("first", Point::default()),
        card("overlapping", Point::default()),
        card("invalid", Point::new(f32::MAX, 0.0)),
    ];
    cards[2].width = f32::MAX;
    let before = cards.clone();
    assert!(arrange_cards(&mut cards).is_err());
    assert_eq!(cards, before);
}

#[test]
fn failed_compaction_does_not_apply_any_placements() {
    let mut cards = vec![
        card("first", Point::new(100.0, 300.0)),
        card("second", Point::new(800.0, 500.0)),
    ];
    cards[1].width = f32::MAX;
    let before = cards.clone();
    assert!(compact_cards(&mut cards, Point::new(f32::MAX, 0.0), &[]).is_err());
    assert_eq!(cards, before);
}

#[test]
fn removal_preserves_shared_cycle_reachable_from_another_branch() {
    let edges = vec![
        edge("root", "removed"),
        edge("removed", "shared"),
        edge("other", "shared"),
        edge("shared", "cycle"),
        edge("cycle", "shared"),
    ];
    let removed = descendant_cards(&edges, &["removed".into()], &[]);
    assert_eq!(removed.into_iter().collect::<Vec<_>>(), vec!["removed"]);
}

#[test]
fn removal_closes_orphaned_cycle_but_preserves_clicked_source() {
    let edges = vec![edge("root", "child"), edge("child", "root")];
    let removed = descendant_cards(&edges, &["child".into()], &["root".into()]);
    assert_eq!(removed.into_iter().collect::<Vec<_>>(), vec!["child"]);
}

#[test]
fn regions_assign_cards_to_the_deepest_package_root() {
    let mut nested = card("nested", Point::default());
    nested.source.symbol.path = PathBuf::from("/project/nested/src/lib.rs");
    let projects = vec![
        ProjectCrate {
            id: "outer".into(),
            name: "outer".into(),
            root: PathBuf::from("/project"),
        },
        ProjectCrate {
            id: "inner".into(),
            name: "inner".into(),
            root: PathBuf::from("/project/nested"),
        },
    ];
    let regions = build_regions(&[nested], &PathBuf::from("/project"), &projects);
    let package = regions
        .iter()
        .find(|region| region.id == "crate:inner")
        .unwrap();
    assert_eq!(package.card_ids, vec!["nested"]);
    assert!(regions.iter().all(|region| region.id != "crate:outer"));
    let file = regions
        .iter()
        .find(|region| region.id.starts_with("module:"))
        .unwrap();
    assert_eq!(file.label, "nested/src/lib.rs");
}
