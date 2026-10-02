//! Pure patch planning: live state is never temporarily modified and restored.
use crate::{Result, state::ApplicationSnapshot};
use refscape_canvas::{
    graph::descendant_cards_with_context,
    layout::{
        CardRect, LayoutCard, LayoutDelta, LayoutInput, LayoutRules,
        nearest_vacant_position_with_context, plan_arrange, plan_resize_with_context,
        source_dimensions, validate_layout_with_context,
    },
    regions::build_regions,
};
use refscape_model::{
    CardId, CardSource, CodeCard, Connection, ConnectionKind, OperationContext, Point, Position,
    ProjectCrate, ProjectEpoch, Symbol,
};
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TopologyRevision(pub u64);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContentRevision(pub u64);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GeometryRevision(pub u64);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentationRevision(pub u64);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditBasis {
    pub project: ProjectEpoch,
    pub topology: TopologyRevision,
    pub content: ContentRevision,
    pub geometry: GeometryRevision,
}
#[derive(Clone, Debug, Default)]
pub struct CanvasEditOutcome {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub targets: Vec<String>,
    pub origin: Option<String>,
    pub expanded: Option<bool>,
}
#[derive(Clone)]
pub struct AcquiredNavigationTarget {
    pub source: CardSource,
    pub location: refscape_analysis::NavigationLocation,
}
#[derive(Clone)]
pub enum PreparedEdit {
    Add {
        source: CardSource,
        position: Point,
    },
    Expand {
        origin: CardId,
        position: Position,
        kind: ConnectionKind,
        anchor: Point,
        sources: Vec<AcquiredNavigationTarget>,
    },
    Hide {
        origin: Option<CardId>,
        targets: Vec<CardId>,
        connection: Option<(Position, ConnectionKind)>,
    },
    Resize {
        id: CardId,
        source: CardSource,
    },
    Move {
        id: CardId,
        position: Point,
    },
    Arrange {
        selected: Option<CardId>,
    },
    Undo {
        positions: Vec<(CardId, refscape_model::WorldPoint)>,
    },
}

pub struct ValidatedCanvasPatch {
    pub(crate) cards: Arc<Vec<CodeCard>>,
    pub(crate) connections: Arc<Vec<Connection>>,
    pub(crate) regions: Arc<Vec<refscape_model::Region>>,
    pub outcome: CanvasEditOutcome,
    pub(crate) topology: bool,
    pub(crate) content: bool,
    pub(crate) geometry: bool,
    pub(crate) invalidate_undo: bool,
    pub(crate) undo: Option<Vec<(CardId, refscape_model::WorldPoint)>>,
}

pub fn same_symbol(a: &Symbol, b: &Symbol) -> bool {
    a.path == b.path && (a.id == b.id || (a.kind == b.kind && a.range == b.range))
}
pub(crate) fn layout_cards(cards: &[CodeCard]) -> Result<Vec<LayoutCard>> {
    cards.iter().map(LayoutCard::try_from).collect()
}
fn apply_delta(cards: &mut [CodeCard], delta: &LayoutDelta) -> Result<()> {
    for change in &delta.changes {
        let card = cards
            .iter_mut()
            .find(|card| card.id == change.id)
            .ok_or("Layout card no longer exists")?;
        if card.position != change.before {
            return Err("Layout snapshot is stale".into());
        }
        card.position = change.after;
    }
    Ok(())
}
fn insert(
    cards: &mut Vec<CodeCard>,
    source: CardSource,
    position: Point,
    min_x: Option<f64>,
    context: &OperationContext,
) -> Result<(CardId, bool)> {
    if let Some(card) = cards
        .iter()
        .find(|card| same_symbol(&card.source.symbol, &source.symbol))
    {
        return Ok((card.id.clone(), false));
    }
    let (width, height) = source_dimensions(&source);
    let occupied: Vec<_> = layout_cards(cards)?.iter().map(CardRect::from).collect();
    context.check()?;
    let rect = nearest_vacant_position_with_context(
        position,
        refscape_model::WorldSize::new(width, height)?,
        min_x,
        &occupied,
        LayoutRules::default(),
        context,
    )?;
    context.check()?;
    let base = format!("card:{}", source.symbol.id);
    let mut id = base.clone();
    let mut suffix = 1_u64;
    while cards.iter().any(|card| card.id == id) {
        id = format!("{base}:{suffix}");
        suffix = suffix.checked_add(1).ok_or("Card ID exhausted")?;
    }
    let id = CardId::new(id)?;
    cards.push(CodeCard {
        id: id.clone(),
        source,
        position: rect.position.try_into()?,
        width,
        height,
    });
    Ok((id, true))
}

