use super::check_cancelled;
use crate::Result;
use refscape_model::{CodeCard, Connection, Position};
use std::collections::{BTreeMap, VecDeque};

pub(super) struct OrderedTree {
    pub root: usize,
    pub children: Vec<Vec<usize>>,
    pub parent: Vec<Option<usize>>,
    pub depth: Vec<usize>,
    pub order: Vec<usize>,
}

impl OrderedTree {
    pub fn build(
        cards: &[CodeCard],
        connections: &[Connection],
        root: usize,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self> {
        let ids: BTreeMap<_, _> = cards
            .iter()
            .enumerate()
            .map(|(i, card)| (card.id.as_str(), i))
            .collect();
        let mut outgoing: Vec<Vec<(Position, usize)>> = vec![Vec::new(); cards.len()];
        for edge in connections {
            check_cancelled(cancelled)?;
            if let (Some(&from), Some(&to)) =
                (ids.get(edge.from.as_str()), ids.get(edge.to.as_str()))
            {
                outgoing[from].push((edge.source, to));
            }
        }
        for edges in &mut outgoing {
            edges.sort_by(|a, b| {
                let left = &cards[a.1];
                let right = &cards[b.1];
                a.0.cmp(&b.0)
                    .then(left.source.symbol.path.cmp(&right.source.symbol.path))
                    .then(
                        left.source
                            .symbol
                            .range
                            .start
                            .cmp(&right.source.symbol.range.start),
                    )
                    .then(
                        left.source
                            .symbol
                            .range
                            .end
                            .cmp(&right.source.symbol.range.end),
                    )
                    .then(left.source.symbol.id.cmp(&right.source.symbol.id))
                    .then(left.id.cmp(&right.id))
            });
        }
        let mut result = Self {
            root,
            children: vec![Vec::new(); cards.len()],
            parent: vec![None; cards.len()],
            depth: vec![0; cards.len()],
            order: Vec::new(),
        };
        let mut seen = vec![false; cards.len()];
        seen[root] = true;
        let mut pending = VecDeque::from([root]);
        while let Some(from) = pending.pop_front() {
            check_cancelled(cancelled)?;
            result.order.push(from);
            for &(_, to) in &outgoing[from] {
                // BFS first visit assigns a primary parent. Self references, cycles,
                // and other edges to shared children remain in the source graph.
                if seen[to] {
                    continue;
                }
                seen[to] = true;
                result.parent[to] = Some(from);
                result.depth[to] = result.depth[from] + 1;
                result.children[from].push(to);
                pending.push_back(to);
            }
        }
        Ok(result)
    }
}
