//! Edge routing: anchors on table borders or column rows, then curved,
//! orthogonal or straight paths (optionally through layout waypoints).
use crate::config::{Direction, EdgeAnchor, EdgeConfig, EdgeStyle};
use crate::graph::{metrics, Edge, Node};
use crate::layout::{Pt, Rect};

pub(crate) const STUB: f64 = 14.0;
pub(crate) const RADIUS: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    pub(crate) fn normal(self) -> Pt {
        match self {
            Side::Left => (-1.0, 0.0),
            Side::Right => (1.0, 0.0),
            Side::Top => (0.0, -1.0),
            Side::Bottom => (0.0, 1.0),
        }
    }
    pub(crate) fn horizontal(self) -> bool {
        matches!(self, Side::Left | Side::Right)
    }
}

/// Where an edge leaves and enters its tables.
pub(crate) struct Ends {
    pub s: Pt,
    pub t: Pt,
    pub ss: Side,
    pub ts: Side,
    pub fy: f64,
    pub ty: f64,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn ends(edge: &Edge, from: &Node, fr: &Rect, to: &Node, tr: &Rect, waypoints: &[Pt], cfg: &EdgeConfig, dir: Direction) -> Ends {
    let fy = anchor_y(from, fr, &edge.from_columns, cfg.anchor);
    let ty = anchor_y(to, tr, &edge.to_columns, cfg.anchor);
    let s_ref = waypoints.first().copied().unwrap_or((tr.cx(), ty));
    let t_ref = waypoints.last().copied().unwrap_or((fr.cx(), fy));
    let h_overlap = fr.x < tr.right() && tr.x < fr.right();
    let v_overlap = fr.y < tr.bottom() && tr.y < fr.bottom();
    let vertical_mode = cfg.anchor == EdgeAnchor::Table && !dir.horizontal() && !(v_overlap && !h_overlap);
    let (ss, ts) = if vertical_mode {
        let ss = if s_ref.1 > fr.cy() { Side::Bottom } else { Side::Top };
        let ts = if t_ref.1 > tr.cy() { Side::Bottom } else { Side::Top };
        (ss, ts)
    } else if h_overlap && waypoints.is_empty() {
        // stacked tables: loop out on the right-hand side
        (Side::Right, Side::Right)
    } else {
        let pick = |r: &Rect, x: f64, other_cx: f64| -> Side {
            if x >= r.right() {
                Side::Right
            } else if x <= r.x {
                Side::Left
            } else if other_cx >= r.cx() {
                Side::Right
            } else {
                Side::Left
            }
        };
        (pick(fr, s_ref.0, tr.cx()), pick(tr, t_ref.0, fr.cx()))
    };
    Ends { s: point_on(fr, ss, fy), t: point_on(tr, ts, ty), ss, ts, fy, ty }
}

#[derive(Debug, Clone)]
pub struct Routed {
    pub d: String,
    pub label: Pt,
}

fn add(a: Pt, b: Pt, k: f64) -> Pt {
    (a.0 + b.0 * k, a.1 + b.1 * k)
}

fn anchor_y(node: &Node, r: &Rect, cols: &[String], anchor: EdgeAnchor) -> f64 {
    if anchor == EdgeAnchor::Column {
        if let Some(y) = cols.first().and_then(|c| node.row_center(c)) {
            return r.y + y;
        }
        return r.y + metrics::HEADER_H / 2.0;
    }
    r.cy()
}

fn point_on(r: &Rect, side: Side, y: f64) -> Pt {
    match side {
        Side::Left => (r.x, y),
        Side::Right => (r.right(), y),
        Side::Top => (r.cx(), r.y),
        Side::Bottom => (r.cx(), r.bottom()),
    }
}

fn lane_offset(id: &str) -> f64 {
    let mut h: u32 = 7;
    for b in id.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u32);
    }
    ((h % 7) as f64 - 3.0) * 4.0
}

