//! Branch removal over indexed ID-only graph inputs.
use refscape_model::{CardId, Connection};
use std::collections::{BTreeSet, HashMap};

struct Adjacency<'a> {
    connections: &'a [Connection],
    index: Option<HashMap<&'a str, Vec<&'a str>>>,
    scratch: Vec<&'a str>,
    scanned_nodes: usize,
}
impl<'a> Adjacency<'a> {
    fn neighbors(&mut self, id: &str, cancelled: &dyn Fn() -> bool) -> crate::Result<&[&'a str]> {
        // A bounded shallow prefix avoids allocating an index for a leaf or tiny
        // branch. Deeper walks build it once, so full-edge scans stay O(E).
        if self.index.is_none() && self.scanned_nodes == 4 {
            let mut index: HashMap<&str, Vec<&str>> = HashMap::new();
            for (i, edge) in self.connections.iter().enumerate() {
                if i % 64 == 0 {
                    crate::layout::check_cancelled(cancelled)?;
                }
                index
                    .entry(edge.from.as_str())
                    .or_default()
                    .push(edge.to.as_str());
            }
            self.index = Some(index);
        }
        if let Some(index) = &self.index {
            return Ok(index.get(id).map_or(&[], Vec::as_slice));
        }
        self.scanned_nodes += 1;
        self.scratch.clear();
        for (i, edge) in self.connections.iter().enumerate() {
            if i % 64 == 0 {
                crate::layout::check_cancelled(cancelled)?;
            }
            if edge.from == id {
                self.scratch.push(edge.to.as_str());
            }
        }
        Ok(&self.scratch)
    }
}

pub fn descendant_cards_with_context(
    connections: &[Connection],
    ids: &[CardId],
    preserved: &[CardId],
    operation: &refscape_model::OperationContext,
) -> Result<BTreeSet<CardId>, refscape_model::RefscapeError> {
    operation.check()?;
    let result =
        descendant_cards_cancellable(connections, ids, preserved, &|| operation.check().is_err());
    operation.check()?;
    result
}

pub fn descendant_cards(
    connections: &[Connection],
    ids: &[CardId],
    preserved: &[CardId],
) -> BTreeSet<CardId> {
    descendant_cards_cancellable(connections, ids, preserved, &|| false)
        .expect("An uncancellable graph traversal cannot be cancelled")
}

