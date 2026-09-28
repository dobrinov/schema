//! Grid, circular and radial layouts.
use std::collections::{HashMap, VecDeque};
use std::f64::consts::TAU;

use super::force::remove_overlaps;
use super::{shelf_pack, Block, BlockResult, Pt};
use crate::config::{GridSort, LayoutConfig};

fn diag(s: Pt) -> f64 {
    (s.0 * s.0 + s.1 * s.1).sqrt()
}

fn adjacency(b: &Block) -> Vec<Vec<usize>> {
    let mut adj = vec![Vec::new(); b.sizes.len()];
    for &(u, v) in &b.edges {
        if u != v {
            adj[u].push(v);
            adj[v].push(u);
        }
    }
    for a in adj.iter_mut() {
        a.sort_unstable();
        a.dedup();
    }
    adj
}

pub(crate) fn grid(b: &Block, cfg: &LayoutConfig, by_name: bool) -> BlockResult {
    let n = b.sizes.len();
    let mut order: Vec<usize> = (0..n).collect();
    let sort = if by_name { GridSort::Name } else { cfg.grid_sort };
    match sort {
        GridSort::Name => order.sort_by(|&a, &c| b.names[a].cmp(b.names[c])),
        GridSort::Degree => order.sort_by(|&a, &c| b.degree_rank[c].partial_cmp(&b.degree_rank[a]).unwrap().then(b.names[a].cmp(b.names[c]))),
        GridSort::Size => order.sort_by(|&a, &c| b.sizes[c].1.partial_cmp(&b.sizes[a].1).unwrap().then(b.names[a].cmp(b.names[c]))),
    }
    let sizes: Vec<Pt> = order.iter().map(|&i| b.sizes[i]).collect();
    let packed = shelf_pack(&sizes, cfg.node_spacing, None);
    let mut pos = vec![(0.0, 0.0); n];
    for (k, &i) in order.iter().enumerate() {
        pos[i] = packed[k];
    }
    BlockResult { pos, waypoints: HashMap::new() }
}

/// Order nodes so neighbours tend to be adjacent on the circle.
fn traversal_order(b: &Block, adj: &[Vec<usize>]) -> Vec<usize> {
    let n = b.sizes.len();
    let mut seen = vec![false; n];
    let mut order = Vec::with_capacity(n);
    let mut starts: Vec<usize> = (0..n).collect();
    starts.sort_by(|&a, &c| b.degree_rank[c].partial_cmp(&b.degree_rank[a]).unwrap().then(b.names[a].cmp(b.names[c])));
    for s in starts {
        if seen[s] {
            continue;
        }
        let mut stack = vec![s];
        while let Some(v) = stack.pop() {
            if seen[v] {
                continue;
            }
            seen[v] = true;
            order.push(v);
            let mut next: Vec<usize> = adj[v].iter().copied().filter(|&w| !seen[w]).collect();
            next.sort_by(|&a, &c| b.degree_rank[a].partial_cmp(&b.degree_rank[c]).unwrap());
            stack.extend(next);
        }
    }
    order
}

pub(crate) fn circular(b: &Block, cfg: &LayoutConfig) -> BlockResult {
    let n = b.sizes.len();
    if n == 1 {
        return BlockResult { pos: vec![(0.0, 0.0)], waypoints: HashMap::new() };
    }
    let adj = adjacency(b);
    let order = traversal_order(b, &adj);
    let arcs: Vec<f64> = order.iter().map(|&i| diag(b.sizes[i]) * 0.75 + cfg.node_spacing).collect();
    let circ: f64 = arcs.iter().sum();
    let radius = (circ / TAU).max(120.0);
    let mut pos = vec![(0.0, 0.0); n];
    let mut acc = 0.0;
    for (k, &i) in order.iter().enumerate() {
        let a = (acc + arcs[k] / 2.0) / circ * TAU - TAU / 4.0;
        acc += arcs[k];
        let (w, h) = b.sizes[i];
        pos[i] = (radius * a.cos() - w / 2.0, radius * a.sin() - h / 2.0);
    }
    remove_overlaps(&mut pos, &b.sizes, cfg.node_spacing * 0.5);
    BlockResult { pos, waypoints: HashMap::new() }
}

