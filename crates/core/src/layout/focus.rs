//! Focus layout: the focused tables in the middle, their neighbourhood in
//! columns on either side (tables they reference on one side, tables that
//! reference them on the other, further hops further out).
//!
//! Crossings are kept low by
//! - ordering each column by the row its relation attaches to in the column
//!   next to it, so edges fan out in the same order as their anchor rows,
//! - putting a neighbour on the side where most of its other relations go,
//!   so few edges have to cross over the focused tables, and
//! - pulling every table towards its anchor row (isotonic regression keeps
//!   the order and spacing), which keeps edges short and close to straight.
use std::collections::{HashMap, VecDeque};

use super::{Block, BlockResult};
use crate::config::{Direction, LayoutConfig};

const LEFT: i8 = -1;
const RIGHT: i8 = 1;

pub(crate) fn layout(b: &Block, cfg: &LayoutConfig) -> BlockResult {
    let n = b.sizes.len();
    let horizontal = cfg.direction.horizontal();
    // work in a virtual frame: columns along x ("layer" axis), stacking along y
    let lext: Vec<f64> = b.sizes.iter().map(|s| if horizontal { s.0 } else { s.1 }).collect();
    let oext: Vec<f64> = b.sizes.iter().map(|s| if horizontal { s.1 } else { s.0 }).collect();
    // anchor offset along the stacking axis for (parent, child) of each edge
    let anchors: Vec<(f64, f64)> = b
        .edges
        .iter()
        .enumerate()
        .map(|(k, &(p, c))| if horizontal { b.anchors[k] } else { (oext[p] / 2.0, oext[c] / 2.0) })
        .collect();

    // neighbours with the anchor offsets on both ends: (other, my_offset, other_offset, i_am_parent)
    let mut adj: Vec<Vec<(usize, f64, f64, bool)>> = vec![Vec::new(); n];
    for (k, &(p, c)) in b.edges.iter().enumerate() {
        if p == c {
            continue;
        }
        let (po, co) = anchors[k];
        adj[p].push((c, po, co, true));
        adj[c].push((p, co, po, false));
    }

    // ---- distance from the focused tables ------------------------------
    let mut dist = vec![usize::MAX; n];
    let mut q = VecDeque::new();
    for &r in &b.roots {
        dist[r] = 0;
        q.push_back(r);
    }
    while let Some(v) = q.pop_front() {
        for &(w, ..) in &adj[v] {
            if dist[w] == usize::MAX {
                dist[w] = dist[v] + 1;
                q.push_back(w);
            }
        }
    }
    let max_d = dist.iter().filter(|&&d| d != usize::MAX).max().copied().unwrap_or(0);

    // ---- sides ------------------------------------------------------------
    // tables the focus references (parents) start on the left, the rest right
    let mut side = vec![0i8; n];
    for v in 0..n {
        if dist[v] == 1 {
            let is_parent_of_root = adj[v].iter().any(|&(w, _, _, i_am_parent)| dist[w] == 0 && i_am_parent);
            side[v] = if is_parent_of_root { LEFT } else { RIGHT };
        }
    }
    let derive_outer = |side: &mut Vec<i8>| {
        for d in 2..=max_d {
            for v in 0..n {
                if dist[v] != d {
                    continue;
                }
                let votes: i32 = adj[v].iter().filter(|&&(w, ..)| dist[w] == d - 1).map(|&(w, ..)| side[w] as i32).sum();
                let as_child = adj[v].iter().any(|&(w, _, _, i_am_parent)| dist[w] == d - 1 && !i_am_parent);
                side[v] = if votes > 0 || (votes == 0 && as_child) { RIGHT } else { LEFT };
            }
        }
    };
    derive_outer(&mut side);
    // flip first-hop tables when that removes edges crossing the focus,
    // without letting one side grow much taller than the other
    let height = |side: &Vec<i8>, s: i8| -> f64 { (0..n).filter(|&v| dist[v] == 1 && side[v] == s).map(|v| oext[v] + cfg.node_spacing).sum() };
    let crossing = |side: &Vec<i8>| -> usize {
        b.edges
            .iter()
            .filter(|&&(p, c)| p != c && dist[p] != 0 && dist[c] != 0 && dist[p] != usize::MAX && dist[c] != usize::MAX && side[p] != side[c])
            .count()
    };
    let total_h = height(&side, LEFT) + height(&side, RIGHT);
    // uneven sides only matter once one of them gets clearly taller than the focus
    let slack = b.roots.iter().map(|&r| oext[r] + cfg.node_spacing).sum::<f64>().max(420.0);
    let cost = |side: &Vec<i8>| -> f64 {
        let excess = ((height(side, LEFT) - height(side, RIGHT)).abs() - slack).max(0.0) / total_h.max(1.0);
        crossing(side) as f64 + 6.0 * excess.powi(2) * (1.0 + b.edges.len() as f64 / 4.0)
    };
    let mut best = cost(&side);
    for _ in 0..6 {
        let mut improved = false;
        for v in 0..n {
            if dist[v] != 1 {
                continue;
            }
            let mut trial = side.clone();
            trial[v] = -trial[v];
            derive_outer(&mut trial);
            let c = cost(&trial);
            if c + 1e-9 < best {
                best = c;
                side = trial;
                improved = true;
            }
        }
        if !improved {
            break;
        }
    }

    // ---- columns ------------------------------------------------------------
    // top y per node (virtual frame), computed column by column outwards
    let mut top = vec![0.0f64; n];
    let mut placed = vec![false; n];
    let gap = cfg.node_spacing;
    let roots: Vec<usize> = {
        let mut r = b.roots.clone();
        r.sort_by(|&a, &c| b.names[a].cmp(b.names[c]));
        r
    };
    let root_stack: f64 = roots.iter().map(|&r| oext[r]).sum::<f64>() + gap * roots.len().saturating_sub(1) as f64;
    let mut y = -root_stack / 2.0;
    for &r in &roots {
        top[r] = y;
        placed[r] = true;
        y += oext[r] + gap;
    }
    let root_w = roots.iter().map(|&r| lext[r]).fold(0.0, f64::max);
    // a column taller than this wraps into several sub-columns
    let area: f64 = (0..n).map(|v| (lext[v] + gap) * (oext[v] + gap)).sum();
    let max_col_h = (root_stack * 2.5).max(area.sqrt() * 1.5).max(900.0);

    // x of each node (virtual frame, left edge)
    let mut left = vec![0.0f64; n];
    for &r in &roots {
        left[r] = (root_w - lext[r]) / 2.0;
    }

    let mut columns: Vec<Vec<usize>> = Vec::new();
    for s in [LEFT, RIGHT] {
        let mut edge_x = if s == RIGHT { root_w } else { 0.0 };
        for d in 1..=max_d {
            let mut col: Vec<usize> = (0..n).filter(|&v| dist[v] == d && side[v] == s).collect();
            if col.is_empty() {
                continue;
            }
            // desired top: line the node's anchor row up with its partner's
            let desired = |v: usize, top: &[f64], placed: &[bool], weight_other: bool| -> (f64, f64) {
                let mut sum = 0.0;
                let mut cnt = 0.0;
                for &(w, mine, theirs, _) in &adj[v] {
                    if placed[w] && dist[w] + 1 == d {
                        sum += top[w] + theirs - mine;
                        cnt += 1.0;
                    }
                }
                if cnt == 0.0 {
                    return (0.0, 0.0);
                }
                let primary = sum / cnt;
                // tie-break equal anchors (e.g. all children of `id`) by the other relations
                let mut o = 0.0;
                let mut oc = 0.0;
                if weight_other {
                    for &(w, ..) in &adj[v] {
                        if placed[w] && dist[w] + 1 != d {
                            o += top[w] + oext[w] / 2.0;
                            oc += 1.0;
                        }
                    }
                }
                let tb = if oc > 0.0 { o / oc } else { 0.0 };
                (primary, tb)
            };
            let order_and_place = |col: &mut Vec<usize>, top: &mut Vec<f64>, placed: &Vec<bool>, weight_other: bool| {
                let keys: HashMap<usize, (f64, f64)> = col.iter().map(|&v| (v, desired(v, top, placed, weight_other))).collect();
                col.sort_by(|&a, &c| {
                    let (ka, ta) = keys[&a];
                    let (kc, tc) = keys[&c];
                    ka.partial_cmp(&kc).unwrap().then(ta.partial_cmp(&tc).unwrap()).then(b.names[a].cmp(b.names[c]))
                });
                keys
            };
            order_and_place(&mut col, &mut top, &placed, false);
            // wrap long columns: consecutive chunks become sub-columns
            let mut chunk_lists = split(&col, &oext, gap, max_col_h);
            for chunk in chunk_lists.iter_mut() {
                let ks: Vec<f64> = chunk.iter().map(|&v| desired(v, &top, &placed, false).0).collect();
                place(chunk, &ks, &oext, gap, &mut top);
            }
            for chunk in &chunk_lists {
                let w = chunk.iter().map(|&v| lext[v]).fold(0.0, f64::max);
                let spacing = cfg.rank_spacing + (chunk.len() as f64).sqrt() * 8.0;
                let (start, end) = if s == RIGHT { (edge_x + spacing, edge_x + spacing + w) } else { (edge_x - spacing - w, edge_x - spacing) };
                for &v in chunk {
                    // align tables to the side facing the focus
                    left[v] = if s == RIGHT { start } else { end - lext[v] };
                    placed[v] = true;
                }
                edge_x = if s == RIGHT { end } else { start };
            }
            // second pass: tie-break by relations to already placed tables
            for chunk in chunk_lists.iter_mut() {
                let mut c = chunk.clone();
                order_and_place(&mut c, &mut top, &placed, true);
                let ks: Vec<f64> = c.iter().map(|&v| desired(v, &top, &placed, true).0).collect();
                place(&c, &ks, &oext, gap, &mut top);
                *chunk = c;
            }
            columns.extend(chunk_lists);
        }
    }

    // ---- local refinement: swap neighbours when it removes crossings --------
    let segs = |top: &[f64]| -> Vec<((f64, f64), (f64, f64), usize, usize)> {
        b.edges
            .iter()
            .enumerate()
            .filter(|&(_, &(p, c))| p != c && placed[p] && placed[c])
            .map(|(k, &(p, c))| {
                let (po, co) = anchors[k];
                let (xp, xc) = if left[p] < left[c] { (left[p] + lext[p], left[c]) } else { (left[p], left[c] + lext[c]) };
                ((xp, top[p] + po), (xc, top[c] + co), p, c)
            })
            .collect()
    };
    let count = |top: &[f64]| -> usize {
        let sg = segs(top);
        let orient = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        let mut n = 0;
        for i in 0..sg.len() {
            for j in i + 1..sg.len() {
                let (a, bb) = (sg[i], sg[j]);
                if a.2 == bb.2 || a.2 == bb.3 || a.3 == bb.2 || a.3 == bb.3 {
                    continue;
                }
                let (d1, d2) = (orient(bb.0, bb.1, a.0), orient(bb.0, bb.1, a.1));
                let (d3, d4) = (orient(a.0, a.1, bb.0), orient(a.0, a.1, bb.1));
                if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
                    n += 1;
                }
            }
        }
        n
    };
    // quadratic in the number of edges: only for neighbourhoods of readable size
    if b.edges.len() <= 150 {
        let desired_tops = |col: &[usize], top: &[f64]| -> Vec<f64> {
            col.iter()
                .map(|&v| {
                    let mut sum = 0.0;
                    let mut cnt = 0.0;
                    for &(w, mine, theirs, _) in &adj[v] {
                        if placed[w] && dist[w] + 1 == dist[v] {
                            sum += top[w] + theirs - mine;
                            cnt += 1.0;
                        }
                    }
                    if cnt > 0.0 { sum / cnt } else { top[v] }
                })
                .collect()
        };
        let mut best = count(&top);
        for _ in 0..4 {
            if best == 0 {
                break;
            }
            let mut improved = false;
            for ci in 0..columns.len() {
                for i in 0..columns[ci].len().saturating_sub(1) {
                    let saved = top.clone();
                    columns[ci].swap(i, i + 1);
                    let ks = desired_tops(&columns[ci], &top);
                    place(&columns[ci], &ks, &oext, gap, &mut top);
                    let c = count(&top);
                    if c < best {
                        best = c;
                        improved = true;
                    } else {
                        columns[ci].swap(i, i + 1);
                        top = saved;
                    }
                }
            }
            if !improved {
                break;
            }
        }
    }

    // anything not connected to the focus (rare) goes underneath in a row
    let bottom = (0..n).filter(|&v| placed[v]).map(|v| top[v] + oext[v]).fold(0.0, f64::max);
    let mut x = 0.0;
    for v in 0..n {
        if !placed[v] {
            left[v] = x;
            top[v] = bottom + cfg.rank_spacing;
            x += lext[v] + gap;
        }
    }

    // ---- back to screen coordinates -----------------------------------
    let min_l = (0..n).map(|v| left[v]).fold(f64::MAX, f64::min);
    let max_r = (0..n).map(|v| left[v] + lext[v]).fold(f64::MIN, f64::max);
    let mirror = matches!(cfg.direction, Direction::RL | Direction::BT);
    let pos = (0..n)
        .map(|v| {
            let l = if mirror { max_r - (left[v] - min_l) - lext[v] } else { left[v] - min_l };
            if horizontal {
                (l, top[v])
            } else {
                (top[v], l)
            }
        })
        .collect();
    BlockResult { pos, waypoints: HashMap::new() }
}

