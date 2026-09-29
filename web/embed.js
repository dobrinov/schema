/* schema embed API.
 *
 *   Schema.mount(element, { sql, baseSql, config, title, subtitle, toolbar })
 *   Schema.render(sql, config, baseSql) -> Promise<svg string>
 *
 * or declaratively:
 *   <div data-schema data-config='{"focus":["users"]}' style="height:500px">
 *     <script type="application/sql">CREATE TABLE ...</script>
 *   </div>
 */
(function (global) {
  "use strict";
  var ready = null;
  var cssDone = false;

  function injectCss() {
    if (cssDone || typeof SCHEMA_EMBED_CSS === "undefined" || !SCHEMA_EMBED_CSS) return;
    var s = document.createElement("style");
    s.textContent = SCHEMA_EMBED_CSS;
    document.head.appendChild(s);
    cssDone = true;
  }

  function b64ToBytes(b64) {
    var bin = atob(b64), out = new Uint8Array(bin.length);
    for (var i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  }

  function wasm() {
    if (!ready) {
      if (typeof SCHEMA_WASM_B64 === "undefined" || !SCHEMA_WASM_B64 || !SCHEMA_WASM_INIT) ready = Promise.reject(new Error("this bundle has no WASM module (static export)"));
      else ready = SCHEMA_WASM_INIT(b64ToBytes(SCHEMA_WASM_B64));
    }
    return ready;
  }

  function esc(s) { return String(s == null ? "" : s).replace(/[&<>"]/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]; }); }
  function display(id) { return id && id.indexOf("public.") === 0 ? id.slice(7) : id; }
  function isObj(v) { return v && typeof v === "object" && !Array.isArray(v); }
  function merge(a, b) {
    Object.keys(b || {}).forEach(function (k) { if (isObj(b[k]) && isObj(a[k])) merge(a[k], b[k]); else a[k] = JSON.parse(JSON.stringify(b[k])); });
    return a;
  }

  function Instance(el, opts) {
    injectCss();
    this.el = el;
    this.opts = opts || {};
    var pre = el.querySelector("svg.sv");
    el.classList.add("sch-embed");
    var dark = (opts.config && opts.config.theme === "dark") || (!opts.config || !opts.config.theme) && global.matchMedia && matchMedia("(prefers-color-scheme: dark)").matches;
    this.dark = !!dark;
    el.setAttribute("data-sch-theme", this.dark ? "dark" : "light");
    el.innerHTML = "";
    if (opts.toolbar !== false) {
      this.bar = document.createElement("div");
      this.bar.className = "sch-bar";
      el.appendChild(this.bar);
    }
    this.stage = document.createElement("div");
    this.stage.className = "sch-stage";
    el.appendChild(this.stage);
    this.info = document.createElement("div");
    this.info.className = "sch-info sch-overlay";
    this.info.hidden = true;
    this.stage.appendChild(this.info);
    var self = this;
    this.viewer = new global.SchemaViewer(this.stage, {
      draggable: !!opts.sql,
      onNodeMove: function (id, x, y) { return self.viz ? JSON.parse(self.viz.move_node(id, x, y)) : []; },
      onNodeDrop: function (id, x, y) { if (self.cfg) { self.cfg.positions[id] = [x, y]; self.render(false); } },
      onNodeClick: function (id) { self.showInfo(id); },
      onNodeDblClick: function (id) { if (self.viz) self.toggleFocus(id); },
      onBackgroundClick: function () { self.info.hidden = true; self.viewer.select(null); },
      persistentHighlight: function () { return self.viewer.selected; },
    });
    this.viewer.setTheme(this.dark);
    if (pre) {
      this.stage.insertBefore(pre, this.stage.firstChild);
      this.viewer.adopt(pre, null);
      this.viewer.fit();
    }
    this.cfg = null;
    if (opts.sql) {
      this.ready = wasm().then(function (wb) {
        self.wb = wb;
        self.viz = new wb.Schema();
        self.viz.set_sql(opts.sql);
        if (opts.baseSql || opts.base_sql) self.viz.set_base_sql(opts.baseSql || opts.base_sql);
        self.defaults = JSON.parse(wb.default_config());
        self.cfg = merge(JSON.parse(JSON.stringify(self.defaults)), opts.config || {});
        self.initialFocus = (self.cfg.focus || []).slice();
        self.hasDiff = !!(opts.baseSql || opts.base_sql);
        self.buildBar();
        self.render(!pre);
        return self;
      }, function (e) {
        if (!pre) self.stage.insertAdjacentHTML("afterbegin", "<div class=\"sch-error\">schema: " + esc(e.message) + "</div>");
        self.buildBar();
        return self;
      });
    } else {
      this.buildBar();
      this.ready = Promise.resolve(this);
    }
  }

  var P = Instance.prototype;

  P.render = function (fit) {
    if (!this.viz) return;
    this.cfg.theme = this.dark ? "dark" : "light";
    var res = JSON.parse(this.viz.view(JSON.stringify(this.cfg)));
    if (res.error) { console.error(res.error); return; }
    this.result = res;
    this.viewer.setContent(res.svg, res, { preserveView: !fit });
    this.viewer.setTheme(this.dark);
    this.updateBar();
  };

  P.update = function (config, fit) {
    if (!this.cfg) return;
    merge(this.cfg, config || {});
    this.render(fit !== false);
  };

  P.toggleFocus = function (id) {
    this.cfg.focus = this.cfg.focus.length === 1 && this.cfg.focus[0] === id ? [] : [id];
    this.render(true);
  };

  P.buildBar = function () {
    if (!this.bar) return;
    var o = this.opts, self = this, live = !!this.viz;
    var sel = function (key, opts, title) {
      return "<select data-k=\"" + key + "\" title=\"" + title + "\">" + opts.map(function (x) { return "<option value=\"" + x[0] + "\">" + x[1] + "</option>"; }).join("") + "</select>";
    };
    this.bar.innerHTML =
      "<div class=\"sch-title\">" + (o.title ? "<b>" + esc(o.title) + "</b>" : "") + (o.subtitle ? "<span>" + esc(o.subtitle) + "</span>" : "") + "</div>" +
      (live ? sel("layout.algorithm", [["layered", "Layered"], ["force", "Force"], ["grid", "Grid"], ["circular", "Circle"], ["radial", "Radial"]], "Layout") +
        sel("layout.direction", [["LR", "→"], ["TB", "↓"], ["RL", "←"], ["BT", "↑"]], "Direction") +
        sel("columns", [["auto", "auto columns"], ["all", "all columns"], ["keys", "keys"], ["relations", "PK/FK"], ["referenced", "referenced"], ["changed", "changed"], ["none", "headers"]], "Columns") +
        sel("edges.style", [["curved", "curved"], ["orthogonal", "orthogonal"], ["straight", "straight"], ["hidden", "no edges"]], "Edges") +
        (this.hasDiff ? "<label class=\"sch-check\"><input type=\"checkbox\" data-k=\"changes_only\"> changes only</label>" : "") +
        "<span class=\"sch-focus\" hidden></span>" +
        "<input type=\"search\" placeholder=\"search…\" class=\"sch-search\">" : "") +
      "<button data-a=\"fit\" title=\"Fit\">⤢</button><button data-a=\"theme\" title=\"Theme\">◐</button>";
    this.bar.querySelectorAll("select,input[type=checkbox]").forEach(function (el) {
      el.addEventListener("change", function () {
        var k = el.getAttribute("data-k").split("."), obj = self.cfg;
        while (k.length > 1) obj = obj[k.shift()];
        obj[k[0]] = el.type === "checkbox" ? el.checked : el.value;
        self.render(true);
      });
    });
    var search = this.bar.querySelector(".sch-search");
    if (search) search.addEventListener("input", function () {
      var q = search.value.trim().toLowerCase();
      var hits = q ? self.result.nodes.filter(function (n) { return n.label.toLowerCase().indexOf(q) >= 0; }).map(function (n) { return n.id; }) : [];
      self.viewer.setSearchHits(hits);
    });
    if (search) search.addEventListener("keydown", function (e) {
      if (e.key === "Enter" && self.viewer.hits.length) { self.viewer.centerOn(self.viewer.hits[0]); self.showInfo(self.viewer.hits[0]); }
    });
    this.bar.querySelector("[data-a=fit]").onclick = function () { self.viewer.fit(); };
    this.bar.querySelector("[data-a=theme]").onclick = function () {
      self.dark = !self.dark;
      self.el.setAttribute("data-sch-theme", self.dark ? "dark" : "light");
      if (self.viz) self.render(false); else self.viewer.setTheme(self.dark);
    };
    this.updateBar();
  };

  P.updateBar = function () {
    if (!this.bar || !this.cfg) return;
    var self = this;
    this.bar.querySelectorAll("[data-k]").forEach(function (el) {
      var v = el.getAttribute("data-k").split(".").reduce(function (a, k) { return a && a[k]; }, self.cfg);
      if (el.type === "checkbox") el.checked = !!v; else el.value = v;
    });
    var dir = this.bar.querySelector("[data-k='layout.direction']");
    if (dir) dir.hidden = this.cfg.layout.algorithm !== "layered";
    var f = this.bar.querySelector(".sch-focus");
    if (f) {
      f.hidden = !this.cfg.focus.length;
      f.innerHTML = "focus: " + this.cfg.focus.map(function (x) { return esc(display(x)); }).join(", ") + " <button title=\"Clear focus\">×</button>";
      var b = f.querySelector("button");
      if (b) b.onclick = function () { self.cfg.focus = []; self.render(true); };
    }
  };

  P.showInfo = function (id) {
    this.viewer.select(id);
    var box = this.info, self = this;
    if (!this.viz) {
      box.hidden = false;
      box.innerHTML = "<b>" + esc(display(id)) + "</b>";
      return;
    }
    var d = JSON.parse(this.viz.table(id));
    var t = d.table;
    var h = "<div class=\"sch-info-head\"><b>" + esc(display(id)) + "</b>" + (d.status !== "unchanged" ? " <span class=\"sch-st " + d.status + "\">" + d.status + "</span>" : "") + "<button title=\"Close\">×</button></div>";
    if (t) {
      if (t.comment) h += "<p>" + esc(t.comment) + "</p>";
      var dc = {};
      ((d.diff && d.diff.columns) || []).forEach(function (c) { dc[c.name] = c; });
      h += "<table>" + t.columns.map(function (c) {
        var st = dc[c.name] ? dc[c.name].status : "";
        var pk = t.primary_key && t.primary_key.columns.indexOf(c.name) >= 0;
        var fk = (t.foreign_keys || []).some(function (f) { return f.columns.indexOf(c.name) >= 0; });
        return "<tr class=\"" + st + "\"><td>" + (pk ? "<i class=pk>PK</i>" : fk ? "<i class=fk>FK</i>" : "") + "</td><td>" + esc(c.name) + (c.nullable ? "?" : "") + "</td><td>" + esc(c.data_type) + "</td></tr>";
      }).join("") + "</table>";
      if (d.referenced_by.length) h += "<p class=\"sch-muted\">referenced by " + d.referenced_by.map(function (r) { return esc(display(r.table)); }).join(", ") + "</p>";
    } else if (d.view) {
      h += "<pre>" + esc(d.view.definition) + "</pre>";
    }
    h += "<p class=\"sch-muted\">double-click a table to focus on it</p>";
    box.innerHTML = h;
    box.hidden = false;
    box.querySelector("button").onclick = function () { box.hidden = true; self.viewer.select(null); };
  };

  function mount(el, opts) {
    if (typeof el === "string") el = document.querySelector(el);
    opts = opts || {};
    return new Instance(el, opts);
  }

  function autoMount() {
    document.querySelectorAll("[data-schema]:not([data-sch-mounted])").forEach(function (el) {
      el.setAttribute("data-sch-mounted", "1");
      var sqlEl = el.querySelector("script[type='application/sql']:not([data-base-sql]),script[type='text/x-sql']:not([data-base-sql]),script[data-sql]");
      var baseEl = el.querySelector("script[data-base-sql]");
      var config = {};
      try { config = JSON.parse(el.getAttribute("data-config") || "{}"); } catch (e) { console.error("schema: bad data-config", e); }
      var opts = {
        sql: sqlEl ? sqlEl.textContent : null,
        baseSql: baseEl ? baseEl.textContent : null,
        config: config,
        title: el.getAttribute("data-title"),
        subtitle: el.getAttribute("data-subtitle"),
        toolbar: el.getAttribute("data-toolbar") !== "false",
      };
      var src = el.getAttribute("data-src");
      if (!opts.sql && src) {
        var baseSrc = el.getAttribute("data-base-src");
        Promise.all([fetch(src).then(function (r) { return r.text(); }), baseSrc ? fetch(baseSrc).then(function (r) { return r.text(); }) : null]).then(function (r) {
          opts.sql = r[0];
          opts.baseSql = r[1];
          mount(el, opts);
        });
      } else mount(el, opts);
    });
  }

  global.Schema = {
    mount: mount,
    autoMount: autoMount,
    ready: wasm,
    render: function (sql, config, baseSql) {
      return wasm().then(function (wb) {
        var v = new wb.Schema();
        v.set_sql(sql);
        if (baseSql) v.set_base_sql(baseSql);
        var r = JSON.parse(v.view(JSON.stringify(config || {})));
        v.free();
        return r.svg;
      });
    },
    diffMarkdown: function (baseSql, sql) {
      return wasm().then(function (wb) {
        var v = new wb.Schema();
        v.set_sql(sql);
        v.set_base_sql(baseSql);
        var md = v.diff_markdown();
        v.free();
        return md;
      });
    },
  };
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", autoMount);
  else setTimeout(autoMount, 0);
})(typeof window !== "undefined" ? window : this);
