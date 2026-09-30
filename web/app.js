/* schema web app. Talks to the local CLI server (/api/*) when present,
 * otherwise runs as a serverless playground (window.SCHEMA_STATIC). */
(function () {
  "use strict";

  var $ = function (s, r) { return (r || document).querySelector(s); };
  var $$ = function (s, r) { return Array.prototype.slice.call((r || document).querySelectorAll(s)); };
  var WORKTREE = "WORKTREE", INDEX = "INDEX", BASEFILE = "BASEFILE";
  var STATIC = window.SCHEMA_STATIC || null;

  var viz, viewer, wb;
  var S = {
    server: null, defaults: null, cfg: null, projectCfg: {}, base: null, compare: WORKTREE,
    sources: new Map(), selected: null, index: [], log: [], refs: { branches: [], tags: [] },
    result: null, lastKey: null, theme: "auto", tables: [], diff: null, enums: [],
    design: null, designSlug: null, designState: null, designUndo: [], designDirty: false, designPath: null, editor: null, playground: { current: null, base: null, name: "structure.sql", baseName: null },
  };

  // ---- utils --------------------------------------------------------------
  /** Run on the next animation frame, or after a short timeout when frames
   *  are paused (background tabs), so rendering never stalls. */
  function nextFrame(fn) {
    var done = false, raf = 0, t = 0;
    var run = function () { if (done) return; done = true; cancelAnimationFrame(raf); clearTimeout(t); fn(); };
    raf = requestAnimationFrame(run);
    t = setTimeout(run, 120);
    return { cancel: function () { done = true; cancelAnimationFrame(raf); clearTimeout(t); } };
  }
  function esc(s) { return String(s == null ? "" : s).replace(/[&<>"']/g, function (c) { return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]; }); }
  function clone(v) { return JSON.parse(JSON.stringify(v)); }
  function isObj(v) { return v && typeof v === "object" && !Array.isArray(v); }
  function merge(a, b) {
    if (!isObj(b)) return a;
    Object.keys(b).forEach(function (k) {
      if (isObj(b[k]) && isObj(a[k])) merge(a[k], b[k]);
      else a[k] = clone(b[k]);
    });
    return a;
  }
  /** Minimal patch turning `base` into `v`. */
  function diffObj(v, base) {
    var out = {};
    Object.keys(v).forEach(function (k) {
      if (isObj(v[k]) && isObj(base[k])) {
        var d = diffObj(v[k], base[k]);
        if (Object.keys(d).length) out[k] = d;
      } else if (JSON.stringify(v[k]) !== JSON.stringify(base[k])) out[k] = clone(v[k]);
    });
    return out;
  }
  function getPath(o, p) { return p.split(".").reduce(function (a, k) { return a == null ? a : a[k]; }, o); }
  function setPath(o, p, v) {
    var ks = p.split("."), last = ks.pop();
    ks.forEach(function (k) { if (!isObj(o[k])) o[k] = {}; o = o[k]; });
    o[last] = v;
  }
  function display(id) { return id && id.indexOf("public.") === 0 ? id.slice(7) : id; }
  function shortType(t) {
    return String(t || "").replace(/public\.|pg_catalog\./g, "").replace("character varying", "varchar").replace(/timestamp(\(\d\))? without time zone/, "timestamp$1")
      .replace(/timestamp(\(\d\))? with time zone/, "timestamptz$1").replace("double precision", "float8").replace("boolean", "bool").replace(/\binteger\b/, "int");
  }
  var toastTimer;
  function toast(msg, ms) {
    var t = $("#toast");
    t.textContent = msg;
    t.hidden = false;
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { t.hidden = true; }, ms || 2600);
  }
  function download(name, blob) {
    var a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = name;
    document.body.appendChild(a);
    a.click();
    setTimeout(function () { URL.revokeObjectURL(a.href); a.remove(); }, 1000);
  }
  function copy(text, what) {
    (navigator.clipboard ? navigator.clipboard.writeText(text) : Promise.reject()).then(
      function () { toast("Copied " + what); },
      function () { window.prompt("Copy " + what + ":", text); }
    );
  }
  function ago(iso) {
    var s = (Date.now() - new Date(iso).getTime()) / 1000;
    if (s < 90) return "just now";
    if (s < 3600) return Math.round(s / 60) + " min ago";
    if (s < 86400 * 2) return Math.round(s / 3600) + " h ago";
    if (s < 86400 * 60) return Math.round(s / 86400) + " days ago";
    return new Date(iso).toLocaleDateString();
  }
  function api(path, opts) {
    return fetch(path, opts).then(function (r) {
      if (!r.ok) return r.text().then(function (t) { throw new Error(t || r.statusText); });
      var ct = r.headers.get("content-type") || "";
      return ct.indexOf("json") >= 0 ? r.json() : r.text();
    });
  }
  function isDark() {
    if (S.theme === "auto") return window.matchMedia && matchMedia("(prefers-color-scheme: dark)").matches;
    return S.theme === "dark";
  }
  function storeKey() { return "schema:" + (S.server ? S.server.file : "playground"); }

  // ---- persistence ----------------------------------------------------------
  function saveState() {
    try {
      var st = { cfg: diffObj(S.cfg, S.defaults), base: S.base, compare: S.compare, lens: S.lens };
      delete st.cfg.theme;
      localStorage.setItem(storeKey(), JSON.stringify(st));
    } catch (e) { /* storage full or disabled */ }
  }
  function loadState() {
    try { return JSON.parse(localStorage.getItem(storeKey()) || "null"); } catch (e) { return null; }
  }

  // ---- sources ------------------------------------------------------------
  function getSource(ref) {
    if (STATIC) {
      var v = ref === BASEFILE ? S.playground.base : S.playground.current;
      return v == null ? Promise.reject(new Error("no file")) : Promise.resolve(v);
    }
    if (S.sources.has(ref)) return Promise.resolve(S.sources.get(ref));
    return api("api/source?ref=" + encodeURIComponent(ref)).then(function (t) { S.sources.set(ref, t); return t; });
  }

  function loadSources() {
    return getSource(S.compare).then(function (cur) {
      var summary = JSON.parse(viz.set_sql(cur));
      if (summary.warnings && summary.warnings.length) console.warn("schema parse warnings", summary.warnings);
      if (!S.base) { viz.set_base_sql(undefined); return; }
      return getSource(S.base).then(function (b) { viz.set_base_sql(b); }, function (e) {
        toast("Base " + S.base + " not available (" + e.message.slice(0, 80) + ") — comparing against an empty schema", 4500);
        viz.set_base_sql("");
      });
    }).then(function () {
      if (S.design) { designSync({ dirty: false, render: false }); return; }
      buildIndex();
      S.diff = JSON.parse(viz.diff());
      renderChanges();
      updateCompareUI();
    });
  }

  function buildIndex() {
    var schema = JSON.parse(viz.schema());
    S.enums = (schema.enums || []).map(function (e) { return e.schema + "." + e.name; });
    S.index = [];
    var names = [];
    schema.tables.forEach(function (t) {
      var id = t.schema + "." + t.name;
      names.push(display(id));
      S.index.push({ id: id, label: display(id), cols: t.columns.map(function (c) { return c.name; }) });
    });
    (schema.views || []).forEach(function (v) { var id = v.schema + "." + v.name; names.push(display(id)); S.index.push({ id: id, label: display(id), cols: [], view: true }); });
    $("#table-names").innerHTML = names.map(function (n) { return "<option value=\"" + esc(n) + "\">"; }).join("");
    S.schemaNames = schema.schemas || [];
  }

  // ---- rendering ----------------------------------------------------------
  var STRUCTURAL = ["focus", "focus_depth", "focus_depths", "focus_direction", "include", "exclude", "schemas", "changes_only", "changes_context", "layout.algorithm", "layout.direction", "layout.group_by", "show_views", "show_partitions", "show_isolated", "enums"];
  var renderQueued = null;
  function render(o) {
    o = o || {};
    if (renderQueued) renderQueued.cancel();
    renderQueued = nextFrame(function () { renderQueued = null; renderNow(o); });
  }
  function renderNow(o) {
    S.cfg.theme = isDark() ? "dark" : "light";
    var key = JSON.stringify(STRUCTURAL.map(function (p) { return getPath(S.cfg, p); }).concat([S.lens && [S.lens.kind, S.lens.label, S.lens.context, S.lens.combine]]));
    var fit = o.fit || S.lastKey === null || (key !== S.lastKey && !o.preserve);
    S.lastKey = key;
    var t0 = performance.now();
    var res = JSON.parse(viz.view(JSON.stringify(viewCfg())));
    if (res.error) { toast(res.error, 5000); return; }
    S.result = res;
    viewer.setContent(res.svg, res, { preserveView: !fit });
    viewer.setTheme(isDark());
    $("#loading").hidden = true;
    S.tables = JSON.parse(viz.tables());
    renderEmpty(res);
    renderStatus(res, performance.now() - t0);
    renderChanges();
    renderFilterBar();
    renderDiffSummary();
    if (S.selected) {
      if (viewer.nodes.has(S.selected)) viewer.select(S.selected);
      renderDetails(S.selected);
    }
    saveState();
  }

  function renderEmpty(res) {
    var el = $("#empty");
    if (res.nodes.length) { el.hidden = true; return; }
    el.hidden = false;
    var changed = S.diff ? S.diff.tables.length : 0;
    if (S.lens) {
      var msg2 = !changed ? S.lens.label + " changes no tables" + (S.diff && S.diff.summary.other_changes ? " (only objects that aren't drawn — see the Changes tab)" : "") + "."
        : S.lens.label + " changes " + changed + " table" + (changed > 1 ? "s" : "") + ", but none match your filters.";
      el.innerHTML = "<div>" + esc(msg2) + "</div>" + (changed && S.lens.combine ? "<button class=\"btn\" id=\"empty-uncombine\">Show them anyway</button>" : "") +
        "<button class=\"btn\" id=\"empty-exit\">Back to my view</button>";
      var u = $("#empty-uncombine");
      if (u) u.onclick = function () { S.lens.combine = false; applyFilter(); };
      $("#empty-exit").onclick = exitLens;
      return;
    }
    var msg = S.tables.length ? "No tables match the current filters." : "No tables found in this file.";
    el.innerHTML = "<div>" + esc(msg) + "</div>" + (S.tables.length ? "<button class=\"btn\" id=\"empty-reset\">Clear filters</button>" : "");
    var b = $("#empty-reset");
    if (b) b.onclick = function () { clearFilters(); };
  }

  function clearFilters() {
    S.cfg.focus = [];
    S.cfg.focus_depths = {};
    S.cfg.focus_direction = "both";
    S.cfg.show_isolated = true;
    S.cfg.include = [];
    S.cfg.exclude = clone(S.defaults.exclude);
    S.cfg.schemas = [];
    syncControls();
    render({ fit: true });
  }

  function renderStatus(res, ms) {
    var st = res.stats, parts = [];
    var nt = st.nodes_visible - (st.enums_visible || 0);
    parts.push("<b>" + nt + "</b> table" + (nt === 1 ? "" : "s") + (st.enums_visible ? " + " + st.enums_visible + " enum" + (st.enums_visible === 1 ? "" : "s") : ""));
    parts.push("<b>" + st.edges_visible + "</b> relation" + (st.edges_visible === 1 ? "" : "s"));
    parts.push(st.column_mode === "none" ? "headers only" : st.column_mode + " columns");
    if (ms > 200) parts.push(ms.toFixed(0) + " ms");
    (st.notices || []).forEach(function (n) { parts.push("<span class=\"notice\">⚠ " + esc(n) + "</span>"); });
    $("#statusbar").innerHTML = parts.join("<span>·</span>");
  }

  function renderDiffSummary() {
    var box = $("#diff-pills"), d = S.diff;
    var active = d && (S.base || S.design);
    $("#changes-count").textContent = active && d.tables.length ? String(d.tables.length) : "";
    if (!box) return;
    if (!active) { box.innerHTML = ""; return; }
    var s = d.summary;
    box.innerHTML = (!d.tables.length && !s.other_changes ? "<span class=\"pill none\">no changes</span>" :
      (s.tables_added ? "<span class=\"pill add\">+" + s.tables_added + "</span>" : "") +
      (s.tables_removed ? "<span class=\"pill del\">−" + s.tables_removed + "</span>" : "") +
      (s.tables_modified ? "<span class=\"pill mod\">~" + s.tables_modified + "</span>" : ""));
    box.title = "Tables added / removed / changed" + (S.lens ? "" : " — press c to show only them");
  }

  // ---- controls -------------------------------------------------------------
  function syncControls() {
    $$("[data-cfg]").forEach(function (el) {
      var v = getPath(S.cfg, el.getAttribute("data-cfg"));
      if (el.type === "checkbox") el.checked = !!v;
      else if (el.hasAttribute("data-list")) { if (document.activeElement !== el) el.value = (v || []).join(", "); }
      else el.value = v == null ? "" : v;
    });
    $$("[data-seg]").forEach(function (seg) {
      var v = getPath(S.cfg, seg.getAttribute("data-seg"));
      $$("button", seg).forEach(function (b) { b.classList.toggle("active", b.getAttribute("data-v") === String(v)); });
    });
    $$("[data-out]").forEach(function (o) { o.textContent = getPath(S.cfg, o.getAttribute("data-out")); });
    $$("[data-show-if]").forEach(function (el) {
      var c = el.getAttribute("data-show-if").split("=");
      var v = getPath(S.cfg, c[0]);
      el.hidden = c.length > 1 ? String(v) !== c[1] : !(Array.isArray(v) ? v.length : v);
    });
    var cl = $("#changes-lens");
    if (cl) cl.checked = !!S.lens;
    var gc = $("#group-custom-opt");
    if (gc) gc.hidden = !(S.cfg.groups && S.cfg.groups.length) && S.cfg.layout.group_by !== "custom";
    renderFilterBar();
  }

  function bindControls() {
    $$("[data-cfg]").forEach(function (el) {
      var path = el.getAttribute("data-cfg");
      var ev = el.type === "text" || el.type === "range" || el.type === "number" ? "input" : "change";
      var t;
      el.addEventListener(ev, function () {
        var v;
        if (el.type === "checkbox") v = el.checked;
        else if (el.hasAttribute("data-list")) v = el.value.split(",").map(function (s) { return s.trim(); }).filter(Boolean);
        else if (el.hasAttribute("data-num")) v = el.value === "" ? 0 : Number(el.value);
        else if (el.hasAttribute("data-nullable") && el.value === "") v = null;
        else v = el.value;
        setPath(S.cfg, path, v);
        syncControls();
        clearTimeout(t);
        t = setTimeout(function () { render(); }, el.type === "text" ? 250 : 0);
      });
    });
    $$("[data-seg]").forEach(function (seg) {
      seg.addEventListener("click", function (e) {
        var b = e.target.closest("button");
        if (!b) return;
        setPath(S.cfg, seg.getAttribute("data-seg"), b.getAttribute("data-v"));
        syncControls();
        render({ fit: true });
      });
    });
    $("#changes-lens").addEventListener("change", function (e) { toggleChangesLens(e.target.checked); });
    $("#reset-positions").onclick = function () { resetPositions(); toast(filterActive() ? "Positions reset for this filter" : "Positions reset"); };
    $("#reset-config").onclick = function () {
      S.cfg = merge(clone(S.defaults), (S.projectCfg && S.projectCfg.default) || {});
      syncControls();
      render({ fit: true });
      toast("Display settings reset");
    };
    $$(".tab").forEach(function (t) {
      t.onclick = function () { showTab(t.getAttribute("data-tab")); };
    });
  }

  function showTab(name) {
    $$(".tab").forEach(function (x) { x.classList.toggle("active", x.getAttribute("data-tab") === name); });
    $$(".panel").forEach(function (p) { p.classList.toggle("active", p.getAttribute("data-panel") === name); });
  }

  // ---- focus / visibility helpers ----------------------------------------
  function focusOn(id, add, depth) {
    if (S.lens && !S.lens.combine) dropLens("Left the changes view — showing your filter");
    setFocus(display(id), depth, !!add);
    syncControls();
    render({ fit: true });
    selectTable(id, { center: false });
  }
  function override(id) { S.cfg.tables[id] = S.cfg.tables[id] || {}; return S.cfg.tables[id]; }
  function cleanOverride(id) {
    var o = S.cfg.tables[id];
    if (!o) return;
    ["hide_columns", "show_columns"].forEach(function (k) { if (o[k] && !o[k].length) delete o[k]; });
    if (!o.collapsed) delete o.collapsed;
    if (!Object.keys(o).length) delete S.cfg.tables[id];
  }
  function hideTable(id) {
    if (S.cfg.exclude.indexOf(id) < 0) S.cfg.exclude.push(id);
    var fp = patternFor(id);
    if (fp) removeFocus(fp);
    if (S.selected === id) closeDetails();
    syncControls();
    render({ preserve: true });
  }
  function showTable(id) {
    var ex = S.cfg.exclude.indexOf(id);
    if (ex >= 0) S.cfg.exclude.splice(ex, 1);
    else if (S.cfg.include.length) S.cfg.include.push(id);
    else if (S.cfg.focus.length) setFocus(display(id), null, true);
    if (S.lens && !(S.diff && S.diff.tables.some(function (t) { return t.id === id; }))) dropLens("Left the changes view to show " + display(id));
    syncControls();
    render({ preserve: true });
  }

  // ---- tables panel -------------------------------------------------------
  // ---- selection & details -----------------------------------------------
  function selectTable(id, o) {
    o = o || {};
    S.selected = id;
    viewer.select(id);
    if (o.center && viewer.nodes.has(id)) viewer.centerOn(id);
    renderDetails(id);
  }
  function closeDetails() {
    S.selected = null;
    viewer.select(null);
    $("#details").hidden = true;
  }

  function colFlags(t, c) {
    var f = "";
    if (t.primary_key && t.primary_key.columns.indexOf(c) >= 0) f += "<b class=\"k pk\">PK</b>";
    if ((t.foreign_keys || []).some(function (x) { return x.columns.indexOf(c) >= 0; })) f += "<b class=\"k fk\">FK</b>";
    var uq = (t.uniques || []).some(function (u) { return u.columns.length === 1 && u.columns[0] === c; }) ||
      (t.indexes || []).some(function (i) { return i.unique && !i.predicate && i.columns.length === 1 && i.columns[0] === c; });
    if (uq) f += "<b class=\"k uq\">UQ</b>";
    return f;
  }

  function renderDetails(id) {
    var d = JSON.parse(viz.table(id));
    var box = $("#details");
    var t = d.table, v = d.view, en = d.enum;
    if (!t && !v && !en) { box.hidden = true; return; }
    if (!t && !v && en) { renderEnumDetails(id, en); return; }
    box.hidden = false;
    var parts = id.split("."), schema = parts[0], name = parts.slice(1).join(".");
    var diff = d.diff || { columns: [], foreign_keys: [], indexes: [], constraints: [], properties: [] };
    var colEnums = d.column_enums || {};
    var status = d.status;
    var ov = S.cfg.tables[id] || {};
    var visible = viewer.nodes.has(id);
    var h = "<div class=\"head\"><h2>" + (schema !== "public" ? "<span class=\"schema\">" + esc(schema) + ".</span>" : "") + esc(name) +
      (status !== "unchanged" ? " <span class=\"pill " + ({ added: "add", removed: "del", modified: "mod" })[status] + "\">" + status + "</span>" : "") +
      "<button class=\"close\" title=\"Close (Esc)\">×</button></h2>";
    var comment = (t && t.comment) || (v && v.comment);
    if (comment) h += "<p class=\"comment\">" + esc(comment) + "</p>";
    h += "<div class=\"tools\">" +
      (S.design && t && status !== "removed" ? "<button class=\"btn small primary\" data-act=\"edit\">✎ Edit table</button>" : "") +
      (patternFor(id) ? "<button class=\"btn small\" data-act=\"unfocus\" title=\"Remove from the filter\">◎ In filter ×</button>"
        : "<button class=\"btn small\" data-act=\"focus\" title=\"Show only this table and its neighbours\">◎ Show only</button>" +
          (S.cfg.focus.length ? "<button class=\"btn small\" data-act=\"addfocus\" title=\"Add to the current filter\">+ Add to filter</button>" : "")) +
      (visible ? "<button class=\"btn small\" data-act=\"hide\">Hide</button>" : "<button class=\"btn small\" data-act=\"show\">Show</button>") +
      (t ? "<select class=\"btn small\" data-act=\"colmode\" title=\"Columns shown for this table\">" +
        [["", "columns: default"], ["all", "all columns"], ["keys", "keys only"], ["relations", "PK/FK only"], ["referenced", "referenced only"], ["changed", "changed only"], ["none", "collapsed"]].map(function (o) {
          var cur = ov.collapsed ? "none" : ov.columns || "";
          return "<option value=\"" + o[0] + "\"" + (cur === o[0] ? " selected" : "") + ">" + o[1] + "</option>";
        }).join("") + "</select>" : "") +
      "<input type=\"color\" data-act=\"color\" title=\"Header colour\" value=\"" + esc(ov.color || "#4f6bed") + "\" style=\"width:30px;height:24px;padding:0;border:1px solid var(--border);border-radius:5px\">" +
      (ov.color ? "<button class=\"btn small\" data-act=\"nocolor\" title=\"Remove colour\">×</button>" : "") +
      "</div></div>";

    if (t) {
      var cols = t.columns.map(function (c) { return { c: c, st: "unchanged" }; });
      var byName = {};
      (diff.columns || []).forEach(function (cd) { byName[cd.name] = cd; });
      if (status === "modified" && d.base) {
        d.base.columns.forEach(function (oc, i) {
          if (byName[oc.name] && byName[oc.name].status === "removed") cols.splice(Math.min(i, cols.length), 0, { c: oc, st: "removed" });
        });
      }
      cols.forEach(function (x) { if (status === "modified" && byName[x.c.name]) x.st = byName[x.c.name].status; if (status === "added" || status === "removed") x.st = status; });
      var node = visible && viewer.nodes.get(id).el;
      h += "<section><h3>Columns (" + t.columns.length + ")</h3><table class=\"cols\"><colgroup><col class=\"c-flags\"><col><col class=\"c-type\"><col class=\"c-eye\"></colgroup>" + cols.map(function (x) {
        var c = x.c, shown = node ? !!node.querySelector("[data-col=\"" + CSS.escape(c.name) + "\"]") : true;
        var chg = "";
        if (x.st === "modified") chg = (byName[c.name].changes || []).map(function (f) { return "<span class=\"chg\">" + esc(f.field) + ": " + esc(f.old || "∅") + " → " + esc(f.new || "∅") + "</span>"; }).join("");
        return "<tr class=\"" + x.st + (shown ? "" : " hidden-col") + "\" title=\"" + esc(c.comment || "") + "\">" +
          "<td class=\"flags\">" + colFlags(x.st === "removed" && d.base ? d.base : t, c.name) + "</td>" +
          "<td class=\"name\">" + esc(c.name) + (c.nullable ? "<span class=\"muted\">?</span>" : "") + chg + (c.default ? "<span class=\"dflt\">= " + esc(c.default) + "</span>" : "") + "</td>" +
          "<td class=\"type\" title=\"" + esc(c.data_type) + (colEnums[c.name] ? " — enum, click for values" : "") + "\">" +
            (colEnums[c.name] ? "<a class=\"link\" data-enum=\"" + esc(colEnums[c.name]) + "\">" + esc(shortType(c.data_type)) + "</a>" : esc(shortType(c.data_type))) + "</td>" +
          "<td>" + (visible && x.st !== "removed" ? "<button class=\"eye\" data-col=\"" + esc(c.name) + "\" title=\"" + (shown ? "Hide in diagram" : "Show in diagram") + "\">" + (shown ? "👁" : "◌") + "</button>" : "") + "</td></tr>";
      }).join("") + "</table></section>";

      var fkSt = {}, fkRen = {};
      (diff.foreign_keys || []).forEach(function (f) {
        fkSt[f.name] = f.status;
        var m = f.name.split(" → ");
        if (m.length === 2) { fkSt[m[1]] = "modified"; fkRen[m[1]] = m[0]; }
      });
      var fks = (t.foreign_keys || []).map(function (f) {
        var key = f.name || "";
        return "<li class=\"" + (fkSt[key] || "") + "\">(" + esc(f.columns.join(", ")) + ") → <a class=\"link\" data-goto=\"" + esc(f.ref_table) + "\">" + esc(display(f.ref_table)) + "</a>(" + esc(f.ref_columns.join(", ")) + ")" +
          (f.on_delete ? " <span class=\"muted\">ON DELETE " + esc(f.on_delete) + "</span>" : "") +
          (fkRen[key] ? " <span class=\"muted\">renamed from " + esc(fkRen[key]) + "</span>" : "") + "</li>";
      });
      (diff.foreign_keys || []).filter(function (f) { return f.status === "removed" && status === "modified"; }).forEach(function (f) { fks.push("<li class=\"removed\">" + esc(f.old) + "</li>"); });
      if (fks.length) h += "<section><h3>References</h3><ul class=\"list\">" + fks.join("") + "</ul></section>";
      if (d.referenced_by.length) {
        h += "<section><h3>Referenced by (" + d.referenced_by.length + ")</h3><ul class=\"list\">" + d.referenced_by.map(function (r) {
          return "<li><a class=\"link\" data-goto=\"" + esc(r.table) + "\">" + esc(display(r.table)) + "</a>.(" + esc(r.columns.join(", ")) + ")" + (r.on_delete ? " <span class=\"muted\">ON DELETE " + esc(r.on_delete) + "</span>" : "") + "</li>";
        }).join("") + "</ul></section>";
      }
      var idxSt = {};
      (diff.indexes || []).forEach(function (i) { idxSt[i.name] = i.status; });
      var idx = (t.indexes || []).map(function (i) {
        return "<li class=\"" + (idxSt[i.name] || "") + "\" title=\"" + esc(i.definition) + "\">" + (i.unique ? "<b class=\"k uq\">UQ</b>" : "<b class=\"k\">IX</b>") + esc(i.name) + " <span class=\"muted\">(" + esc(i.columns.join(", ")) + ")" + (i.predicate ? " WHERE " + esc(i.predicate) : "") + "</span></li>";
      });
      (diff.indexes || []).filter(function (i) { return i.status === "removed" || (i.status === "modified" && i.name.indexOf("→") >= 0); }).forEach(function (i) {
        idx.push("<li class=\"" + i.status + "\">" + esc(i.name) + " <span class=\"muted\">" + esc(i.old || "") + "</span></li>");
      });
      if (idx.length) h += "<section><h3>Indexes (" + (t.indexes || []).length + ")</h3><ul class=\"list\">" + idx.join("") + "</ul></section>";
      var cons = [];
      if (t.primary_key) cons.push("<li>PRIMARY KEY (" + esc(t.primary_key.columns.join(", ")) + ")</li>");
      (t.uniques || []).forEach(function (u) { cons.push("<li>UNIQUE (" + esc(u.columns.join(", ")) + ")</li>"); });
      (t.checks || []).forEach(function (c) { cons.push("<li>" + esc(c.name ? c.name + ": " : "") + "CHECK (" + esc(c.expression) + ")</li>"); });
      (diff.constraints || []).forEach(function (c) { cons.push("<li class=\"" + c.status + "\">" + esc(c.name) + ": " + esc(c.new || c.old) + "</li>"); });
      if (cons.length) h += "<section><h3>Constraints</h3><ul class=\"list\">" + cons.join("") + "</ul></section>";
      (diff.properties || []).forEach(function (p) {
        h += "<section><h3>" + esc(p.field) + " changed</h3><ul class=\"list\"><li class=\"removed\">" + esc(p.old || "∅") + "</li><li class=\"added\">" + esc(p.new || "∅") + "</li></ul></section>";
      });
      if (t.partition_by) h += "<section><h3>Partitioned by</h3><ul class=\"list\"><li>" + esc(t.partition_by) + "</li></ul></section>";
      if (d.triggers.length) h += "<section><h3>Triggers</h3><ul class=\"list\">" + d.triggers.map(function (tr) { return "<li title=\"" + esc(tr.definition) + "\">" + esc(tr.name) + "</li>"; }).join("") + "</ul></section>";
      if (d.used_by_views.length) h += "<section><h3>Used by views</h3><ul class=\"list\">" + d.used_by_views.map(function (vid) { return "<li><a class=\"link\" data-goto=\"" + esc(vid) + "\">" + esc(display(vid)) + "</a></li>"; }).join("") + "</ul></section>";
    }
    if (v) {
      h += "<section><h3>" + (v.materialized ? "Materialized view" : "View") + " reads</h3><ul class=\"list\">" + v.depends_on.map(function (dep) { return "<li><a class=\"link\" data-goto=\"" + esc(dep) + "\">" + esc(display(dep)) + "</a></li>"; }).join("") + "</ul></section>";
      h += "<section><h3>Definition</h3><pre class=\"def\">" + esc(v.definition) + "</pre></section>";
    }
    box.innerHTML = h;
    $(".close", box).onclick = closeDetails;
    $$("[data-goto]", box).forEach(function (a) {
      a.onclick = function () {
        var target = a.getAttribute("data-goto");
        if (viewer.nodes.has(target)) selectTable(target, { center: true });
        else { showTable(target); setTimeout(function () { selectTable(target, { center: true }); }, 80); }
      };
    });
    $$("[data-enum]", box).forEach(function (a) { a.onclick = function () { showEnum(a.getAttribute("data-enum")); }; });
    $$("[data-act]", box).forEach(function (b) {
      var act = b.getAttribute("data-act");
      var handler = function () {
        if (act === "edit") openTableEditor(id);
        else if (act === "focus") focusOn(id);
        else if (act === "unfocus") { removeFocus(patternFor(id)); applyFilter(); renderDetails(id); }
        else if (act === "addfocus") focusOn(id, true);
        else if (act === "hide") hideTable(id);
        else if (act === "show") showTable(id);
        else if (act === "colmode") {
          var o = override(id);
          delete o.columns;
          delete o.collapsed;
          if (b.value === "none") o.collapsed = true;
          else if (b.value) o.columns = b.value;
          cleanOverride(id);
          render({ preserve: true });
        } else if (act === "color") { override(id).color = b.value; render({ preserve: true }); }
        else if (act === "nocolor") { delete override(id).color; cleanOverride(id); render({ preserve: true }); }
      };
      b.addEventListener(b.tagName === "SELECT" || b.type === "color" ? "change" : "click", handler);
    });
    $$(".eye", box).forEach(function (b) {
      b.onclick = function () { toggleColumn(id, b.getAttribute("data-col")); };
    });
  }

  /** Show an enum's values (and select its node when it is drawn). */
  function showEnum(enumId) {
    if (viewer.nodes.has(enumId)) selectTable(enumId, { center: false });
    else { S.selected = enumId; renderDetails(enumId); }
  }

  function renderEnumDetails(id, en) {
    var box = $("#details");
    box.hidden = false;
    var parts = id.split("."), schema = parts[0], name = parts.slice(1).join(".");
    var status = en.status || "unchanged";
    var base = en.base_values || null;
    var rows = en.values.map(function (v) { return { v: v, st: base && status === "modified" && base.indexOf(v) < 0 ? "added" : "" }; });
    if (base && status === "modified") base.forEach(function (v, i) { if (en.values.indexOf(v) < 0) rows.splice(Math.min(i, rows.length), 0, { v: v, st: "removed" }); });
    var h = "<div class=\"head\"><h2>" + (schema !== "public" ? "<span class=\"schema\">" + esc(schema) + ".</span>" : "") + esc(name) +
      " <span class=\"kind\">enum</span>" +
      (status !== "unchanged" ? " <span class=\"pill " + ({ added: "add", removed: "del", modified: "mod" })[status] + "\">" + status + "</span>" : "") +
      "<button class=\"close\" title=\"Close (Esc)\">×</button></h2></div>" +
      "<section><h3>Values (" + en.values.length + ")</h3><ul class=\"list\">" + rows.map(function (r) { return "<li class=\"" + r.st + "\">" + (r.st === "added" ? "+ " : r.st === "removed" ? "− " : "") + esc(r.v) + "</li>"; }).join("") + "</ul></section>";
    if (en.used_by && en.used_by.length) {
      h += "<section><h3>Used by (" + en.used_by.length + ")</h3><ul class=\"list\">" + en.used_by.map(function (u) {
        return "<li><a class=\"link\" data-goto=\"" + esc(u.table) + "\">" + esc(display(u.table)) + "</a>." + esc(u.column) + "</li>";
      }).join("") + "</ul></section>";
    }
    box.innerHTML = h;
    $(".close", box).onclick = closeDetails;
    $$("[data-goto]", box).forEach(function (a) {
      a.onclick = function () {
        var target = a.getAttribute("data-goto");
        if (viewer.nodes.has(target)) selectTable(target, { center: true });
        else { showTable(target); setTimeout(function () { selectTable(target, { center: true }); }, 80); }
      };
    });
  }

  function toggleColumn(id, col) {
    var o = override(id);
    o.hide_columns = o.hide_columns || [];
    o.show_columns = o.show_columns || [];
    var node = viewer.nodes.get(id);
    var shown = node && node.el.querySelector("[data-col=\"" + CSS.escape(col) + "\"]");
    if (shown) {
      o.show_columns = o.show_columns.filter(function (c) { return c !== col; });
      if (o.hide_columns.indexOf(col) < 0) o.hide_columns.push(col);
    } else {
      var before = o.hide_columns.length;
      o.hide_columns = o.hide_columns.filter(function (c) { return c !== col; });
      if (before === o.hide_columns.length && o.show_columns.indexOf(col) < 0) o.show_columns.push(col);
    }
    cleanOverride(id);
    render({ preserve: true });
  }

  // ---- changes panel --------------------------------------------------------
  var SIGN = { added: "+", removed: "−", modified: "~" };
  function renderChanges() {
    var box = $("#changes");
    var d = S.diff;
    if (!d || (!S.base && !S.design)) {
      box.innerHTML = "<div class=\"no-diff\"><p><b>No comparison active.</b></p>" +
        (STATIC ? "<p>Load a second file with <b>Compare with…</b> in the top bar to see what changed.</p>"
          : S.server && S.server.is_git ? "<p>Pick a <b>base</b> version in the top bar, or choose a commit from the history below to see what it changed.</p>"
            : "<p>This file is not in a git repository. Start with <code>--base-file old.sql</code> to compare two files.</p>") + "</div>";
      $("#changes-count").textContent = "";
      return;
    }
    // The diagram shows the actual changes; this panel only lists what
    // changed so you can jump to it.
    var h = "";
    if (!d.tables.length && !d.summary.other_changes) h += "<div class=\"no-diff\">No schema changes between these versions.</div>";
    var entries = d.tables.map(function (t) { return { id: t.id, status: t.status, kind: "" }; })
      .concat((d.enums || []).map(function (e) { return { id: e.name, status: e.status, kind: "enum" }; }));
    if (entries.length) {
      var order = { added: 0, modified: 1, removed: 2 };
      entries.sort(function (a, b) { return order[a.status] - order[b.status] || display(a.id).localeCompare(display(b.id)); });
      h += "<ul class=\"changed-list\">" + entries.map(function (t) {
        var visible = viewer.nodes.has(t.id);
        return "<li data-goto=\"" + esc(t.id) + "\" class=\"" + (visible ? "" : "off") + "\" title=\"" + (visible ? "Show in the diagram" : t.kind === "enum" ? "Not drawn — enable enum types in Display → Objects" : "Hidden by the current filter — click to show") + "\">" +
          "<span class=\"dot " + t.status + "\"></span><span class=\"name\">" + esc(display(t.id)) + "</span>" + (t.kind ? "<span class=\"kind\">" + t.kind + "</span>" : "") +
          "<span class=\"st " + t.status + "\">" + ({ added: "new", modified: "changed", removed: "dropped" })[t.status] + "</span></li>";
      }).join("") + "</ul>";
    }
    var other = [["view", d.views], ["function", d.functions], ["trigger", d.triggers], ["extension", d.extensions]]
      .filter(function (g) { return g[1] && g[1].length; });
    if (other.length) {
      h += "<p class=\"other-changes\" title=\"" + esc(other.map(function (g) { return g[1].map(function (x) { return x.status + " " + g[0] + " " + display(x.name); }).join("\n"); }).join("\n")) + "\">Also changed (not drawn): " +
        other.map(function (g) { return g[1].length + " " + g[0] + (g[1].length > 1 ? "s" : ""); }).join(", ") + "</p>";
    }
    box.innerHTML = h;
    $$("[data-goto]", box).forEach(function (hd) {
      hd.onclick = function () {
        var id = hd.getAttribute("data-goto");
        if (viewer.nodes.has(id)) selectTable(id, { center: true });
        else if (hd.querySelector(".kind")) { S.cfg.enums = "all"; syncControls(); render({ preserve: true }); setTimeout(function () { selectTable(id, { center: true }); }, 120); }
        else { showTable(id); setTimeout(function () { selectTable(id, { center: true }); }, 80); }
      };
    });
  }

  function renderHistory() {
    var ul = $("#history");
    if (STATIC || !S.server || !S.server.is_git || !S.log.length) { ul.innerHTML = ""; $("#history-title").hidden = true; return; }
    $("#history-title").hidden = false;
    var items = [];
    if (S.server.dirty) items.push("<li data-base=\"HEAD\" data-compare=\"WORKTREE\" class=\"" + (S.base === "HEAD" && S.compare === WORKTREE ? "active" : "") + "\"><span class=\"sha\">work</span><span class=\"subj\">Uncommitted changes</span><span class=\"when\">HEAD → working tree</span></li>");
    S.log.forEach(function (c) {
      var active = S.compare === c.sha && S.base === c.sha + "^";
      items.push("<li data-base=\"" + c.sha + "^\" data-compare=\"" + c.sha + "\" class=\"" + (active ? "active" : "") + "\" title=\"" + esc(c.subject) + "\"><span class=\"sha\">" + esc(c.short) + "</span><span class=\"subj\">" + esc(c.subject) + "</span><span class=\"when\">" + esc(c.author) + " · " + ago(c.date) + "</span></li>");
    });
    ul.innerHTML = items.join("");
    $$("li", ul).forEach(function (li) {
      li.onclick = function () {
        var sha = li.getAttribute("data-compare"), c = S.log.find(function (x) { return x.sha === sha; });
        setComparison(li.getAttribute("data-base"), sha, { lens: {
          kind: "commit",
          label: c ? c.short : "Uncommitted changes",
          prev: (S.lens && S.lens.prev) || { base: S.base, compare: S.compare },
        } });
      };
    });
  }

  // ---- compare bar ------------------------------------------------------------
  function refLabel(r) {
    if (r === WORKTREE) return STATIC ? S.playground.name : "working tree";
    if (r === INDEX) return "index";
    if (r === BASEFILE) return STATIC ? S.playground.baseName || "base file" : (S.server.base_file || "base file").split("/").pop();
    if (!r) return "—";
    var m = /^([0-9a-f]{7,40})(\^?)$/.exec(r);
    if (m) {
      var c = S.log.find(function (x) { return x.sha === m[1]; });
      return (c ? c.short : m[1].slice(0, 7)) + m[2];
    }
    return r;
  }
  function updateCompareUI() {
    if (STATIC) return;
    var git = S.server.is_git && (S.server.tracked || S.base);
    $("#compare-bar").hidden = !(git || S.server.base_file) || !!S.design;
    var b = $("#base-select"), c = $("#compare-select");
    b.textContent = S.base ? refLabel(S.base) : "— none —";
    b.classList.toggle("unset", !S.base);
    c.textContent = refLabel(S.compare);
    renderHistory();
  }
  function setComparison(base, compare, o) {
    o = o || {};
    // choosing a comparison by hand ends a commit view (without restoring)
    if (!o.lens && S.lens && S.lens.kind === "commit" && !o.fit) S.lens = null;
    S.base = base || null;
    S.compare = compare || WORKTREE;
    $("#loading").hidden = false;
    loadSources().then(function () {
      if (o.lens) startLens(o.lens);
      render(o.lens || o.fit ? { fit: true } : { fit: false, preserve: true });
    }, function (e) {
      toast("Could not load " + refLabel(S.compare) + ": " + e.message, 5000);
      $("#loading").hidden = true;
    });
  }
  // ---- ref picker: search branches, tags, commits; type any ref -------------
  var pick = null; // { which, items, pos }
  function refItems(which) {
    var quick = [];
    if (which === "base") quick.push({ v: "", l: "— none —", d: "no comparison" });
    if (which === "compare") quick.push({ v: WORKTREE, l: "working tree", d: "uncommitted changes" });
    if (S.server.base_file) quick.push({ v: BASEFILE, l: "file: " + S.server.base_file.split("/").pop() });
    if (S.server.is_git) {
      quick.push({ v: INDEX, l: "index", d: "staged changes" });
      quick.push({ v: "HEAD", l: "HEAD", d: S.server.branch ? "tip of " + S.server.branch : "" });
      quick.push({ v: "HEAD~1", l: "HEAD~1", d: "one commit back" });
      ["main", "master", "develop"].forEach(function (m) {
        if (S.refs.branches.indexOf(m) >= 0 && S.server.branch !== m) quick.push({ v: m + "..." + (S.compare === WORKTREE || S.compare === INDEX ? "HEAD" : S.compare), l: m + " (merge base)", d: "what this branch changes vs " + m, only: "base" });
      });
    }
    var byId = {};
    S.log.forEach(function (c) { byId[c.sha] = c; });
    return [
      ["", quick.filter(function (q) { return !q.only || q.only === which; })],
      ["Branches", S.refs.branches.map(function (b) { return { v: b, l: b }; })],
      ["Tags", S.refs.tags.map(function (t) { return { v: t, l: t }; })],
      ["Commits", S.log.map(function (c) { return { v: c.sha, l: c.short, d: c.subject + " · " + c.author + " · " + ago(c.date) }; })],
    ];
  }
  function openRefPicker(which) {
    var pop = $("#ref-pop"), anchor = $("#" + which + "-select");
    if (!pop.hidden && pick && pick.which === which) { closeRefPicker(); return; }
    pick = { which: which, groups: refItems(which), pos: 0, q: "" };
    pop.innerHTML = "<input id=\"ref-q\" placeholder=\"Search branches, tags, commits — or type a ref (HEAD~3, sha)\" autocomplete=\"off\" spellcheck=\"false\"><div class=\"ref-list\" id=\"ref-list\"></div>";
    pop.hidden = false;
    var r = anchor.getBoundingClientRect();
    pop.style.top = (r.bottom + 6) + "px";
    pop.style.left = Math.max(8, Math.min(r.left, innerWidth - pop.offsetWidth - 8)) + "px";
    renderRefList();
    var inp = $("#ref-q");
    inp.focus();
    inp.addEventListener("input", function () { pick.q = inp.value; pick.pos = 0; renderRefList(); });
    inp.addEventListener("keydown", function (e) {
      var n = pick.flat ? pick.flat.length : 0;
      if (e.key === "ArrowDown") { e.preventDefault(); pick.pos = Math.min(n - 1, pick.pos + 1); renderRefList(); }
      else if (e.key === "ArrowUp") { e.preventDefault(); pick.pos = Math.max(0, pick.pos - 1); renderRefList(); }
      else if (e.key === "Enter") { e.preventDefault(); if (pick.flat && pick.flat[pick.pos]) chooseRef(pick.flat[pick.pos]); }
      else if (e.key === "Escape") { closeRefPicker(); }
    });
  }
  function closeRefPicker() { $("#ref-pop").hidden = true; pick = null; }
  function renderRefList() {
    var list = $("#ref-list"), q = pick.q.trim().toLowerCase(), flat = [], sections = [];
    var match = function (it) { return !q || it.l.toLowerCase().indexOf(q) >= 0 || (it.d || "").toLowerCase().indexOf(q) >= 0 || it.v.toLowerCase().indexOf(q) >= 0; };
    var exact = false;
    pick.groups.forEach(function (g) {
      var all = g[1].filter(match);
      all.forEach(function (it) { if (it.l.toLowerCase() === q || it.v.toLowerCase() === q) exact = true; });
      if (all.length) sections.push({ title: g[0], items: all.slice(0, 40), total: all.length });
    });
    // anything typed that isn't in the lists can be used as a ref (HEAD~3, a sha,
    // origin/x); it comes after the matches so Enter picks the best match first
    if (q && !exact && /^[\w./~^@{}+-]+$/.test(pick.q.trim())) {
      sections.push({ title: sections.length ? "Other" : "", items: [{ v: pick.q.trim(), l: pick.q.trim(), d: "use as a git ref", custom: true }], total: 1 });
    }
    var current = pick.which === "base" ? (S.base || "") : S.compare, h = "";
    sections.forEach(function (sec) {
      if (sec.title) h += "<div class=\"ref-group\">" + esc(sec.title) + (sec.total > sec.items.length ? " <span>" + sec.items.length + " of " + sec.total + "</span>" : "") + "</div>";
      sec.items.forEach(function (it) {
        var i = flat.push(it) - 1;
        h += "<button class=\"ref-item" + (i === pick.pos ? " active" : "") + (it.v === current ? " current" : "") + (it.custom ? " custom" : "") + "\" data-i=\"" + i + "\">" +
          "<span class=\"l\">" + esc(it.l) + "</span>" + (it.d ? "<span class=\"d\">" + esc(it.d) + "</span>" : "") + "</button>";
      });
    });
    if (!flat.length) h = "<div class=\"ref-empty\">Nothing matches</div>";
    pick.flat = flat;
    list.innerHTML = h;
    $$(".ref-item", list).forEach(function (b) { b.onclick = function () { chooseRef(flat[Number(b.getAttribute("data-i"))]); }; });
    var act = list.querySelector(".ref-item.active");
    if (act) act.scrollIntoView({ block: "nearest" });
  }
  function chooseRef(it) {
    var which = pick.which;
    closeRefPicker();
    // a new comparison opens on what changed, like one started from the CLI
    var apply = function (v) {
      var base = which === "base" ? (v || null) : S.base, compare = which === "base" ? S.compare : v;
      if (!base) { S.lens = null; setComparison(null, compare, { fit: true }); return; }
      setComparison(base, compare, { lens: { kind: "diff", label: "Changed tables" } });
    };
    if (it.v.indexOf("...") > 0) {
      // merge base of two refs: ask the server to resolve it
      var parts = it.v.split("...");
      api("api/git/merge-base?a=" + encodeURIComponent(parts[0]) + "&b=" + encodeURIComponent(parts[1])).then(function (r) { apply(r.sha); }, function () { toast("No merge base for " + it.v); });
      return;
    }
    if (it.custom) {
      api("api/git/resolve?ref=" + encodeURIComponent(it.v)).then(function () { apply(it.v); }, function () { toast("Unknown git ref " + it.v, 3000); });
      return;
    }
    apply(it.v);
  }
  function bindCompare() {
    $("#base-select").onclick = function (e) { e.stopPropagation(); openRefPicker("base"); };
    $("#compare-select").onclick = function (e) { e.stopPropagation(); openRefPicker("compare"); };
    document.addEventListener("click", function (e) { if (pick && !e.target.closest("#ref-pop")) closeRefPicker(); });
    $("#swap-btn").onclick = function () {
      if (!S.base) return;
      setComparison(S.compare, S.base);
    };
  }

  // ---- search -----------------------------------------------------------------
  var searchHits = [], searchPos = -1;
  function bindSearch() {
    var input = $("#search"), box = $("#search-results");
    input.addEventListener("input", function () {
      var q = input.value.trim().toLowerCase();
      if (!q) { box.hidden = true; viewer.setSearchHits([]); searchHits = []; return; }
      var hits = [];
      S.index.forEach(function (t) {
        if (t.label.toLowerCase().indexOf(q) >= 0) hits.push({ id: t.id, label: t.label, col: null, score: t.label.toLowerCase() === q ? 0 : t.label.toLowerCase().indexOf(q) === 0 ? 1 : 2 });
        t.cols.forEach(function (c) { if (c.toLowerCase().indexOf(q) >= 0) hits.push({ id: t.id, label: t.label, col: c, score: c.toLowerCase() === q ? 3 : 4 }); });
      });
      hits.sort(function (a, b) { return a.score - b.score || a.label.localeCompare(b.label); });
      searchHits = hits.slice(0, 80);
      searchPos = -1;
      viewer.setSearchHits(Array.from(new Set(searchHits.map(function (h) { return h.id; }))));
      box.hidden = !searchHits.length;
      box.innerHTML = searchHits.map(function (h, i) {
        return "<button data-i=\"" + i + "\">" + esc(h.label) + (h.col ? "<span class=\"col\">." + esc(h.col) + "</span>" : "") + (viewer.nodes.has(h.id) ? "" : "<span class=\"hidden-tag\">hidden</span>") + "</button>";
      }).join("");
    });
    input.addEventListener("keydown", function (e) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        if (!searchHits.length) return;
        searchPos = (searchPos + (e.key === "ArrowDown" ? 1 : -1) + searchHits.length) % searchHits.length;
        $$("button", box).forEach(function (b, i) { b.classList.toggle("active", i === searchPos); });
      } else if (e.key === "Enter") {
        var h = searchHits[Math.max(0, searchPos)];
        if (h) goToHit(h);
      } else if (e.key === "Escape") {
        input.value = "";
        input.dispatchEvent(new Event("input"));
        input.blur();
      }
    });
    box.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (b) goToHit(searchHits[Number(b.getAttribute("data-i"))]);
    });
    document.addEventListener("click", function (e) { if (!e.target.closest(".search")) box.hidden = true; });
    input.addEventListener("focus", function () { if (searchHits.length) box.hidden = false; });
  }
  function goToHit(h) {
    $("#search-results").hidden = true;
    if (viewer.nodes.has(h.id)) selectTable(h.id, { center: true });
    else { focusOn(h.id); toast(display(h.id) + " was hidden — focused on it"); }
  }

  // ---- context menu -------------------------------------------------------------
  function contextMenu(id, col, e) {
    var m = $("#context-menu");
    var items = [];
    if (id) {
      var ov = S.cfg.tables[id] || {};
      items.push(["title", display(id) + (col ? "." + col : "")]);
      if (S.design) {
        var removed = (S.tables.find(function (x) { return x.id === id; }) || {}).status === "removed";
        if (!removed) {
          items.push(["✎ Edit table…", function () { openTableEditor(id); }]);
          items.push(["+ Add column…", function () { openTableEditor(id, { addColumn: true }); }]);
          items.push([isCreated(id) ? "Remove table from design" : "Drop table", function () { dropTable(id); }]);
        } else {
          items.push(["Undo drop", function () {
            var i = S.design.ops.map(function (o) { return o.op === "drop_table" && qid(o.table) === id; }).lastIndexOf(true);
            if (i >= 0) designEdit(function () { S.design.ops.splice(i, 1); });
          }]);
        }
        items.push(["-"]);
      }
      var fp = patternFor(id);
      if (fp) {
        items.push(["More neighbours (+" + (fdepth(fp) + 1) + ")", function () { S.cfg.focus_depths[fp] = fdepth(fp) + 1; applyFilter(); }]);
        if (fdepth(fp) > 0) items.push(["Fewer neighbours", function () { S.cfg.focus_depths[fp] = fdepth(fp) - 1; applyFilter(); }]);
        items.push(["Remove from filter", function () { removeFocus(fp); applyFilter(); }]);
      } else if (S.cfg.focus.length) {
        items.push(["Show its neighbours too", function () { setFocus(display(id), 1, true); applyFilter({ preserve: true }); }]);
      }
      items.push(["Show only this table", function () { focusOn(id, false, 0); }]);
      items.push(["Show with its neighbours", function () { focusOn(id, false, null); }]);
      if (S.cfg.focus.length && !fp) items.push(["Add to filter", function () { focusOn(id, true, null); }]);
      items.push(["Hide table", function () { hideTable(id); }]);
      items.push(["-"]);
      items.push([ov.collapsed ? "Expand columns" : "Collapse columns", function () { var o = override(id); o.collapsed = !o.collapsed; cleanOverride(id); render({ preserve: true }); }]);
      items.push(["Keys only for this table", function () { var o = override(id); o.columns = "keys"; o.collapsed = false; cleanOverride(id); render({ preserve: true }); }]);
      items.push(["All columns for this table", function () { var o = override(id); o.columns = "all"; o.collapsed = false; cleanOverride(id); render({ preserve: true }); }]);
      if (col) {
        items.push(["-"]);
        items.push(["Hide “" + col + "” here", function () { toggleColumn(id, col); }]);
        items.push(["Hide “" + col + "” in all tables", function () { if (S.cfg.hide_columns.indexOf(col) < 0) S.cfg.hide_columns.push(col); syncControls(); render({ preserve: true }); }]);
      }
      if (positionStore(false)[id]) items.push(["Reset position", function () { delete positionStore(false)[id]; saveState(); render({ preserve: true }); }]);
      items.push(["-"]);
      items.push(["Copy name", function () { copy(display(id), "table name"); }]);
    } else {
      if (S.design) {
        var at = viewer.toDiagram(e.clientX, e.clientY);
        items.push(["+ New table here…", function () { openTableEditor(null, { pos: [Math.max(0, Math.round(at[0])), Math.max(0, Math.round(at[1]))] }); }]);
        items.push(["-"]);
      }
      items.push(["Fit to screen", function () { viewer.fit(); }]);
      if (userFiltersActive()) items.push(["Show all tables (clear filters)", function () { clearFilters(); }]);
      if (Object.keys(positionStore(false)).length) items.push(["Reset dragged positions", function () { resetPositions(); }]);
    }
    m.innerHTML = items.map(function (it, i) {
      if (it[0] === "-") return "<hr>";
      if (it[0] === "title") return "<div class=\"title\">" + esc(it[1]) + "</div>";
      return "<button data-i=\"" + i + "\">" + esc(it[0]) + "</button>";
    }).join("");
    m.hidden = false;
    var x = Math.min(e.clientX, innerWidth - m.offsetWidth - 8), y = Math.min(e.clientY, innerHeight - m.offsetHeight - 8);
    m.style.left = x + "px";
    m.style.top = y + "px";
    $$("button", m).forEach(function (b) {
      b.onclick = function () { m.hidden = true; items[Number(b.getAttribute("data-i"))][1](); };
    });
  }

  // ---- export / views -------------------------------------------------------------
  function cliCommand() {
    var parts = ["schema", S.server ? (S.server.rel || S.server.name) : "structure.sql"];
    if (S.base && S.base !== BASEFILE) parts.push(S.compare === WORKTREE ? S.base : S.base + ".." + (S.compare === INDEX ? "INDEX" : S.compare));
    var patch = diffObj(S.cfg, S.defaults);
    delete patch.theme;
    delete patch.positions;
    delete patch.filter_positions;
    if (Object.keys(patch).length) parts.push("--config '" + JSON.stringify(patch).replace(/'/g, "'\\''") + "'");
    return parts.join(" ");
  }
  function bindExport() {
    var menu = $("#export-menu");
    $("#export-btn").onclick = function (e) { e.stopPropagation(); menu.hidden = !menu.hidden; };
    document.addEventListener("click", function (e) {
      if (!e.target.closest(".menu-wrap")) menu.hidden = true;
      if (!e.target.closest(".context-menu")) $("#context-menu").hidden = true;
    });
    var base = function () { return (S.server ? S.server.name : S.playground.name).split("/").pop().replace(/\.sql$/, ""); };
    menu.addEventListener("click", function (e) {
      var b = e.target.closest("[data-export]");
      if (!b) return;
      menu.hidden = true;
      var what = b.getAttribute("data-export");
      if (what === "svg") download(base() + ".svg", new Blob([viewer.exportSvg()], { type: "image/svg+xml" }));
      else if (what === "png") viewer.exportPng(2).then(function (blob) { download(base() + ".png", blob); }, function (err) { toast(err.message); });
      else if (what === "html") {
        if (STATIC) { toast("Use `schema html` from the CLI to create standalone HTML"); return; }
        api("api/export/html", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ base: S.base, compare: S.compare, config: S.cfg }) })
          .then(function (html) { download(base() + ".html", new Blob([html], { type: "text/html" })); }, function (err) { toast(err.message); });
      } else if (what === "markdown") copy(viz.diff_markdown() || "No schema changes.", "diff as Markdown");
      else if (what === "config") { var p = diffObj(S.cfg, S.defaults); delete p.theme; delete p.filter_positions; copy(JSON.stringify(p, null, 2), "view config"); }
      else if (what === "cli") copy(cliCommand(), "CLI command");
      else if (what === "save-view" || what === "save-default") {
        if (STATIC) { toast("Saving views needs the CLI server"); return; }
        var name = what === "save-default" ? null : window.prompt("Name for this view (saved to .schema.json)");
        if (what === "save-view" && !name) return;
        var patch = diffObj(S.cfg, S.defaults);
        delete patch.theme;
        var pc = clone(S.projectCfg || {});
        if (name) { pc.views = pc.views || {}; pc.views[name] = patch; }
        else pc.default = patch;
        api("api/config", { method: "PUT", body: JSON.stringify(pc) }).then(function (r) {
          S.projectCfg = pc;
          renderViews(name || "");
          toast("Saved to " + r.path);
        }, function (err) { toast("Save failed: " + err.message); });
      }
    });
    $("#views-select").addEventListener("change", function (e) {
      var v = e.target.value;
      if (v === "__current__") return;
      var cfg = merge(clone(S.defaults), (S.projectCfg && S.projectCfg.default) || {});
      if (v && S.projectCfg.views && S.projectCfg.views[v]) merge(cfg, S.projectCfg.views[v]);
      S.cfg = cfg;
      syncControls();
      render({ fit: true });
      toast(v ? "View “" + v + "”" : "Project default view");
    });
  }
  function renderViews(selected) {
    var sel = $("#views-select");
    var names = Object.keys((S.projectCfg && S.projectCfg.views) || {});
    if (STATIC || (!names.length && !(S.projectCfg && S.projectCfg.default))) { sel.hidden = true; return; }
    sel.hidden = false;
    sel.innerHTML = "<option value=\"__current__\">views…</option><option value=\"\">default</option>" + names.map(function (n) { return "<option value=\"" + esc(n) + "\">" + esc(n) + "</option>"; }).join("");
    sel.value = selected == null ? "__current__" : selected;
  }

  // ---- keyboard -------------------------------------------------------------
  var ALGS = ["layered", "force", "grid", "circular", "radial"];
  var COLS = ["auto", "all", "keys", "relations", "referenced", "changed", "none"];
  var EDGES = ["curved", "orthogonal", "straight", "hidden"];
  function cycle(list, path) {
    var i = list.indexOf(getPath(S.cfg, path));
    setPath(S.cfg, path, list[(i + 1) % list.length]);
    syncControls();
    render({ preserve: true });
    toast(path.split(".").pop() + ": " + getPath(S.cfg, path), 1200);
  }
  function bindKeys() {
    document.addEventListener("keydown", function (e) {
      var tag = (e.target.tagName || "").toLowerCase();
      if (tag === "input" || tag === "select" || tag === "textarea" || e.metaKey || e.ctrlKey || e.altKey) {
        if ((e.metaKey || e.ctrlKey) && e.key === "k") { e.preventDefault(); $("#search").focus(); }
        if (S.design && (e.metaKey || e.ctrlKey) && e.key === "z" && !e.shiftKey && tag !== "input" && tag !== "textarea" && !$("#table-editor").open) { e.preventDefault(); designUndo(); }
        return;
      }
      var k = e.key;
      if (S.design && k === "n" && !e.metaKey && !e.ctrlKey) { e.preventDefault(); openTableEditor(null); return; }
      if (k === "/") { e.preventDefault(); $("#search").focus(); }
      else if (k === "f") viewer.fit();
      else if (k === "+" || k === "=") viewer.zoomBy(1.25);
      else if (k === "-") viewer.zoomBy(0.8);
      else if (k === "0") viewer.setZoom(1);
      else if (k >= "1" && k <= "5") { S.cfg.layout.algorithm = ALGS[Number(k) - 1]; syncControls(); render({ fit: true }); }
      else if (k === "c") toggleChangesLens(!S.lens);
      else if (k === "k") cycle(COLS, "columns");
      else if (k === "e") cycle(EDGES, "edges.style");
      else if (k === "?") $("#help").showModal();
      else if (k === "Escape") {
        $("#context-menu").hidden = true;
        if (S.selected) closeDetails();
        else if (S.lens) exitLens();
        else if (S.cfg.focus.length) { S.cfg.focus = []; S.cfg.focus_depths = {}; syncControls(); render({ fit: true }); }
      }
    });
  }

  // ---- live reload ------------------------------------------------------------
  function poll() {
    if (STATIC) return;
    setInterval(function () {
      if (document.hidden) return;
      api("api/state").then(function (st) {
        if (st.update && !(S.server.update && S.server.update.message === st.update.message)) { S.server.update = st.update; renderUpdateNote(); }
        if (st.fingerprint === S.server.fingerprint) return;
        S.server = st;
        // drop cached refs that can move (everything but full commit shas)
        Array.from(S.sources.keys()).forEach(function (k) { if (!/^[0-9a-f]{40}\^?$/.test(k)) S.sources.delete(k); });
        loadGit().then(loadSources).then(function () { render({ preserve: true }); toast("Schema updated from disk"); });
      }, function () { /* server gone; keep the last view */ });
    }, 2000);
  }
  function loadGit() {
    if (!S.server || !S.server.is_git) return Promise.resolve();
    return Promise.all([api("api/git/log?limit=200"), api("api/git/refs")]).then(function (r) { S.log = r[0]; S.refs = r[1]; });
  }

  function renderUpdateNote() {
    var u = S.server && S.server.update, el = $("#update-note");
    if (!el) return;
    el.hidden = !(u && u.available);
    if (u && u.available) {
      el.textContent = "Update available" + (u.behind ? " · " + u.behind + " commit" + (u.behind === 1 ? "" : "s") + " behind" : "");
      el.title = u.message + "\nClick to copy: " + u.command;
      el.onclick = function () { copy(u.command, "update command"); };
    }
  }

  function renderFileInfo() {
    if (STATIC) return;
    var s = S.server;
    $("#file-name").textContent = s.name;
    document.title = s.name.split("/").pop() + " — schema";
    var meta = [];
    if (s.repo_name) meta.push(s.repo_name);
    if (s.branch) meta.push("⎇ " + s.branch);
    if (s.head) meta.push(s.head);
    if (s.dirty) meta.push("● modified");
    if (!s.is_git) meta.push("not in git");
    $("#file-meta").textContent = meta.join("  ");
  }

  // ---- playground (no server) ---------------------------------------------------
  function setupPlayground() {
    var bar = document.createElement("div");
    bar.className = "compare";
    bar.innerHTML = "<button class=\"btn\" id=\"pg-open\">Open .sql…</button><button class=\"btn\" id=\"pg-base\">Compare with…</button>" +
      "<button class=\"btn\" id=\"pg-clear-base\" hidden>× base</button><span class=\"diff-pills\" id=\"diff-pills\"></span>" +
      ((STATIC.examples || []).length ? "<select id=\"pg-examples\" class=\"btn\"><option value=\"\">Examples…</option>" + STATIC.examples.map(function (x, i) { return "<option value=\"" + i + "\">" + esc(x.name) + "</option>"; }).join("") + "</select>" : "") +
      "<input type=\"file\" id=\"pg-file\" accept=\".sql,text/plain\" hidden>";
    $("#compare-bar").replaceWith(bar);
    var target = "current";
    var fileInput = $("#pg-file");
    $("#pg-open").onclick = function () { target = "current"; fileInput.click(); };
    $("#pg-base").onclick = function () { target = "base"; fileInput.click(); };
    $("#pg-clear-base").onclick = function () { S.playground.base = null; S.base = null; refreshPlayground(); };
    fileInput.onchange = function () {
      var f = fileInput.files[0];
      if (!f) return;
      f.text().then(function (t) { loadText(target, t, f.name); });
      fileInput.value = "";
    };
    var ex = $("#pg-examples");
    if (ex) ex.onchange = function () {
      var x = STATIC.examples[Number(ex.value)];
      ex.value = "";
      if (x) loadExample(x);
    };
    var canvas = $("#canvas");
    canvas.addEventListener("dragover", function (e) { e.preventDefault(); });
    canvas.addEventListener("drop", function (e) {
      e.preventDefault();
      var files = Array.prototype.slice.call(e.dataTransfer.files);
      if (!files.length) return;
      Promise.all(files.map(function (f) { return f.text().then(function (t) { return [f.name, t]; }); })).then(function (all) {
        if (all.length >= 2) { loadText("base", all[0][1], all[0][0], true); loadText("current", all[1][1], all[1][0]); }
        else loadText("current", all[0][1], all[0][0]);
      });
    });
  }
  function loadExample(x) {
    var tasks = [fetch(x.url).then(function (r) { return r.text(); })];
    if (x.base_url) tasks.push(fetch(x.base_url).then(function (r) { return r.text(); }));
    return Promise.all(tasks).then(function (r) {
      S.playground.current = r[0];
      S.playground.name = x.url.split("/").pop();
      S.playground.base = r[1] || null;
      S.playground.baseName = x.base_url ? x.base_url.split("/").pop() : null;
      S.base = r[1] ? BASEFILE : null;
      if (x.config) { S.cfg = merge(clone(S.defaults), x.config); syncControls(); }
      return refreshPlayground(true);
    });
  }
  function loadText(which, text, name, quiet) {
    if (which === "base") { S.playground.base = text; S.playground.baseName = name; S.base = BASEFILE; }
    else { S.playground.current = text; S.playground.name = name; }
    if (!quiet) refreshPlayground(true);
  }
  function refreshPlayground(fit) {
    S.lens = null;
    $("#file-name").textContent = S.playground.name;
    $("#file-meta").textContent = S.playground.base ? "compared with " + S.playground.baseName : "playground — files never leave your browser";
    $("#pg-clear-base").hidden = !S.playground.base;
    return loadSources().then(function () {
      // like the CLI: a comparison opens on what changed
      if (fit && S.base && S.diff && S.diff.tables.length) startLens({ kind: "diff", label: "Changed tables" });
      render({ fit: !!fit });
    });
  }

  // ---- temporary views ------------------------------------------------------------
  // A temporary view is a feature's own way of showing something: a commit's
  // changes, a diff opened from the CLI, "only changed tables". It pauses the
  // user's filters instead of changing them; leaving it restores everything.
  function userFiltersActive() {
    return !!(S.cfg.focus.length || S.cfg.include.length || manualExcludes().length || S.cfg.schemas.length || !S.cfg.show_isolated);
  }
  /** The filters actually applied: the user's, the view's, or both combined. */
  function effectiveCfg() {
    var c = clone(S.cfg);
    delete c.filter_positions;
    c.changes_only = false;
    if (S.lens) {
      if (!S.lens.combine) {
        c.focus = []; c.focus_depths = {}; c.include = []; c.exclude = clone(S.defaults.exclude);
        c.schemas = []; c.show_isolated = true; c.focus_direction = "both";
      }
      c.changes_only = true;
      c.changes_context = S.lens.context;
    }
    return c;
  }
  function startLens(l) {
    // every view starts fresh: changed tables only, your filters paused;
    // nothing carries over from a previous view
    l.context = l.context != null ? l.context : S.cfg.changes_context;
    l.combine = !!l.combine;
    S.lens = l;
    syncControls();
  }
  /** Leave the temporary view: user filters (and the previous comparison) come back. */
  function exitLens() {
    var prev = S.lens && S.lens.prev;
    S.lens = null;
    syncControls();
    if (prev) setComparison(prev.base, prev.compare, { fit: true });
    else render({ fit: true });
  }
  /** Leave the view but keep the current comparison (e.g. to show a table it hides). */
  function dropLens(why) {
    if (!S.lens) return;
    S.lens = null;
    if (why) toast(why, 2600);
  }
  function toggleChangesLens(on) {
    if (on && !(S.diff && (S.base || S.design))) { toast("Pick something to compare first"); syncControls(); return; }
    if (on) { startLens({ kind: "changes", label: "Changed tables" }); render({ fit: true }); }
    else exitLens();
  }

  // ---- filter bar (over the diagram) ---------------------------------------------
  // Tables are filtered by "focus" patterns, each with its own neighbour depth,
  // plus include / exclude patterns, schemas, changes-only and isolated tables.
  function fdepth(p) {
    var d = S.cfg.focus_depths && S.cfg.focus_depths[p];
    return d == null ? S.cfg.focus_depth : d;
  }
  function setFocus(pattern, depth, add) {
    S.cfg.focus_depths = S.cfg.focus_depths || {};
    if (!add) { S.cfg.focus = []; S.cfg.focus_depths = {}; }
    if (S.cfg.focus.indexOf(pattern) < 0) S.cfg.focus.push(pattern);
    if (depth == null) delete S.cfg.focus_depths[pattern];
    else S.cfg.focus_depths[pattern] = Math.max(0, depth);
  }
  function removeFocus(p) {
    S.cfg.focus = S.cfg.focus.filter(function (x) { return x !== p; });
    if (S.cfg.focus_depths) delete S.cfg.focus_depths[p];
  }
  /** The focus entry naming exactly this table, if any. */
  function patternFor(id) {
    return S.cfg.focus.find(function (p) { return p === id || qid(p) === id; }) || null;
  }
  function manualExcludes() {
    return S.cfg.exclude.filter(function (x) { return S.defaults.exclude.indexOf(x) < 0; });
  }
  function filterActive() { return !!S.lens || userFiltersActive(); }
  function applyFilter(o) {
    syncControls();
    render({ fit: !(o && o.preserve), preserve: !!(o && o.preserve) });
  }
  function patternFromInput(v) {
    v = v.trim();
    if (!v) return null;
    var exact = S.index.find(function (t) { return t.label === v || t.id === v; });
    if (exact || v.indexOf("*") >= 0 || v.indexOf("?") >= 0) return exact ? exact.label : v;
    return "*" + v + "*";
  }

  function renderFilterBar() {
    var bar = $("#filter-bar");
    if (!bar || !S.cfg) return;
    var st = (S.result && S.result.stats) || {};
    var active = filterActive();
    bar.classList.toggle("active", active);
    var chip = function (kind, label, extra, title) {
      return "<span class=\"fchip " + kind + "\" title=\"" + esc(title || "") + "\">" + label + (extra || "") + "</span>";
    };
    var x = function (attr) { return "<button class=\"fx\" " + attr + " title=\"Remove\">×</button>"; };
    var parts = [];
    var paused = !!(S.lens && !S.lens.combine);
    if (S.lens) {
      var L = S.lens, c = L.context;
      parts.push("<span class=\"fchip lens\" title=\"A temporary view: your own filters are " + (paused ? "paused" : "applied too") + ". × returns to what you had before.\">" +
        "<span class=\"llbl\">" + esc(L.label) + "</span>" + (L.kind === "commit" ? "<span class=\"lsub\">changed tables</span>" : "") +
        "<span class=\"fdepth\"><button data-lens-dec" + (c ? "" : " disabled") + " title=\"Fewer neighbours\">−</button><span>" + (c ? "+" + c + " hop" + (c > 1 ? "s" : "") : "only") + "</span><button data-lens-inc title=\"More neighbours\">+</button></span>" +
        (userFiltersActive() ? "<label class=\"lcomb\" title=\"Also apply your own filters\"><input type=\"checkbox\" data-lens-combine" + (L.combine ? " checked" : "") + "> + my filters</label>" : "") +
        "<button class=\"fx\" data-lens-exit title=\"Leave this view" + (L.prev ? " (back to " + refLabel(L.prev.base) + " → " + refLabel(L.prev.compare) + ")" : "") + "\">×</button></span>");
      if (paused && userFiltersActive()) parts.push("<span class=\"fpaused\" title=\"Your filters are kept and come back when you leave the view\">your filters paused:</span>");
    }
    S.cfg.focus.forEach(function (p) {
      var d = fdepth(p), n = (st.focus_matches || {})[p];
      var multi = n != null && (p.indexOf("*") >= 0 || p.indexOf("?") >= 0);
      parts.push(chip("focus",
        "<button class=\"flbl\" data-center=\"" + esc(p) + "\" title=\"Show on the diagram\">" + esc(display(p)) + "</button>" +
        (multi ? "<span class=\"fn\">" + n + "</span>" : n === 0 ? "<span class=\"fn warn\" title=\"matches no table\">0</span>" : ""),
        "<span class=\"fdepth\"><button data-dec=\"" + esc(p) + "\" title=\"Fewer neighbours\"" + (d ? "" : " disabled") + ">−</button>" +
        "<span title=\"Neighbours: tables up to this many relations away" + (S.cfg.focus_depths && S.cfg.focus_depths[p] != null ? "" : " (default depth, set in Options)") + "\">" + (d ? "+" + d + " hop" + (d > 1 ? "s" : "") : "only") + "</span>" +
        "<button data-inc=\"" + esc(p) + "\" title=\"More neighbours\">+</button></span>" + x("data-rm-focus=\"" + esc(p) + "\""),
        "Showing " + display(p) + (d ? " and tables up to " + d + " relation" + (d > 1 ? "s" : "") + " away" : " only")));
    });
    if (S.cfg.focus.length && S.cfg.focus_direction !== "both") {
      parts.push(chip("opt", S.cfg.focus_direction === "outgoing" ? "neighbours: referenced only" : "neighbours: referencing only", x("data-rm=\"direction\"")));
    }
    S.cfg.include.forEach(function (p, i) { parts.push(chip("inc", "only <b>" + esc(p) + "</b>", x("data-rm-inc=\"" + i + "\""), "Only tables matching " + p)); });
    var ex = manualExcludes();
    var shown = S.fbAllHidden ? ex : ex.slice(0, 3);
    shown.forEach(function (p) { parts.push(chip("exc", "hidden <b>" + esc(display(p)) + "</b>", x("data-rm-exc=\"" + esc(p) + "\""), "Hidden: " + p)); });
    if (ex.length > shown.length) parts.push("<button class=\"fmore\" data-all-hidden>+" + (ex.length - shown.length) + " hidden</button>");
    if (S.cfg.schemas.length) parts.push(chip("opt", "schemas <b>" + esc(S.cfg.schemas.join(", ")) + "</b>", x("data-rm=\"schemas\"")));
    if (!S.cfg.show_isolated) parts.push(chip("opt", "no isolated tables", x("data-rm=\"isolated\"")));
    var total = (st.tables_total || 0) + (S.cfg.show_views ? st.views_total || 0 : 0);
    if (paused) parts = parts.map(function (h, i) { return i === 0 || h.indexOf("fpaused") >= 0 ? h : h.replace(/class="(fchip|fmore)/, 'class="$1 paused'); });
    bar.classList.toggle("lensed", !!S.lens);
    bar.innerHTML =
      "<span class=\"fb-icon\" title=\"Filter\">⧩</span>" + parts.join("") +
      "<input id=\"fb-input\" list=\"table-names\" autocomplete=\"off\" spellcheck=\"false\" placeholder=\"" + (parts.length ? "add table…" : "Show only… (table or pattern)") + "\">" +
      "<button class=\"fb-btn\" id=\"fb-options\" title=\"More filters\">Options ▾</button>" +
      (active ? "<button class=\"fb-count\" id=\"fb-why\" title=\"What is hidden and why\">" + ((st.nodes_visible || 0) - (st.enums_visible || 0)) + " of " + total + " tables ▾</button>" +
        (userFiltersActive() ? "<button class=\"fb-btn clear\" id=\"fb-clear\" title=\"Remove your filters\">Clear</button>" : "") : "");
  }

  function bindFilterBar() {
    var bar = $("#filter-bar");
    bar.addEventListener("keydown", function (e) {
      if (e.target.id !== "fb-input") return;
      if (e.key === "Enter") {
        var p = patternFromInput(e.target.value);
        if (!p) return;
        // typing a table means "show me this": a temporary view would hide it
        if (S.lens && !S.lens.combine) dropLens("Left the " + S.lens.label.toLowerCase() + " view");
        setFocus(p, null, true);
        applyFilter();
        setTimeout(function () { var i = $("#fb-input"); if (i) i.focus(); }, 30);
      } else if (e.key === "Escape") { e.target.value = ""; e.target.blur(); }
      else if (e.key === "Backspace" && !e.target.value && S.cfg.focus.length) {
        removeFocus(S.cfg.focus[S.cfg.focus.length - 1]);
        applyFilter();
        setTimeout(function () { var i = $("#fb-input"); if (i) i.focus(); }, 30);
      }
    });
    // picking a datalist suggestion fires `input` with the full name
    bar.addEventListener("input", function (e) {
      if (e.target.id !== "fb-input" || !(e.inputType === "insertReplacementText" || e.inputType == null)) return;
      var v = e.target.value.trim();
      if (S.index.some(function (t) { return t.label === v; })) { setFocus(v, null, true); applyFilter(); setTimeout(function () { var i = $("#fb-input"); if (i) i.focus(); }, 30); }
    });
    bar.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (!b) return;
      var a = function (n) { return b.getAttribute(n); };
      if (b.hasAttribute("data-inc")) { S.cfg.focus_depths[a("data-inc")] = fdepth(a("data-inc")) + 1; applyFilter(); }
      else if (b.hasAttribute("data-dec")) { S.cfg.focus_depths[a("data-dec")] = Math.max(0, fdepth(a("data-dec")) - 1); applyFilter(); }
      else if (b.hasAttribute("data-rm-focus")) { removeFocus(a("data-rm-focus")); applyFilter(); }
      else if (b.hasAttribute("data-rm-inc")) { S.cfg.include.splice(Number(a("data-rm-inc")), 1); applyFilter(); }
      else if (b.hasAttribute("data-rm-exc")) { S.cfg.exclude = S.cfg.exclude.filter(function (x) { return x !== a("data-rm-exc"); }); applyFilter({ preserve: true }); }
      else if (b.hasAttribute("data-all-hidden")) { S.fbAllHidden = true; renderFilterBar(); }
      else if (b.hasAttribute("data-center")) {
        var p = a("data-center");
        var hit = S.result.nodes.find(function (n) { return n.id === qid(p) || n.label === p; }) ||
          S.result.nodes.find(function (n) { return fbMatch(p, n.id); });
        if (hit) selectTable(hit.id, { center: true });
      } else if (a("data-rm") === "direction") { S.cfg.focus_direction = "both"; applyFilter(); }
      else if (a("data-rm") === "schemas") { S.cfg.schemas = []; applyFilter(); }
      else if (b.hasAttribute("data-lens-exit")) exitLens();
      else if (b.hasAttribute("data-lens-inc")) { S.lens.context += 1; applyFilter(); }
      else if (b.hasAttribute("data-lens-dec")) { S.lens.context = Math.max(0, S.lens.context - 1); applyFilter(); }
      else if (a("data-rm") === "isolated") { S.cfg.show_isolated = true; applyFilter(); }
      else if (b.id === "fb-clear") { clearFilters(); }
      else if (b.id === "fb-options") { togglePop("options", b); }
      else if (b.id === "fb-why") { togglePop("why", b); }
    });
    bar.addEventListener("change", function (e) {
      if (e.target.hasAttribute("data-lens-combine") && S.lens) { S.lens.combine = e.target.checked; applyFilter(); }
    });
    document.addEventListener("click", function (e) {
      if (!e.target.closest("#fb-pop,#fb-options,#fb-why")) $("#fb-pop").hidden = true;
    });
    $("#fb-pop").addEventListener("click", function (e) {
      var b = e.target.closest("[data-unhide]");
      if (b) { S.cfg.exclude = S.cfg.exclude.filter(function (x) { return x !== b.getAttribute("data-unhide"); }); applyFilter({ preserve: true }); renderPop(); }
    });
    $("#fb-pop").addEventListener("change", onPopInput);
    $("#fb-pop").addEventListener("input", function (e) { if (e.target.type === "text") { clearTimeout(S.popT); S.popT = setTimeout(function () { onPopInput(e); }, 350); } });
  }

  function fbMatch(p, id) {
    var re = new RegExp("^" + p.toLowerCase().replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*").replace(/\?/g, ".") + "$");
    return p.indexOf(".") >= 0 ? re.test(id.toLowerCase()) : re.test(id.split(".").slice(1).join(".").toLowerCase());
  }

  function togglePop(kind, anchor) {
    var pop = $("#fb-pop");
    if (!pop.hidden && S.popKind === kind) { pop.hidden = true; return; }
    S.popKind = kind;
    renderPop();
    pop.hidden = false;
    var c = $("#canvas").getBoundingClientRect(), r = anchor.getBoundingClientRect();
    pop.style.top = (r.bottom - c.top + 6) + "px";
    pop.style.left = Math.max(8, Math.min(r.left - c.left, c.width - pop.offsetWidth - 8)) + "px";
  }

  function renderPop() {
    var pop = $("#fb-pop"), st = (S.result && S.result.stats) || {}, h = "";
    if (S.popKind === "why") {
      var hd = st.hidden || {};
      var rows = [
        [hd.outside_focus, "not connected closely enough to the filtered tables"],
        [hd.exclude, "hidden by name (incl. " + S.defaults.exclude.length + " Rails bookkeeping tables by default)"],
        [hd.include, "don't match the “only” patterns"],
        [hd.schema, "in other schemas"],
        [hd.unchanged, "unchanged (not part of " + (S.lens ? S.lens.label : "the changes") + ")"],
        [hd.isolated, "without relations"],
        [hd.partitions, "partitions folded into their parent table"],
      ].filter(function (r) { return r[0]; });
      if (!S.cfg.show_views && st.views_total) rows.push([st.views_total, "views (turn on in Display → Objects)"]);
      h = "<h4>Not shown</h4>" + (rows.length ? "<ul class=\"why\">" + rows.map(function (r) { return "<li><b>" + r[0] + "</b> " + esc(r[1]) + "</li>"; }).join("") + "</ul>" : "<p class=\"muted\">Nothing is hidden.</p>");
      var ex = manualExcludes();
      if (ex.length) {
        h += "<h4>Hidden tables</h4><ul class=\"why\">" + ex.map(function (x) { return "<li><code>" + esc(display(x)) + "</code> <button class=\"btn small\" data-unhide=\"" + esc(x) + "\">show</button></li>"; }).join("") + "</ul>";
      }
    } else {
      var hasDiff = st.has_diff;
      h = "<h4>Neighbours of filtered tables</h4>" +
        "<label class=\"row\" title=\"How many relations away to show, for tables in the filter without their own depth (use − / + on a chip to override)\"><span>Depth</span><select data-pop=\"focus_depth\">" +
        [0, 1, 2, 3, 4, 5].map(function (n) { return "<option value=\"" + n + "\"" + (S.cfg.focus_depth === n ? " selected" : "") + ">" + (n ? "+" + n + " hop" + (n > 1 ? "s" : "") : "none") + "</option>"; }).join("") + "</select></label>" +
        "<label class=\"row\"><span>Direction</span><select data-pop=\"focus_direction\"><option value=\"both\">both ways</option><option value=\"outgoing\">tables they reference</option><option value=\"incoming\">tables referencing them</option></select></label>" +
        "<h4>Patterns</h4>" +
        "<label class=\"row col\"><span>Only tables matching</span><input type=\"text\" data-pop=\"include\" placeholder=\"billing.*, user*\" value=\"" + esc(S.cfg.include.join(", ")) + "\"></label>" +
        "<label class=\"row col\"><span>Hide tables matching</span><input type=\"text\" data-pop=\"exclude\" placeholder=\"audit_*\" value=\"" + esc(S.cfg.exclude.join(", ")) + "\"></label>" +
        ((S.schemaNames || []).length > 1 ? "<h4>Schemas</h4><div class=\"schema-checks\">" + S.schemaNames.map(function (s) {
          return "<label class=\"check\"><input type=\"checkbox\" data-pop-schema=\"" + esc(s) + "\"" + (!S.cfg.schemas.length || S.cfg.schemas.indexOf(s) >= 0 ? " checked" : "") + "> " + esc(s) + "</label>";
        }).join("") + "</div>" : "") +
        "<h4>More</h4>" +
        (hasDiff ? "<label class=\"check\"><input type=\"checkbox\" data-pop=\"changes_only\"" + (S.lens ? " checked" : "") + "> Only changed tables <span class=\"muted\">(temporary view)</span></label>" : "") +
        "<label class=\"check\"><input type=\"checkbox\" data-pop=\"hide_isolated\"" + (S.cfg.show_isolated ? "" : " checked") + "> Hide tables without relations</label>";
    }
    pop.innerHTML = h;
    var dir = pop.querySelector("[data-pop=focus_direction]");
    if (dir) dir.value = S.cfg.focus_direction;
  }

  function onPopInput(e) {
    var el = e.target, k = el.getAttribute("data-pop");
    if (el.hasAttribute("data-pop-schema")) {
      var all = $$("[data-pop-schema]", $("#fb-pop"));
      var on = all.filter(function (x) { return x.checked; }).map(function (x) { return x.getAttribute("data-pop-schema"); });
      S.cfg.schemas = on.length === all.length ? [] : on;
    } else if (k === "include" || k === "exclude") {
      S.cfg[k] = splitList(el.value);
    } else if (k === "focus_direction") S.cfg.focus_direction = el.value;
    else if (k === "focus_depth") S.cfg.focus_depth = Number(el.value);
    else if (k === "changes_only") { toggleChangesLens(el.checked); renderPop(); return; }
    else if (k === "hide_isolated") S.cfg.show_isolated = !el.checked;
    else return;
    applyFilter();
  }

  // ---- design mode ------------------------------------------------------------------
  // A design is a list of operations applied on top of the loaded schema (in
  // Rust); the view shows the result as a diff against the loaded schema.
  var TYPES = ["bigint", "bigserial", "integer", "smallint", "numeric(12,2)", "text", "varchar(255)", "citext", "boolean", "uuid", "jsonb",
    "date", "timestamp(6)", "timestamptz", "inet", "bytea", "double precision", "integer[]", "text[]"];
  var ON_DELETE = ["", "CASCADE", "SET NULL", "RESTRICT", "NO ACTION"];

  function qid(t) { t = String(t || "").trim(); return t.indexOf(".") >= 0 ? t : "public." + t; }
  function toId(t, to) { return to.indexOf(".") >= 0 ? to : t.split(".")[0] + "." + to; }
  function splitList(s) { return String(s || "").split(",").map(function (x) { return x.trim(); }).filter(Boolean); }
  function designKey() { return "schema:design:" + (S.server ? S.server.file : "playground"); }
  function saveDesignDraft() {
    try {
      if (S.design) localStorage.setItem(designKey(), JSON.stringify({ design: S.design, slug: S.designSlug, dirty: S.designDirty }));
      else localStorage.removeItem(designKey());
    } catch (e) { /* storage full or disabled */ }
  }
  function loadDesignDraft() {
    try { return JSON.parse(localStorage.getItem(designKey()) || "null"); } catch (e) { return null; }
  }

  /** Config used for rendering: in design mode the design's own positions win
   *  and unchanged tables keep their columns (you design against them). */
  function viewCfg() {
    var c = effectiveCfg();
    if (filterActive()) c.positions = Object.assign({}, positionStore(false));
    else if (S.design) c.positions = Object.assign({}, S.cfg.positions, S.design.positions || {});
    if (S.design) c.unchanged_columns = null;
    return c;
  }

  // Dragged positions belong to the view they were made in: the unfiltered
  // diagram keeps `positions` (and a design's frozen layout), each filter
  // gets its own arrangement so filtered tables are laid out afresh.
  function filterKey() {
    var c = effectiveCfg();
    return JSON.stringify([c.focus, c.focus_depths || {}, c.focus_depth, c.focus_direction, c.include, c.exclude, c.schemas,
      c.changes_only, c.changes_context, c.show_isolated]);
  }
  function positionStore(create) {
    if (!filterActive()) return S.design ? S.design.positions : S.cfg.positions;
    var owner = S.design || S.cfg, k = filterKey();
    owner.filter_positions = owner.filter_positions || {};
    if (!owner.filter_positions[k]) {
      if (!create) return {};
      var keys = Object.keys(owner.filter_positions);
      if (keys.length >= 20) delete owner.filter_positions[keys[0]];
      owner.filter_positions[k] = {};
    }
    return owner.filter_positions[k];
  }
  function resetPositions() {
    if (filterActive()) {
      var owner = S.design || S.cfg;
      if (owner.filter_positions) delete owner.filter_positions[filterKey()];
    } else if (S.design) {
      S.design.positions = {};
    } else {
      S.cfg.positions = {};
    }
    if (S.design) { S.designDirty = true; saveDesignDraft(); }
    render({ preserve: true });
    if (S.design && !filterActive()) setTimeout(function () { snapshotPositions(); saveDesignDraft(); }, 200);
  }

  function designSync(o) {
    o = o || {};
    var res = JSON.parse(viz.set_design(JSON.stringify(S.design)));
    if (res.error) { toast(res.error, 5000); return; }
    S.designState = res;
    if (o.dirty !== false) { S.designDirty = true; S.design.updated = new Date().toISOString(); }
    saveDesignDraft();
    buildIndex();
    S.diff = JSON.parse(viz.diff());
    renderChanges();
    renderDesignPanel();
    updateCompareUI();
    if (o.render !== false) render({ preserve: true });
  }

  function designEdit(fn) {
    S.designUndo.push(JSON.stringify({ ops: S.design.ops, notes: S.design.notes, positions: S.design.positions }));
    if (S.designUndo.length > 200) S.designUndo.shift();
    fn();
    designSync();
  }
  function designUndo() {
    var u = S.designUndo.pop();
    if (!u) { toast("Nothing to undo", 1200); return; }
    var x = JSON.parse(u);
    S.design.ops = x.ops;
    S.design.notes = x.notes;
    S.design.positions = x.positions;
    designSync();
    toast("Undone", 1000);
  }

  function snapshotPositions() {
    S.design.positions = {};
    viewer.nodes.forEach(function (n, id) { S.design.positions[id] = [Math.round(n.x), Math.round(n.y)]; });
  }

  function startDesign(d, o) {
    o = o || {};
    d.version = d.version || 1;
    d.ops = d.ops || [];
    d.notes = d.notes || {};
    d.positions = d.positions || {};
    d.base_tables = d.base_tables || [];
    S.design = d;
    S.designSlug = o.slug || null;
    S.designUndo = [];
    S.designDirty = !!o.dirty;
    S.lens = null;
    var freeze = !Object.keys(d.positions).length;
    designSync({ dirty: !!o.dirty, render: false });
    // Freeze the layout (as rendered in design mode, with all columns) so
    // tables don't jump around while editing.
    renderNow({ preserve: true });
    // (only the unfiltered diagram is frozen; filtered views lay out afresh)
    if (freeze && viewer.nodes.size && !filterActive()) { snapshotPositions(); saveDesignDraft(); }
    showTab("design");
  }

  function newDesign(name) {
    startDesign({
      version: 1, name: name, description: "",
      source: { file: S.server ? (S.server.rel || S.server.name) : S.playground.name, ref: STATIC ? null : S.compare, commit: S.server ? S.server.head : null },
      ops: [], notes: {}, positions: {}, created: new Date().toISOString(),
    }, { dirty: true });
  }

  function closeDesign() {
    if (S.designDirty && !window.confirm("Close the design? Unsaved changes are kept only in this browser's draft until you start another design.")) return;
    S.design = null;
    S.designSlug = null;
    viz.clear_design();
    saveDesignDraft();
    buildIndex();
    S.diff = JSON.parse(viz.diff());
    renderChanges();
    renderDesignPanel();
    updateCompareUI();
    render({ preserve: true });
  }

  /** Map every op to the table "entity" it touches, following renames.
   *  Created tables are keyed `c<op index>`, existing ones `e:<original id>`. */
  function designEntities() {
    var names = {}, ent = [];
    S.design.ops.forEach(function (op, i) {
      var t = qid(op.table);
      if (op.op === "create_table") { names[t] = "c" + i; ent.push("c" + i); return; }
      var k = names[t] || "e:" + t;
      ent.push(k);
      if (op.op === "rename_table") { names[toId(t, op.to)] = k; delete names[t]; }
      if (op.op === "drop_table") delete names[t];
    });
    return { ent: ent, names: names };
  }
  function entityKey(id) { return designEntities().names[id] || "e:" + id; }
  function isCreated(id) { return entityKey(id).charAt(0) === "c"; }

  function freeSpot() {
    var x = 0, y = Infinity;
    viewer.nodes.forEach(function (n) { x = Math.max(x, n.x + n.w); y = Math.min(y, n.y); });
    return [Math.round(x + 80), Math.round(isFinite(y) ? y : 0)];
  }

  function dropTable(id) {
    if (isCreated(id)) {
      var key = entityKey(id), ents = designEntities().ent;
      designEdit(function () {
        S.design.ops = S.design.ops.filter(function (op, i) { return ents[i] !== key; });
        delete S.design.notes[id];
        delete S.design.positions[id];
      });
      toast("Removed " + display(id) + " from the design");
    } else {
      designEdit(function () { S.design.ops.push({ op: "drop_table", table: id }); });
      toast("Dropping " + display(id) + " — undo with ⌘Z");
    }
    closeDetails();
  }

  // ---- table editor -------------------------------------------------------------
  function openTableEditor(id, o) {
    o = o || {};
    var t = null;
    if (id) {
      var d = JSON.parse(viz.table(id));
      t = d.table;
      if (!t || d.status === "removed") { toast("This table is dropped in the design — undo the drop to edit it"); return; }
    }
    var pk = t && t.primary_key ? t.primary_key.columns : [];
    var st = t ? {
      name: display(id), comment: t.comment || "", note: S.design.notes[id] || "",
      columns: t.columns.map(function (c) { return { orig: c.name, name: c.name, type: c.data_type, nullable: c.nullable, default: c.default || "", pk: pk.indexOf(c.name) >= 0 }; }),
      fks: (t.foreign_keys || []).map(function (f) { return { orig: f.name || "", columns: f.columns.join(", "), ref: display(f.ref_table), ref_columns: f.ref_columns.join(", "), on_delete: f.on_delete || "" }; }),
      indexes: (t.indexes || []).map(function (x) { return { orig: x.name, columns: x.columns.join(", "), unique: x.unique, where: x.predicate || "" }; }),
    } : {
      name: o.name || "", comment: "", note: "",
      columns: [
        { name: "id", type: "bigserial", nullable: false, default: "", pk: true },
        { name: "created_at", type: "timestamp(6)", nullable: false, default: "", pk: false },
        { name: "updated_at", type: "timestamp(6)", nullable: false, default: "", pk: false },
      ],
      fks: [], indexes: [],
    };
    if (o.addColumn) st.columns.push({ name: "", type: "text", nullable: true, default: "", pk: false, focus: true });
    S.editor = { id: id || null, created: id ? isCreated(id) : true, orig: t, st: st, pos: o.pos || null, error: "" };
    drawEditor();
    $("#table-editor").showModal();
    var f = $("#table-editor [data-focus]") || $("#table-editor [data-f=name]");
    if (f) f.focus();
  }

  function drawEditor() {
    var ed = S.editor, st = ed.st, dlg = $("#table-editor");
    var enumTypes = (S.enums || []).map(display);
    var cell = function (sec, i, f, val, attrs) {
      return "<input data-sec=\"" + sec + "\" data-i=\"" + i + "\" data-f=\"" + f + "\" value=\"" + esc(val) + "\" " + (attrs || "") + ">";
    };
    var check = function (sec, i, f, on, title) {
      return "<input type=\"checkbox\" data-sec=\"" + sec + "\" data-i=\"" + i + "\" data-f=\"" + f + "\"" + (on ? " checked" : "") + " title=\"" + title + "\">";
    };
    var del = function (sec, i) { return "<button type=\"button\" class=\"te-del\" data-del=\"" + sec + "\" data-i=\"" + i + "\" title=\"Remove\">✕</button>"; };
    var h = "<form method=\"dialog\" class=\"te\">" +
      "<h3>" + (ed.id ? "Edit table" : "New table") + (ed.id && !ed.created ? " <span class=\"te-sub\">changes are recorded as operations</span>" : "") + "</h3>" +
      "<div class=\"te-top\"><label>Table name<input data-f=\"name\" value=\"" + esc(st.name) + "\" placeholder=\"cards or billing.cards\" spellcheck=\"false\"></label>" +
      "<label>Comment<input data-f=\"comment\" value=\"" + esc(st.comment) + "\" placeholder=\"What is stored here\"></label></div>" +
      "<h4>Columns</h4><div class=\"te-scroll\"><table class=\"te-grid\"><thead><tr><th>Name</th><th>Type</th><th title=\"NOT NULL\">Required</th><th>Default</th><th title=\"Primary key\">PK</th><th></th></tr></thead><tbody>" +
      st.columns.map(function (c, i) {
        return "<tr" + (c.orig ? "" : " class=\"te-new\"") + "><td>" + cell("columns", i, "name", c.name, "spellcheck=\"false\" placeholder=\"column_name\"" + (c.focus ? " data-focus" : "")) + "</td>" +
          "<td>" + cell("columns", i, "type", c.type, "list=\"te-types\" spellcheck=\"false\"") + "</td>" +
          "<td class=\"c\">" + check("columns", i, "notnull", !c.nullable, "NOT NULL") + "</td>" +
          "<td>" + cell("columns", i, "default", c.default, "spellcheck=\"false\" placeholder=\"none\"") + "</td>" +
          "<td class=\"c\">" + check("columns", i, "pk", c.pk, "Primary key") + "</td><td>" + del("columns", i) + "</td></tr>";
      }).join("") + "</tbody></table></div><button type=\"button\" class=\"btn small\" data-add=\"columns\">+ Column</button>" +
      "<h4>Foreign keys</h4>" + (st.fks.length ? "<div class=\"te-scroll\"><table class=\"te-grid\"><thead><tr><th>Column(s)</th><th>References table</th><th>Column(s)</th><th>On delete</th><th></th></tr></thead><tbody>" +
      st.fks.map(function (f, i) {
        return "<tr" + (f.orig ? "" : " class=\"te-new\"") + "><td>" + cell("fks", i, "columns", f.columns, "list=\"te-cols\" spellcheck=\"false\" placeholder=\"user_id\"") + "</td>" +
          "<td>" + cell("fks", i, "ref", f.ref, "list=\"table-names\" spellcheck=\"false\" placeholder=\"users\"") + "</td>" +
          "<td>" + cell("fks", i, "ref_columns", f.ref_columns, "spellcheck=\"false\" placeholder=\"id (primary key)\"") + "</td>" +
          "<td><select data-sec=\"fks\" data-i=\"" + i + "\" data-f=\"on_delete\">" + ON_DELETE.map(function (x) { return "<option value=\"" + x + "\"" + (x === (f.on_delete || "") ? " selected" : "") + ">" + (x || "—") + "</option>"; }).join("") + "</select></td>" +
          "<td>" + del("fks", i) + "</td></tr>";
      }).join("") + "</tbody></table></div>" : "") + "<button type=\"button\" class=\"btn small\" data-add=\"fks\">+ Foreign key</button>" +
      "<h4>Indexes</h4>" + (st.indexes.length ? "<div class=\"te-scroll\"><table class=\"te-grid\"><thead><tr><th>Column(s)</th><th>Unique</th><th>Where (partial)</th><th></th></tr></thead><tbody>" +
      st.indexes.map(function (x, i) {
        return "<tr" + (x.orig ? "" : " class=\"te-new\"") + "><td>" + cell("indexes", i, "columns", x.columns, "list=\"te-cols\" spellcheck=\"false\" placeholder=\"account_id, created_at\"") + "</td>" +
          "<td class=\"c\">" + check("indexes", i, "unique", x.unique, "Unique") + "</td>" +
          "<td>" + cell("indexes", i, "where", x.where, "spellcheck=\"false\" placeholder=\"deleted_at IS NULL\"") + "</td><td>" + del("indexes", i) + "</td></tr>";
      }).join("") + "</tbody></table></div>" : "") + "<button type=\"button\" class=\"btn small\" data-add=\"indexes\">+ Index</button>" +
      "<h4>Note for the implementer</h4><textarea data-f=\"note\" rows=\"2\" placeholder=\"Intent, constraints, backfill or data-migration hints\">" + esc(st.note) + "</textarea>" +
      "<div class=\"te-err\"" + (ed.error ? "" : " hidden") + ">" + esc(ed.error) + "</div>" +
      "<div class=\"te-actions\">" + (ed.id ? "<button type=\"button\" class=\"btn danger\" data-act=\"drop\">" + (ed.created ? "Remove table" : "Drop table") + "</button>" : "") +
      "<span class=\"spacer\"></span><button class=\"btn\" value=\"cancel\">Cancel</button><button type=\"button\" class=\"btn primary\" data-act=\"save\">" + (ed.id ? "Apply changes" : "Create table") + "</button></div>" +
      "</form><datalist id=\"te-types\">" + TYPES.concat(enumTypes).map(function (x) { return "<option value=\"" + esc(x) + "\">"; }).join("") + "</datalist>" +
      "<datalist id=\"te-cols\">" + st.columns.filter(function (c) { return c.name; }).map(function (c) { return "<option value=\"" + esc(c.name) + "\">"; }).join("") + "</datalist>";
    dlg.innerHTML = h;
  }

  function bindEditor() {
    var dlg = $("#table-editor");
    dlg.addEventListener("input", function (e) {
      var el = e.target, f = el.getAttribute("data-f");
      if (!f || !S.editor) return;
      var st = S.editor.st, sec = el.getAttribute("data-sec");
      var target = sec ? st[sec][Number(el.getAttribute("data-i"))] : st;
      if (el.type === "checkbox") {
        if (f === "notnull") target.nullable = !el.checked; else target[f] = el.checked;
        if (f === "pk" && el.checked) { target.nullable = false; drawEditor(); }
      } else target[f] = el.value;
    });
    dlg.addEventListener("change", function (e) {
      if (e.target.tagName === "SELECT" && S.editor) {
        var el = e.target;
        S.editor.st[el.getAttribute("data-sec")][Number(el.getAttribute("data-i"))][el.getAttribute("data-f")] = el.value;
      }
    });
    dlg.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (!b || !S.editor) return;
      var st = S.editor.st;
      if (b.hasAttribute("data-add")) {
        var sec = b.getAttribute("data-add");
        if (sec === "columns") st.columns.push({ name: "", type: "text", nullable: true, default: "", pk: false, focus: true });
        if (sec === "fks") st.fks.push({ columns: "", ref: "", ref_columns: "", on_delete: "" });
        if (sec === "indexes") st.indexes.push({ columns: "", unique: false, where: "" });
        st.columns.forEach(function (c, i) { if (i < st.columns.length - 1) delete c.focus; });
        drawEditor();
        var nf = dlg.querySelector("[data-focus]") || dlg.querySelector("[data-sec=" + sec + "]:last-of-type");
        if (sec !== "columns") { var rows = dlg.querySelectorAll("[data-sec=" + sec + "][data-f=columns]"); nf = rows[rows.length - 1]; }
        if (nf) nf.focus();
      } else if (b.hasAttribute("data-del")) {
        st[b.getAttribute("data-del")].splice(Number(b.getAttribute("data-i")), 1);
        drawEditor();
      } else if (b.getAttribute("data-act") === "save") {
        saveEditor();
      } else if (b.getAttribute("data-act") === "drop") {
        var id = S.editor.id;
        dlg.close();
        dropTable(id);
      }
    });
  }

  function validateEditor(st) {
    if (!/^[A-Za-z_][\w$]*(\.[A-Za-z_][\w$]*)?$/.test(st.name.trim())) return "Give the table a valid name (letters, digits, _; optionally schema.name).";
    var seen = {};
    for (var i = 0; i < st.columns.length; i++) {
      var c = st.columns[i], n = c.name.trim();
      if (!n) return "Column " + (i + 1) + " needs a name.";
      if (!c.type.trim()) return "Column " + n + " needs a type.";
      if (seen[n]) return "Duplicate column " + n + ".";
      seen[n] = true;
    }
    if (!st.columns.length) return "A table needs at least one column.";
    for (var j = 0; j < st.fks.length; j++) {
      var f = st.fks[j], fc = splitList(f.columns);
      if (!fc.length || !f.ref.trim()) return "Foreign key " + (j + 1) + " needs column(s) and a referenced table.";
      for (var k = 0; k < fc.length; k++) if (!seen[fc[k]]) return "Foreign key column " + fc[k] + " is not a column of this table.";
    }
    for (var m = 0; m < st.indexes.length; m++) {
      var xc = splitList(st.indexes[m].columns);
      if (!xc.length) return "Index " + (m + 1) + " needs column(s).";
      for (var q = 0; q < xc.length; q++) if (!seen[xc[q]]) return "Index column " + xc[q] + " is not a column of this table.";
    }
    var id = qid(st.name);
    if (id !== S.editor.id && S.index.some(function (t) { return t.id === id; })) return "A table named " + display(id) + " already exists.";
    return "";
  }

  /** Turn the editor state into design operations. */
  function editorOps(ed) {
    var st = ed.st, newId = qid(st.name), ops = [];
    var colSpec = function (c) {
      var s = { name: c.name.trim(), type: c.type.trim(), nullable: !!c.nullable };
      if (c.default.trim()) s.default = c.default.trim();
      return s;
    };
    var fkOp = function (tid, f) {
      var op = { op: "add_foreign_key", table: tid, columns: splitList(f.columns), references: qid(f.ref), ref_columns: splitList(f.ref_columns) };
      if (f.on_delete) op.on_delete = f.on_delete;
      return op;
    };
    var idxOp = function (tid, x) {
      var op = { op: "add_index", table: tid, columns: splitList(x.columns), unique: !!x.unique };
      if (x.where.trim()) op.where = x.where.trim();
      return op;
    };
    if (!ed.id || ed.created) {
      var create = { op: "create_table", table: newId, columns: st.columns.map(colSpec), primary_key: st.columns.filter(function (c) { return c.pk; }).map(function (c) { return c.name.trim(); }) };
      if (st.comment.trim()) create.comment = st.comment.trim();
      ops.push(create);
      st.fks.forEach(function (f) { ops.push(fkOp(newId, f)); });
      st.indexes.forEach(function (x) { ops.push(idxOp(newId, x)); });
      return { ops: ops, newId: newId };
    }
    var t = ed.orig, tid = ed.id;
    if (newId !== tid) {
      ops.push({ op: "rename_table", table: tid, to: st.name.trim().indexOf(".") >= 0 ? newId : newId.split(".")[1] });
      tid = newId;
    }
    var ren = {};
    st.columns.forEach(function (c) { if (c.orig) ren[c.orig] = c.name.trim(); });
    var mapCols = function (cols) { return cols.map(function (c) { return ren[c] !== undefined ? ren[c] : c; }); };
    // foreign keys and indexes that changed are dropped and re-added
    var keptFk = {}, addFks = [];
    st.fks.forEach(function (f) { if (f.orig) keptFk[f.orig] = f; });
    (t.foreign_keys || []).forEach(function (f) {
      var e = keptFk[f.name || ""];
      var same = e && splitList(e.columns).join() === mapCols(f.columns).join() && qid(e.ref) === f.ref_table &&
        (!splitList(e.ref_columns).length || splitList(e.ref_columns).join() === f.ref_columns.join()) && (e.on_delete || "") === (f.on_delete || "");
      if (!same) { ops.push({ op: "drop_foreign_key", table: tid, name: f.name }); if (e) addFks.push(e); }
    });
    st.fks.forEach(function (f) { if (!f.orig) addFks.push(f); });
    var keptIdx = {}, addIdx = [];
    st.indexes.forEach(function (x) { if (x.orig) keptIdx[x.orig] = x; });
    (t.indexes || []).forEach(function (x) {
      var e = keptIdx[x.name];
      var same = e && splitList(e.columns).join() === mapCols(x.columns).join() && !!e.unique === !!x.unique && e.where.trim() === (x.predicate || "");
      if (!same) { ops.push({ op: "drop_index", table: tid, name: x.name }); if (e) addIdx.push(e); }
    });
    st.indexes.forEach(function (x) { if (!x.orig) addIdx.push(x); });
    // columns
    var kept = {};
    st.columns.forEach(function (c) { if (c.orig) kept[c.orig] = c; });
    t.columns.forEach(function (c) { if (!kept[c.name]) ops.push({ op: "drop_column", table: tid, column: c.name }); });
    st.columns.forEach(function (c) {
      if (!c.orig) return;
      var o = t.columns.find(function (x) { return x.name === c.orig; }), name = c.name.trim();
      if (name !== c.orig) ops.push({ op: "rename_column", table: tid, column: c.orig, to: name });
      var alter = { op: "alter_column", table: tid, column: name }, changed = false;
      if (c.type.trim() !== o.data_type) { alter.type = c.type.trim(); changed = true; }
      if (!!c.nullable !== !!o.nullable) { alter.nullable = !!c.nullable; changed = true; }
      var dflt = c.default.trim();
      if (dflt !== (o.default || "")) { if (dflt) alter.default = dflt; else alter.drop_default = true; changed = true; }
      if (changed) ops.push(alter);
    });
    st.columns.forEach(function (c) { if (!c.orig) ops.push({ op: "add_column", table: tid, column: colSpec(c) }); });
    var newPk = st.columns.filter(function (c) { return c.pk; }).map(function (c) { return c.name.trim(); });
    var oldPk = mapCols((t.primary_key && t.primary_key.columns) || []);
    if (newPk.join() !== oldPk.join()) ops.push({ op: "set_primary_key", table: tid, columns: newPk });
    addFks.forEach(function (f) { ops.push(fkOp(tid, f)); });
    addIdx.forEach(function (x) { ops.push(idxOp(tid, x)); });
    if (st.comment.trim() !== (t.comment || "")) {
      var sc = { op: "set_comment", table: tid };
      if (st.comment.trim()) sc.comment = st.comment.trim();
      ops.push(sc);
    }
    return { ops: ops, newId: tid };
  }

  function saveEditor() {
    var ed = S.editor, st = ed.st;
    ed.error = validateEditor(st);
    if (ed.error) { drawEditor(); return; }
    var r = editorOps(ed);
    var noteChanged = (st.note.trim() || "") !== (S.design.notes[ed.id] || "");
    if (!r.ops.length && !noteChanged) { $("#table-editor").close(); toast("No changes", 1200); return; }
    designEdit(function () {
      if (ed.id && ed.created) {
        // regenerate a table created in this design in place
        var key = entityKey(ed.id), ents = designEntities().ent;
        var at = ents.indexOf(key);
        S.design.ops = S.design.ops.filter(function (op, i) { return ents[i] !== key; });
        Array.prototype.splice.apply(S.design.ops, [at < 0 ? S.design.ops.length : at, 0].concat(r.ops));
        if (r.newId !== ed.id) {
          S.design.ops.forEach(function (op) { if (op.references && qid(op.references) === ed.id) op.references = r.newId; });
        }
      } else {
        S.design.ops = S.design.ops.concat(r.ops);
      }
      if (ed.id && r.newId !== ed.id) {
        if (S.design.positions[ed.id]) { S.design.positions[r.newId] = S.design.positions[ed.id]; delete S.design.positions[ed.id]; }
        delete S.design.notes[ed.id];
      }
      if (st.note.trim()) S.design.notes[r.newId] = st.note.trim(); else delete S.design.notes[r.newId];
      if (!ed.id) positionStore(true)[r.newId] = ed.pos || freeSpot();
    });
    $("#table-editor").close();
    // new tables must be visible even when a focus is active
    if (!ed.id && S.cfg.focus.length) { setFocus(display(r.newId), 0, true); syncControls(); }
    if (S.cfg.exclude.indexOf(r.newId) >= 0) S.cfg.exclude.splice(S.cfg.exclude.indexOf(r.newId), 1);
    setTimeout(function () { if (viewer.nodes.has(r.newId)) selectTable(r.newId, { center: !ed.id }); }, 60);
  }

  // ---- design panel -------------------------------------------------------------
  function listDesigns() {
    if (STATIC) {
      var all = {};
      try { all = JSON.parse(localStorage.getItem("schema:designs:playground") || "{}"); } catch (e) { /* ignore */ }
      return Promise.resolve(Object.keys(all).map(function (k) { return { slug: k, name: all[k].name, ops: all[k].ops.length, updated: all[k].updated }; }));
    }
    return api("api/designs");
  }
  function loadDesign(slug) {
    if (STATIC) {
      var all = JSON.parse(localStorage.getItem("schema:designs:playground") || "{}");
      return all[slug] ? Promise.resolve(all[slug]) : Promise.reject(new Error("not found"));
    }
    return api("api/designs/" + encodeURIComponent(slug));
  }

  function renderDesignPanel() {
    var box = $("#design-panel");
    if (!box) return;
    $("#design-count").textContent = S.design ? String(S.design.ops.length) : "";
    if (!S.design) {
      box.innerHTML = "<div class=\"design-intro\"><p><b>Design schema changes</b> on top of the loaded schema: create tables, add or change columns, foreign keys and indexes.</p>" +
        "<p>Your changes show up as a diff, and export as a spec an agent can implement with migrations.</p>" +
        "<label class=\"row col\"><span>Design name</span><input type=\"text\" id=\"design-new-name\" placeholder=\"e.g. Card payments\"></label>" +
        "<button class=\"btn primary\" id=\"design-start\">Start designing</button></div>" +
        "<h4 class=\"history-title\">Saved designs</h4><ul class=\"design-list\" id=\"design-list\"><li class=\"muted\">loading…</li></ul>";
      $("#design-start").onclick = function () {
        var n = $("#design-new-name").value.trim();
        if (!n) { $("#design-new-name").focus(); toast("Name the design first", 1500); return; }
        newDesign(n);
      };
      $("#design-new-name").onkeydown = function (e) { if (e.key === "Enter") $("#design-start").click(); };
      listDesigns().then(function (list) {
        var ul = $("#design-list");
        if (!ul) return;
        if (!list.length) { ul.innerHTML = "<li class=\"muted\">none yet</li>"; return; }
        ul.innerHTML = list.map(function (d) {
          return "<li data-slug=\"" + esc(d.slug) + "\"><span class=\"name\">" + esc(d.name || d.slug) + "</span><span class=\"meta\">" + d.ops + " ops" + (d.updated ? " · " + ago(d.updated) : "") + "</span></li>";
        }).join("");
        $$("li[data-slug]", ul).forEach(function (li) {
          li.onclick = function () {
            var slug = li.getAttribute("data-slug");
            loadDesign(slug).then(function (d) { startDesign(d, { slug: slug }); toast("Opened design " + (d.name || slug)); }, function (e) { toast("Could not open: " + e.message); });
          };
        });
      }, function () { var ul = $("#design-list"); if (ul) ul.innerHTML = "<li class=\"muted\">could not list designs</li>"; });
      return;
    }
    var d = S.design, st = S.designState || { ops: [], errors: [] };
    var errs = {};
    (st.errors || []).forEach(function (e) { errs[e.op] = (errs[e.op] ? errs[e.op] + "; " : "") + e.message; });
    var saved = S.designSlug && !S.designDirty;
    box.innerHTML =
      "<div class=\"design-head\"><span class=\"badge\">✎ designing</span><span class=\"state\">" + (saved ? "saved" : S.designSlug ? "unsaved changes" : "not saved yet") + "</span></div>" +
      "<label class=\"row col\"><span>Name</span><input type=\"text\" id=\"design-name\" value=\"" + esc(d.name) + "\"></label>" +
      "<label class=\"row col\"><span>Description</span><textarea id=\"design-desc\" rows=\"3\" placeholder=\"Goal of the change, context for the implementer\">" + esc(d.description || "") + "</textarea></label>" +
      "<div class=\"btn-row\"><button class=\"btn primary small\" id=\"design-new-table\" title=\"Or right-click the canvas\">+ New table</button>" +
      "<button class=\"btn small\" id=\"design-undo\" title=\"⌘Z\"" + (S.designUndo.length ? "" : " disabled") + ">Undo</button>" +
      "<button class=\"btn small\" id=\"design-relayout\" title=\"Lay the diagram out again\">Re-layout</button></div>" +
      "<p class=\"design-tip\">Right-click a table to edit or drop it, or double-click it to edit.</p>" +
      "<h4 class=\"history-title\">Operations (" + d.ops.length + ")</h4>" +
      (d.ops.length ? "<ol class=\"op-list\">" + d.ops.map(function (op, i) {
        return "<li class=\"" + (errs[i] ? "err" : "") + "\" title=\"" + esc(errs[i] || "") + "\"><span class=\"lbl\">" + esc((st.ops || [])[i] || op.op) + (errs[i] ? "<span class=\"why\">⚠ " + esc(errs[i]) + "</span>" : "") + "</span>" +
          "<button data-del-op=\"" + i + "\" title=\"Remove this operation\">✕</button></li>";
      }).join("") + "</ol>" : "<p class=\"muted\">No changes yet.</p>") +
      "<h4 class=\"history-title\">Export for an agent</h4>" +
      "<div class=\"btn-row\">" + (STATIC ? "<button class=\"btn small primary\" id=\"design-save\">Save in browser</button>" : "<button class=\"btn small primary\" id=\"design-save\">Save to repo</button>") +
      "<button class=\"btn small\" data-dexp=\"prompt\" title=\"Saves, then copies a self-contained prompt: instructions plus the full spec\">Copy agent prompt</button></div>" +
      (S.designPath && !STATIC ? "<p class=\"design-path\" title=\"" + esc(S.designPath) + "\">" + esc(S.designPath.replace(/^.*\/(\.schema\/)/, "$1")) + "</p>" : "") +
      "<div class=\"btn-row\"><button class=\"btn small\" data-dexp=\"md-copy\">Copy spec (Markdown)</button><button class=\"btn small\" data-dexp=\"sql-copy\">Copy SQL</button></div>" +
      "<div class=\"btn-row\"><span class=\"muted\">Download</span><button class=\"btn small\" data-dexp=\"md\">.md</button><button class=\"btn small\" data-dexp=\"sql\">.sql</button><button class=\"btn small\" data-dexp=\"json\">.json</button></div>" +
      "<div class=\"btn-row end\"><button class=\"btn small danger\" id=\"design-close\">Close design</button></div>";
    $("#design-name").oninput = function (e) { d.name = e.target.value; S.designDirty = true; saveDesignDraft(); renderDiffSummary(); };
    $("#design-desc").oninput = function (e) { d.description = e.target.value; S.designDirty = true; saveDesignDraft(); };
    $("#design-new-table").onclick = function () { openTableEditor(null); };
    $("#design-undo").onclick = designUndo;
    $("#design-relayout").onclick = function () {
      designEdit(function () { d.positions = {}; d.filter_positions = {}; });
      setTimeout(function () { if (!filterActive()) { snapshotPositions(); saveDesignDraft(); } }, 200);
    };
    $("#design-close").onclick = closeDesign;
    $("#design-save").onclick = function () { saveDesign().catch(function (e) { toast("Save failed: " + e.message, 4000); }); };
    $$("[data-del-op]", box).forEach(function (b) {
      b.onclick = function () { var i = Number(b.getAttribute("data-del-op")); designEdit(function () { d.ops.splice(i, 1); }); };
    });
    $$("[data-dexp]", box).forEach(function (b) { b.onclick = function () { designExport(b.getAttribute("data-dexp")); }; });
  }

  /** Push name / description edits (made without a re-render) to the engine. */
  function pushDesign() { viz.set_design(JSON.stringify(S.design)); }

  function saveDesign() {
    var d = S.design;
    pushDesign();
    var slug = S.designSlug || (wb.slugify ? wb.slugify(d.name) : d.name.toLowerCase().replace(/[^a-z0-9]+/g, "-"));
    var body = viz.design_export("json");
    if (STATIC) {
      var all = {};
      try { all = JSON.parse(localStorage.getItem("schema:designs:playground") || "{}"); } catch (e) { /* ignore */ }
      all[slug] = JSON.parse(body);
      localStorage.setItem("schema:designs:playground", JSON.stringify(all));
      S.designSlug = slug;
      S.designDirty = false;
      saveDesignDraft();
      renderDesignPanel();
      toast("Saved in this browser");
      return Promise.resolve();
    }
    return api("api/designs/" + encodeURIComponent(slug), { method: "PUT", body: body }).then(function (r) {
      S.designSlug = slug;
      S.designDirty = false;
      S.designPath = r.md_path;
      saveDesignDraft();
      renderDesignPanel();
      toast("Saved " + r.md_path.replace(/^.*\/(\.schema\/)/, "$1"));
    });
  }

  function designExport(what) {
    pushDesign();
    var slug = S.designSlug || (wb.slugify ? wb.slugify(S.design.name) : "design");
    if (what === "prompt") {
      // self-contained: the whole spec is inline; saving keeps `schema design check` working
      saveDesign().then(function () { copy(viz.design_export("prompt"), "agent prompt"); }, function (e) { toast("Save failed: " + e.message, 4000); });
      return;
    }
    if (what === "md-copy") return copy(viz.design_export("markdown"), "design spec");
    if (what === "sql-copy") return copy(viz.design_export("sql"), "SQL");
    var ext = { md: "markdown", sql: "sql", json: "json" }[what];
    var type = { md: "text/markdown", sql: "application/sql", json: "application/json" }[what];
    download(slug + "." + what, new Blob([viz.design_export(ext)], { type: type }));
  }

  function firstLaunchTip() {
    try {
      if (localStorage.getItem("schema:tip")) return;
      localStorage.setItem("schema:tip", "1");
    } catch (e) { return; }
    setTimeout(function () { toast("Tip: type a table name in the bar above the diagram to show just it and its neighbours · press ? for help", 7000); }, 800);
  }

  /** Open ?design=NAME, or resume the draft left in this browser. */
  function restoreDesign() {
    return new Promise(function (resolve) { nextFrame(function () { nextFrame(resolve); }); }).then(function () {
      if (S.designParam) {
        var slug = S.designParam;
        S.designParam = null;
        return loadDesign(slug).then(function (d) { startDesign(d, { slug: slug }); }, function () { toast("No saved design named " + slug); });
      }
      var draft = loadDesignDraft();
      if (draft && draft.design) {
        startDesign(draft.design, { slug: draft.slug, dirty: draft.dirty });
        toast("Resumed design “" + draft.design.name + "”");
      }
    });
  }

  // ---- boot -----------------------------------------------------------------------
  function initViewer() {
    viewer = new SchemaViewer($("#canvas"), {
      onNodeClick: function (id, info) {
        if (info.more) { var o = override(id); o.columns = "all"; o.collapsed = false; render({ preserve: true }); return; }
        selectTable(id);
        // a column typed with an enum opens that enum
        if (info.col) {
          var ce = JSON.parse(viz.table(id)).column_enums || {};
          if (ce[info.col]) showEnum(ce[info.col]);
        }
      },
      onNodeDblClick: function (id) {
        if (S.design) { openTableEditor(id); return; }
        if (S.cfg.focus.length === 1 && patternFor(id)) { S.cfg.focus = []; S.cfg.focus_depths = {}; syncControls(); render({ fit: true }); }
        else focusOn(id);
      },
      onBackgroundClick: function () { closeDetails(); $("#search-results").hidden = true; },
      onNodeMove: function (id, x, y) { return JSON.parse(viz.move_node(id, x, y)); },
      onNodeDrop: function (id, x, y) {
        positionStore(true)[id] = [x, y];
        if (S.design) { S.designDirty = true; saveDesignDraft(); renderDesignPanel(); }
        else saveState();
        // re-route every edge: lanes and crossing hops depend on all positions
        render({ preserve: true });
      },
      onContextMenu: contextMenu,
      onZoom: function (k) { $("#zoom-level").textContent = Math.round(k * 100) + "%"; },
      persistentHighlight: function () { return S.selected; },
    });
    $("#zoom-in").onclick = function () { viewer.zoomBy(1.25); };
    $("#zoom-out").onclick = function () { viewer.zoomBy(0.8); };
    $("#zoom-fit").onclick = function () { viewer.fit(); };
    $("#zoom-level").onclick = function () { viewer.setZoom(1); };
  }
  function initTheme() {
    try { S.theme = localStorage.getItem("schema:theme") || "auto"; } catch (e) { /* ignore */ }
    var apply = function () { document.documentElement.setAttribute("data-theme", isDark() ? "dark" : "light"); };
    apply();
    $("#theme-btn").onclick = function () {
      S.theme = isDark() ? "light" : "dark";
      try { localStorage.setItem("schema:theme", S.theme); } catch (e) { /* ignore */ }
      apply();
      render({ preserve: true });
    };
    if (window.matchMedia) matchMedia("(prefers-color-scheme: dark)").addEventListener("change", function () { if (S.theme === "auto") { apply(); render({ preserve: true }); } });
    $("#help-btn").onclick = function () { $("#help").showModal(); };
  }

  function boot() {
    initTheme();
    var wasmUrl = (window.SCHEMA_BASE || "") + "pkg/schema_wasm_bg.wasm";
    wasm_bindgen({ module_or_path: wasmUrl }).then(function () {
      wb = wasm_bindgen;
      viz = new wb.Schema();
      S.defaults = JSON.parse(wb.default_config());
      initViewer();
      bindControls();
      bindCompare();
      bindSearch();
      bindExport();
      bindKeys();
      bindEditor();
      bindFilterBar();
      renderDesignPanel();
      return STATIC ? bootStatic() : bootServer();
    }).catch(function (e) {
      console.error(e);
      $("#loading").textContent = "Failed to start: " + e.message;
    });
  }

  function bootServer() {
    return Promise.all([api("api/state"), api("api/config").catch(function () { return {}; })]).then(function (r) {
      S.server = r[0];
      S.projectCfg = r[1] || {};
      var params = new URLSearchParams(location.search);
      S.designParam = params.get("design");
      var stored = loadState();
      var cfg = merge(clone(S.defaults), S.projectCfg.default || {});
      var explicit = {};
      if (params.has("cfg")) {
        try { explicit = JSON.parse(params.get("cfg")); } catch (e) { /* reported below */ }
      }
      // a fresh CLI launch with a comparison opens on what changed (unless told otherwise)
      S.autoChanges = params.has("base") && !!params.get("base") && !("changes_only" in explicit) && !("changes_only" in (S.projectCfg.default || {}));
      if (params.has("cfg")) {
        try { merge(cfg, JSON.parse(params.get("cfg"))); } catch (e) { toast("Invalid cfg parameter"); }
      } else if (stored && stored.cfg) { merge(cfg, stored.cfg); S.storedLens = stored.lens || null; }
      S.cfg = cfg;
      if (params.has("compare")) { S.base = params.get("base") || null; S.compare = params.get("compare"); }
      else if (stored && stored.compare) { S.base = stored.base; S.compare = stored.compare; }
      else { S.base = S.server.initial.base; S.compare = S.server.initial.compare; }
      if (params.toString()) history.replaceState(null, "", location.pathname);
      renderFileInfo();
      renderViews(params.get("view"));
      syncControls();
      return loadGit();
    }).then(loadSources).then(function () {
      // "changes only" from the CLI (flag or default for comparisons) opens as a temporary view
      var wantLens = S.autoChanges || S.cfg.changes_only;
      S.cfg.changes_only = false;
      if (wantLens && S.diff && S.diff.tables.length) startLens({ kind: "diff", label: "Changed tables", context: S.cfg.changes_context });
      else if (S.storedLens && S.diff && (S.base || S.design)) {
        if (S.storedLens.kind !== "commit") S.storedLens.label = "Changed tables";
        startLens(S.storedLens);
      }
      render({ fit: true });
      if (S.base && S.diff && S.diff.tables.length) showTab("changes");
      poll();
      firstLaunchTip();
      return restoreDesign();
    });
  }

  function bootStatic() {
    setupPlayground();
    renderViews();
    var stored = loadState();
    S.cfg = merge(clone(S.defaults), (stored && stored.cfg) || {});
    syncControls();
    var params = new URLSearchParams(location.search);
    var exs = STATIC.examples || [];
    var pick = exs.find(function (x) { return x.id === params.get("example"); }) || exs[0];
    if (pick) return loadExample(pick).then(restoreDesign);
    $("#loading").textContent = "Open or drop a structure.sql file";
  }

  boot();
})();
