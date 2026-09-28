//! Deterministic Fruchterman–Reingold layout with box-aware overlap removal.
use std::collections::HashMap;

use super::{Block, BlockResult, Pt};
use crate::config::LayoutConfig;

pub(crate) fn layout(b: &Block, cfg: &LayoutConfig) -> BlockResult {
    let n = b.sizes.len();
    let mut pos: Vec<Pt> = Vec::with_capacity(n);
    if n == 0 {
        return BlockResult { pos, waypoints: HashMap::new() };
    }
    let radius: Vec<f64> = b.sizes.iter().map(|s| (s.0 * s.0 + s.1 * s.1).sqrt() / 2.0).collect();
    let avg_r = radius.iter().sum::<f64>() / n as f64;
    let k = avg_r * 0.9 + cfg.rank_spacing * 0.3;

    // start on a golden-angle spiral, highest degree in the middle
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &c| b.degree_rank[c].partial_cmp(&b.degree_rank[a]).unwrap().then(b.names[a].cmp(b.names[c])));
    pos.resize(n, (0.0, 0.0));
    for (i, &v) in order.iter().enumerate() {
        let r = k * 0.6 * (i as f64).sqrt();
        let a = i as f64 * 2.399_963;
        pos[v] = (r * a.cos(), r * a.sin());
    }
    let mut disp = vec![(0.0f64, 0.0f64); n];
    let iters = cfg.iterations.clamp(10, 2000);
    let mut temp = k * (n as f64).sqrt();
    let cool = temp / iters as f64;
    for _ in 0..iters {
        for d in disp.iter_mut() {
            *d = (0.0, 0.0);
        }
        for i in 0..n {
            for j in i + 1..n {
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let dist = (dx * dx + dy * dy).sqrt().max(0.01);
                // subtract radii so big tables push harder
                let eff = (dist - (radius[i] + radius[j]) * 0.5).max(k * 0.1);
                let f = k * k / eff;
                let (ux, uy) = (dx / dist, dy / dist);
                disp[i].0 += ux * f;
                disp[i].1 += uy * f;
                disp[j].0 -= ux * f;
                disp[j].1 -= uy * f;
            }
        }
        for &(u, v) in &b.edges {
            if u == v {
                continue;
            }
            let dx = pos[u].0 - pos[v].0;
            let dy = pos[u].1 - pos[v].1;
            let dist = (dx * dx + dy * dy).sqrt().max(0.01);
            let eff = (dist - (radius[u] + radius[v]) * 0.5).max(0.0);
            let f = eff * eff / k;
            let (ux, uy) = (dx / dist, dy / dist);
            disp[u].0 -= ux * f;
            disp[u].1 -= uy * f;
            disp[v].0 += ux * f;
            disp[v].1 += uy * f;
        }
        for i in 0..n {
            // mild gravity keeps components compact
            // stronger vertical pull: screens are wider than tall
            disp[i].0 -= pos[i].0 * 0.06;
            disp[i].1 -= pos[i].1 * 0.14;
            let (dx, dy) = disp[i];
            let len = (dx * dx + dy * dy).sqrt().max(0.01);
            let step = len.min(temp);
            pos[i].0 += dx / len * step;
            pos[i].1 += dy / len * step;
        }
        temp = (temp - cool).max(1.0);
    }
    // convert centres to top-left corners
    let mut tl: Vec<Pt> = pos.iter().zip(&b.sizes).map(|(p, s)| (p.0 - s.0 / 2.0, p.1 - s.1 / 2.0)).collect();
    remove_overlaps(&mut tl, &b.sizes, cfg.node_spacing);
    BlockResult { pos: tl, waypoints: HashMap::new() }
}

/// Push apart overlapping boxes along the axis of least overlap until none
/// remain (or an iteration cap is hit, followed by a guaranteed sweep).
pub(crate) fn remove_overlaps(pos: &mut [Pt], sizes: &[Pt], gap: f64) {
    let n = pos.len();
    for _ in 0..200 {
        let mut any = false;
        for i in 0..n {
            for j in i + 1..n {
                let (ax0, ay0) = pos[i];
                let (bx0, by0) = pos[j];
                let ox = (ax0 + sizes[i].0 + gap).min(bx0 + sizes[j].0 + gap) - ax0.max(bx0);
                let oy = (ay0 + sizes[i].1 + gap).min(by0 + sizes[j].1 + gap) - ay0.max(by0);
                if ox > 0.0 && oy > 0.0 {
                    any = true;
                    let ci = (ax0 + sizes[i].0 / 2.0, ay0 + sizes[i].1 / 2.0);
                    let cj = (bx0 + sizes[j].0 / 2.0, by0 + sizes[j].1 / 2.0);
                    if ox < oy {
                        let s = if ci.0 <= cj.0 { -1.0 } else { 1.0 };
                        pos[i].0 += s * ox / 2.0;
                        pos[j].0 -= s * ox / 2.0;
                    } else {
                        let s = if ci.1 <= cj.1 { -1.0 } else { 1.0 };
                        pos[i].1 += s * oy / 2.0;
                        pos[j].1 -= s * oy / 2.0;
                    }
                }
            }
        }
        if !any {
            return;
        }
    }
    // fallback: sweep along x, guaranteeing no overlap
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| pos[a].0.partial_cmp(&pos[b].0).unwrap());
    for k in 0..order.len() {
        let i = order[k];
        loop {
            let mut moved = false;
            for &j in &order[..k] {
                let ox = (pos[i].0 + sizes[i].0 + gap).min(pos[j].0 + sizes[j].0 + gap) - pos[i].0.max(pos[j].0);
                let oy = (pos[i].1 + sizes[i].1 + gap).min(pos[j].1 + sizes[j].1 + gap) - pos[i].1.max(pos[j].1);
                if ox > 0.0 && oy > 0.0 {
                    pos[i].0 = pos[j].0 + sizes[j].0 + gap;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
    }
}
