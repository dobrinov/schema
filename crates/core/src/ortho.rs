//! Global orthogonal routing.
//!
//! Edges run out of a table sideways, along a *lane* in the gap between two
//! columns of tables, and sideways into the target. Lanes are assigned per
//! gap so that parallel edges never overlap and pairs that don't have to
//! cross don't; where a crossing is unavoidable the horizontal line hops
//! over the vertical one with a small arc, so crossings can't be mistaken
//! for junctions.
use std::collections::HashMap;

use crate::config::{Direction, EdgeConfig};
use crate::graph::Graph;
use crate::layout::{Layout, Pt, Rect};
use crate::route::{ends, loop_points, midpoint, p, simplify, Routed, Side, RADIUS, STUB};

const LANE_GAP: f64 = 9.0;
const JUMP_R: f64 = 4.0;
const MARGIN: f64 = 6.0;

/// One vertical (or, in vertical mode, horizontal) run through a channel.
struct LaneSeg {
    edge: usize,
    /// index of the stop pair along the edge's polyline
    pair: usize,
    /// coordinate across the channel where the run enters / leaves
    entry: f64,
    exit: f64,
    /// allowed range along the channel axis for this particular run
    lo: f64,
    hi: f64,
}

/// A gap between two columns (or rows) of tables along axis `p`.
#[derive(Clone, Copy)]
struct Gap {
    lo: f64,
    hi: f64,
}

/// Merge the tables' extents along one axis into clusters and return the
/// gaps between consecutive clusters.
fn gaps(rects: &[Rect], swap: bool) -> Vec<Gap> {
    let mut spans: Vec<(f64, f64)> = rects.iter().map(|r| if swap { (r.y, r.bottom()) } else { (r.x, r.right()) }).collect();
    spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for s in spans {
        match merged.last_mut() {
            Some(m) if s.0 <= m.1 => m.1 = m.1.max(s.1),
            _ => merged.push(s),
        }
    }
    merged.windows(2).map(|w| Gap { lo: w[0].1, hi: w[1].0 }).collect()
}

fn swap_pt(pt: Pt, swap: bool) -> Pt {
    if swap {
        (pt.1, pt.0)
    } else {
        pt
    }
}

/// Assign lane indices to runs sharing a channel. Runs are ordered so that
/// non-inverted pairs nest instead of crossing; overlapping runs get
/// distinct lanes, disjoint ones may share.
fn assign_lanes(segs: &mut [LaneSeg]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..segs.len()).collect();
    let key = |s: &LaneSeg| -> (f64, f64) {
        let down = s.exit > s.entry;
        (if down { -s.entry } else { s.entry }, -(s.exit - s.entry).abs())
    };
    order.sort_by(|&a, &b| key(&segs[a]).partial_cmp(&key(&segs[b])).unwrap());
    let mut lane = vec![0usize; segs.len()];
    let mut placed: Vec<usize> = Vec::new();
    for &i in &order {
        let (a0, a1) = (segs[i].entry.min(segs[i].exit), segs[i].entry.max(segs[i].exit));
        let mut l = 0;
        for &j in &placed {
            let (b0, b1) = (segs[j].entry.min(segs[j].exit), segs[j].entry.max(segs[j].exit));
            if a0 < b1 + 1.0 && b0 < a1 + 1.0 {
                l = l.max(lane[j] + 1);
            }
        }
        lane[i] = l;
        placed.push(i);
    }
    lane
}

fn lane_positions(n: usize, lo: f64, hi: f64) -> Vec<f64> {
    let width = (hi - lo - MARGIN * 2.0).max(0.0);
    let spacing = if n > 1 { LANE_GAP.min(width / (n - 1) as f64) } else { 0.0 };
    let center = (lo + hi) / 2.0;
    (0..n).map(|i| center + (i as f64 - (n as f64 - 1.0) / 2.0) * spacing).collect()
}

