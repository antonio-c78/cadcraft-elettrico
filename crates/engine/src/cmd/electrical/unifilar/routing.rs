//! Bounded Manhattan visibility grid. Symbol and label rectangles are obstacles.
use super::super::*;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
pub(super) type Rect = [f64; 4];
pub(super) fn blocked(a: Vec2, b: Vec2, boxes: &[Rect]) -> bool {
    boxes.iter().any(|r| {
        // Slab clipping works for horizontal, vertical and diagonal segments.
        let mut lo: f64 = 0.;
        let mut hi: f64 = 1.;
        for (x, delta, min, max) in [(a.x, b.x - a.x, r[0] + 1e-8, r[2] - 1e-8), (a.y, b.y - a.y, r[1] + 1e-8, r[3] - 1e-8)] {
            if delta.abs() < 1e-12 {
                if x <= min || x >= max {
                    return false;
                }
            } else {
                let u = (min - x) / delta;
                let v = (max - x) / delta;
                lo = lo.max(u.min(v));
                hi = hi.min(u.max(v));
                if lo >= hi {
                    return false;
                }
            }
        }
        hi > 0. && lo < 1.
    })
}
pub(super) fn overlaps_wire(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let v = b - a;
    let n = v.len();
    if n < 1e-7 {
        return false;
    }
    let cross = |u: Vec2, w: Vec2| u.x * w.y - u.y * w.x;
    if cross(v, c - a).abs() / n > 0.15 || cross(v, d - a).abs() / n > 0.15 {
        return false;
    }
    let u = v / n;
    let x = (c - a).dot(u);
    let y = (d - a).dot(u);
    x.max(y).min(n) - x.min(y).max(0.) > 0.15
}

pub(super) fn simplify(points: Vec<Vec2>) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::new();
    for p in points {
        if out.last().is_some_and(|v| v.near(p, 1e-7)) {
            continue;
        }
        while out.len() >= 2 {
            let n = out.len();
            let a = out[n - 2];
            let b = out[n - 1];
            if (b - a).dot(p - b) >= 0. && ((b - a).x * (p - b).y - (b - a).y * (p - b).x).abs() < 1e-7 {
                out.pop();
            } else {
                break;
            }
        }
        out.push(p);
    }
    out
}
pub(super) fn route(a: Vec2, b: Vec2, y: f64, area: Rect, boxes: &[Rect], lanes: &[f64], wires: &[(Vec2, Vec2)]) -> Result<Vec<Vec2>> {
    let fail = || bad("electrical.unifilar.link", "Percorso non disponibile: scegli un'altra altezza o sposta il simbolo.");
    if !a.is_finite() || !b.is_finite() || !y.is_finite() || y < area[1] || y > area[3] || boxes.len() > 2048 {
        return Err(fail());
    }
    let inside = |p: Vec2| p.x >= area[0] && p.x <= area[2] && p.y >= area[1] && p.y <= area[3];
    if !inside(a) || !inside(b) {
        return Err(fail());
    }
    let direct = if (a.x - b.x).abs() < 1e-8 { vec![a, b] } else { vec![a, Vec2::new(a.x, y), Vec2::new(b.x, y), b] };
    let direct = simplify(direct);
    if direct.windows(2).all(|p| !blocked(p[0], p[1], boxes) && !wires.iter().any(|(c, d)| overlaps_wire(p[0], p[1], *c, *d))) {
        return Ok(direct);
    }
    let mut xs = vec![a.x, b.x, area[0] + 1.0, area[2] - 1.0];
    let mut ys = vec![a.y, b.y, y, area[1] + 1.0, area[3] - 1.0];
    xs.extend_from_slice(lanes);
    for r in boxes {
        xs.extend([r[0] - 0.8, r[2] + 0.8]);
        ys.extend([r[1] - 0.8, r[3] + 0.8]);
    }
    let normalize = |v: &mut Vec<f64>, min: f64, max: f64| {
        v.retain(|n| n.is_finite() && *n >= min && *n <= max);
        v.sort_by(f64::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 1e-8);
    };
    normalize(&mut xs, area[0], area[2]);
    normalize(&mut ys, area[1], area[3]);
    let width = xs.len();
    let count = width.checked_mul(ys.len()).filter(|n| *n <= 40000).ok_or_else(fail)?;
    let locate = |p: Vec2| -> Option<usize> {
        Some(ys.iter().position(|v| (*v - p.y).abs() < 1e-8)? * width + xs.iter().position(|v| (*v - p.x).abs() < 1e-8)?)
    };
    let start = locate(a).ok_or_else(fail)?;
    let goal = locate(b).ok_or_else(fail)?;
    let point = |n: usize| Vec2::new(xs[n % width], ys[n / width]);
    // direction 0=initial, 1=horizontal, 2=vertical; bit 0 records the requested routing level.
    let encode = |n: usize, dir: usize, level: usize| (n * 3 + dir) * 2 + level;
    let mut dist = vec![u64::MAX; count * 6];
    let mut prev = vec![usize::MAX; count * 6];
    let mut queue = BinaryHeap::new();
    let initial = encode(start, 0, usize::from((a.x - b.x).abs() < 1e-8));
    dist[initial] = 0;
    queue.push(Reverse((0u64, initial)));
    let mut found = None;
    while let Some(Reverse((cost, state))) = queue.pop() {
        if dist[state] != cost {
            continue;
        }
        let n = state / 6;
        let direction = (state / 2) % 3;
        let level = state % 2;
        if n == goal && level == 1 {
            found = Some(state);
            break;
        }
        let col = n % width;
        let row = n / width;
        let neighbors =
            [(col > 0).then(|| n - 1), (col + 1 < width).then(|| n + 1), (row > 0).then(|| n - width), (row + 1 < ys.len()).then(|| n + width)];
        for next in neighbors.into_iter().flatten() {
            let p = point(n);
            let q = point(next);
            if blocked(p, q, boxes) || wires.iter().any(|(a, b)| overlaps_wire(p, q, *a, *b)) {
                continue;
            }
            let dir = if (p.y - q.y).abs() < 1e-8 { 1 } else { 2 };
            let on_level = level.max(usize::from(dir == 1 && (p.y - y).abs() < 1e-8));
            let target = encode(next, dir, on_level);
            let extra = (p.dist(q) * 1000.0).ceil() as u64 + if direction != 0 && direction != dir { 3000 } else { 0 };
            let new = cost.saturating_add(extra);
            if new < dist[target] {
                dist[target] = new;
                prev[target] = state;
                queue.push(Reverse((new, target)));
            }
        }
    }
    let mut at = found.ok_or_else(fail)?;
    let mut path = Vec::new();
    for _ in 0..count * 6 {
        path.push(point(at / 6));
        if at == initial {
            path.reverse();
            let path = simplify(path);
            if path.len() > 128
                || (a.x - b.x).abs() > 1e-8
                    && !path.windows(2).any(|p| (p[0].y - y).abs() < 1e-8 && (p[1].y - y).abs() < 1e-8 && (p[0].x - p[1].x).abs() > 1e-8)
            {
                return Err(fail());
            }
            return Ok(path);
        }
        at = *prev.get(at).filter(|v| **v != usize::MAX).ok_or_else(fail)?;
    }
    Err(fail())
}
