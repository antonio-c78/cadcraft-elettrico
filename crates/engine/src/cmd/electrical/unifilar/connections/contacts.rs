//! Shared terminal runs belong to the explicitly attached parent, not to the branch.
use super::*;
const EPS: f64 = 1e-6;
fn coverage(a: Vec2, b: Vec2, parent: &[Vec2]) -> f64 {
    let delta = b - a;
    let len = delta.len();
    if len < EPS {
        return 1.;
    }
    let unit = delta / len;
    let cross = |v: Vec2| unit.x * v.y - unit.y * v.x;
    let mut intervals = Vec::new();
    for p in parent.windows(2) {
        if cross(p[0] - a).abs() > EPS || cross(p[1] - a).abs() > EPS {
            continue;
        }
        let x = (p[0] - a).dot(unit);
        let y = (p[1] - a).dot(unit);
        let lo = x.min(y).max(0.);
        let hi = x.max(y).min(len);
        if hi >= lo {
            intervals.push((lo, hi));
        }
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut end: f64 = 0.;
    for (lo, hi) in intervals {
        if lo > end + EPS {
            break;
        }
        end = end.max(hi);
    }
    (end / len).clamp(0., 1.)
}
/// Walk only a contiguous shared prefix; an unrelated crossing is never a contact.
pub(super) fn departure(path: &[Vec2], parent: &[Vec2]) -> Option<(usize, Vec2)> {
    let mut at = *path.first()?;
    let mut index = 0;
    for (i, p) in path.windows(2).enumerate() {
        let fraction = coverage(p[0], p[1], parent);
        if fraction <= EPS {
            break;
        }
        at = p[0] + (p[1] - p[0]) * fraction;
        index = i + 1;
        if fraction < 1. - EPS {
            break;
        }
    }
    Some((index, at))
}
fn fraction(parent: &[Vec2], at: Vec2) -> Result<f64> {
    let total: f64 = parent.windows(2).map(|p| p[0].dist(p[1])).sum();
    if total < EPS {
        return Err(err("Il filo condiviso non ha lunghezza."));
    }
    let mut before = 0.;
    for p in parent.windows(2) {
        let d = p[1] - p[0];
        let len = d.len();
        if len > EPS {
            let t = ((at - p[0]).dot(d) / (len * len)).clamp(0., 1.);
            if (p[0] + d * t).near(at, EPS) {
                return Ok((before + t * len) / total);
            }
        }
        before += len;
    }
    Err(err("Il punto di separazione non appartiene al filo."))
}
fn on_segment(p: Vec2, a: Vec2, b: Vec2) -> bool {
    let delta = b - a;
    let len = delta.len();
    if len < EPS {
        return p.near(a, EPS);
    }
    let t = (p - a).dot(delta) / (len * len);
    (-EPS..=1. + EPS).contains(&t) && (a + delta * t).near(p, EPS)
}
/// A parent may overshoot a corner and double back before entering a device.
/// When the branch attaches to that retraced tip, its physical T is at the fold,
/// not at the tip. Only overlapping parent runs qualify, never mere crossings.
pub(super) fn attachment(path: &[Vec2], parent: &[Vec2]) -> Option<Vec2> {
    let (_, at) = departure(path, parent)?;
    let segments: Vec<_> = parent.windows(2).filter(|s| on_segment(at, s[0], s[1])).collect();
    let mut folds = Vec::new();
    for (i, a) in segments.iter().enumerate() {
        for b in segments.iter().skip(i + 1) {
            if coverage(a[0], a[1], b) <= EPS && coverage(b[0], b[1], a) <= EPS {
                continue;
            }
            for p in [a[0], a[1], b[0], b[1]] {
                if !on_segment(p, a[0], a[1]) || !on_segment(p, b[0], b[1]) || folds.iter().any(|q: &Vec2| q.near(p, EPS)) {
                    continue;
                }
                let mut rays = Vec::<Vec2>::new();
                for s in parent.windows(2).filter(|s| on_segment(p, s[0], s[1])) {
                    for end in s {
                        let delta = *end - p;
                        if delta.len() > EPS {
                            let ray = delta / delta.len();
                            if !rays.iter().any(|r| r.near(ray, EPS)) {
                                rays.push(ray);
                            }
                        }
                    }
                }
                if rays.len() >= 3 {
                    folds.push(p);
                }
            }
        }
    }
    folds.into_iter().min_by(|a, b| a.dist(at).total_cmp(&b.dist(at))).or(Some(at))
}
pub(super) fn junction(env: &Env, tap: &Tap, path: &[Vec2], end: bool) -> Result<Vec2> {
    let parent = env.tap_path(tap)?;
    let reversed: Vec<_> = if end { path.iter().rev().copied().collect() } else { path.to_vec() };
    attachment(&reversed, &parent).ok_or_else(|| err("Percorso vuoto."))
}
/// Normalize only when the caller has established that no downstream anchors can be lost.
pub(super) fn normalize(env: &Env, link: &Link, path: Vec<Vec2>) -> Result<Link> {
    if link.kind != "wire" {
        return Ok(link.clone());
    }
    let mut result = link.clone();
    let mut path = path;
    let mut changed = false;
    for end in [false, true] {
        let tap = if end { result.end_tap.as_mut() } else { result.tap.as_mut() };
        let Some(tap) = tap else {
            continue;
        };
        let parent = env.tap_path(tap)?;
        if end {
            path.reverse();
        }
        if let Some((index, at)) = departure(&path, &parent)
            && index > 0
        {
            tap.fraction = fraction(&parent, at)?;
            path = std::iter::once(at).chain(path.into_iter().skip(index)).collect();
            path = routing::simplify(path);
            changed = true;
        }
        if let Some(at) = attachment(&path, &parent)
            && path.first().is_some_and(|p| !p.near(at, EPS))
        {
            tap.fraction = fraction(&parent, at)?;
            path.insert(0, at);
            path = routing::simplify(path);
            changed = true;
        }
        if end {
            path.reverse();
        }
    }
    if !changed {
        return Ok(result);
    }
    if path.len() < 2 {
        return Err(err("Il percorso coincide con il filo esistente: scegli un punto di separazione."));
    }
    result.waypoints = path.iter().skip(1).take(path.len().saturating_sub(2)).map(|p| [p.x, p.y]).collect();
    if result.waypoints.len() > 32 {
        return Err(err("Troppi vertici nel percorso condiviso."));
    }
    result.segment_modes = path
        .windows(2)
        .map(|p| {
            let d = p[1] - p[0];
            if d.x.abs() < EPS || d.y.abs() < EPS {
                tracing::RouteMode::Orthogonal
            } else if (d.x.abs() - d.y.abs()).abs() < EPS {
                tracing::RouteMode::Diagonal
            } else {
                tracing::RouteMode::Free
            }
        })
        .collect();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }
    #[test]
    fn shared_prefix_can_follow_multiple_parent_segments_and_turns() {
        let parent = [v(0., 0.), v(0., 5.), v(0., 10.), v(10., 10.)];
        let path = [v(0., 2.), v(0., 10.), v(5., 10.), v(5., 15.)];
        assert_eq!(departure(&path, &parent), Some((2, v(5., 10.))));
    }
    #[test]
    fn diagonal_run_and_partial_segment_stop_at_the_parent_endpoint() {
        let parent = [v(0., 0.), v(10., 10.)];
        assert_eq!(departure(&[v(2., 2.), v(8., 8.), v(12., 8.)], &parent), Some((1, v(8., 8.))));
        assert_eq!(departure(&[v(2., 2.), v(15., 15.), v(20., 15.)], &parent), Some((1, v(10., 10.))));
    }
    #[test]
    fn crossings_gaps_and_parallel_offsets_are_not_shared_runs() {
        let parent = [v(0., 0.), v(0., 10.)];
        assert_eq!(departure(&[v(0., 5.), v(5., 5.)], &parent), Some((0, v(0., 5.))));
        assert_eq!(departure(&[v(0.01, 5.), v(0.01, 8.)], &parent), Some((0, v(0.01, 5.))));
        assert_eq!(coverage(v(0., 2.), v(0., 10.), &[v(0., 0.), v(0., 4.), v(8., 4.), v(8., 7.), v(0., 7.), v(0., 10.)]), 0.25);
    }
    #[test]
    fn attachment_finds_fold_junction_without_joining_an_unrelated_crossing() {
        let parent = [v(0., 0.), v(0., 10.), v(10.4, 10.), v(10., 10.), v(10., 0.)];
        assert_eq!(attachment(&[v(10.4, 10.), v(20., 10.)], &parent), Some(v(10., 10.)));
        let ordinary = [v(0., 0.), v(0., 10.), v(10., 10.), v(10., 0.)];
        assert_eq!(attachment(&[v(5., 10.), v(5., 20.)], &ordinary), Some(v(5., 10.)));
    }
}