pub fn route_all(g: &Graph, l: &Layout, cfg: &EdgeConfig, dir: Direction) -> Vec<Routed> {
    let idx = g.node_index();
    let n = g.edges.len();
    // stops (anchors + waypoints) and the side used at each end
    struct Plan {
        stops: Vec<Pt>,
        ss: Side,
        ts: Side,
        node_from: usize,
        node_to: usize,
        loop_ys: Option<(f64, f64)>,
    }
    let plans: Vec<Plan> = g
        .edges
        .iter()
        .enumerate()
        .map(|(ei, e)| {
            let (a, b) = (idx[e.from.as_str()], idx[e.to.as_str()]);
            let wps = l.waypoints.get(ei).map(|v| v.as_slice()).unwrap_or(&[]);
            let en = ends(e, &g.nodes[a], &l.nodes[a], &g.nodes[b], &l.nodes[b], wps, cfg, dir);
            let mut stops = vec![en.s];
            stops.extend_from_slice(wps);
            stops.push(en.t);
            Plan { stops, ss: en.ss, ts: en.ts, node_from: a, node_to: b, loop_ys: if e.from == e.to { Some((en.fy, en.ty)) } else { None } }
        })
        .collect();

    let gaps_x = gaps(&l.nodes, false);
    let gaps_y = gaps(&l.nodes, true);
    let mut polylines: Vec<Vec<Pt>> = vec![Vec::new(); n];

    // ---- runs through channels between columns --------------------------
    // channel key: (swap, gap index or -1 for ad-hoc, rounded lo, rounded hi)
    let mut channels: HashMap<(bool, i64, i64, i64), Vec<LaneSeg>> = HashMap::new();
    let mut channel_range: HashMap<(bool, i64, i64, i64), (f64, f64)> = HashMap::new();
    // same-side runs (both ends on the right, or both on the left) and loops
    let mut side_groups: HashMap<(Side, i64), Vec<LaneSeg>> = HashMap::new();
    let column_of = |r: &Rect, side: Side| -> i64 {
        // cluster id: index of the first gap past the table's far edge
        let swap = !side.horizontal();
        let far = match side {
            Side::Right => r.right(),
            Side::Left => r.x,
            Side::Bottom => r.bottom(),
            Side::Top => r.y,
        };
        let gs = if swap { &gaps_y } else { &gaps_x };
        gs.iter().position(|gp| gp.lo >= far - 0.5).map(|i| i as i64).unwrap_or(gs.len() as i64)
    };

    for (ei, pl) in plans.iter().enumerate() {
        if let Some((fy, ty)) = pl.loop_ys {
            let r = &l.nodes[pl.node_from];
            side_groups.entry((Side::Right, column_of(r, Side::Right))).or_default().push(LaneSeg {
                edge: ei,
                pair: 0,
                entry: fy.min(ty),
                exit: fy.max(ty),
                lo: r.right(),
                hi: r.right(),
            });
            continue;
        }
        if pl.ss.horizontal() != pl.ts.horizontal() {
            continue; // mixed sides: routed per edge below
        }
        if pl.ss == pl.ts {
            let swap = !pl.ss.horizontal();
            let (s, t) = (swap_pt(pl.stops[0], swap), swap_pt(pl.stops[pl.stops.len() - 1], swap));
            let r = &l.nodes[pl.node_from];
            let outer = match pl.ss {
                Side::Right => l.nodes[pl.node_from].right().max(l.nodes[pl.node_to].right()),
                Side::Left => l.nodes[pl.node_from].x.min(l.nodes[pl.node_to].x),
                Side::Bottom => l.nodes[pl.node_from].bottom().max(l.nodes[pl.node_to].bottom()),
                Side::Top => l.nodes[pl.node_from].y.min(l.nodes[pl.node_to].y),
            };
            side_groups.entry((pl.ss, column_of(r, pl.ss))).or_default().push(LaneSeg { edge: ei, pair: 0, entry: s.1, exit: t.1, lo: outer, hi: outer });
            continue;
        }
        let swap = !pl.ss.horizontal();
        let gs = if swap { &gaps_y } else { &gaps_x };
        let stops: Vec<Pt> = pl.stops.iter().map(|&q| swap_pt(q, swap)).collect();
        for k in 0..stops.len() - 1 {
            let (a, b) = (stops[k], stops[k + 1]);
            let stub_a = if k == 0 { STUB } else { 0.0 };
            let stub_b = if k == stops.len() - 2 { STUB } else { 0.0 };
            let (lo, hi, entry, exit) = if a.0 <= b.0 { (a.0 + stub_a, b.0 - stub_b, a.1, b.1) } else { (b.0 + stub_b, a.0 - stub_a, b.1, a.1) };
            let (lo, hi) = if lo < hi { (lo, hi) } else { ((lo + hi) / 2.0 - 1.0, (lo + hi) / 2.0 + 1.0) };
            // the gap this run passes through: the one overlapping its corridor most
            let mut best: Option<(usize, f64)> = None;
            for (gi, gp) in gs.iter().enumerate() {
                let ov = gp.hi.min(hi) - gp.lo.max(lo);
                if ov > 0.0 && best.map_or(true, |(_, bo)| ov > bo) {
                    best = Some((gi, ov));
                }
            }
            let (key, range) = match best {
                Some((gi, _)) => ((swap, gi as i64, 0, 0), (gs[gi].lo.max(lo - STUB), gs[gi].hi.min(hi + STUB))),
                None => ((swap, -1, lo.round() as i64, hi.round() as i64), (lo, hi)),
            };
            channel_range.entry(key).and_modify(|r| { r.0 = r.0.min(range.0); r.1 = r.1.max(range.1); }).or_insert(range);
            channels.entry(key).or_default().push(LaneSeg { edge: ei, pair: k, entry, exit, lo, hi });
        }
    }

    // lane coordinate per (edge, pair)
    let mut lane_at: HashMap<(usize, usize), f64> = HashMap::new();
    for (key, segs) in channels.iter_mut() {
        let lanes = assign_lanes(segs);
        let count = lanes.iter().max().map_or(0, |m| m + 1);
        let (lo, hi) = channel_range[key];
        let pos = lane_positions(count, lo, hi);
        for (s, &li) in segs.iter().zip(&lanes) {
            lane_at.insert((s.edge, s.pair), pos[li].clamp(s.lo, s.hi));
        }
    }
    // same-side groups: shorter spans nearer the tables
    let mut side_lane: HashMap<usize, f64> = HashMap::new();
    for ((side, _), segs) in side_groups.iter_mut() {
        segs.sort_by(|a, b| (a.exit - a.entry).abs().partial_cmp(&(b.exit - b.entry).abs()).unwrap());
        let mut lanes = vec![0usize; segs.len()];
        for i in 0..segs.len() {
            let (a0, a1) = (segs[i].entry.min(segs[i].exit), segs[i].entry.max(segs[i].exit));
            let mut lv = 0;
            for j in 0..i {
                let (b0, b1) = (segs[j].entry.min(segs[j].exit), segs[j].entry.max(segs[j].exit));
                if a0 < b1 + 1.0 && b0 < a1 + 1.0 {
                    lv = lv.max(lanes[j] + 1);
                }
            }
            lanes[i] = lv;
        }
        let outward = matches!(side, Side::Right | Side::Bottom);
        for (s, &li) in segs.iter().zip(&lanes) {
            let d = STUB + 14.0 + li as f64 * LANE_GAP;
            side_lane.insert(s.edge, if outward { s.lo + d } else { s.lo - d });
        }
    }

    // ---- polylines --------------------------------------------------------
    for (ei, pl) in plans.iter().enumerate() {
        if let Some((fy, ty)) = pl.loop_ys {
            let r = &l.nodes[pl.node_from];
            let bulge = side_lane.get(&ei).map_or(r.right() + STUB + 20.0, |x| *x) - r.right();
            polylines[ei] = simplify(&loop_points(r, fy, ty, bulge));
            continue;
        }
        let (s, t) = (pl.stops[0], pl.stops[pl.stops.len() - 1]);
        if pl.ss.horizontal() != pl.ts.horizontal() {
            // mixed sides: stub out of each, meet at the corner
            let (ns, nt) = (pl.ss.normal(), pl.ts.normal());
            let s1 = (s.0 + ns.0 * STUB, s.1 + ns.1 * STUB);
            let t1 = (t.0 + nt.0 * STUB, t.1 + nt.1 * STUB);
            let corner = if pl.ss.horizontal() { (t1.0, s1.1) } else { (s1.0, t1.1) };
            polylines[ei] = simplify(&[s, s1, corner, t1, t]);
            continue;
        }
        let swap = !pl.ss.horizontal();
        if pl.ss == pl.ts {
            let lane = side_lane[&ei];
            let (sp, tp) = (swap_pt(s, swap), swap_pt(t, swap));
            let pts = [sp, (lane, sp.1), (lane, tp.1), tp];
            polylines[ei] = simplify(&pts.iter().map(|&q| swap_pt(q, swap)).collect::<Vec<_>>());
            continue;
        }
        let stops: Vec<Pt> = pl.stops.iter().map(|&q| swap_pt(q, swap)).collect();
        let mut pts: Vec<Pt> = vec![stops[0]];
        for k in 0..stops.len() - 1 {
            let (a, b) = (stops[k], stops[k + 1]);
            let lane = lane_at.get(&(ei, k)).copied().unwrap_or((a.0 + b.0) / 2.0);
            pts.push((lane, a.1));
            pts.push((lane, b.1));
            pts.push(b);
        }
        polylines[ei] = simplify(&pts.iter().map(|&q| swap_pt(q, swap)).collect::<Vec<_>>());
    }

    // ---- crossings: horizontal segments hop over vertical ones -----------
    let mut verticals: Vec<(usize, f64, f64, f64)> = Vec::new(); // (edge, x, y0, y1)
    for (ei, pts) in polylines.iter().enumerate() {
        for w in pts.windows(2) {
            if (w[0].0 - w[1].0).abs() < 0.01 {
                verticals.push((ei, w[0].0, w[0].1.min(w[1].1), w[0].1.max(w[1].1)));
            }
        }
    }
    verticals.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
    let vxs: Vec<f64> = verticals.iter().map(|v| v.1).collect();
    let mut out = Vec::with_capacity(n);
    for (ei, pts) in polylines.iter().enumerate() {
        let mut jumps: Vec<Vec<f64>> = vec![Vec::new(); pts.len().saturating_sub(1)];
        for (k, w) in pts.windows(2).enumerate() {
            if (w[0].1 - w[1].1).abs() >= 0.01 {
                continue;
            }
            let y = w[0].1;
            let (x0, x1) = (w[0].0.min(w[1].0) + 2.0, w[0].0.max(w[1].0) - 2.0);
            let start = vxs.partition_point(|&x| x < x0);
            for v in &verticals[start..] {
                if v.1 > x1 {
                    break;
                }
                if v.0 != ei && y > v.2 + 2.0 && y < v.3 - 2.0 {
                    jumps[k].push(v.1);
                }
            }
        }
        out.push(Routed { d: build_path(pts, &jumps), label: midpoint(pts) });
    }
    out
}

