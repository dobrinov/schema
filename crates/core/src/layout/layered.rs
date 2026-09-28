//! Sugiyama-style layered layout:
//! cycle removal → layering (width bounded) → dummy nodes → barycentric
//! crossing reduction → coordinate assignment via isotonic regression.
use std::collections::HashMap;

use super::{Block, BlockResult, Pt};
use crate::config::{Direction, LayoutConfig};

const DUMMY_SIZE: f64 = 8.0;
const DUMMY_SEP: f64 = 14.0;

pub(crate) fn layout(b: &Block, cfg: &LayoutConfig) -> BlockResult {
    let n = b.sizes.len();
    if n == 0 {
        return BlockResult { pos: vec![], waypoints: HashMap::new() };
    }
    let horizontal = cfg.direction.horizontal();
    // extent along the layer axis and the ordering axis
    let lext: Vec<f64> = b.sizes.iter().map(|s| if horizontal { s.0 } else { s.1 }).collect();
    let oext: Vec<f64> = b.sizes.iter().map(|s| if horizontal { s.1 } else { s.0 }).collect();

    // simple, deduplicated edges without self loops
    let mut simple: Vec<(usize, usize)> = Vec::new();
    let mut simple_of: Vec<Option<usize>> = Vec::with_capacity(b.edges.len());
    let mut seen: HashMap<(usize, usize), usize> = HashMap::new();
    for &(u, v) in &b.edges {
        if u == v {
            simple_of.push(None);
            continue;
        }
        let key = (u.min(v), u.max(v));
        let k = *seen.entry(key).or_insert_with(|| {
            simple.push((u, v));
            simple.len() - 1
        });
        simple_of.push(Some(k));
    }

    // ---- cycle removal (Eades–Lin–Smyth greedy) --------------------------
    let order = greedy_order(n, &simple);
    let mut rank_pos = vec![0usize; n];
    for (p, &v) in order.iter().enumerate() {
        rank_pos[v] = p;
    }
    let dag: Vec<(usize, usize, bool)> =
        simple.iter().map(|&(u, v)| if rank_pos[u] < rank_pos[v] { (u, v, false) } else { (v, u, true) }).collect();
    let mut succ: Vec<Vec<usize>> = vec![vec![]; n];
    let mut pred: Vec<Vec<usize>> = vec![vec![]; n];
    for &(u, v, _) in &dag {
        succ[u].push(v);
        pred[v].push(u);
    }

    // ---- layering: longest path, then pull sources towards children ------
    let mut layer = vec![0usize; n];
    for &v in &order {
        for &u in &pred[v] {
            layer[v] = layer[v].max(layer[u] + 1);
        }
    }
    for &v in order.iter().rev() {
        if pred[v].is_empty() && !succ[v].is_empty() {
            let m = succ[v].iter().map(|&s| layer[s]).min().unwrap();
            layer[v] = m.saturating_sub(1);
        }
    }
    // width bound: push movable nodes down when a layer gets too crowded
    let max_w = if cfg.max_layer_width > 0 { cfg.max_layer_width } else { ((n as f64).sqrt() * 1.6).ceil().max(4.0) as usize };
    let mut li = 0;
    loop {
        let max_layer = *layer.iter().max().unwrap();
        if li > max_layer {
            break;
        }
        let mut members: Vec<usize> = (0..n).filter(|&v| layer[v] == li).collect();
        if members.len() > max_w {
            // prefer moving sinks, then nodes whose children are far away
            members.sort_by_key(|&v| {
                let min_succ = succ[v].iter().map(|&s| layer[s]).min().unwrap_or(usize::MAX);
                (std::cmp::Reverse(min_succ), b.names[v])
            });
            let mut excess = members.len() - max_w;
            for &v in &members {
                if excess == 0 {
                    break;
                }
                let ok = succ[v].iter().all(|&s| layer[s] > li + 1);
                if ok {
                    layer[v] = li + 1;
                    excess -= 1;
                }
            }
        }
        li += 1;
    }
    // compact empty layers
    let mut used: Vec<usize> = layer.clone();
    used.sort_unstable();
    used.dedup();
    let remap: HashMap<usize, usize> = used.iter().enumerate().map(|(i, &l)| (l, i)).collect();
    for l in layer.iter_mut() {
        *l = remap[l];
    }
    let num_layers = used.len();

    // ---- proper layering with dummy vertices ----------------------------
    let mut vlayer: Vec<usize> = layer.clone();
    let mut vext_o: Vec<f64> = oext.clone();
    let mut is_dummy: Vec<bool> = vec![false; n];
    let mut up: Vec<Vec<usize>> = vec![vec![]; n];
    let mut down: Vec<Vec<usize>> = vec![vec![]; n];
    let mut chains: Vec<Vec<usize>> = Vec::with_capacity(dag.len());
    for &(u, v, _) in &dag {
        let mut chain = vec![u];
        let mut prev = u;
        for l in layer[u] + 1..layer[v] {
            let d = vlayer.len();
            vlayer.push(l);
            vext_o.push(DUMMY_SIZE);
            is_dummy.push(true);
            up.push(vec![]);
            down.push(vec![]);
            down[prev].push(d);
            up[d].push(prev);
            chain.push(d);
            prev = d;
        }
        down[prev].push(v);
        up[v].push(prev);
        chain.push(v);
        chains.push(chain);
    }
    let total = vlayer.len();

    // ---- initial order: DFS from sources --------------------------------
    let mut layers: Vec<Vec<usize>> = vec![vec![]; num_layers];
    let mut visited = vec![false; total];
    let mut starts: Vec<usize> = (0..n).filter(|&v| up[v].is_empty()).collect();
    starts.sort_by(|&a, &c| b.degree_rank[c].partial_cmp(&b.degree_rank[a]).unwrap().then(b.names[a].cmp(b.names[c])));
    for s in starts.into_iter().chain(0..total) {
        let mut stack = vec![s];
        while let Some(v) = stack.pop() {
            if visited[v] {
                continue;
            }
            visited[v] = true;
            layers[vlayer[v]].push(v);
            for &w in down[v].iter().rev() {
                if !visited[w] {
                    stack.push(w);
                }
            }
        }
    }

    // ---- crossing reduction ---------------------------------------------
    let mut pos = vec![0.0f64; total];
    let set_pos = |layers: &Vec<Vec<usize>>, pos: &mut Vec<f64>| {
        for l in layers {
            for (i, &v) in l.iter().enumerate() {
                pos[v] = i as f64;
            }
        }
    };
    set_pos(&layers, &mut pos);
    let mut best = layers.clone();
    let mut best_c = crossings(&layers, &down, &pos);
    for iter in 0..24 {
        if best_c == 0 {
            break;
        }
        let downward = iter % 2 == 0;
        let range: Vec<usize> = if downward { (1..num_layers).collect() } else { (0..num_layers.saturating_sub(1)).rev().collect() };
        for l in range {
            let mut keyed: Vec<(f64, usize)> = layers[l]
                .iter()
                .map(|&v| {
                    let nb = if downward { &up[v] } else { &down[v] };
                    let key = if nb.is_empty() { pos[v] } else { median(nb.iter().map(|&w| pos[w]).collect()) };
                    (key, v)
                })
                .collect();
            keyed.sort_by(|a, c| a.0.partial_cmp(&c.0).unwrap().then(pos[a.1].partial_cmp(&pos[c.1]).unwrap()));
            layers[l] = keyed.into_iter().map(|(_, v)| v).collect();
            for (i, &v) in layers[l].iter().enumerate() {
                pos[v] = i as f64;
            }
        }
        transpose(&mut layers, &up, &down, &mut pos);
        let c = crossings(&layers, &down, &pos);
        if c < best_c {
            best_c = c;
            best = layers.clone();
        }
    }
    let layers = best;
    set_pos(&layers, &mut pos);

    // ---- coordinates along the ordering axis ----------------------------
    let sep = |a: usize, c: usize| -> f64 {
        match (is_dummy[a], is_dummy[c]) {
            (false, false) => cfg.node_spacing,
            (true, true) => DUMMY_SEP,
            _ => cfg.node_spacing * 0.6,
        }
    };
    let mut center = vec![0.0f64; total];
    for l in &layers {
        let mut y = 0.0;
        for (i, &v) in l.iter().enumerate() {
            if i > 0 {
                y += sep(l[i - 1], v);
            }
            center[v] = y + vext_o[v] / 2.0;
            y += vext_o[v];
        }
    }
    // centre layers against each other before refining
    let widest = layers.iter().map(|l| l.last().map_or(0.0, |&v| center[v] + vext_o[v] / 2.0)).fold(0.0, f64::max);
    for l in &layers {
        let w = l.last().map_or(0.0, |&v| center[v] + vext_o[v] / 2.0);
        let shift = (widest - w) / 2.0;
        for &v in l {
            center[v] += shift;
        }
    }
    for iter in 0..12 {
        let downward = iter % 2 == 0;
        let range: Vec<usize> = if downward { (1..num_layers).collect() } else { (0..num_layers.saturating_sub(1)).rev().collect() };
        for l in range {
            place_layer(&layers[l], &mut center, &vext_o, &sep, |v| if downward { &up[v] } else { &down[v] }, &is_dummy);
        }
    }
    for l in 0..num_layers {
        place_layer_both(&layers[l], &mut center, &vext_o, &sep, &up, &down, &is_dummy);
    }

    // ---- coordinates along the layer axis -------------------------------
    let mut lsize = vec![0.0f64; num_layers];
    for v in 0..n {
        lsize[layer[v]] = lsize[layer[v]].max(lext[v]);
    }
    let mut lstart = vec![0.0f64; num_layers];
    for l in 1..num_layers {
        // more room between layers crossed by many edges
        let crossing_edges = layers[l].iter().map(|&v| up[v].len()).sum::<usize>() as f64;
        let extra = (crossing_edges.sqrt() * 6.0).min(cfg.rank_spacing);
        lstart[l] = lstart[l - 1] + lsize[l - 1] + cfg.rank_spacing + extra;
    }
    let total_l = lstart[num_layers - 1] + lsize[num_layers - 1];
    let lcenter = |l: usize| lstart[l] + lsize[l] / 2.0;

    let mirror = matches!(cfg.direction, Direction::RL | Direction::BT);
    let to_xy = |lpos: f64, opos: f64| -> Pt {
        let lpos = if mirror { total_l - lpos } else { lpos };
        if horizontal {
            (lpos, opos)
        } else {
            (opos, lpos)
        }
    };
    let mut out = vec![(0.0, 0.0); n];
    for v in 0..n {
        // align nodes to the start of their layer (reads like columns / rows)
        let lp = if horizontal { lstart[layer[v]] + (lsize[layer[v]] - lext[v]) / 2.0 } else { lstart[layer[v]] };
        let op = center[v] - oext[v] / 2.0;
        let (x, y) = to_xy(lp, op);
        // mirrored axes need the far corner
        out[v] = if mirror {
            if horizontal {
                (x - lext[v], y)
            } else {
                (x, y - lext[v])
            }
        } else {
            (x, y)
        };
    }

    let mut waypoints: HashMap<usize, Vec<Pt>> = HashMap::new();
    for (k, s) in simple_of.iter().enumerate() {
        let Some(si) = s else { continue };
        let chain = &chains[*si];
        if chain.len() <= 2 {
            continue;
        }
        let (u, _v, reversed) = dag[*si];
        let _ = u;
        let mut pts: Vec<Pt> = chain[1..chain.len() - 1].iter().map(|&d| to_xy(lcenter(vlayer[d]), center[d])).collect();
        // chains run along the DAG; make them parent→child for this edge
        let (eu, _ev) = b.edges[k];
        let chain_starts_at_parent = chain[0] == eu;
        if !chain_starts_at_parent {
            pts.reverse();
        }
        let _ = reversed;
        waypoints.insert(b.edge_ids[k], pts);
    }
    BlockResult { pos: out, waypoints }
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let m = v.len() / 2;
    if v.len() % 2 == 1 {
        v[m]
    } else {
        (v[m - 1] + v[m]) / 2.0
    }
}

