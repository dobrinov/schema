//! Node placement. Every algorithm works on a list of box sizes plus edges
//! and returns top-left positions; this module handles grouping, connected
//! components, packing and pinned positions.
use std::collections::HashMap;

use serde::Serialize;

use crate::config::{Algorithm, GroupBy, LayoutConfig, ViewConfig};
use crate::graph::Graph;

mod focus;
mod force;
mod layered;
mod simple;

pub type Pt = (f64, f64);

#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn cx(&self) -> f64 {
        self.x + self.w / 2.0
    }
    pub fn cy(&self) -> f64 {
        self.y + self.h / 2.0
    }
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupBox {
    pub name: String,
    pub color: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Layout {
    pub nodes: Vec<Rect>,
    /// Intermediate points per edge, ordered from `edge.from` to `edge.to`.
    #[serde(skip)]
    pub waypoints: Vec<Vec<Pt>>,
    pub groups: Vec<GroupBox>,
    pub width: f64,
    pub height: f64,
}

/// Input for a single layout run over a subset of nodes.
pub(crate) struct Block<'a> {
    pub sizes: Vec<Pt>,
    /// (referenced/parent, referencing/child) local indices
    pub edges: Vec<(usize, usize)>,
    /// Index into the caller's edge list for each entry of `edges`.
    pub edge_ids: Vec<usize>,
    pub degree_rank: Vec<f64>,
    pub names: Vec<&'a str>,
    pub roots: Vec<usize>,
    /// Per edge: offset of the anchor row from the top of (parent, child).
    pub anchors: Vec<Pt>,
}

pub(crate) struct BlockResult {
    pub pos: Vec<Pt>,
    pub waypoints: HashMap<usize, Vec<Pt>>,
}

const GROUP_PAD: f64 = 24.0;
const GROUP_TITLE: f64 = 26.0;