/// SVG path with rounded corners and jump arcs on horizontal segments.
fn build_path(pts: &[Pt], jumps: &[Vec<f64>]) -> String {
    if pts.len() < 2 {
        return pts.first().map(|q| format!("M{}", p(*q))).unwrap_or_default();
    }
    let len = |a: Pt, b: Pt| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let radius_at = |i: usize| -> f64 {
        if i == 0 || i + 1 >= pts.len() {
            0.0
        } else {
            RADIUS.min(len(pts[i - 1], pts[i]) / 2.0).min(len(pts[i], pts[i + 1]) / 2.0)
        }
    };
    let toward = |a: Pt, b: Pt, d: f64| -> Pt {
        let l = len(a, b);
        if l < 0.01 {
            a
        } else {
            (a.0 + (b.0 - a.0) / l * d, a.1 + (b.1 - a.1) / l * d)
        }
    };
    let mut d = String::new();
    for i in 0..pts.len() - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        let start = toward(a, b, radius_at(i));
        let end = toward(b, a, radius_at(i + 1));
        if i == 0 {
            d.push_str(&format!("M{}", p(start)));
        }
        let horizontal = (a.1 - b.1).abs() < 0.01;
        if horizontal && !jumps[i].is_empty() {
            let dir = if b.0 > a.0 { 1.0 } else { -1.0 };
            let mut xs: Vec<f64> = jumps[i].iter().copied().filter(|&x| (x - start.0) * dir > JUMP_R + 1.0 && (end.0 - x) * dir > JUMP_R + 1.0).collect();
            xs.sort_by(|u, v| ((u - start.0) * dir).partial_cmp(&((v - start.0) * dir)).unwrap());
            let mut last = start.0;
            for x in xs {
                if (x - last) * dir < JUMP_R * 2.0 + 0.5 {
                    continue; // too close to the previous hop: merge visually
                }
                d.push_str(&format!(" L{}", p((x - dir * JUMP_R, a.1))));
                d.push_str(&format!(" A{r} {r} 0 0 {sweep} {}", p((x + dir * JUMP_R, a.1)), r = JUMP_R, sweep = if dir > 0.0 { 1 } else { 0 }));
                last = x + dir * JUMP_R;
            }
        }
        d.push_str(&format!(" L{}", p(end)));
        if i + 2 < pts.len() && radius_at(i + 1) > 0.4 {
            let next_start = toward(b, pts[i + 2], radius_at(i + 1));
            d.push_str(&format!(" Q{} {}", p(b), p(next_start)));
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lanes_nest_non_inverted_pairs() {
        // two downward runs from sources at y=100 and y=200 to targets at 500 and 600:
        // the lower source (200) must take the lane nearer the source column (index 0)
        let mut segs = vec![
            LaneSeg { edge: 0, pair: 0, entry: 100.0, exit: 500.0, lo: 0.0, hi: 100.0 },
            LaneSeg { edge: 1, pair: 0, entry: 200.0, exit: 600.0, lo: 0.0, hi: 100.0 },
            LaneSeg { edge: 2, pair: 0, entry: 900.0, exit: 950.0, lo: 0.0, hi: 100.0 },
        ];
        let lanes = assign_lanes(&mut segs);
        assert_eq!(lanes, vec![1, 0, 0]);
    }

    #[test]
    fn path_has_rounded_corners_and_jumps() {
        let pts = [(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)];
        let d = build_path(&pts, &[vec![50.0], vec![]]);
        assert!(d.contains(" A4 4 0 0 1 54.0,0.0"), "{d}");
        assert!(d.contains(" Q100.0,0.0 100.0,8.0"), "{d}");
        assert!(d.starts_with("M0.0,0.0"));
    }
}