/// Order for cycle breaking: edges pointing backwards in this order get reversed.
fn greedy_order(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    let mut outs: Vec<Vec<usize>> = vec![vec![]; n];
    let mut ins: Vec<Vec<usize>> = vec![vec![]; n];
    for &(u, v) in edges {
        outs[u].push(v);
        ins[v].push(u);
    }
    let mut indeg: Vec<i64> = ins.iter().map(|x| x.len() as i64).collect();
    let mut outdeg: Vec<i64> = outs.iter().map(|x| x.len() as i64).collect();
    let mut removed = vec![false; n];
    let mut s1 = Vec::new();
    let mut s2 = Vec::new();
    let mut left = n;
    let remove = |v: usize, removed: &mut Vec<bool>, indeg: &mut Vec<i64>, outdeg: &mut Vec<i64>| {
        removed[v] = true;
        for &w in &outs[v] {
            indeg[w] -= 1;
        }
        for &w in &ins[v] {
            outdeg[w] -= 1;
        }
    };
    while left > 0 {
        let mut progress = true;
        while progress {
            progress = false;
            for v in 0..n {
                if !removed[v] && outdeg[v] == 0 {
                    s2.push(v);
                    remove(v, &mut removed, &mut indeg, &mut outdeg);
                    left -= 1;
                    progress = true;
                }
            }
            for v in 0..n {
                if !removed[v] && indeg[v] == 0 {
                    s1.push(v);
                    remove(v, &mut removed, &mut indeg, &mut outdeg);
                    left -= 1;
                    progress = true;
                }
            }
        }
        if left == 0 {
            break;
        }
        let v = (0..n).filter(|&v| !removed[v]).max_by_key(|&v| (outdeg[v] - indeg[v], std::cmp::Reverse(v))).unwrap();
        s1.push(v);
        remove(v, &mut removed, &mut indeg, &mut outdeg);
        left -= 1;
    }
    s2.reverse();
    s1.extend(s2);
    s1
}

