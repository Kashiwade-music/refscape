use super::{CardRect, LayoutRules};
use crate::Result;
use refscape_model::Point;

type CandidateRank = (f64, f64, bool, f64, f64);

/// Exact nearest point in the continuous free plane, with safe f32 boundary rounding.
/// Between forbidden rectangle edges the active intervals are constant. Therefore
/// an optimum x is the desired x projected to the half-plane, or an edge. For each
/// such x only the desired y and merged interval endpoints can minimize distance.
pub fn nearest_vacant_position(
    position: Point,
    width: f32,
    height: f32,
    min_x: Option<f64>,
    occupied: &[CardRect],
    rules: LayoutRules,
) -> Result<CardRect> {
    nearest_vacant_position_bounded(position, width, height, min_x, None, occupied, rules)
}

pub(crate) fn nearest_vacant_position_bounded(
    position: Point,
    width: f32,
    height: f32,
    min_x: Option<f64>,
    max_x: Option<f64>,
    occupied: &[CardRect],
    rules: LayoutRules,
) -> Result<CardRect> {
    rules.validate()?;
    CardRect {
        position,
        width,
        height,
    }
    .validate()?;
    if min_x.is_some_and(|x| !x.is_finite()) || max_x.is_some_and(|x| !x.is_finite()) {
        return Err("Horizontal limits must be finite".into());
    }
    for rect in occupied {
        rect.validate()?;
    }
    let minimum = min_x.unwrap_or(f64::NEG_INFINITY);
    let maximum = max_x.unwrap_or(f64::INFINITY);
    if minimum > maximum {
        return Err("Inconsistent horizontal placement limits".into());
    }
    let desired_x = f64::from(position.x).max(minimum).min(maximum);
    let projected_x = representations(desired_x)
        .into_iter()
        .filter(|x| x.is_finite() && f64::from(*x) >= minimum && f64::from(*x) <= maximum)
        .min_by(|a, b| {
            (f64::from(*a) - f64::from(position.x))
                .abs()
                .total_cmp(&(f64::from(*b) - f64::from(position.x)).abs())
        });
    if let Some(x) = projected_x {
        let rect = CardRect {
            position: Point::new(x, position.y),
            width,
            height,
        };
        if rect.validate().is_ok()
            && occupied
                .iter()
                .all(|other| !rect.overlaps_with(*other, rules))
        {
            return Ok(rect);
        }
    }
    let mut xs = representations(desired_x);
    let mut forbidden = Vec::with_capacity(occupied.len());
    for rect in occupied {
        let gap = f64::from(rules.gap);
        let left = f64::from(rect.position.x) - f64::from(width) - gap;
        let right = rect.right() + gap;
        let top = f64::from(rect.position.y) - f64::from(height) - gap;
        let bottom = rect.bottom() + gap;
        forbidden.push((left, right, top, bottom));
        xs.extend(representations(left));
        xs.extend(representations(right));
    }
    let x_boundary_count = xs.len();
    for index in 0..x_boundary_count {
        let x = xs[index];
        if x.is_finite()
            && (CardRect {
                position: Point::new(x, position.y),
                width,
                height,
            })
            .validate()
            .is_err()
        {
            xs.push(x.next_down());
            xs.push(x.next_up());
        }
    }
    xs.retain(|x| x.is_finite() && f64::from(*x) >= minimum && f64::from(*x) <= maximum);
    xs.sort_by(|a, b| a.total_cmp(b));
    xs.dedup();
    xs.sort_by(|a, b| {
        (f64::from(*a) - f64::from(position.x))
            .abs()
            .total_cmp(&(f64::from(*b) - f64::from(position.x)).abs())
            .then(a.total_cmp(b))
    });
    let mut best: Option<(CardRect, CandidateRank)> = None;
    let mut intervals = Vec::with_capacity(occupied.len());
    let mut merged: Vec<(f64, f64)> = Vec::with_capacity(occupied.len());
    for x in xs {
        let dx = f64::from(x) - f64::from(position.x);
        if best.as_ref().is_some_and(|(_, rank)| dx * dx > rank.0) {
            continue;
        }
        intervals.clear();
        intervals.extend(
            forbidden
                .iter()
                .filter(|r| f64::from(x) > r.0 && f64::from(x) < r.1)
                .map(|r| (r.2, r.3)),
        );
        intervals.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        merged.clear();
        for &(top, bottom) in &intervals {
            if let Some(last) = merged.last_mut()
                && top < last.1
            {
                last.1 = last.1.max(bottom);
            } else {
                merged.push((top, bottom));
            }
        }
        // A touching open-interval hole remains legal in continuous space, but may
        // have no f32 representation. Rounding its endpoints can enter the adjacent
        // interval. Include every union endpoint so its farther outer edge remains
        // reachable. Binary-search membership and one full check per x preserve
        // O(N² log N), rather than rechecking every y against every obstacle.
        let mut ys = vec![position.y];
        for &(top, bottom) in &merged {
            ys.extend(representations(top));
            ys.extend(representations(bottom));
        }
        let mut local_best: Option<(CardRect, CandidateRank)> = None;
        let mut index = 0;
        let boundary_count = ys.len();
        while index < ys.len() {
            let y = ys[index];
            index += 1;
            let rect = CardRect {
                position: Point::new(x, y),
                width,
                height,
            };
            if rect.validate().is_err() {
                // Half-ulp extents can disappear at an even boundary but remain
                // representable at its adjacent float. Do not add gratuitous
                // neighbors of valid exact boundaries, which can cause drift.
                if index <= boundary_count && y.is_finite() {
                    ys.push(y.next_down());
                    ys.push(y.next_up());
                }
                continue;
            }
            let active = merged.partition_point(|&(top, _)| top < f64::from(y));
            if active > 0 && f64::from(y) < merged[active - 1].1 {
                continue;
            }
            let dy = f64::from(y) - f64::from(position.y);
            let rank = (
                dx * dx + dy * dy,
                dy.abs(),
                dy < 0.0,
                f64::from(x),
                f64::from(y),
            );
            if local_best.as_ref().is_none_or(|(_, old)| rank < *old) {
                local_best = Some((rect, rank));
            }
        }
        if let Some((rect, rank)) = local_best
            && occupied
                .iter()
                .all(|other| !rect.overlaps_with(*other, rules))
            && best.as_ref().is_none_or(|(_, old)| rank < *old)
        {
            best = Some((rect, rank));
        }
    }
    best.map(|(rect, _)| rect)
        .ok_or_else(|| "Cannot find a representable unoccupied card position".into())
}

fn representations(value: f64) -> Vec<f32> {
    let rounded = value as f32;
    match f64::from(rounded).total_cmp(&value) {
        std::cmp::Ordering::Equal => vec![rounded],
        std::cmp::Ordering::Less => vec![rounded, rounded.next_up()],
        std::cmp::Ordering::Greater => vec![rounded.next_down(), rounded],
    }
}