pub fn compute(g: &Graph, cfg: &ViewConfig) -> Layout {
    let n = g.nodes.len();
    let mut layout = Layout { nodes: vec![Rect::default(); n], waypoints: vec![Vec::new(); g.edges.len()], ..Default::default() };
    if n == 0 {
        return layout;
    }
    let idx = g.node_index();
    let edges: Vec<(usize, usize)> = g.edges.iter().map(|e| (idx[e.to.as_str()], idx[e.from.as_str()])).collect();
    // where each relation attaches, relative to the top of (parent, child)
    let anchor = |node: &crate::graph::Node, cols: &[String]| {
        cols.first().and_then(|c| node.row_center(c)).unwrap_or(crate::graph::metrics::HEADER_H / 2.0)
    };
    let anchors: Vec<Pt> = g
        .edges
        .iter()
        .map(|e| (anchor(&g.nodes[idx[e.to.as_str()]], &e.to_columns), anchor(&g.nodes[idx[e.from.as_str()]], &e.from_columns)))
        .collect();

    // partition into groups (clusters)
    let mut group_names: Vec<Option<String>> = Vec::new();
    let mut group_of = vec![0usize; n];
    if cfg.layout.group_by != GroupBy::None {
        for (i, node) in g.nodes.iter().enumerate() {
            let key = node.group.clone();
            let gi = match group_names.iter().position(|k| *k == key) {
                Some(p) => p,
                None => {
                    group_names.push(key);
                    group_names.len() - 1
                }
            };
            group_of[i] = gi;
        }
        // named groups first, ungrouped last
        let mut order: Vec<usize> = (0..group_names.len()).collect();
        order.sort_by_key(|&k| (group_names[k].is_none(), group_names[k].clone()));
        let remap: HashMap<usize, usize> = order.iter().enumerate().map(|(new, &old)| (old, new)).collect();
        group_names = order.iter().map(|&k| group_names[k].clone()).collect();
        for gi in group_of.iter_mut() {
            *gi = remap[gi];
        }
    } else {
        group_names.push(None);
    }

    let mut blocks: Vec<(Vec<usize>, BlockResult, Pt)> = Vec::new();
    for (gi, _) in group_names.iter().enumerate() {
        let members: Vec<usize> = (0..n).filter(|&i| group_of[i] == gi).collect();
        let (res, size) = layout_members(g, &members, &edges, &anchors, &cfg.layout);
        blocks.push((members, res, size));
    }

    // pack groups
    let grouped = cfg.layout.group_by != GroupBy::None;
    let mut boxes: Vec<Pt> = blocks
        .iter()
        .enumerate()
        .map(|(gi, (_, _, s))| if grouped && group_names[gi].is_some() { (s.0 + GROUP_PAD * 2.0, s.1 + GROUP_PAD * 2.0 + GROUP_TITLE) } else { *s })
        .collect();
    if boxes.is_empty() {
        boxes.push((0.0, 0.0));
    }
    let offsets = shelf_pack(&boxes, cfg.layout.node_spacing * 2.0, None);
    for (gi, (members, res, size)) in blocks.iter().enumerate() {
        let (mut ox, mut oy) = offsets[gi];
        if grouped {
            if let Some(name) = &group_names[gi] {
                let color = cfg
                    .groups
                    .iter()
                    .find(|d| &d.name == name)
                    .and_then(|d| d.color.clone())
                    .unwrap_or_else(|| crate::graph::color_for(name));
                layout.groups.push(GroupBox { name: name.clone(), color, rect: Rect { x: ox, y: oy, w: boxes[gi].0, h: boxes[gi].1 } });
                ox += GROUP_PAD;
                oy += GROUP_PAD + GROUP_TITLE;
            }
        }
        let _ = size;
        for (li, &gi_node) in members.iter().enumerate() {
            let (x, y) = res.pos[li];
            layout.nodes[gi_node] = Rect { x: x + ox, y: y + oy, w: g.nodes[gi_node].width, h: g.nodes[gi_node].height };
        }
        for (eid, pts) in &res.waypoints {
            // waypoints come parent→child; edges are drawn from→to (child→parent)
            layout.waypoints[*eid] = pts.iter().rev().map(|&(x, y)| (x + ox, y + oy)).collect();
        }
    }

    // pinned positions
    let mut moved = vec![false; n];
    for (i, node) in g.nodes.iter().enumerate() {
        if let Some(p) = cfg.positions.get(&node.id) {
            layout.nodes[i].x = p[0];
            layout.nodes[i].y = p[1];
            moved[i] = true;
        }
    }
    if moved.iter().any(|m| *m) {
        // Unpinned tables (e.g. new ones in a design) must not land on pinned
        // ones: move overlapping ones into free space to the right.
        let gap = cfg.layout.node_spacing.max(24.0);
        let overlaps = |a: &Rect, b: &Rect| a.x < b.right() + gap && b.x < a.right() + gap && a.y < b.bottom() + gap && b.y < a.bottom() + gap;
        let mut placed: Vec<Rect> = (0..n).filter(|&i| moved[i]).map(|i| layout.nodes[i]).collect();
        let mut free_x = placed.iter().map(|r| r.right()).fold(f64::MIN, f64::max) + gap * 2.0;
        let top = placed.iter().map(|r| r.y).fold(f64::MAX, f64::min);
        let mut free_y = top;
        let mut col_w: f64 = 0.0;
        let col_h = placed.iter().map(|r| r.bottom()).fold(f64::MIN, f64::max) - top;
        for i in 0..n {
            if moved[i] {
                continue;
            }
            let r = layout.nodes[i];
            if placed.iter().any(|p| overlaps(&r, p)) {
                if free_y > top && free_y + r.h > top + col_h.max(r.h) {
                    free_x += col_w + gap;
                    free_y = top;
                    col_w = 0.0;
                }
                layout.nodes[i].x = free_x;
                layout.nodes[i].y = free_y;
                free_y += r.h + gap;
                col_w = col_w.max(r.w);
                moved[i] = true;
            }
            placed.push(layout.nodes[i]);
        }
        // Pinned tables can collide when they grow (e.g. columns added while
        // designing): nudge apart only the ones that actually overlap.
        let mut pos: Vec<Pt> = layout.nodes.iter().map(|r| (r.x, r.y)).collect();
        let sizes: Vec<Pt> = layout.nodes.iter().map(|r| (r.w, r.h)).collect();
        force::remove_overlaps(&mut pos, &sizes, (cfg.layout.node_spacing * 0.5).max(12.0));
        for (i, p) in pos.into_iter().enumerate() {
            if (p.0 - layout.nodes[i].x).abs() > 0.01 || (p.1 - layout.nodes[i].y).abs() > 0.01 {
                layout.nodes[i].x = p.0;
                layout.nodes[i].y = p.1;
                moved[i] = true;
            }
        }
        for (ei, e) in g.edges.iter().enumerate() {
            if moved[idx[e.from.as_str()]] || moved[idx[e.to.as_str()]] {
                layout.waypoints[ei].clear();
            }
        }
        if !layout.groups.is_empty() {
            refit_groups(&mut layout, g);
        }
    }
    normalize(&mut layout);
    layout
}