/// Total crossings between adjacent layers (inversion counting with a Fenwick tree).
fn crossings(layers: &[Vec<usize>], down: &[Vec<usize>], pos: &[f64]) -> usize {
    let mut total = 0;
    for l in 0..layers.len().saturating_sub(1) {
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for &u in &layers[l] {
            for &v in &down[u] {
                pairs.push((pos[u] as usize, pos[v] as usize));
            }
        }
        pairs.sort_unstable();
        let m = layers[l + 1].len() + 1;
        let mut tree = vec![0usize; m + 1];
        for (seen, &(_, v)) in pairs.iter().enumerate() {
            // count earlier pairs with target > v
            let mut i = v + 1;
            let mut le = 0;
            while i > 0 {
                le += tree[i];
                i -= i & i.wrapping_neg();
            }
            total += seen - le;
            let mut i = v + 1;
            while i <= m {
                tree[i] += 1;
                i += i & i.wrapping_neg();
            }
        }
    }
    total
}

/// Swap adjacent nodes when that reduces local crossings.
fn transpose(layers: &mut [Vec<usize>], up: &[Vec<usize>], down: &[Vec<usize>], pos: &mut [f64]) {
    let pair_cross = |a: usize, b: usize, pos: &[f64]| -> usize {
        let mut c = 0;
        for nb in [&up[a], &down[a]].iter().zip([&up[b], &down[b]].iter()) {
            for &x in nb.0.iter() {
                for &y in nb.1.iter() {
                    if pos[x] > pos[y] {
                        c += 1;
                    }
                }
            }
        }
        c
    };
    for _ in 0..4 {
        let mut improved = false;
        for l in layers.iter_mut() {
            for i in 0..l.len().saturating_sub(1) {
                let (a, b) = (l[i], l[i + 1]);
                if pair_cross(a, b, pos) > pair_cross(b, a, pos) {
                    l.swap(i, i + 1);
                    pos[a] = (i + 1) as f64;
                    pos[b] = i as f64;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Place a layer so nodes sit close to the average of their neighbours while
/// keeping order and separation (weighted isotonic regression / PAV).
fn place_layer<'a, F, S>(l: &[usize], center: &mut [f64], ext: &[f64], sep: &S, nbs: F, is_dummy: &[bool])
where
    F: Fn(usize) -> &'a Vec<usize>,
    S: Fn(usize, usize) -> f64,
{
    let targets: Vec<(f64, f64)> = l
        .iter()
        .map(|&v| {
            let nb = nbs(v);
            if nb.is_empty() {
                (center[v], 0.05)
            } else {
                let avg = nb.iter().map(|&w| center[w]).sum::<f64>() / nb.len() as f64;
                (avg, if is_dummy[v] { 2.0 } else { 1.0 })
            }
        })
        .collect();
    solve(l, center, ext, sep, &targets);
}

fn place_layer_both<S>(l: &[usize], center: &mut [f64], ext: &[f64], sep: &S, up: &[Vec<usize>], down: &[Vec<usize>], is_dummy: &[bool])
where
    S: Fn(usize, usize) -> f64,
{
    let targets: Vec<(f64, f64)> = l
        .iter()
        .map(|&v| {
            let nb: Vec<usize> = up[v].iter().chain(down[v].iter()).copied().collect();
            if nb.is_empty() {
                (center[v], 0.05)
            } else {
                (nb.iter().map(|&w| center[w]).sum::<f64>() / nb.len() as f64, if is_dummy[v] { 2.0 } else { 1.0 })
            }
        })
        .collect();
    solve(l, center, ext, sep, &targets);
}

fn solve<S>(l: &[usize], center: &mut [f64], ext: &[f64], sep: &S, targets: &[(f64, f64)])
where
    S: Fn(usize, usize) -> f64,
{
    if l.is_empty() {
        return;
    }
    // offsets: minimal centre-to-centre distances accumulated
    let mut off = vec![0.0f64; l.len()];
    for i in 1..l.len() {
        off[i] = off[i - 1] + ext[l[i - 1]] / 2.0 + sep(l[i - 1], l[i]) + ext[l[i]] / 2.0;
    }
    // PAV on z_i = c_i - off_i (must be non-decreasing)
    struct Blk {
        w: f64,
        wt: f64,
        n: usize,
    }
    let mut stack: Vec<Blk> = Vec::new();
    for i in 0..l.len() {
        let (t, w) = targets[i];
        let w = w.max(1e-6);
        stack.push(Blk { w, wt: w * (t - off[i]), n: 1 });
        while stack.len() >= 2 {
            let k = stack.len();
            let a = stack[k - 2].wt / stack[k - 2].w;
            let b2 = stack[k - 1].wt / stack[k - 1].w;
            if a <= b2 {
                break;
            }
            let top = stack.pop().unwrap();
            let prev = stack.last_mut().unwrap();
            prev.w += top.w;
            prev.wt += top.wt;
            prev.n += top.n;
        }
    }
    let mut i = 0;
    for blk in stack {
        let z = blk.wt / blk.w;
        for _ in 0..blk.n {
            center[l[i]] = z + off[i];
            i += 1;
        }
    }
}