/// Split an ordered column into consecutive chunks no taller than `max_h`.
fn split(col: &[usize], ext: &[f64], gap: f64, max_h: f64) -> Vec<Vec<usize>> {
    let total: f64 = col.iter().map(|&v| ext[v] + gap).sum();
    // wrapping only helps long lists of tables; a single huge table just makes a tall column
    let parts = ((total / max_h).ceil() as usize).max(1).min(col.len() / 2);
    if col.len() < 4 || parts <= 1 {
        return vec![col.to_vec()];
    }
    let target = total / parts as f64;
    let mut out = vec![Vec::new()];
    let mut h = 0.0;
    for &v in col {
        if h > 0.0 && h + (ext[v] + gap) / 2.0 > target && out.len() < parts {
            out.push(Vec::new());
            h = 0.0;
        }
        out.last_mut().unwrap().push(v);
        h += ext[v] + gap;
    }
    out
}

/// Place an ordered column as close as possible to the desired tops while
/// keeping order and separation (pool-adjacent-violators).
fn place(col: &[usize], desired: &[f64], ext: &[f64], gap: f64, top: &mut [f64]) {
    if col.is_empty() {
        return;
    }
    let mut off = vec![0.0f64; col.len()];
    for i in 1..col.len() {
        off[i] = off[i - 1] + ext[col[i - 1]] + gap;
    }
    struct Blk {
        sum: f64,
        n: usize,
    }
    let mut stack: Vec<Blk> = Vec::new();
    for i in 0..col.len() {
        stack.push(Blk { sum: desired[i] - off[i], n: 1 });
        while stack.len() >= 2 {
            let k = stack.len();
            if stack[k - 2].sum / stack[k - 2].n as f64 <= stack[k - 1].sum / stack[k - 1].n as f64 {
                break;
            }
            let t = stack.pop().unwrap();
            let p = stack.last_mut().unwrap();
            p.sum += t.sum;
            p.n += t.n;
        }
    }
    let mut i = 0;
    for blk in stack {
        let z = blk.sum / blk.n as f64;
        for _ in 0..blk.n {
            top[col[i]] = z + off[i];
            i += 1;
        }
    }
}