pub fn plan_edit(
    snapshot: &ApplicationSnapshot,
    crates: &[ProjectCrate],
    edit: &PreparedEdit,
    context: &OperationContext,
) -> Result<ValidatedCanvasPatch> {
    context.check()?;
    let mut cards = (*snapshot.cards).clone();
    let mut edges = (*snapshot.connections).clone();
    let before: BTreeSet<_> = cards.iter().map(|card| card.id.clone()).collect();
    let mut outcome = CanvasEditOutcome::default();
    let (mut topology, mut content, mut geometry) = (false, false, false);
    let mut undo = None;
    let mut invalidates_undo = false;
    match edit {
        PreparedEdit::Add { source, position } => {
            let (id, added) = insert(&mut cards, source.clone(), *position, None, context)?;
            outcome.targets.push(id.to_string());
            topology = added;
            content = added;
            geometry = added;
        }
        PreparedEdit::Expand {
            origin,
            position,
            kind,
            anchor,
            sources,
        } => {
            let parent = cards
                .iter()
                .find(|card| card.id == *origin)
                .ok_or("Expansion source disappeared")?;
            let desired = Point::new(parent.position.x + anchor.x, parent.position.y + anchor.y);
            let min_x = f64::from(parent.position.x) + f64::from(parent.width) + 100.0;
            for acquired in sources {
                context.check()?;
                acquired.location.target_range.validate()?;
                acquired.location.selection_range.validate()?;
                if let Some(origin) = acquired.location.origin_range {
                    origin.validate()?;
                }
                let (target, added) = insert(
                    &mut cards,
                    acquired.source.clone(),
                    desired,
                    Some(min_x),
                    context,
                )?;
                topology |= added;
                content |= added;
                geometry |= added;
                if !outcome.targets.contains(&target.to_string()) {
                    outcome.targets.push(target.to_string());
                }
                if !edges.iter().any(|edge| {
                    edge.from == *origin
                        && edge.to == target
                        && edge.source == *position
                        && edge.kind == *kind
                }) {
                    let mut number = edges.len();
                    let id = loop {
                        let id = format!("connection:{number}");
                        if !edges.iter().any(|edge| edge.id == id) {
                            break id;
                        }
                        number = number.checked_add(1).ok_or("Connection IDs exhausted")?;
                    };
                    edges.push(Connection {
                        id: id.into(),
                        from: origin.clone(),
                        to: target,
                        source: *position,
                        kind: *kind,
                    });
                    topology = true;
                }
            }
            outcome.origin = Some(origin.to_string());
            outcome.expanded = Some(true);
        }
        PreparedEdit::Hide {
            origin,
            targets,
            connection,
        } => {
            let targets: Vec<_> = targets.clone();
            let preserved: Vec<_> = origin.iter().cloned().collect();
            let removed = descendant_cards_with_context(&edges, &targets, &preserved, context)?;
            cards.retain(|card| !removed.contains(&card.id));
            edges.retain(|edge| !removed.contains(&edge.from) && !removed.contains(&edge.to));
            if let (Some(origin), Some((position, kind))) = (origin, connection) {
                edges.retain(|edge| {
                    !(edge.from == *origin && edge.source == *position && edge.kind == *kind)
                });
            }
            topology = true;
            content = !removed.is_empty();
            geometry = content;
            outcome.origin = origin.as_ref().map(ToString::to_string);
            outcome.expanded = Some(false);
        }
        PreparedEdit::Resize { id, source } => {
            let index = cards
                .iter()
                .position(|card| card.id == *id)
                .ok_or("Resized card disappeared")?;
            let (width, height) = source_dimensions(source);
            let delta = plan_resize_with_context(
                &layout_cards(&cards)?,
                id,
                refscape_model::WorldSize::new(width, height)?,
                LayoutRules::default(),
                context,
            )?;
            cards[index].source = source.clone();
            cards[index].width = width;
            cards[index].height = height;
            apply_delta(&mut cards, &delta)?;
            content = true;
            geometry = true;
            invalidates_undo = true;
            outcome.origin = Some(id.to_string());
        }
        PreparedEdit::Move { id, position } => {
            let index = cards
                .iter()
                .position(|card| card.id == *id)
                .ok_or("Unknown card")?;
            let occupied: Vec<_> = layout_cards(&cards)?
                .iter()
                .filter(|card| card.id != *id)
                .map(CardRect::from)
                .collect();
            let rect = nearest_vacant_position_with_context(
                *position,
                refscape_model::WorldSize::new(cards[index].width, cards[index].display_height())?,
                None,
                &occupied,
                LayoutRules::default(),
                context,
            )?;
            geometry = cards[index].position != rect.position;
            cards[index].position = rect.position.try_into()?;
            invalidates_undo = true;
            outcome.origin = Some(id.to_string());
        }
        PreparedEdit::Arrange { selected } => {
            let delta = plan_arrange(
                &LayoutInput {
                    cards: layout_cards(&cards)?,
                    connections: edges.clone(),
                },
                selected.as_deref(),
                LayoutRules::default(),
                context,
            )?;
            if !delta.changes.is_empty() {
                undo = Some(
                    delta
                        .changes
                        .iter()
                        .map(|change| (change.id.clone(), change.before))
                        .collect(),
                );
                geometry = true;
            }
            apply_delta(&mut cards, &delta)?;
        }
        PreparedEdit::Undo { positions } => {
            for (id, position) in positions {
                cards
                    .iter_mut()
                    .find(|card| card.id == *id)
                    .ok_or("Layout undo no longer matches cards")?
                    .position = *position;
            }
            geometry = !positions.is_empty();
            invalidates_undo = true;
        }
    }
    context.check()?;
    validate_layout_with_context(&layout_cards(&cards)?, LayoutRules::default(), context)?;
    let after: BTreeSet<_> = cards.iter().map(|card| card.id.clone()).collect();
    outcome.added = after.difference(&before).map(ToString::to_string).collect();
    outcome.removed = before.difference(&after).map(ToString::to_string).collect();
    let regions = build_regions(&layout_cards(&cards)?, &snapshot.project_root, crates);
    let candidate = ApplicationSnapshot {
        cards: Arc::new(cards),
        connections: Arc::new(edges),
        regions: Arc::new(regions),
        ..snapshot.clone()
    };
    candidate.validate()?;
    context.check()?;
    Ok(ValidatedCanvasPatch {
        cards: candidate.cards,
        connections: candidate.connections,
        regions: candidate.regions,
        outcome,
        topology,
        content,
        geometry,
        invalidate_undo: invalidates_undo,
        undo,
    })
}
