use super::*;

#[test]
fn metadata_groups_workspace_packages_by_deepest_root() {
    let mut explorer = explorer();
    explorer.language.crates = vec![
        ProjectCrate {
            id: "outer".into(),
            name: "outer".into(),
            root: "/project".into(),
        },
        ProjectCrate {
            id: "nested".into(),
            name: "nested".into(),
            root: "/project/nested".into(),
        },
    ];
    explorer
        .open_project(
            &std::env::current_dir().unwrap(),
            &ProjectOptions::default(),
        )
        .unwrap();
    let outer = explorer
        .add_symbol(symbol("outer"), Point::default())
        .unwrap();
    let mut nested_symbol = symbol("nested");
    nested_symbol.path = "/project/nested/src/lib.rs".into();
    let nested = explorer
        .add_symbol(nested_symbol, Point::default())
        .unwrap();
    let regions = &explorer.session.regions;
    assert_eq!(
        regions
            .iter()
            .find(|region| region.id == "crate:outer")
            .unwrap()
            .card_ids,
        vec![outer]
    );
    assert_eq!(
        regions
            .iter()
            .find(|region| region.id == "crate:nested")
            .unwrap()
            .card_ids,
        vec![nested]
    );
    assert!(
        regions
            .iter()
            .all(|region| !region.id.starts_with("project:"))
    );
    explorer.session.validate().unwrap();
}
