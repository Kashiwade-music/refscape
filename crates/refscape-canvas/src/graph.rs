//! Branch removal policy for shared and cyclic card graphs.

use refscape_model::Connection;
use std::collections::BTreeSet;

/// Close the branch, preserving the clicked source and descendants reached by another branch.
/// Work from the original graph so cycles cannot strand an orphaned group.
pub fn descendant_cards(
    connections: &[Connection],
    ids: &[String],
    preserved: &[String],
) -> BTreeSet<String> {
    let explicit: BTreeSet<_> = ids.iter().cloned().collect();
    let mut candidates = explicit.clone();
    let mut pending = ids.to_vec();
    while let Some(id) = pending.pop() {
        for edge in connections.iter().filter(|edge| edge.from == id) {
            if !preserved.contains(&edge.to) && candidates.insert(edge.to.clone()) {
                pending.push(edge.to.clone());
            }
        }
    }
    let mut retained = BTreeSet::new();
    let mut pending: Vec<_> = connections
        .iter()
        .filter(|edge| {
            !candidates.contains(&edge.from)
                && candidates.contains(&edge.to)
                && !explicit.contains(&edge.to)
        })
        .map(|edge| edge.to.clone())
        .collect();
    while let Some(id) = pending.pop() {
        if !retained.insert(id.clone()) {
            continue;
        }
        for edge in connections.iter().filter(|edge| edge.from == id) {
            if candidates.contains(&edge.to) && !explicit.contains(&edge.to) {
                pending.push(edge.to.clone());
            }
        }
    }
    candidates.retain(|id| !retained.contains(id));
    candidates
}