#[allow(clippy::too_many_arguments)]
pub fn route(edge: &Edge, from: &Node, fr: &Rect, to: &Node, tr: &Rect, waypoints: &[Pt], cfg: &EdgeConfig, dir: Direction) -> Routed {
    let e = ends(edge, from, fr, to, tr, waypoints, cfg, dir);
    let (fy, ty) = (e.fy, e.ty);
    if edge.from == edge.to {
        return self_loop(fr, fy, ty, cfg.style);
    }
    let (s, t, ss, ts) = (e.s, e.t, e.ss, e.ts);
    let (ns, nt) = (ss.normal(), ts.normal());

    match cfg.style {
        EdgeStyle::Straight | EdgeStyle::Hidden => {
            let mut pts = vec![s, add(s, ns, STUB)];
            pts.extend_from_slice(waypoints);
            pts.push(add(t, nt, STUB));
            pts.push(t);
            Routed { d: polyline(&pts), label: midpoint(&pts) }
        }
        EdgeStyle::Curved => {
            if waypoints.is_empty() {
                let dist = if ss.horizontal() { (t.0 - s.0).abs() } else { (t.1 - s.1).abs() };
                let mut k = (dist / 2.0).clamp(30.0, 180.0);
                if ss == ts {
                    k = k.max(40.0 + (if ss.horizontal() { (t.1 - s.1).abs() } else { (t.0 - s.0).abs() }) * 0.15);
                }
                let (c1, c2) = if ss == ts && ss.horizontal() {
                    // same side: push both control points past the outermost border
                    let x = if ss == Side::Right { s.0.max(t.0) + k } else { s.0.min(t.0) - k };
                    ((x, s.1), (x, t.1))
                } else {
                    (add(s, ns, k), add(t, nt, k))
                };
                let d = format!("M{} C{} {} {}", p(s), p(c1), p(c2), p(t));
                let mid = bezier(s, c1, c2, t, 0.5);
                Routed { d, label: mid }
            } else {
                let mut pts = vec![s, add(s, ns, STUB * 2.5)];
                pts.extend_from_slice(waypoints);
                pts.push(add(t, nt, STUB * 2.5));
                pts.push(t);
                Routed { d: catmull_rom(&pts), label: midpoint(&pts) }
            }
        }
        EdgeStyle::Orthogonal => {
            let lane = lane_offset(&edge.id);
            let s1 = add(s, ns, STUB);
            let t1 = add(t, nt, STUB);
            let mut pts = vec![s, s1];
            if waypoints.is_empty() {
                if ss == ts && ss.horizontal() {
                    let x = if ss == Side::Right { s1.0.max(t1.0) + 16.0 + lane.abs() } else { s1.0.min(t1.0) - 16.0 - lane.abs() };
                    pts.push((x, s1.1));
                    pts.push((x, t1.1));
                } else if ss == ts {
                    let y = if ss == Side::Bottom { s1.1.max(t1.1) + 16.0 } else { s1.1.min(t1.1) - 16.0 };
                    pts.push((s1.0, y));
                    pts.push((t1.0, y));
                } else {
                    connect(&mut pts, s1, t1, ss.horizontal(), lane);
                }
            } else {
                let mut prev = s1;
                let horizontal = ss.horizontal();
                for &w in waypoints {
                    connect(&mut pts, prev, w, horizontal, lane);
                    pts.push(w);
                    prev = w;
                }
                connect(&mut pts, prev, t1, horizontal, lane);
            }
            pts.push(t1);
            pts.push(t);
            let pts = simplify(&pts);
            Routed { d: rounded(&pts), label: midpoint(&pts) }
        }
    }
}

/// Manhattan points of a self loop leaving and re-entering on the right.
pub(crate) fn loop_points(r: &Rect, fy: f64, ty: f64, bulge: f64) -> Vec<Pt> {
    let ty = if (ty - fy).abs() < 1.0 { fy + metrics::ROW_H * 0.6 } else { ty };
    let x = r.right();
    vec![(x, fy), (x + STUB, fy), (x + bulge, fy), (x + bulge, ty), (x + STUB, ty), (x, ty)]
}

fn self_loop(r: &Rect, fy: f64, ty: f64, style: EdgeStyle) -> Routed {
    let ty = if (ty - fy).abs() < 1.0 { fy + metrics::ROW_H * 0.6 } else { ty };
    let x = r.right();
    let bulge = 34.0 + (ty - fy).abs() * 0.08;
    let s = (x, fy);
    let t = (x, ty);
    match style {
        EdgeStyle::Curved => {
            let d = format!("M{} C{} {} {}", p(s), p((x + bulge * 1.3, fy)), p((x + bulge * 1.3, ty)), p(t));
            Routed { d, label: (x + bulge, (fy + ty) / 2.0) }
        }
        _ => {
            let pts = vec![s, (x + STUB, fy), (x + bulge, fy), (x + bulge, ty), (x + STUB, ty), t];
            let pts = simplify(&pts);
            Routed { d: rounded(&pts), label: (x + bulge, (fy + ty) / 2.0) }
        }
    }
}

