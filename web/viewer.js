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
    this.selected = null;
    this.hits = [];
    this.pointers = new Map();
    this.drag = null;
    container.classList.add("sch-viewer");
    if (getComputedStyle(container).position === "static") container.style.position = "relative";
    this._bind();
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
    if (this.selected && this.nodes.has(this.selected)) this.nodes.get(this.selected).el.classList.add("sv-selected");
    else this.selected = null;
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
    var bw = this.dataW + MARGIN * 2, bh = this.dataH + MARGIN * 2;
    var k = Math.min((s.w - pad * 2) / bw, (s.h - pad * 2) / bh, 1.25);
    this.k = Math.max(0.03, k);
    this.tx = (s.w - this.dataW * this.k) / 2;
    this.ty = (s.h - this.dataH * this.k) / 2;
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
    if (this.selected && this.nodes.has(this.selected)) this.nodes.get(this.selected).el.classList.remove("sv-selected");
    this.selected = id && this.nodes.has(id) ? id : null;
    if (this.selected) this.nodes.get(this.selected).el.classList.add("sv-selected");
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

  // ---- interaction ------------------------------------------------------
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
      self.drag = {
        mode: id && self.opts.draggable !== false ? "node-pending" : "pan-pending",
        id: id, sx: e.clientX, sy: e.clientY, tx0: self.tx, ty0: self.ty,
        nx: id ? self.nodes.get(id).x : 0, ny: id ? self.nodes.get(id).y : 0,
        target: e.target,
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
      if (d.mode === "node-pending" && Math.hypot(dx, dy) > 4) { d.mode = "node"; el.classList.add("sch-dragging"); }
      if (d.mode === "pan-pending" && Math.hypot(dx, dy) > 3) { d.mode = "pan"; el.classList.add("sch-panning"); }
      if (d.mode === "pan") {
        self.tx = d.tx0 + dx;
        self.ty = d.ty0 + dy;
        self._apply();
      } else if (d.mode === "node") {
        var n = self.nodes.get(d.id);
        n.x = Math.round(d.nx + dx / self.k);
        n.y = Math.round(d.ny + dy / self.k);
        n.el.setAttribute("transform", "translate(" + n.x + "," + n.y + ")");
        if (!self._moveRaf) {
          self._moveRaf = requestAnimationFrame(function () {
            self._moveRaf = null;
            if (self.opts.onNodeMove) self.updateEdges(self.opts.onNodeMove(d.id, n.x, n.y));
          });
        }
        self._scheduleMinimap();
      }
    });
    function end(e) {
      self.pointers.delete(e.pointerId);
      var d = self.drag;
      if (!d) return;
      if (d.mode === "pinch") { if (self.pointers.size === 0) self.drag = null; return; }
      self.drag = null;
      el.classList.remove("sch-dragging", "sch-panning");
      if (e.type === "pointercancel") return;
      if (d.mode === "node") {
        var n = self.nodes.get(d.id);
        if (self.opts.onNodeMove) self.updateEdges(self.opts.onNodeMove(d.id, n.x, n.y));
        if (self.opts.onNodeDrop) self.opts.onNodeDrop(d.id, n.x, n.y);
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
      } else if (d.mode === "pan-pending") {
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
      if (self._hover) { self._hover = null; self.highlight(null); if (self.opts.onHover) self.opts.onHover(null); }
    });
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
