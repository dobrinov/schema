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
    result: null, lastKey: null, theme: "auto", tables: [], diff: null, playground: { current: null, base: null, name: "structure.sql", baseName: null },
  };

  // ---- utils --------------------------------------------------------------
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
      var st = { cfg: diffObj(S.cfg, S.defaults), base: S.base, compare: S.compare };
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
      buildIndex();
      S.diff = JSON.parse(viz.diff());
      renderChanges();
      updateCompareUI();
    });
  }

  function buildIndex() {
    var schema = JSON.parse(viz.schema());
    S.index = [];
    var names = [];
    schema.tables.forEach(function (t) {
      var id = t.schema + "." + t.name;
      names.push(display(id));
      S.index.push({ id: id, label: display(id), cols: t.columns.map(function (c) { return c.name; }) });
    });
    (schema.views || []).forEach(function (v) { var id = v.schema + "." + v.name; names.push(display(id)); S.index.push({ id: id, label: display(id), cols: [], view: true }); });
    $("#table-names").innerHTML = names.map(function (n) { return "<option value=\"" + esc(n) + "\">"; }).join("");
    var schemas = schema.schemas || [];
    var box = $("#schema-filter");
    if (schemas.length > 1) {
      box.innerHTML = "<span>Schemas</span><div class=\"schema-checks\">" + schemas.map(function (s) {
        return "<label class=\"check\"><input type=\"checkbox\" data-schema=\"" + esc(s) + "\"> " + esc(s) + "</label>";
      }).join("") + "</div>";
      syncSchemaChecks();
    } else box.innerHTML = "";
  }

  // ---- rendering ----------------------------------------------------------
  var STRUCTURAL = ["focus", "focus_depth", "focus_direction", "include", "exclude", "schemas", "changes_only", "changes_context", "layout.algorithm", "layout.direction", "layout.group_by", "show_views", "show_partitions", "show_isolated"];
  var renderQueued = null;
  function render(o) {
    o = o || {};
    if (renderQueued) cancelAnimationFrame(renderQueued);
    renderQueued = requestAnimationFrame(function () { renderQueued = null; renderNow(o); });
  }
  function renderNow(o) {
    S.cfg.theme = isDark() ? "dark" : "light";
    var key = JSON.stringify(STRUCTURAL.map(function (p) { return getPath(S.cfg, p); }));
    var fit = o.fit || S.lastKey === null || (key !== S.lastKey && !o.preserve);
    S.lastKey = key;
    var t0 = performance.now();
    var res = JSON.parse(viz.view(JSON.stringify(S.cfg)));
    if (res.error) { toast(res.error, 5000); return; }
    S.result = res;
    viewer.setContent(res.svg, res, { preserveView: !fit });
    viewer.setTheme(isDark());
    $("#loading").hidden = true;
    S.tables = JSON.parse(viz.tables());
    renderEmpty(res);
    renderStatus(res, performance.now() - t0);
    renderTables();
    renderFocusChips();
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
    var msg = S.tables.length ? "No tables match the current filters." : "No tables found in this file.";
    el.innerHTML = "<div>" + esc(msg) + "</div>" + (S.tables.length ? "<button class=\"btn\" id=\"empty-reset\">Clear filters</button>" : "");
    var b = $("#empty-reset");
    if (b) b.onclick = function () { clearFilters(); };
  }

  function clearFilters() {
    S.cfg.focus = [];
    S.cfg.include = [];
    S.cfg.exclude = clone(S.defaults.exclude);
    S.cfg.schemas = [];
    S.cfg.changes_only = false;
    syncControls();
    render({ fit: true });
  }

  function renderStatus(res, ms) {
    var st = res.stats, parts = [];
    parts.push("<b>" + st.nodes_visible + "</b> of " + (st.tables_total + (S.cfg.show_views ? st.views_total : 0)) + " tables");
    parts.push("<b>" + st.edges_visible + "</b> relations");
    parts.push("columns: " + st.column_mode);
    if (st.hidden_by_filter) parts.push(st.hidden_by_filter + " filtered out");
    parts.push(ms.toFixed(0) + " ms");
    (st.notices || []).forEach(function (n) { parts.push("<span class=\"notice\">⚠ " + esc(n) + "</span>"); });
    $("#statusbar").innerHTML = parts.join("<span>·</span>");
  }

  function renderDiffSummary() {
    var box = $("#diff-summary");
    var d = S.diff;
    if (!d || !S.base) { box.hidden = true; $("#changes-count").textContent = ""; return; }
    var s = d.summary, total = d.tables.length;
    $("#changes-count").textContent = total ? String(total) : "";
    box.hidden = false;
    box.innerHTML = "<span class=\"label\">" + esc(refLabel(S.base)) + " → " + esc(refLabel(S.compare)) + "</span>" +
      (total === 0 && !s.other_changes ? "<span class=\"muted\">no changes</span>" :
        (s.tables_added ? "<span class=\"pill add\">+" + s.tables_added + "</span>" : "") +
        (s.tables_removed ? "<span class=\"pill del\">−" + s.tables_removed + "</span>" : "") +
        (s.tables_modified ? "<span class=\"pill mod\">~" + s.tables_modified + "</span>" : "") +
        "<button id=\"toggle-changes\" class=\"" + (S.cfg.changes_only ? "on" : "") + "\">only changes</button>");
    var t = $("#toggle-changes");
    if (t) t.onclick = function () { S.cfg.changes_only = !S.cfg.changes_only; syncControls(); render({ fit: true }); };
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
    syncSchemaChecks();
  }
  function syncSchemaChecks() {
    $$("[data-schema]").forEach(function (el) {
      el.checked = !S.cfg.schemas.length || S.cfg.schemas.indexOf(el.getAttribute("data-schema")) >= 0;
    });
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
    $("#schema-filter").addEventListener("change", function () {
      var all = $$("[data-schema]");
      var on = all.filter(function (e) { return e.checked; }).map(function (e) { return e.getAttribute("data-schema"); });
      S.cfg.schemas = on.length === all.length ? [] : on;
      render({ fit: true });
    });
    $("#focus-input").addEventListener("change", function (e) {
      var v = e.target.value.trim();
      if (!v) return;
      if (S.cfg.focus.indexOf(v) < 0) S.cfg.focus.push(v);
      e.target.value = "";
      syncControls();
      render({ fit: true });
    });
    $("#reset-positions").onclick = function () { S.cfg.positions = {}; render({ preserve: true }); toast("Positions reset"); };
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

  function renderFocusChips() {
    $("#focus-chips").innerHTML = S.cfg.focus.map(function (f, i) {
      return "<span class=\"chip\">" + esc(display(f)) + "<button data-i=\"" + i + "\" title=\"Remove\">×</button></span>";
    }).join("");
    $$("#focus-chips button").forEach(function (b) {
      b.onclick = function () { S.cfg.focus.splice(Number(b.getAttribute("data-i")), 1); syncControls(); render({ fit: true }); };
    });
  }

  // ---- focus / visibility helpers ----------------------------------------
  function focusOn(id, add) {
    if (add) { if (S.cfg.focus.indexOf(id) < 0) S.cfg.focus.push(id); }
    else S.cfg.focus = [id];
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
    S.cfg.focus = S.cfg.focus.filter(function (f) { return f !== id; });
    if (S.selected === id) closeDetails();
    syncControls();
    render({ preserve: true });
  }
  function showTable(id) {
    var ex = S.cfg.exclude.indexOf(id);
    if (ex >= 0) S.cfg.exclude.splice(ex, 1);
    else if (S.cfg.include.length) S.cfg.include.push(id);
    else if (S.cfg.focus.length) S.cfg.focus.push(id);
    else if (S.cfg.changes_only) S.cfg.changes_only = false;
    syncControls();
    render({ preserve: true });
  }

  // ---- tables panel -------------------------------------------------------
  function renderTables() {
    var q = $("#tables-filter").value.trim().toLowerCase();
    var list = S.tables.filter(function (t) { return !q || t.label.toLowerCase().indexOf(q) >= 0; });
    var visible = S.tables.filter(function (t) { return t.visible; }).length;
    $("#tables-count").textContent = visible + "/" + S.tables.length;
    $("#table-list").innerHTML = list.map(function (t) {
      var kind = t.kind === "table" ? (t.partition_of ? "<span class=\"kind\">part</span>" : "") : "<span class=\"kind\">" + (t.kind === "view" ? "view" : "mview") + "</span>";
      return "<li data-id=\"" + esc(t.id) + "\" class=\"" + (t.visible ? "" : "off") + (t.id === S.selected ? " selected" : "") + "\" title=\"" + esc(t.comment || t.id) + "\">" +
        "<input type=\"checkbox\" " + (t.visible ? "checked" : "") + (t.partition_of && !S.cfg.show_partitions ? " disabled" : "") + ">" +
        "<span class=\"dot " + t.status + "\"></span><span class=\"name\">" + esc(t.label) + "</span>" + kind +
        "<span class=\"meta\">" + (t.columns || "") + (t.fk_in + t.fk_out ? " · " + (t.fk_in + t.fk_out) + "↔" : "") + "</span>" +
        "<button class=\"focus-btn\" title=\"Focus on this table\">◎</button></li>";
    }).join("");
  }
  function bindTables() {
    $("#tables-filter").addEventListener("input", renderTables);
    $("#table-list").addEventListener("click", function (e) {
      var li = e.target.closest("li");
      if (!li) return;
      var id = li.getAttribute("data-id");
      if (e.target.type === "checkbox") { e.target.checked ? showTable(id) : hideTable(id); return; }
      if (e.target.classList.contains("focus-btn")) { focusOn(id); return; }
      if (viewer.nodes.has(id)) selectTable(id, { center: true });
      else { showTable(id); setTimeout(function () { selectTable(id, { center: true }); }, 60); }
    });
    $("#table-list").addEventListener("mouseover", function (e) {
      var li = e.target.closest("li");
      viewer.highlight(li ? li.getAttribute("data-id") : null);
    });
    $("#table-list").addEventListener("mouseleave", function () { viewer.highlight(null); });
    $("#tables-show-all").onclick = function () {
      S.cfg.exclude = clone(S.defaults.exclude);
      S.cfg.include = [];
      S.cfg.focus = [];
      S.cfg.changes_only = false;
      syncControls();
      render({ fit: true });
    };
    $("#tables-only-matching").onclick = function () {
      var q = $("#tables-filter").value.trim();
      if (!q) { toast("Type a filter first"); return; }
      S.cfg.include = [q.indexOf("*") >= 0 ? q : "*" + q + "*"];
      S.cfg.focus = [];
      syncControls();
      render({ fit: true });
    };
  }

  // ---- selection & details -----------------------------------------------
  function selectTable(id, o) {
    o = o || {};
    S.selected = id;
    viewer.select(id);
    if (o.center && viewer.nodes.has(id)) viewer.centerOn(id);
    renderDetails(id);
    $$("#table-list li").forEach(function (li) { li.classList.toggle("selected", li.getAttribute("data-id") === id); });
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
    var t = d.table, v = d.view;
    if (!t && !v) { box.hidden = true; return; }
    box.hidden = false;
    var parts = id.split("."), schema = parts[0], name = parts.slice(1).join(".");
    var diff = d.diff || { columns: [], foreign_keys: [], indexes: [], constraints: [], properties: [] };
    var status = d.status;
    var ov = S.cfg.tables[id] || {};
    var visible = viewer.nodes.has(id);
    var h = "<div class=\"head\"><h2>" + (schema !== "public" ? "<span class=\"schema\">" + esc(schema) + ".</span>" : "") + esc(name) +
      (status !== "unchanged" ? " <span class=\"pill " + ({ added: "add", removed: "del", modified: "mod" })[status] + "\">" + status + "</span>" : "") +
      "<button class=\"close\" title=\"Close (Esc)\">×</button></h2>";
    var comment = (t && t.comment) || (v && v.comment);
    if (comment) h += "<p class=\"comment\">" + esc(comment) + "</p>";
    h += "<div class=\"tools\">" +
      "<button class=\"btn small\" data-act=\"focus\">◎ Focus</button>" +
      "<button class=\"btn small\" data-act=\"addfocus\">+ Add to focus</button>" +
      (visible ? "<button class=\"btn small\" data-act=\"hide\">Hide</button>" : "<button class=\"btn small\" data-act=\"show\">Show</button>") +
      (t ? "<select class=\"btn small\" data-act=\"colmode\" title=\"Columns shown for this table\">" +
        [["", "columns: default"], ["all", "all columns"], ["keys", "keys only"], ["relations", "PK/FK only"], ["changed", "changed only"], ["none", "collapsed"]].map(function (o) {
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
      h += "<section><h3>Columns (" + t.columns.length + ")</h3><table class=\"cols\">" + cols.map(function (x) {
        var c = x.c, shown = node ? !!node.querySelector("[data-col=\"" + CSS.escape(c.name) + "\"]") : true;
        var chg = "";
        if (x.st === "modified") chg = (byName[c.name].changes || []).map(function (f) { return "<span class=\"chg\">" + esc(f.field) + ": " + esc(f.old || "∅") + " → " + esc(f.new || "∅") + "</span>"; }).join("");
        return "<tr class=\"" + x.st + (shown ? "" : " hidden-col") + "\" title=\"" + esc(c.comment || "") + "\">" +
          "<td class=\"flags\">" + colFlags(x.st === "removed" && d.base ? d.base : t, c.name) + "</td>" +
          "<td class=\"name\">" + esc(c.name) + (c.nullable ? "<span class=\"muted\">?</span>" : "") + chg + (c.default ? "<span class=\"dflt\">= " + esc(c.default) + "</span>" : "") + "</td>" +
          "<td class=\"type\">" + esc(shortType(c.data_type)) + "</td>" +
          "<td>" + (visible && x.st !== "removed" ? "<button class=\"eye\" data-col=\"" + esc(c.name) + "\" title=\"" + (shown ? "Hide in diagram" : "Show in diagram") + "\">" + (shown ? "👁" : "◌") + "</button>" : "") + "</td></tr>";
      }).join("") + "</table></section>";

      var fkSt = {};
      (diff.foreign_keys || []).forEach(function (f) { fkSt[f.name] = f.status; });
      var fks = (t.foreign_keys || []).map(function (f) {
        var key = f.name || "";
        return "<li class=\"" + (fkSt[key] || "") + "\">(" + esc(f.columns.join(", ")) + ") → <a class=\"link\" data-goto=\"" + esc(f.ref_table) + "\">" + esc(display(f.ref_table)) + "</a>(" + esc(f.ref_columns.join(", ")) + ")" +
          (f.on_delete ? " <span class=\"muted\">ON DELETE " + esc(f.on_delete) + "</span>" : "") + "</li>";
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
    $$("[data-act]", box).forEach(function (b) {
      var act = b.getAttribute("data-act");
      var handler = function () {
        if (act === "focus") focusOn(id);
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
    if (!d || !S.base) {
      box.innerHTML = "<div class=\"no-diff\"><p><b>No comparison active.</b></p>" +
        (STATIC ? "<p>Load a second file with <b>Compare with…</b> in the top bar to see what changed.</p>"
          : S.server && S.server.is_git ? "<p>Pick a <b>base</b> version in the top bar, or choose a commit from the history below to see what it changed.</p>"
            : "<p>This file is not in a git repository. Start with <code>--base-file old.sql</code> to compare two files.</p>") + "</div>";
      $("#changes-count").textContent = "";
      return;
    }
    var s = d.summary;
    var h = "<div class=\"change-summary\">" +
      "<span class=\"pill add\">+" + s.tables_added + " tables</span><span class=\"pill del\">−" + s.tables_removed + " tables</span><span class=\"pill mod\">~" + s.tables_modified + " tables</span>" +
      "<span class=\"pill add\">+" + s.columns_added + " cols</span><span class=\"pill del\">−" + s.columns_removed + " cols</span><span class=\"pill mod\">~" + s.columns_modified + " cols</span>" +
      "<span class=\"pill add\">+" + (s.indexes_added + s.foreign_keys_added) + " idx/fk</span><span class=\"pill del\">−" + (s.indexes_removed + s.foreign_keys_removed) + " idx/fk</span></div>";
    if (!d.tables.length && !s.other_changes) h += "<div class=\"no-diff\">No schema changes between these versions.</div>";
    d.tables.forEach(function (t) {
      ["columns", "foreign_keys", "indexes", "constraints", "properties"].forEach(function (k) { t[k] = t[k] || []; });
      h += "<div class=\"change\"><header data-goto=\"" + esc(t.id) + "\"><span class=\"dot " + t.status + "\"></span>" + esc(display(t.id)) + "<span class=\"st " + t.status + "\">" + t.status + "</span></header><ul>";
      if (t.status === "modified") {
        t.columns.forEach(function (c) {
          h += "<li class=\"" + c.status + "\"><span class=\"sign\">" + SIGN[c.status] + "</span>" + esc(c.name) +
            (c.changes || []).map(function (f) { return " <span class=\"muted\">" + esc(f.field) + "</span> <span class=\"old\">" + esc(f.old || "∅") + "</span> → <span class=\"new\">" + esc(f.new || "∅") + "</span>"; }).join(";") + "</li>";
        });
        [["fk", t.foreign_keys], ["index", t.indexes], ["constraint", t.constraints]].forEach(function (g) {
          (g[1] || []).forEach(function (i) {
            h += "<li class=\"" + i.status + "\"><span class=\"sign\">" + SIGN[i.status] + "</span><span class=\"muted\">" + g[0] + "</span> " + esc(i.name) + " <span class=\"muted\">" + esc(i.new || i.old || "") + "</span></li>";
          });
        });
        (t.properties || []).forEach(function (p) {
          h += "<li class=\"modified\"><span class=\"sign\">~</span>" + esc(p.field) + " <span class=\"old\">" + esc(p.old || "∅") + "</span> → <span class=\"new\">" + esc(p.new || "∅") + "</span></li>";
        });
      } else {
        h += "<li class=\"muted\">" + t.columns.length + " columns: " + esc(t.columns.map(function (c) { return c.name; }).join(", ")) + "</li>";
      }
      h += "</ul></div>";
    });
    [["Views", d.views], ["Enums", d.enums], ["Functions", d.functions], ["Triggers", d.triggers], ["Extensions", d.extensions]].forEach(function (g) {
      if (!g[1] || !g[1].length) return;
      h += "<div class=\"change\"><header>" + g[0] + "</header><ul>" + g[1].map(function (i) {
        return "<li class=\"" + i.status + "\" title=\"" + esc((i.old ? "OLD: " + i.old + "\n\n" : "") + (i.new ? "NEW: " + i.new : "")) + "\"><span class=\"sign\">" + SIGN[i.status] + "</span>" + esc(display(i.name)) + "</li>";
      }).join("") + "</ul></div>";
    });
    box.innerHTML = h;
    $$("[data-goto]", box).forEach(function (hd) {
      hd.onclick = function () {
        var id = hd.getAttribute("data-goto");
        if (viewer.nodes.has(id)) selectTable(id, { center: true });
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
      li.onclick = function () { setComparison(li.getAttribute("data-base"), li.getAttribute("data-compare")); };
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
  function refOptions(includeNone, includeWork) {
    var o = [];
    if (includeNone) o.push(["", "— none —"]);
    if (includeWork) o.push([WORKTREE, "working tree"]);
    if (S.server && S.server.base_file) o.push([BASEFILE, "file: " + S.server.base_file.split("/").pop()]);
    if (S.server && S.server.is_git) {
      o.push([INDEX, "index (staged)"]);
      o.push(["HEAD", "HEAD"]);
      if (S.log.length) o.push(["-", "── commits ──"]);
      S.log.forEach(function (c) { o.push([c.sha, c.short + "  " + c.subject.slice(0, 50)]); });
      if (S.refs.branches.length) o.push(["-", "── branches ──"]);
      S.refs.branches.forEach(function (b) { o.push([b, b]); });
      if (S.refs.tags.length) o.push(["-", "── tags ──"]);
      S.refs.tags.forEach(function (t) { o.push([t, t]); });
      o.push(["-", "──"]);
      o.push(["__custom__", "other ref…"]);
    }
    return o;
  }
  function fillSelect(sel, opts, value) {
    var known = opts.some(function (o) { return o[0] === (value || ""); });
    if (!known && value) opts.splice(includesNone(opts) ? 1 : 0, 0, [value, refLabel(value)]);
    sel.innerHTML = opts.map(function (o) {
      return o[0] === "-" ? "<option disabled>" + esc(o[1]) + "</option>" : "<option value=\"" + esc(o[0]) + "\">" + esc(o[1]) + "</option>";
    }).join("");
    sel.value = value || "";
  }
  function includesNone(opts) { return opts.length && opts[0][0] === ""; }
  function updateCompareUI() {
    if (STATIC) return;
    var git = S.server.is_git && (S.server.tracked || S.base);
    $("#compare-bar").hidden = !(git || S.server.base_file);
    fillSelect($("#base-select"), refOptions(true, false), S.base);
    fillSelect($("#compare-select"), refOptions(false, true), S.compare);
    renderHistory();
  }
  function setComparison(base, compare) {
    S.base = base || null;
    S.compare = compare || WORKTREE;
    $("#loading").hidden = false;
    loadSources().then(function () { render({ fit: false, preserve: true }); }, function (e) {
      toast("Could not load " + refLabel(S.compare) + ": " + e.message, 5000);
      $("#loading").hidden = true;
    });
  }
  function bindCompare() {
    function onChange(which) {
      return function (e) {
        var v = e.target.value;
        if (v === "__custom__") {
          v = window.prompt("Git ref (branch, tag, sha, HEAD~3, …)");
          if (!v) { updateCompareUI(); return; }
          api("api/git/resolve?ref=" + encodeURIComponent(v)).then(function () {
            which === "base" ? setComparison(v, S.compare) : setComparison(S.base, v);
          }, function () { toast("Unknown ref " + v); updateCompareUI(); });
          return;
        }
        which === "base" ? setComparison(v, S.compare) : setComparison(S.base, v);
      };
    }
    $("#base-select").addEventListener("change", onChange("base"));
    $("#compare-select").addEventListener("change", onChange("compare"));
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
      items.push(["Focus on this table", function () { focusOn(id); }]);
      items.push(["Focus with 2 hops", function () { S.cfg.focus_depth = 2; focusOn(id); }]);
      items.push(["Add to focus", function () { focusOn(id, true); }]);
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
      if (S.cfg.positions[id]) items.push(["Reset position", function () { delete S.cfg.positions[id]; render({ preserve: true }); }]);
      items.push(["-"]);
      items.push(["Copy name", function () { copy(display(id), "table name"); }]);
    } else {
      items.push(["Fit to screen", function () { viewer.fit(); }]);
      if (S.cfg.focus.length) items.push(["Clear focus", function () { S.cfg.focus = []; syncControls(); render({ fit: true }); }]);
      items.push(["Show all tables", function () { clearFilters(); }]);
      if (Object.keys(S.cfg.positions).length) items.push(["Reset dragged positions", function () { S.cfg.positions = {}; render({ preserve: true }); }]);
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
      else if (what === "config") { var p = diffObj(S.cfg, S.defaults); delete p.theme; copy(JSON.stringify(p, null, 2), "view config"); }
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
  var COLS = ["auto", "all", "keys", "relations", "changed", "none"];
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
        return;
      }
      var k = e.key;
      if (k === "/") { e.preventDefault(); $("#search").focus(); }
      else if (k === "f") viewer.fit();
      else if (k === "+" || k === "=") viewer.zoomBy(1.25);
      else if (k === "-") viewer.zoomBy(0.8);
      else if (k === "0") viewer.setZoom(1);
      else if (k >= "1" && k <= "5") { S.cfg.layout.algorithm = ALGS[Number(k) - 1]; syncControls(); render({ fit: true }); }
      else if (k === "c") { S.cfg.changes_only = !S.cfg.changes_only; syncControls(); render({ fit: true }); }
      else if (k === "k") cycle(COLS, "columns");
      else if (k === "e") cycle(EDGES, "edges.style");
      else if (k === "?") $("#help").showModal();
      else if (k === "Escape") {
        $("#context-menu").hidden = true;
        if (S.selected) closeDetails();
        else if (S.cfg.focus.length) { S.cfg.focus = []; syncControls(); render({ fit: true }); }
      }
    });
  }

  // ---- live reload ------------------------------------------------------------
  function poll() {
    if (STATIC) return;
    setInterval(function () {
      if (document.hidden) return;
      api("api/state").then(function (st) {
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
    return Promise.all([api("api/git/log?limit=60"), api("api/git/refs")]).then(function (r) { S.log = r[0]; S.refs = r[1]; });
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
      "<button class=\"btn\" id=\"pg-clear-base\" hidden>× base</button>" +
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
    $("#file-name").textContent = S.playground.name;
    $("#file-meta").textContent = S.playground.base ? "compared with " + S.playground.baseName : "playground — files never leave your browser";
    $("#pg-clear-base").hidden = !S.playground.base;
    return loadSources().then(function () { render({ fit: !!fit }); });
  }

  // ---- boot -----------------------------------------------------------------------
  function initViewer() {
    viewer = new SchemaViewer($("#canvas"), {
      onNodeClick: function (id, info) {
        if (info.more) { var o = override(id); o.columns = "all"; o.collapsed = false; render({ preserve: true }); return; }
        selectTable(id);
      },
      onNodeDblClick: function (id) {
        if (S.cfg.focus.length === 1 && S.cfg.focus[0] === id) { S.cfg.focus = []; syncControls(); render({ fit: true }); }
        else focusOn(id);
      },
      onBackgroundClick: function () { closeDetails(); $("#search-results").hidden = true; },
      onNodeMove: function (id, x, y) { return JSON.parse(viz.move_node(id, x, y)); },
      onNodeDrop: function (id, x, y) { S.cfg.positions[id] = [x, y]; saveState(); },
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
      bindTables();
      bindCompare();
      bindSearch();
      bindExport();
      bindKeys();
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
      var stored = loadState();
      var cfg = merge(clone(S.defaults), S.projectCfg.default || {});
      if (params.has("cfg")) {
        try { merge(cfg, JSON.parse(params.get("cfg"))); } catch (e) { toast("Invalid cfg parameter"); }
      } else if (stored && stored.cfg) merge(cfg, stored.cfg);
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
      render({ fit: true });
      if (S.base && S.diff && S.diff.tables.length) showTab("changes");
      poll();
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
    if (pick) return loadExample(pick);
    $("#loading").textContent = "Open or drop a structure.sql file";
  }

  boot();
})();