fn refit_groups(layout: &mut Layout, g: &Graph) {
    for gb in &mut layout.groups {
        let rects: Vec<Rect> = g.nodes.iter().zip(&layout.nodes).filter(|(n, _)| n.group.as_deref() == Some(gb.name.as_str())).map(|(_, r)| *r).collect();
        if rects.is_empty() {
            continue;
        }
        let x0 = rects.iter().map(|r| r.x).fold(f64::MAX, f64::min) - GROUP_PAD;
        let y0 = rects.iter().map(|r| r.y).fold(f64::MAX, f64::min) - GROUP_PAD - GROUP_TITLE;
        let x1 = rects.iter().map(|r| r.right()).fold(f64::MIN, f64::max) + GROUP_PAD;
        let y1 = rects.iter().map(|r| r.bottom()).fold(f64::MIN, f64::max) + GROUP_PAD;
        gb.rect = Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
    }
}

/// Shift everything so the drawing starts at (0, 0) and compute its size.
pub fn normalize(layout: &mut Layout) {
    let mut x0 = f64::MAX;
    let mut y0 = f64::MAX;
    let mut x1 = f64::MIN;
    let mut y1 = f64::MIN;
    let mut acc = |r: &Rect| {
        x0 = x0.min(r.x);
        y0 = y0.min(r.y);
        x1 = x1.max(r.right());
        y1 = y1.max(r.bottom());
    };
    layout.nodes.iter().for_each(&mut acc);
    layout.groups.iter().for_each(|g| acc(&g.rect));
    for w in layout.waypoints.iter().flatten() {
        acc(&Rect { x: w.0, y: w.1, w: 0.0, h: 0.0 });
    }
    if x0 == f64::MAX {
        return;
    }
    for r in &mut layout.nodes {
        r.x -= x0;
        r.y -= y0;
    }
    for g in &mut layout.groups {
        g.rect.x -= x0;
        g.rect.y -= y0;
    }
    for w in layout.waypoints.iter_mut().flatten() {
        w.0 -= x0;
        w.1 -= y0;
    }
    layout.width = x1 - x0;
    layout.height = y1 - y0;
}