/// Append Manhattan corner points between `a` and `b`.
fn connect(pts: &mut Vec<Pt>, a: Pt, b: Pt, horizontal_first: bool, lane: f64) {
    if horizontal_first {
        if (a.1 - b.1).abs() < 0.5 {
            return;
        }
        let mx = (a.0 + b.0) / 2.0 + lane;
        pts.push((mx, a.1));
        pts.push((mx, b.1));
    } else {
        if (a.0 - b.0).abs() < 0.5 {
            return;
        }
        let my = (a.1 + b.1) / 2.0 + lane;
        pts.push((a.0, my));
        pts.push((b.0, my));
    }
}

pub(crate) fn simplify(pts: &[Pt]) -> Vec<Pt> {
    let mut out: Vec<Pt> = Vec::with_capacity(pts.len());
    for &q in pts {
        if let Some(&l) = out.last() {
            if (l.0 - q.0).abs() < 0.01 && (l.1 - q.1).abs() < 0.01 {
                continue;
            }
        }
        // drop collinear middle points
        if out.len() >= 2 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let cross = (b.0 - a.0) * (q.1 - a.1) - (b.1 - a.1) * (q.0 - a.0);
            let dot = (b.0 - a.0) * (q.0 - b.0) + (b.1 - a.1) * (q.1 - b.1);
            if cross.abs() < 0.01 && dot >= 0.0 {
                out.pop();
            }
        }
        out.push(q);
    }
    out
}

pub fn p(pt: Pt) -> String {
    format!("{:.1},{:.1}", pt.0, pt.1)
}

fn polyline(pts: &[Pt]) -> String {
    let mut d = format!("M{}", p(pts[0]));
    for &q in &pts[1..] {
        d.push_str(&format!(" L{}", p(q)));
    }
    d
}

fn rounded(pts: &[Pt]) -> String {
    if pts.len() < 3 {
        return polyline(pts);
    }
    let mut d = format!("M{}", p(pts[0]));
    for i in 1..pts.len() - 1 {
        let (a, b, c) = (pts[i - 1], pts[i], pts[i + 1]);
        let l1 = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let l2 = ((c.0 - b.0).powi(2) + (c.1 - b.1).powi(2)).sqrt();
        let r = RADIUS.min(l1 / 2.0).min(l2 / 2.0);
        if r < 0.5 {
            d.push_str(&format!(" L{}", p(b)));
            continue;
        }
        let p1 = (b.0 + (a.0 - b.0) / l1 * r, b.1 + (a.1 - b.1) / l1 * r);
        let p2 = (b.0 + (c.0 - b.0) / l2 * r, b.1 + (c.1 - b.1) / l2 * r);
        d.push_str(&format!(" L{} Q{} {}", p(p1), p(b), p(p2)));
    }
    d.push_str(&format!(" L{}", p(pts[pts.len() - 1])));
    d
}

fn catmull_rom(pts: &[Pt]) -> String {
    let mut d = format!("M{}", p(pts[0]));
    for i in 0..pts.len() - 1 {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let p1 = pts[i];
        let p2 = pts[i + 1];
        let p3 = if i + 2 < pts.len() { pts[i + 2] } else { p2 };
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        d.push_str(&format!(" C{} {} {}", p(c1), p(c2), p(p2)));
    }
    d
}

fn bezier(a: Pt, b: Pt, c: Pt, d: Pt, t: f64) -> Pt {
    let u = 1.0 - t;
    let f = |a: f64, b: f64, c: f64, d: f64| u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d;
    (f(a.0, b.0, c.0, d.0), f(a.1, b.1, c.1, d.1))
}

pub(crate) fn midpoint(pts: &[Pt]) -> Pt {
    let total: f64 = pts.windows(2).map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt()).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let l = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
        if acc + l >= total / 2.0 && l > 0.0 {
            let t = (total / 2.0 - acc) / l;
            return (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t);
        }
        acc += l;
    }
    pts[0]
}
