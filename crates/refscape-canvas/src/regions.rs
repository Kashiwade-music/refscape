//! Source-file and project grouping derived from the visible cards.

use refscape_model::{CardId, ProjectCrate, Region, RegionKind};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub fn build_regions(
    cards: &[crate::layout::LayoutCard],
    project_root: &Path,
    project_crates: &[ProjectCrate],
) -> Vec<Region> {
    let mut modules: BTreeMap<PathBuf, Vec<CardId>> = BTreeMap::new();
    let mut crates: BTreeMap<String, (String, PathBuf, Vec<CardId>)> = BTreeMap::new();
    let mut unmatched = Vec::new();
    for card in cards {
        modules
            .entry(card.order.path.clone())
            .or_default()
            .push(card.id.clone());
        match project_crates
            .iter()
            .filter(|project| card.order.path.starts_with(&project.root))
            .max_by_key(|project| project.root.components().count())
        {
            Some(project) => crates
                .entry(project.id.clone())
                .or_insert_with(|| (project.name.clone(), project.root.clone(), Vec::new()))
                .2
                .push(card.id.clone()),
            None => unmatched.push(card.id.clone()),
        }
    }
    let mut regions: Vec<Region> = modules
        .into_iter()
        .map(|(path, card_ids)| Region {
            kind: RegionKind::Module,
            id: format!("module:{}", path.to_string_lossy()),
            label: path
                .strip_prefix(project_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned(),
            path,
            card_ids,
        })
        .collect();
    for (id, (label, path, card_ids)) in crates.into_iter().rev() {
        regions.insert(
            0,
            Region {
                kind: RegionKind::Crate,
                id: format!("crate:{id}"),
                label,
                path,
                card_ids,
            },
        );
    }
    if !unmatched.is_empty() {
        regions.insert(
            0,
            Region {
                kind: RegionKind::Project,
                id: format!("project:{}", project_root.to_string_lossy()),
                label: project_root
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path: project_root.to_path_buf(),
                card_ids: unmatched,
            },
        );
    }
    regions
}