/// Lay out a subset of nodes, splitting it into connected components when
/// configured. Returns local positions and the bounding size.
fn layout_members(g: &Graph, members: &[usize], edges: &[(usize, usize)], anchors: &[Pt], cfg: &LayoutConfig) -> (BlockResult, Pt) {
    let local: HashMap<usize, usize> = members.iter().enumerate().map(|(l, &gi)| (gi, l)).collect();
    let mut block_edges = Vec::new();
    let mut block_ids = Vec::new();
    for (ei, &(a, b)) in edges.iter().enumerate() {
        if let (Some(&la), Some(&lb)) = (local.get(&a), local.get(&b)) {
            block_edges.push((la, lb));
            block_ids.push(ei);
        }
    }
    let m = members.len();
    let sizes: Vec<Pt> = members.iter().map(|&i| (g.nodes[i].width, g.nodes[i].height)).collect();
    let names: Vec<&str> = members.iter().map(|&i| g.nodes[i].id.as_str()).collect();
    let roots: Vec<usize> = members.iter().enumerate().filter(|(_, &i)| g.nodes[i].focused).map(|(l, _)| l).collect();

    let components = if cfg.pack_components && !matches!(cfg.algorithm, Algorithm::Grid | Algorithm::Circular) {
        connected_components(m, &block_edges)
    } else {
        vec![(0..m).collect()]
    };

    // isolated nodes are collected into one grid block
    let mut comp_list: Vec<Vec<usize>> = Vec::new();
    let mut singles: Vec<usize> = Vec::new();
    for c in components {
        let self_only = c.len() == 1;
        if self_only && components_len_gt1(&c, &block_edges) {
            comp_list.push(c);
        } else if self_only && cfg.pack_components {
            singles.push(c[0]);
        } else {
            comp_list.push(c);
        }
    }
    comp_list.sort_by(|a, b| b.len().cmp(&a.len()));

    let mut parts: Vec<(Vec<usize>, BlockResult, Pt)> = Vec::new();
    for comp in comp_list {
        let res = run_block(&comp, &sizes, &names, &block_edges, &block_ids, anchors, &roots, cfg, false);
        parts.push(res);
    }
    if !singles.is_empty() {
        singles.sort_by(|&a, &b| names[a].cmp(names[b]));
        let res = run_block(&singles, &sizes, &names, &block_edges, &block_ids, anchors, &roots, cfg, true);
        parts.push(res);
    }
    if parts.len() == 1 {
        let (comp, res, size) = parts.pop().unwrap();
        let mut pos = vec![(0.0, 0.0); m];
        for (li, &node) in comp.iter().enumerate() {
            pos[node] = res.pos[li];
        }
        return (BlockResult { pos, waypoints: res.waypoints }, size);
    }
    let sizes_b: Vec<Pt> = parts.iter().map(|p| p.2).collect();
    let offsets = shelf_pack(&sizes_b, cfg.rank_spacing.max(cfg.node_spacing * 2.0), None);
    let mut pos = vec![(0.0, 0.0); m];
    let mut waypoints = HashMap::new();
    let (mut w, mut h) = (0.0f64, 0.0f64);
    for (pi, (comp, res, size)) in parts.into_iter().enumerate() {
        let (ox, oy) = offsets[pi];
        for (li, &node) in comp.iter().enumerate() {
            pos[node] = (res.pos[li].0 + ox, res.pos[li].1 + oy);
        }
        for (eid, pts) in res.waypoints {
            waypoints.insert(eid, pts.into_iter().map(|(x, y)| (x + ox, y + oy)).collect());
        }
        w = w.max(ox + size.0);
        h = h.max(oy + size.1);
    }
    (BlockResult { pos, waypoints }, (w, h))
}

fn components_len_gt1(c: &[usize], edges: &[(usize, usize)]) -> bool {
    // a single node that only has a self loop is still "connected"
    c.len() == 1 && edges.iter().any(|&(a, b)| a == c[0] && b == c[0])
}

#[allow(clippy::too_many_arguments)]
fn run_block(
    comp: &[usize],
    sizes: &[Pt],
    names: &[&str],
    edges: &[(usize, usize)],
    edge_ids: &[usize],
    anchors: &[Pt],
    roots: &[usize],
    cfg: &LayoutConfig,
    force_grid: bool,
) -> (Vec<usize>, BlockResult, Pt) {
    let local: HashMap<usize, usize> = comp.iter().enumerate().map(|(l, &i)| (i, l)).collect();
    let mut b = Block { sizes: comp.iter().map(|&i| sizes[i]).collect(), edges: vec![], edge_ids: vec![], degree_rank: vec![], names: comp.iter().map(|&i| names[i]).collect(), roots: vec![], anchors: vec![] };
    for (k, &(u, v)) in edges.iter().enumerate() {
        if let (Some(&a), Some(&c)) = (local.get(&u), local.get(&v)) {
            b.edges.push((a, c));
            b.edge_ids.push(edge_ids[k]);
            b.anchors.push(anchors[edge_ids[k]]);
        }
    }
    let mut deg = vec![0.0; comp.len()];
    for &(u, v) in &b.edges {
        deg[u] += 1.0;
        deg[v] += 1.0;
    }
    b.degree_rank = deg;
    b.roots = roots.iter().filter_map(|r| local.get(r).copied()).collect();
    let res = if force_grid {
        simple::grid(&b, cfg, true)
    } else {
        match cfg.algorithm {
            Algorithm::Layered if cfg.focus_layout && !b.roots.is_empty() => focus::layout(&b, cfg),
            Algorithm::Layered => layered::layout(&b, cfg),
            Algorithm::Force => force::layout(&b, cfg),
            Algorithm::Grid => simple::grid(&b, cfg, false),
            Algorithm::Circular => simple::circular(&b, cfg),
            Algorithm::Radial => simple::radial(&b, cfg),
        }
    };
    let res = normalize_block(res, &b.sizes);
    let size = bounds(&res.pos, &b.sizes, &res.waypoints);
    (comp.to_vec(), res, size)
}