pub(crate) fn descendant_cards_cancellable(
    connections: &[Connection],
    ids: &[CardId],
    preserved: &[CardId],
    cancelled: &dyn Fn() -> bool,
) -> crate::Result<BTreeSet<CardId>> {
    // Keys are lookup-only; saved adjacency order and sorted result sets determine
    // behavior, so hash iteration order never participates in branch policy.
    let mut outgoing = Adjacency {
        connections,
        index: None,
        scratch: Vec::new(),
        scanned_nodes: 0,
    };
    let explicit: BTreeSet<_> = ids.iter().cloned().collect();
    let preserved: BTreeSet<_> = preserved.iter().map(CardId::as_str).collect();
    let mut candidates = explicit.clone();
    let mut pending = ids.to_vec();
    while let Some(id) = pending.pop() {
        crate::layout::check_cancelled(cancelled)?;
        for &to in outgoing.neighbors(id.as_str(), cancelled)? {
            if !preserved.contains(to) && candidates.insert(CardId::from(to)) {
                pending.push(CardId::from(to));
            }
        }
    }
    let mut retained = BTreeSet::new();
    let mut pending = Vec::new();
    for (index, edge) in connections.iter().enumerate() {
        if index % 64 == 0 {
            crate::layout::check_cancelled(cancelled)?;
        }
        if !candidates.contains(&edge.from)
            && candidates.contains(&edge.to)
            && !explicit.contains(&edge.to)
            && retained.insert(edge.to.clone())
        {
            pending.push(edge.to.clone());
        }
    }
    while let Some(id) = pending.pop() {
        crate::layout::check_cancelled(cancelled)?;
        for &to in outgoing.neighbors(id.as_str(), cancelled)? {
            if candidates.contains(to)
                && !explicit.contains(to)
                && retained.insert(CardId::from(to))
            {
                pending.push(CardId::from(to));
            }
        }
    }
    candidates.retain(|id| !retained.contains(id));
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use refscape_model::{ConnectionKind, Position};
    use std::cell::Cell;
    fn edge(from: usize, to: usize) -> Connection {
        Connection {
            id: format!("{from}:{to}").into(),
            from: from.to_string().into(),
            to: to.to_string().into(),
            source: Position::default(),
            kind: ConnectionKind::Definition,
        }
    }
    // Matrix fixed-point oracle deliberately does not traverse the planner's adjacency map.
    #[test]
    fn every_three_node_graph_matches_independent_removal_oracle() {
        for mask in 0..512 {
            let edges: Vec<_> = (0..9)
                .filter(|&bit| mask & (1 << bit) != 0)
                .map(|bit| edge(bit / 3, bit % 3))
                .collect();
            for explicit_mask in 1..8 {
                for preserved_mask in 0..8 {
                    let explicit =
                        std::array::from_fn::<_, 3, _>(|i| explicit_mask & (1 << i) != 0);
                    let preserved =
                        std::array::from_fn::<_, 3, _>(|i| preserved_mask & (1 << i) != 0);
                    let mut candidate = explicit;
                    for _ in 0..3 {
                        for from in 0..3 {
                            for to in 0..3 {
                                if candidate[from]
                                    && !preserved[to]
                                    && mask & (1 << (from * 3 + to)) != 0
                                {
                                    candidate[to] = true;
                                }
                            }
                        }
                    }
                    let mut retained = [false; 3];
                    for from in 0..3 {
                        for to in 0..3 {
                            if !candidate[from]
                                && candidate[to]
                                && !explicit[to]
                                && mask & (1 << (from * 3 + to)) != 0
                            {
                                retained[to] = true;
                            }
                        }
                    }
                    for _ in 0..3 {
                        for from in 0..3 {
                            for to in 0..3 {
                                if retained[from]
                                    && candidate[to]
                                    && !explicit[to]
                                    && mask & (1 << (from * 3 + to)) != 0
                                {
                                    retained[to] = true;
                                }
                            }
                        }
                    }
                    let expected: BTreeSet<CardId> = (0..3)
                        .filter(|&i| candidate[i] && !retained[i])
                        .map(|i| i.to_string().into())
                        .collect();
                    let ids: Vec<_> = (0..3)
                        .filter(|&i| explicit[i])
                        .map(|i| i.to_string().into())
                        .collect();
                    let keep: Vec<_> = (0..3)
                        .filter(|&i| preserved[i])
                        .map(|i| i.to_string().into())
                        .collect();
                    assert_eq!(
                        descendant_cards(&edges, &ids, &keep),
                        expected,
                        "graph={mask},explicit={explicit_mask},preserved={preserved_mask}"
                    );
                }
            }
        }
    }
    #[test]
    fn cancellation_at_every_graph_checkpoint_never_changes_input() {
        let edges: Vec<_> = (0..10).map(|from| edge(from, (from + 1) % 10)).collect();
        let saved = edges.clone();
        let count = Cell::new(0);
        descendant_cards_cancellable(&edges, &["1".into()], &[], &|| {
            count.set(count.get() + 1);
            false
        })
        .unwrap();
        for stop in 1..=count.get() {
            let calls = Cell::new(0);
            assert!(
                descendant_cards_cancellable(&edges, &["1".into()], &[], &|| {
                    calls.set(calls.get() + 1);
                    calls.get() >= stop
                })
                .is_err()
            );
            assert_eq!(edges, saved);
        }
    }

    #[test]
    fn deep_traversal_builds_one_index_and_retains_a_supported_shared_cycle() {
        let mut edges: Vec<_> = (0..8).map(|from| edge(from, from + 1)).collect();
        edges.extend([edge(8, 6), edge(9, 6)]);
        let removed = descendant_cards(&edges, &["1".into()], &[]);
        assert_eq!(
            removed,
            (1..6).map(|id| CardId::from(id.to_string())).collect()
        );
        let mut adjacency = Adjacency {
            connections: &edges,
            index: None,
            scratch: Vec::new(),
            scanned_nodes: 0,
        };
        for id in 0..4 {
            assert_eq!(
                adjacency.neighbors(&id.to_string(), &|| false).unwrap(),
                [&(id + 1).to_string()]
            );
            assert!(adjacency.index.is_none());
        }
        assert_eq!(adjacency.neighbors("4", &|| false).unwrap(), ["5"]);
        assert!(adjacency.index.is_some());
        assert_eq!(adjacency.neighbors("9", &|| false).unwrap(), ["6"]);
        assert_eq!(adjacency.scanned_nodes, 4);
    }
}