pub(crate) fn radial(b: &Block, cfg: &LayoutConfig) -> BlockResult {
    let n = b.sizes.len();
    let adj = adjacency(b);
    let mut roots = b.roots.clone();
    if roots.is_empty() {
        let best = (0..n).max_by(|&a, &c| b.degree_rank[a].partial_cmp(&b.degree_rank[c]).unwrap().then(b.names[c].cmp(b.names[a]))).unwrap();
        roots.push(best);
    }
    let mut level = vec![usize::MAX; n];
    let mut parent = vec![usize::MAX; n];
    let mut q = VecDeque::new();
    for &r in &roots {
        level[r] = 0;
        q.push_back(r);
    }
    while let Some(v) = q.pop_front() {
        for &w in &adj[v] {
            if level[w] == usize::MAX {
                level[w] = level[v] + 1;
                parent[w] = v;
                q.push_back(w);
            }
        }
    }
    let max_level = level.iter().filter(|&&l| l != usize::MAX).max().copied().unwrap_or(0);
    for l in level.iter_mut() {
        if *l == usize::MAX {
            *l = max_level + 1;
        }
    }
    let rings = *level.iter().max().unwrap() + 1;
    let mut members: Vec<Vec<usize>> = vec![vec![]; rings];
    for v in 0..n {
        members[level[v]].push(v);
    }
    let mut angle = vec![0.0f64; n];
    let mut centre = vec![(0.0f64, 0.0f64); n];
    let mut prev_r = 0.0;
    let mut prev_diag = 0.0;
    for (k, ring) in members.iter_mut().enumerate() {
        if ring.is_empty() {
            continue;
        }
        let max_d = ring.iter().map(|&v| diag(b.sizes[v])).fold(0.0, f64::max);
        let arcs: f64 = ring.iter().map(|&v| diag(b.sizes[v]) * 0.8 + cfg.node_spacing).sum();
        let r = if k == 0 && ring.len() == 1 {
            0.0
        } else {
            let min_r = if k == 0 { 0.0 } else { prev_r + (prev_diag + max_d) / 2.0 + cfg.rank_spacing * 0.4 };
            min_r.max(arcs / TAU)
        };
        // order by parent angle so subtrees stay together
        if k > 0 {
            ring.sort_by(|&a, &c| {
                let pa = if parent[a] == usize::MAX { 0.0 } else { angle[parent[a]] };
                let pc = if parent[c] == usize::MAX { 0.0 } else { angle[parent[c]] };
                pa.partial_cmp(&pc).unwrap().then(b.names[a].cmp(b.names[c]))
            });
        }
        let total: f64 = ring.iter().map(|&v| diag(b.sizes[v]) * 0.8 + cfg.node_spacing).sum();
        let start = if k > 0 && parent[ring[0]] != usize::MAX { angle[parent[ring[0]]] } else { -TAU / 4.0 };
        let mut acc = 0.0;
        for &v in ring.iter() {
            let arc = diag(b.sizes[v]) * 0.8 + cfg.node_spacing;
            let a = start + (acc + arc / 2.0) / total * TAU;
            acc += arc;
            angle[v] = a;
            centre[v] = (r * a.cos(), r * a.sin());
        }
        prev_r = r;
        prev_diag = max_d;
    }
    let mut pos: Vec<Pt> = centre.iter().zip(&b.sizes).map(|(c, s)| (c.0 - s.0 / 2.0, c.1 - s.1 / 2.0)).collect();
    remove_overlaps(&mut pos, &b.sizes, cfg.node_spacing * 0.5);
    BlockResult { pos, waypoints: HashMap::new() }
}