fn normalize_block(mut r: BlockResult, sizes: &[Pt]) -> BlockResult {
    let mut x0 = f64::MAX;
    let mut y0 = f64::MAX;
    for p in &r.pos {
        x0 = x0.min(p.0);
        y0 = y0.min(p.1);
    }
    for w in r.waypoints.values().flatten() {
        x0 = x0.min(w.0);
        y0 = y0.min(w.1);
    }
    if x0 == f64::MAX || sizes.is_empty() {
        return r;
    }
    for p in &mut r.pos {
        p.0 -= x0;
        p.1 -= y0;
    }
    for w in r.waypoints.values_mut().flatten() {
        w.0 -= x0;
        w.1 -= y0;
    }
    r
}

fn bounds(pos: &[Pt], sizes: &[Pt], wps: &HashMap<usize, Vec<Pt>>) -> Pt {
    let mut w: f64 = 0.0;
    let mut h: f64 = 0.0;
    for (p, s) in pos.iter().zip(sizes) {
        w = w.max(p.0 + s.0);
        h = h.max(p.1 + s.1);
    }
    for p in wps.values().flatten() {
        w = w.max(p.0);
        h = h.max(p.1);
    }
    (w, h)
}

pub(crate) fn connected_components(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut c = x;
        while p[c] != r {
            let nx = p[c];
            p[c] = r;
            c = nx;
        }
        r
    }
    for &(a, b) in edges {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        if ra != rb {
            parent[ra] = rb;
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut v: Vec<Vec<usize>> = groups.into_values().collect();
    v.sort_by_key(|c| c[0]);
    v
}

/// Shelf packing: boxes keep their order, rows wrap at a width chosen to
/// give a pleasant aspect ratio.
pub(crate) fn shelf_pack(sizes: &[Pt], gap: f64, max_width: Option<f64>) -> Vec<Pt> {
    let area: f64 = sizes.iter().map(|s| (s.0 + gap) * (s.1 + gap)).sum();
    let widest = sizes.iter().map(|s| s.0).fold(0.0, f64::max);
    let target = max_width.unwrap_or_else(|| (area * 1.6).sqrt()).max(widest);
    let mut out = Vec::with_capacity(sizes.len());
    let (mut x, mut y, mut row_h) = (0.0, 0.0, 0.0f64);
    for s in sizes {
        if x > 0.0 && x + s.0 > target {
            x = 0.0;
            y += row_h + gap;
            row_h = 0.0;
        }
        out.push((x, y));
        x += s.0 + gap;
        row_h = row_h.max(s.1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Algorithm, ViewConfig};
    use crate::graph::build;
    use crate::parser::parse;

    fn sample() -> crate::model::Schema {
        let mut sql = String::new();
        sql.push_str("CREATE TABLE public.users (id bigint NOT NULL, name text);\n");
        for i in 0..12 {
            sql.push_str(&format!("CREATE TABLE public.t{i} (id bigint NOT NULL, user_id bigint, parent_id bigint);\n"));
            sql.push_str(&format!("ALTER TABLE ONLY public.t{i} ADD CONSTRAINT fk_u{i} FOREIGN KEY (user_id) REFERENCES public.users(id);\n"));
            if i > 0 {
                sql.push_str(&format!("ALTER TABLE ONLY public.t{i} ADD CONSTRAINT fk_p{i} FOREIGN KEY (parent_id) REFERENCES public.t{}(id);\n", i - 1));
            }
        }
        sql.push_str("CREATE TABLE public.lonely (id int);\nCREATE TABLE public.lonely2 (id int);\n");
        sql.push_str("ALTER TABLE ONLY public.t3 ADD CONSTRAINT fk_cycle FOREIGN KEY (parent_id) REFERENCES public.t7(id);\n");
        parse(&sql)
    }

    fn overlaps(l: &Layout) -> Option<(usize, usize)> {
        for i in 0..l.nodes.len() {
            for j in i + 1..l.nodes.len() {
                let (a, b) = (l.nodes[i], l.nodes[j]);
                if a.x < b.right() - 0.5 && b.x < a.right() - 0.5 && a.y < b.bottom() - 0.5 && b.y < a.bottom() - 0.5 {
                    return Some((i, j));
                }
            }
        }
        None
    }

    #[test]
    fn all_algorithms_avoid_overlap() {
        let s = sample();
        for alg in [Algorithm::Layered, Algorithm::Force, Algorithm::Grid, Algorithm::Circular, Algorithm::Radial] {
            for dir in [crate::config::Direction::LR, crate::config::Direction::TB] {
                let mut cfg = ViewConfig::default();
                cfg.layout.algorithm = alg;
                cfg.layout.direction = dir;
                let g = build(&s, None, None, &cfg);
                let l = compute(&g, &cfg);
                assert_eq!(l.nodes.len(), 15);
                assert!(overlaps(&l).is_none(), "{alg:?} {dir:?} overlap {:?}", overlaps(&l));
                assert!(l.width > 0.0 && l.height > 0.0);
                for r in &l.nodes {
                    assert!(r.x.is_finite() && r.y.is_finite());
                }
            }
        }
    }

    /// Crossings between straight segments joining the anchor points of each
    /// relation (a good proxy for how tangled the drawn edges are).
    fn crossings(g: &Graph, l: &Layout) -> usize {
        let idx = g.node_index();
        let seg: Vec<((f64, f64), (f64, f64), usize, usize)> = g
            .edges
            .iter()
            .filter(|e| e.from != e.to)
            .map(|e| {
                let (a, b) = (idx[e.from.as_str()], idx[e.to.as_str()]);
                let (ra, rb) = (l.nodes[a], l.nodes[b]);
                let ya = ra.y + e.from_columns.first().and_then(|c| g.nodes[a].row_center(c)).unwrap_or(16.0);
                let yb = rb.y + e.to_columns.first().and_then(|c| g.nodes[b].row_center(c)).unwrap_or(16.0);
                let (xa, xb) = if rb.cx() > ra.cx() { (ra.right(), rb.x) } else { (ra.x, rb.right()) };
                ((xa, ya), (xb, yb), a, b)
            })
            .collect();
        let cross = |p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), p4: (f64, f64)| {
            let d = |a: (f64, f64), b: (f64, f64), c: (f64, f64)| (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            let (d1, d2, d3, d4) = (d(p3, p4, p1), d(p3, p4, p2), d(p1, p2, p3), d(p1, p2, p4));
            d1 * d2 < 0.0 && d3 * d4 < 0.0
        };
        let mut n = 0;
        for i in 0..seg.len() {
            for j in i + 1..seg.len() {
                let (s, t) = (seg[i], seg[j]);
                let shared = s.2 == t.2 || s.2 == t.3 || s.3 == t.2 || s.3 == t.3;
                if !shared && cross(s.0, s.1, t.0, t.1) {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn focus_layout_is_readable() {
        let s = parse(include_str!("../../../../examples/structure.sql"));
        for (focus, depth) in [("tasks", 1), ("users", 1), ("accounts", 1), ("tasks", 2), ("billing.invoices", 2)] {
            let mut cfg = ViewConfig::default();
            cfg.focus = vec![focus.into()];
            cfg.focus_depth = depth;
            let g = build(&s, None, None, &cfg);
            let l = compute(&g, &cfg);
            assert!(overlaps(&l).is_none(), "{focus}: overlap {:?}", overlaps(&l));
            let idx = g.node_index();
            let id = if focus.contains('.') { focus.to_string() } else { format!("public.{focus}") };
            let f = l.nodes[idx[id.as_str()]];
            // referenced tables to the left, referencing tables to the right (unless moved to avoid crossings)
            if depth == 1 {
                for e in &g.edges {
                    if e.from == id && e.to != id {
                        let t = l.nodes[idx[e.to.as_str()]];
                        assert!(t.right() <= f.x || t.x >= f.right(), "{focus}: {} overlaps the focus column", e.to);
                    }
                }
            }
            let ours = crossings(&g, &l);
            for alg in [Algorithm::Radial, Algorithm::Force] {
                let mut other = cfg.clone();
                other.layout.algorithm = alg;
                let lo = compute(&g, &other);
                assert!(ours <= crossings(&g, &lo), "{focus}/{depth}: focus layout has {ours} crossings, {alg:?} has {}", crossings(&g, &lo));
            }
            let mut plain = cfg.clone();
            plain.layout.focus_layout = false;
            let lp = compute(&g, &plain);
            eprintln!("{focus} depth {depth}: {} tables, crossings focus={ours} layered={}", g.nodes.len(), crossings(&g, &lp));
        }
    }

    #[test]
    fn focus_layout_orders_parents_by_anchor_row() {
        // `tasks` references projects (row 2), users (rows 3, 4) and itself; parents
        // must be stacked in the same order as the FK rows so edges don't cross
        let s = parse(include_str!("../../../../examples/structure.sql"));
        let mut cfg = ViewConfig::default();
        cfg.focus = vec!["tasks".into()];
        cfg.focus_direction = crate::config::FocusDirection::Outgoing;
        let g = build(&s, None, None, &cfg);
        let l = compute(&g, &cfg);
        assert_eq!(crossings(&g, &l), 0);
        let idx = g.node_index();
        let (t, p, u) = (l.nodes[idx["public.tasks"]], l.nodes[idx["public.projects"]], l.nodes[idx["public.users"]]);
        assert!(p.right() <= t.x && u.right() <= t.x, "parents on the left");
        assert!(p.y < u.y, "projects (project_id is above assignee_id) sits above users");
    }

    #[test]
    fn unpinned_nodes_avoid_pinned_ones() {
        let s = sample();
        let mut cfg = ViewConfig::default();
        let g = build(&s, None, None, &cfg);
        let l = compute(&g, &cfg);
        // pin everything except two tables onto one spot's neighbourhood
        for (i, node) in g.nodes.iter().enumerate() {
            if node.id != "public.t5" && node.id != "public.lonely" {
                cfg.positions.insert(node.id.clone(), [l.nodes[i].x, l.nodes[i].y]);
            }
        }
        // pin `users` right where the unpinned `t5` would be laid out
        let t5 = g.nodes.iter().position(|n| n.id == "public.t5").unwrap();
        cfg.positions.insert("public.users".into(), [l.nodes[t5].x, l.nodes[t5].y]);
        let l2 = compute(&g, &cfg);
        assert!(overlaps(&l2).is_none(), "{:?}", overlaps(&l2));
        // two pinned tables on the same spot are pushed apart
        let mut cfg3 = cfg.clone();
        cfg3.positions.insert("public.t1".into(), [l.nodes[t5].x, l.nodes[t5].y]);
        assert!(overlaps(&compute(&g, &cfg3)).is_none());
        // pinned tables stay exactly where they were put (relative to each other)
        let users = g.nodes.iter().position(|n| n.id == "public.users").unwrap();
        let t0 = g.nodes.iter().position(|n| n.id == "public.t0").unwrap();
        let (dx, dy) = (l2.nodes[users].x - l2.nodes[t0].x, l2.nodes[users].y - l2.nodes[t0].y);
        assert!((dx - (l.nodes[t5].x - l.nodes[t0].x)).abs() < 0.01 && (dy - (l.nodes[t5].y - l.nodes[t0].y)).abs() < 0.01);
    }

    #[test]
    fn layered_puts_parents_first() {
        let s = sample();
        let cfg = ViewConfig::default();
        let g = build(&s, None, None, &cfg);
        let l = compute(&g, &cfg);
        let idx = g.node_index();
        let users = l.nodes[idx["public.users"]];
        let t0 = l.nodes[idx["public.t0"]];
        assert!(users.x < t0.x);
    }
}
