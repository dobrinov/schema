/* schema viewer: pan / zoom / drag / hover / minimap over the SVG
 * produced by the Rust renderer. Shared by the app and the embed bundle. */
(function (global) {
  "use strict";
  var MARGIN = 40;

  function parseTranslate(t) {
    var m = /translate\(\s*([-\d.e]+)[ ,]+([-\d.e]+)\s*\)/.exec(t || "");
    return m ? [parseFloat(m[1]), parseFloat(m[2])] : [0, 0];
  }

  function SchemaViewer(container, opts) {
    this.el = container;
    this.opts = opts || {};
    this.k = 1;
    this.tx = 0;
    this.ty = 0;
    this.svg = null;
    this.vp = null;
    this.nodes = new Map(); // id -> {el, x, y, w, h}
    this.edges = new Map(); // id -> {el, from, to}
    this.adj = new Map(); // node id -> [edge ids]
    this.selected = null; // the primary selection (details panel)
    this.sel = new Set(); // every selected table (pointer tool: several)
    this.tool = null; // "pointer" | "hand" | null (drag the background to pan, a table to move it)
    this.hits = [];
    this.pointers = new Map();
    this.drag = null;
    container.classList.add("sch-viewer");
    if (getComputedStyle(container).position === "static") container.style.position = "relative";
    this._bind();
    if (this.opts.tool) this.setTool(this.opts.tool);
    if (this.opts.minimap !== false) this._initMinimap();
  }

  var P = SchemaViewer.prototype;

  P.setContent = function (svgMarkup, info, o) {
    o = o || {};
    var prev = this.svg ? { k: this.k, tx: this.tx, ty: this.ty } : null;
    if (this.svg) this.svg.remove();
    var tmp = document.createElement("div");
    tmp.innerHTML = svgMarkup;
    var svg = tmp.querySelector("svg");
    this.el.insertBefore(svg, this.el.firstChild);
    this.adopt(svg, info);
    if (o.preserveView && prev) {
      this.k = prev.k;
      this.tx = prev.tx;
      this.ty = prev.ty;
      this._apply();
    } else {
      this.fit();
    }
  };

  /** Take over an SVG already in the DOM (static embeds). */
  P.adopt = function (svg, info) {
    this.svg = svg;
    this.dataW = parseFloat(svg.getAttribute("data-width")) || 800;
    this.dataH = parseFloat(svg.getAttribute("data-height")) || 600;
    svg.removeAttribute("viewBox");
    svg.setAttribute("width", "100%");
    svg.setAttribute("height", "100%");
    svg.style.display = "block";
    svg.style.touchAction = "none";
    svg.style.userSelect = "none";
    this.vp = svg.querySelector(".sv-viewport");
    // native <title> tooltips are slow and unstyled: keep the text in a <desc>
    // (no native tooltip) and show it in our own tooltip; exports restore <title>
    svg.querySelectorAll("title").forEach(function (t) {
      var d = document.createElementNS("http://www.w3.org/2000/svg", "desc");
      d.setAttribute("class", "sv-tip");
      d.textContent = t.textContent;
      t.parentNode.replaceChild(d, t);
    });
    this._hideTip();
    this.nodes.clear();
    this.edges.clear();
    this.adj.clear();
    var self = this;
    var byId = new Map();
    if (info && info.nodes) info.nodes.forEach(function (n) { byId.set(n.id, n); });
    svg.querySelectorAll(".sv-node").forEach(function (el) {
      var id = el.getAttribute("data-id");
      var n = byId.get(id);
      var t = parseTranslate(el.getAttribute("transform"));
      var body = el.querySelector(".sv-body");
      self.nodes.set(id, {
        el: el,
        x: n ? n.x : t[0],
        y: n ? n.y : t[1],
        w: n ? n.w : parseFloat(body.getAttribute("width")),
        h: n ? n.h : parseFloat(body.getAttribute("height")),
        status: n ? n.status : (el.getAttribute("class").match(/sv-st-(\w+)/) || [])[1] || "unchanged",
      });
      self.adj.set(id, []);
    });
    svg.querySelectorAll(".sv-edge").forEach(function (el) {
      var id = el.getAttribute("data-id");
      var e = { el: el, from: el.getAttribute("data-from"), to: el.getAttribute("data-to") };
      self.edges.set(id, e);
      if (self.adj.has(e.from)) self.adj.get(e.from).push(id);
      if (self.adj.has(e.to) && e.to !== e.from) self.adj.get(e.to).push(id);
    });
    this.sel.forEach(function (id) { if (self.nodes.has(id)) self.nodes.get(id).el.classList.add("sv-selected"); else self.sel.delete(id); });
    if (!this.sel.has(this.selected)) this.selected = null;
    this.setSearchHits(this.hits);
    this.setTheme(this._dark);
    this._apply();
  };

  P.setTheme = function (dark) {
    this._dark = !!dark;
    if (this.svg) this.svg.classList.toggle("sv-dark", this._dark);
    this._drawMinimap();
  };

  P._apply = function () {
    if (this.vp) this.vp.setAttribute("transform", "translate(" + this.tx.toFixed(2) + "," + this.ty.toFixed(2) + ") scale(" + this.k.toFixed(4) + ")");
    if (this.opts.onZoom) this.opts.onZoom(this.k);
    this._scheduleMinimap();
  };

  P.size = function () {
    var r = this.el.getBoundingClientRect();
    return { w: r.width || 800, h: r.height || 600 };
  };

  P.fit = function (pad) {
    pad = pad == null ? 24 : pad;
    var s = this.size();
    // room the host's overlays take (e.g. a tool bar along the bottom)
    var ins = this.opts.fitInsets || {}, t = ins.top || 0, b = ins.bottom || 0, l = ins.left || 0, r = ins.right || 0;
    var w = s.w - l - r, h = s.h - t - b;
    var bw = this.dataW + MARGIN * 2, bh = this.dataH + MARGIN * 2;
    var k = Math.min((w - pad * 2) / bw, (h - pad * 2) / bh, 1.25);
    this.k = Math.max(0.03, k);
    this.tx = l + (w - this.dataW * this.k) / 2;
    this.ty = t + (h - this.dataH * this.k) / 2;
    this._size = s;
    this._apply();
  };

  P.zoomBy = function (f, cx, cy) {
    var s = this.size();
    if (cx == null) { cx = s.w / 2; cy = s.h / 2; }
    var k = Math.min(4, Math.max(0.03, this.k * f));
    f = k / this.k;
    this.tx = cx - (cx - this.tx) * f;
    this.ty = cy - (cy - this.ty) * f;
    this.k = k;
    this._apply();
  };

  P.setZoom = function (k) { this.zoomBy(k / this.k); };

  P._animateTo = function (k, tx, ty) {
    var self = this, k0 = this.k, x0 = this.tx, y0 = this.ty, t0 = performance.now(), dur = 260;
    if (this._anim) cancelAnimationFrame(this._anim);
    function step(now) {
      var t = Math.min(1, (now - t0) / dur);
      var e = t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2;
      self.k = k0 + (k - k0) * e;
      self.tx = x0 + (tx - x0) * e;
      self.ty = y0 + (ty - y0) * e;
      self._apply();
      if (t < 1) self._anim = requestAnimationFrame(step);
    }
    this._anim = requestAnimationFrame(step);
  };

  P.centerOn = function (id, o) {
    o = o || {};
    var n = this.nodes.get(id);
    if (!n) return false;
    var s = this.size();
    var k = o.zoom || Math.max(this.k, Math.min(1, Math.min(s.w / (n.w * 3), s.h / (n.h * 2))));
    var tx = s.w / 2 - (n.x + n.w / 2) * k;
    var ty = s.h / 2 - (n.y + n.h / 2) * k;
    if (o.animate === false) { this.k = k; this.tx = tx; this.ty = ty; this._apply(); } else this._animateTo(k, tx, ty);
    return true;
  };

  P.select = function (id) {
    this.setSelection(id ? [id] : [], id);
  };

  /** Select several tables; `primary` (one of them, or none) is the one the details are about. */
  P.setSelection = function (ids, primary) {
    var self = this;
    this.sel.forEach(function (id) { var n = self.nodes.get(id); if (n) n.el.classList.remove("sv-selected"); });
    this.sel = new Set((ids || []).filter(function (id) { return self.nodes.has(id); }));
    this.sel.forEach(function (id) { self.nodes.get(id).el.classList.add("sv-selected"); });
    this.selected = primary && this.sel.has(primary) ? primary : null;
  };

  P.selection = function () { return Array.from(this.sel); };

  /** "pointer": drag the background to select, drag tables to move them; hold Space to pan.
   *  "hand": drag anywhere to pan. */
  P.setTool = function (tool) {
    this.tool = tool;
    this._syncTool();
  };

  P._effectiveTool = function () { return this.tool === "pointer" && this._space ? "hand" : this.tool; };

  P._syncTool = function () {
    var t = this._effectiveTool();
    this.el.classList.toggle("sch-tool-pointer", t === "pointer");
    this.el.classList.toggle("sch-tool-hand", t === "hand");
  };

  P._notifySelection = function () {
    if (this.opts.onSelectionChange) this.opts.onSelectionChange(this.selection(), this.selected);
  };

  // the tables a rectangle (diagram coordinates) touches
  P._nodesIn = function (x0, y0, x1, y1) {
    var out = [];
    this.nodes.forEach(function (n, id) {
      if (n.x < x1 && n.x + n.w > x0 && n.y < y1 && n.y + n.h > y0) out.push(id);
    });
    return out;
  };

  P.setSearchHits = function (ids) {
    var self = this;
    this.hits = ids || [];
    this.nodes.forEach(function (n) { n.el.classList.remove("sv-search-hit"); });
    this.hits.forEach(function (id) { var n = self.nodes.get(id); if (n) n.el.classList.add("sv-search-hit"); });
  };

  /** Highlight a node and its relations, dimming everything else. */
  P.highlight = function (id) {
    if (!this.svg) return;
    var self = this;
    this.svg.querySelectorAll(".sv-hl").forEach(function (e) { e.classList.remove("sv-hl"); });
    if (!id) { this.svg.classList.remove("sv-hovering"); return; }
    this.svg.classList.add("sv-hovering");
    var ids = Array.isArray(id) ? id : [id];
    ids.forEach(function (nid) {
      var n = self.nodes.get(nid);
      if (!n) return;
      n.el.classList.add("sv-hl");
      (self.adj.get(nid) || []).forEach(function (eid) {
        var e = self.edges.get(eid);
        e.el.classList.add("sv-hl");
        [e.from, e.to].forEach(function (o) { var m = self.nodes.get(o); if (m) m.el.classList.add("sv-hl"); });
      });
    });
  };

  P._highlightEdge = function (eid) {
    var e = this.edges.get(eid);
    if (!e) return;
    this.highlight(null);
    this.svg.classList.add("sv-hovering");
    e.el.classList.add("sv-hl");
    var self = this;
    [e.from, e.to].forEach(function (o) { var m = self.nodes.get(o); if (m) m.el.classList.add("sv-hl"); });
  };

  P.updateEdges = function (list) {
    var self = this;
    (list || []).forEach(function (u) {
      var e = self.edges.get(u.id);
      if (!e) return;
      e.el.querySelectorAll("path").forEach(function (p) { p.setAttribute("d", u.d); });
      var label = e.el.querySelector(".sv-edge-label");
      if (label && u.label) { label.setAttribute("x", u.label[0]); label.setAttribute("y", u.label[1] - 4); }
    });
  };

  P.toDiagram = function (clientX, clientY) {
    var r = this.el.getBoundingClientRect();
    return [(clientX - r.left - this.tx) / this.k, (clientY - r.top - this.ty) / this.k];
  };

  /** Standalone SVG markup of the whole diagram (for export). */
  P.exportSvg = function () {
    var clone = this.svg.cloneNode(true);
    var w = this.dataW + MARGIN * 2, h = this.dataH + MARGIN * 2;
    clone.setAttribute("viewBox", -MARGIN + " " + -MARGIN + " " + w + " " + h);
    clone.setAttribute("width", w);
    clone.setAttribute("height", h);
    clone.removeAttribute("style");
    clone.classList.remove("sv-hovering");
    clone.querySelectorAll(".sv-hl,.sv-selected,.sv-search-hit").forEach(function (e) { e.classList.remove("sv-hl", "sv-selected", "sv-search-hit"); });
    clone.querySelectorAll("desc.sv-tip").forEach(function (d) {
      var t = document.createElementNS("http://www.w3.org/2000/svg", "title");
      t.textContent = d.textContent;
      d.parentNode.replaceChild(t, d);
    });
    var vp = clone.querySelector(".sv-viewport");
    if (vp) vp.removeAttribute("transform");
    return new XMLSerializer().serializeToString(clone);
  };

  P.exportPng = function (scale) {
    scale = scale || 2;
    var markup = this.exportSvg();
    var w = (this.dataW + MARGIN * 2) * scale, h = (this.dataH + MARGIN * 2) * scale;
    return new Promise(function (resolve, reject) {
      var img = new Image();
      img.onload = function () {
        var c = document.createElement("canvas");
        c.width = Math.min(w, 16000);
        c.height = Math.min(h, 16000);
        var ctx = c.getContext("2d");
        ctx.drawImage(img, 0, 0, c.width, c.height);
        c.toBlob(function (b) { b ? resolve(b) : reject(new Error("PNG export failed")); }, "image/png");
      };
      img.onerror = function () { reject(new Error("PNG export failed")); };
      img.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(markup);
    });
  };

  // ---- tooltip ----------------------------------------------------------
  // The element a tooltip belongs to: the row under the pointer if it has one,
  // else the node (header, tags) or the edge. Rows without text get nothing.
  P._tipTarget = function (target) {
    var el = target;
    while (el && el !== this.svg && el.nodeType === 1) {
      var kids = el.children, tip = null;
      for (var i = 0; i < kids.length; i++) { if (kids[i].tagName === "desc" && kids[i].classList.contains("sv-tip")) { tip = kids[i]; break; } }
      if (tip) return { el: el, text: tip.textContent };
      if (el.classList.contains("sv-row")) return null;
      el = el.parentNode;
    }
    return null;
  };
  function tipHtml(text) {
    var lines = text.split("\n").filter(function (l) { return l.trim(); });
    var escape = function (s) { return s.replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; }); };
    var h = "<b>" + escape(lines[0] || "") + "</b>";
    lines.slice(1).forEach(function (l) {
      var m = /^(→|←|primary key|unique index|unique|index|default:|identity:|generated:|enum |changed:|inferred:|\+|−|~)/.exec(l);
      var glyph = !m ? "" : m[1] === "→" || m[1] === "←" ? m[1] : m[1] === "primary key" ? "PK" : m[1] === "unique" ? "UQ" : /index/.test(m[1]) ? "IX" : m[1] === "default:" ? "=" : "";
      var body = m && (m[1] === "→" || m[1] === "←") ? l.slice(1).trim() : l;
      h += "<div class=\"l" + (m ? "" : " note") + "\"><i>" + glyph + "</i><span>" + escape(body) + "</span></div>";
    });
    return h;
  }
  P._showTip = function (hit, e) {
    var self = this;
    if (!this.tip) {
      this.tip = document.createElement("div");
      this.tip.className = "sch-tip sch-overlay";
      this.tip.hidden = true;
      this.el.appendChild(this.tip);
    }
    clearTimeout(this._tipT);
    var place = function () {
      var tip = self.tip, c = self.el.getBoundingClientRect();
      tip.innerHTML = tipHtml(hit.text);
      tip.hidden = false;
      tip.classList.remove("above");
      var r = hit.el.getBoundingClientRect();
      var onEdge = hit.el.classList.contains("sv-edge");
      // where the arrow points (client coords): a little into wide elements, the middle of small ones
      var ax = onEdge ? e.clientX : r.left + Math.min(24, r.width / 2);
      var ay = onEdge ? e.clientY + 10 : r.bottom;
      var w = tip.offsetWidth, h = tip.offsetHeight;
      var left = Math.max(8, Math.min(ax - c.left - 18, c.width - w - 8));
      var top = ay - c.top + 8;
      if (top + h > c.height - 8 && (onEdge ? e.clientY - 10 : r.top) - c.top - h - 8 > 0) {
        top = (onEdge ? e.clientY - 10 : r.top) - c.top - h - 8;
        tip.classList.add("above");
      }
      tip.style.left = left + "px";
      tip.style.top = top + "px";
      tip.style.setProperty("--ax", Math.max(10, Math.min(w - 18, ax - c.left - left)) + "px");
      self._tipFor = hit.el;
    };
    // quick to appear; instant when moving from one row to the next
    if (this.tip.hidden) this._tipT = setTimeout(place, 120); else place();
  };
  P._hideTip = function () {
    clearTimeout(this._tipT);
    this._tipFor = null;
    if (this.tip) this.tip.hidden = true;
  };

  // ---- interaction ------------------------------------------------------
  // re-route the edges of tables being moved (an edge between two of them once)
  P._moveEdges = function (group) {
    if (!this.opts.onNodeMove) return;
    var self = this, byId = new Map();
    group.forEach(function (g) {
      var n = self.nodes.get(g.id);
      (self.opts.onNodeMove(g.id, n.x, n.y) || []).forEach(function (u) { byId.set(u.id, u); });
    });
    this.updateEdges(Array.from(byId.values()));
  };

  // the selection rectangle; shift / ⌘ / ctrl adds to the selection
  P._drawMarquee = function (d, e) {
    if (!this.marquee) {
      this.marquee = document.createElement("div");
      this.marquee.className = "sch-marquee";
      this.el.appendChild(this.marquee);
    }
    var r = this.el.getBoundingClientRect();
    var x0 = Math.min(d.sx, e.clientX) - r.left, y0 = Math.min(d.sy, e.clientY) - r.top;
    var x1 = Math.max(d.sx, e.clientX) - r.left, y1 = Math.max(d.sy, e.clientY) - r.top;
    var m = this.marquee.style;
    this.marquee.hidden = false;
    m.left = x0 + "px"; m.top = y0 + "px"; m.width = x1 - x0 + "px"; m.height = y1 - y0 + "px";
    var k = this.k, tx = this.tx, ty = this.ty;
    var hit = this._nodesIn((x0 - tx) / k, (y0 - ty) / k, (x1 - tx) / k, (y1 - ty) / k);
    var ids = d.additive ? d.sel0.concat(hit.filter(function (h) { return d.sel0.indexOf(h) < 0; })) : hit;
    this.setSelection(ids, ids.length === 1 ? ids[0] : null);
  };

  P._bind = function () {
    var self = this, el = this.el;
    el.addEventListener("pointerdown", function (e) {
      if (!self.svg || e.target.closest(".sch-minimap,.sch-overlay")) return;
      if (e.target !== el && !e.target.closest("svg.sv")) return; // overlays / toolbars
      if (e.button !== 0 && e.pointerType === "mouse") return;
      self.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      try { el.setPointerCapture(e.pointerId); } catch (err) { /* synthetic or already released pointer */ }
      if (self.pointers.size === 2) {
        var p = Array.from(self.pointers.values());
        self.drag = { mode: "pinch", d0: Math.hypot(p[0].x - p[1].x, p[0].y - p[1].y), k0: self.k, tx0: self.tx, ty0: self.ty, cx: (p[0].x + p[1].x) / 2, cy: (p[0].y + p[1].y) / 2 };
        return;
      }
      var nodeEl = e.target.closest(".sv-node");
      var id = nodeEl && nodeEl.getAttribute("data-id");
      var tool = self._effectiveTool(), additive = e.shiftKey || e.metaKey || e.ctrlKey;
      var mode = id && self.opts.draggable !== false ? "node-pending" : "pan-pending";
      if (tool === "hand") mode = "pan-pending";
      else if (tool === "pointer" && !id) mode = "marquee-pending";
      // the tables a drag moves: the whole selection when the table is part of it
      var group = [];
      if (mode === "node-pending") {
        var ids = tool === "pointer" && self.sel.has(id) ? self.selection() : [id];
        group = ids.map(function (g) { var n = self.nodes.get(g); return { id: g, x: n.x, y: n.y }; });
      }
      self.drag = {
        mode: mode, id: id, sx: e.clientX, sy: e.clientY, tx0: self.tx, ty0: self.ty,
        tool: tool, group: group, additive: additive, sel0: self.selection(), target: e.target,
      };
    });
    el.addEventListener("pointermove", function (e) {
      if (self.pointers.has(e.pointerId)) self.pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      var d = self.drag;
      if (!d) return;
      if (d.mode === "pinch") {
        var p = Array.from(self.pointers.values());
        if (p.length < 2) return;
        var dist = Math.hypot(p[0].x - p[1].x, p[0].y - p[1].y);
        var r = el.getBoundingClientRect();
        var k = Math.min(4, Math.max(0.03, d.k0 * dist / d.d0)), f = k / d.k0;
        var cx = d.cx - r.left, cy = d.cy - r.top;
        self.k = k; self.tx = cx - (cx - d.tx0) * f; self.ty = cy - (cy - d.ty0) * f;
        self._apply();
        return;
      }
      var dx = e.clientX - d.sx, dy = e.clientY - d.sy;
      if (d.mode === "node-pending" && Math.hypot(dx, dy) > 4) {
        d.mode = "node";
        el.classList.add("sch-dragging");
        // dragging a table outside the selection selects it (pointer tool)
        if (self.tool && !self.sel.has(d.id)) { self.setSelection([d.id], d.id); self._notifySelection(); }
      }
      if (d.mode === "pan-pending" && Math.hypot(dx, dy) > 3) { d.mode = "pan"; el.classList.add("sch-panning"); }
      if (d.mode === "marquee-pending" && Math.hypot(dx, dy) > 3) {
        d.mode = "marquee";
        el.classList.add("sch-selecting");
        // the hover highlight would fade the tables being selected
        if (self._hover) { self._hover = null; self.highlight(null); if (self.opts.onHover) self.opts.onHover(null); }
      }
      if (d.mode === "pan") {
        self.tx = d.tx0 + dx;
        self.ty = d.ty0 + dy;
        self._apply();
      } else if (d.mode === "node") {
        d.group.forEach(function (g) {
          var n = self.nodes.get(g.id);
          n.x = Math.round(g.x + dx / self.k);
          n.y = Math.round(g.y + dy / self.k);
          n.el.setAttribute("transform", "translate(" + n.x + "," + n.y + ")");
        });
        if (!self._moveRaf) {
          self._moveRaf = requestAnimationFrame(function () {
            self._moveRaf = null;
            self._moveEdges(d.group);
          });
        }
        self._scheduleMinimap();
      } else if (d.mode === "marquee") {
        self._drawMarquee(d, e);
      }
    });
    function end(e) {
      self.pointers.delete(e.pointerId);
      var d = self.drag;
      if (!d) return;
      if (d.mode === "pinch") { if (self.pointers.size === 0) self.drag = null; return; }
      self.drag = null;
      el.classList.remove("sch-dragging", "sch-panning", "sch-selecting");
      if (self.marquee) self.marquee.hidden = true;
      if (e.type === "pointercancel") return;
      // the hand tool pans from anywhere, but a click on a table still is one
      if (d.mode === "pan-pending" && d.id && d.tool === "hand") d.mode = "node-pending";
      if (d.mode === "node") {
        if (self._moveRaf) { cancelAnimationFrame(self._moveRaf); self._moveRaf = null; }
        self._moveEdges(d.group);
        var moved = d.group.map(function (g) { var n = self.nodes.get(g.id); return { id: g.id, x: n.x, y: n.y }; });
        if (self.opts.onNodesDrop) self.opts.onNodesDrop(moved);
        else if (self.opts.onNodeDrop) moved.forEach(function (m) { self.opts.onNodeDrop(m.id, m.x, m.y); });
      } else if (d.mode === "marquee") {
        self._notifySelection();
      } else if (d.mode === "node-pending" && d.tool === "pointer" && d.additive) {
        // shift / ⌘ / ctrl-click adds a table to the selection or takes it out
        var next = d.sel0.filter(function (s) { return s !== d.id; });
        if (next.length === d.sel0.length) next.push(d.id);
        self.setSelection(next, next.indexOf(d.id) >= 0 ? d.id : next[next.length - 1]);
        self._notifySelection();
      } else if (d.mode === "node-pending") {
        var row = d.target.closest(".sv-row");
        var info = { col: row && row.getAttribute("data-col"), more: !!(row && row.hasAttribute("data-more")), event: e };
        // Double clicks are detected here: with pointer capture the native
        // dblclick event targets the container, not the table.
        var now = performance.now(), last = self._lastClick;
        if (last && last.id === d.id && now - last.t < 400 && Math.hypot(e.clientX - last.x, e.clientY - last.y) < 8) {
          self._lastClick = null;
          if (self.opts.onNodeDblClick) self.opts.onNodeDblClick(d.id, e);
        } else {
          self._lastClick = { id: d.id, t: now, x: e.clientX, y: e.clientY };
          if (self.opts.onNodeClick) self.opts.onNodeClick(d.id, info);
        }
      } else if (d.mode === "pan-pending" || d.mode === "marquee-pending") {
        var edge = d.target.closest && d.target.closest(".sv-edge");
        if (edge && self.opts.onEdgeClick) self.opts.onEdgeClick(edge.getAttribute("data-id"), e);
        else if (self.opts.onBackgroundClick) self.opts.onBackgroundClick(e);
      }
    }
    el.addEventListener("pointerup", end);
    el.addEventListener("pointercancel", end);
    el.addEventListener("pointerdown", function (e) { self._downOnNode = !!(e.target.closest && e.target.closest(".sv-node")); }, true);
    el.addEventListener("dblclick", function (e) {
      // tables handle their own double clicks (see pointerup); only the background zooms
      if (self._downOnNode || !self.svg || e.target.closest(".sch-overlay,.sch-minimap")) return;
      if (e.target !== el && !e.target.closest("svg.sv")) return;
      self.zoomBy(1.6, e.clientX - el.getBoundingClientRect().left, e.clientY - el.getBoundingClientRect().top);
    });
    el.addEventListener("contextmenu", function (e) {
      if (!self.opts.onContextMenu || e.target.closest(".sch-overlay")) return;
      e.preventDefault();
      var nodeEl = e.target.closest(".sv-node");
      var row = e.target.closest(".sv-row");
      self.opts.onContextMenu(nodeEl && nodeEl.getAttribute("data-id"), row && row.getAttribute("data-col"), e);
    });
    el.addEventListener("wheel", function (e) {
      if (!self.svg || e.target.closest(".sch-overlay")) return;
      if (e.target !== el && !e.target.closest("svg.sv")) return;
      e.preventDefault();
      var r = el.getBoundingClientRect();
      var cx = e.clientX - r.left, cy = e.clientY - r.top;
      var dy = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY;
      // pinch (ctrl) or a classic mouse wheel zooms; trackpad scrolling pans
      var mouseWheel = e.deltaMode === 1 || (e.deltaX === 0 && Math.abs(dy) >= 50 && Number.isInteger(dy));
      if (e.ctrlKey || e.metaKey || mouseWheel || self.opts.wheelZoom) {
        self.zoomBy(Math.exp(-dy * (e.ctrlKey ? 0.01 : 0.0022)), cx, cy);
      } else {
        self.tx -= e.deltaX;
        self.ty -= dy;
        self._apply();
      }
    }, { passive: false });
    el.addEventListener("pointerover", function (e) {
      if (self.drag && self.drag.mode !== "node-pending" && self.drag.mode !== "pan-pending") return;
      if (self.opts.tooltips !== false && !(e.target.closest && e.target.closest(".sch-overlay,.sch-minimap"))) {
        var hit = e.target.closest && e.target.closest("svg.sv") ? self._tipTarget(e.target) : null;
        if (!hit) self._hideTip();
        else if (hit.el !== self._tipFor) self._showTip(hit, e);
      }
      var nodeEl = e.target.closest && e.target.closest(".sv-node");
      var edgeEl = !nodeEl && e.target.closest && e.target.closest(".sv-edge");
      if (nodeEl) {
        var id = nodeEl.getAttribute("data-id");
        if (self._hover !== id) { self._hover = id; self.highlight(id); if (self.opts.onHover) self.opts.onHover(id); }
      } else if (edgeEl) {
        var eid = "edge:" + edgeEl.getAttribute("data-id");
        if (self._hover !== eid) { self._hover = eid; self._highlightEdge(edgeEl.getAttribute("data-id")); }
      } else if (self._hover) {
        self._hover = null;
        self.highlight(self.opts.persistentHighlight ? self.opts.persistentHighlight() : null);
        if (self.opts.onHover) self.opts.onHover(null);
      }
    });
    el.addEventListener("pointerleave", function () {
      self._hideTip();
      if (self._hover) { self._hover = null; self.highlight(null); if (self.opts.onHover) self.opts.onHover(null); }
    });
    // anything that moves the diagram or starts an action takes the tooltip away
    el.addEventListener("pointerdown", function () { self._hideTip(); }, true);
    el.addEventListener("wheel", function () { self._hideTip(); }, { passive: true, capture: true });
    el.addEventListener("contextmenu", function () { self._hideTip(); }, true);
    // pointer tool: hold Space to pan
    var typing = function (t) { return t && (/^(input|textarea|select)$/i.test(t.tagName) || t.isContentEditable); };
    // Space belongs to the diagram when nothing else has focus, focus is in the
    // diagram, or the pointer is over it; otherwise it keeps pressing focused buttons
    el.addEventListener("pointerenter", function () { self._over = true; });
    el.addEventListener("pointerleave", function () { self._over = false; });
    var spaceIsOurs = function (t) {
      if (!t || t === document.body || t === document.documentElement || el.contains(t)) return true;
      return self._over && !(t.closest && t.closest("dialog[open],[role=menu],.menu,.popover"));
    };
    document.addEventListener("keydown", function (e) {
      if (e.code !== "Space" || self.tool !== "pointer" || typing(e.target) || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.target.closest && e.target.closest("dialog[open]")) return;
      if (!self._space && !spaceIsOurs(e.target)) return;
      e.preventDefault(); // no page scroll, no click on a focused button
      if (!self._space) { self._space = true; self._syncTool(); }
    });
    var spaceUp = function (e) {
      if (e && e.type === "keyup" && e.code !== "Space") return;
      if (self._space) { self._space = false; self._syncTool(); }
    };
    document.addEventListener("keyup", spaceUp);
    global.addEventListener("blur", spaceUp);
    if (global.ResizeObserver) {
      // keep the view centred when the container resizes (e.g. side panels)
      new ResizeObserver(function () {
        var s = self.size(), last = self._size;
        if (last && self.svg && (s.w !== last.w || s.h !== last.h)) { self.tx += (s.w - last.w) / 2; self.ty += (s.h - last.h) / 2; self._apply(); }
        self._size = s;
      }).observe(el);
    }
  };

  // ---- minimap ----------------------------------------------------------
  P._initMinimap = function () {
    var self = this;
    var c = document.createElement("canvas");
    c.className = "sch-minimap sch-overlay";
    c.width = 180 * 2;
    c.height = 120 * 2;
    this.el.appendChild(c);
    this.mini = c;
    function go(e) {
      var r = c.getBoundingClientRect();
      var m = self._miniScale;
      if (!m) return;
      var x = (e.clientX - r.left) / r.width * c.width / 2 / m.s - m.ox;
      var y = (e.clientY - r.top) / r.height * c.height / 2 / m.s - m.oy;
      var s = self.size();
      self.tx = s.w / 2 - x * self.k;
      self.ty = s.h / 2 - y * self.k;
      self._apply();
    }
    var down = false;
    c.addEventListener("pointerdown", function (e) { down = true; c.setPointerCapture(e.pointerId); go(e); e.stopPropagation(); });
    c.addEventListener("pointermove", function (e) { if (down) go(e); });
    c.addEventListener("pointerup", function () { down = false; });
  };

  P._scheduleMinimap = function () {
    var self = this;
    if (!this.mini || this._miniRaf) return;
    this._miniRaf = requestAnimationFrame(function () { self._miniRaf = null; self._drawMinimap(); });
  };

  P._drawMinimap = function () {
    var c = this.mini;
    if (!c || !this.svg) return;
    var big = this.nodes.size > 6;
    c.style.display = big && !this.opts.hideMinimap ? "" : "none";
    if (!big) return;
    var ctx = c.getContext("2d");
    var W = c.width / 2, H = c.height / 2, pad = 6;
    ctx.setTransform(2, 0, 0, 2, 0, 0);
    ctx.clearRect(0, 0, W, H);
    var s = Math.min((W - pad * 2) / Math.max(1, this.dataW), (H - pad * 2) / Math.max(1, this.dataH));
    var ox = (W / s - this.dataW) / 2, oy = (H / s - this.dataH) / 2;
    this._miniScale = { s: s, ox: ox, oy: oy };
    var dark = this._dark;
    var colors = dark ? { added: "#5cc47a", removed: "#f07a7f", modified: "#e3ae4c", unchanged: "#3a414b" }
      : { added: "#1b7f3a", removed: "#c62a31", modified: "#c58a1a", unchanged: "#c9ced5" };
    var changed = [];
    this.nodes.forEach(function (n) {
      if (n.status && n.status !== "unchanged") { changed.push(n); return; }
      ctx.fillStyle = colors.unchanged;
      ctx.fillRect((n.x + ox) * s, (n.y + oy) * s, Math.max(1.5, n.w * s), Math.max(1.5, n.h * s));
    });
    // changes stay visible however far the diagram is zoomed out
    changed.forEach(function (n) {
      var w = Math.max(5, n.w * s), h = Math.max(5, n.h * s);
      var x = (n.x + ox) * s + (n.w * s - w) / 2, y = (n.y + oy) * s + (n.h * s - h) / 2;
      ctx.fillStyle = colors[n.status];
      ctx.fillRect(x, y, w, h);
      ctx.strokeStyle = dark ? "#16191e" : "#ffffff";
      ctx.lineWidth = 1;
      ctx.strokeRect(x, y, w, h);
    });
    var v = this.size();
    ctx.strokeStyle = dark ? "#6aa6f9" : "#0a62d0";
    ctx.lineWidth = 1.5;
    ctx.strokeRect((-this.tx / this.k + ox) * s, (-this.ty / this.k + oy) * s, v.w / this.k * s, v.h / this.k * s);
  };

  global.SchemaViewer = SchemaViewer;
})(typeof window !== "undefined" ? window : this);
