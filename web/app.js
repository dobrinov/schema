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
    mode: "browse", prevMode: null, viewName: "",
    design: null, designSlug: null, designState: null, designUndo: [], designDirty: false, designPath: null, editor: null, playground: { current: null, base: null, name: "structure.sql", baseName: null },
  };
  var MODE_LABEL = { browse: "Browse", compare: "Compare", design: "Design" };

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
      var st = { cfg: diffObj(S.cfg, S.defaults), base: S.base, compare: S.compare, lens: S.lens, mode: S.mode, browseRef: S.browseRef || null, mergeBaseOf: S.mergeBaseOf || null };
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
  var STRUCTURAL = ["focus", "focus_depth", "focus_depths", "focus_direction", "include", "exclude", "schemas", "changes_only", "changes_context", "layout.algorithm", "layout.direction", "layout.group_by", "groups", "show_views", "show_partitions", "show_isolated", "enums"];
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
    renderStatus(res);
    renderChanges();
    renderFilterBar();
    renderDiffSummary();
    if (S.mode === "browse") renderBrowseList();
    if (S.selected) {
      if (viewer.nodes.has(S.selected)) viewer.select(S.selected);
      renderDetails(S.selected);
    } else if (S.selectedObj) {
      // keep the object's definition diff open across re-renders; drop it when the comparison no longer has it
      var it = ((S.diff || {})[S.selectedObj.group] || []).find(function (x) { return x.name === S.selectedObj.name; });
      if (it) renderObjectDetails(S.selectedObj.group, it); else closeDetails();
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
      el.innerHTML = "<div>" + esc(msg2) + "</div><div class=\"btns\">" + (changed && S.lens.combine ? "<button class=\"btn\" id=\"empty-uncombine\">Show them anyway</button>" : "") +
        "<button class=\"btn\" id=\"empty-exit\">All tables</button></div>";
      var u = $("#empty-uncombine");
      if (u) u.onclick = function () { S.lens.combine = false; applyFilter(); };
      $("#empty-exit").onclick = exitLens;
      return;
    }
    var msg = S.tables.length ? "No tables match the current filters." : "No tables found in this file.";
    el.innerHTML = "<div>" + esc(msg) + "</div>" + (S.tables.length ? "<div class=\"btns\"><button class=\"btn\" id=\"empty-reset\">Clear filters</button></div>" : "");
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

  /** The canvas bar's right side: how many tables are shown, and the Display
   *  button's summary. Notices (layout fallbacks etc.) become a toast. */
  var DIRS = { LR: "→", TB: "↓", RL: "←", BT: "↑" };
  function renderStatus(res) {
    var st = res.stats;
    var nt = st.nodes_visible - (st.enums_visible || 0);
    var total = (st.tables_total || 0) + (S.cfg.show_views ? st.views_total || 0 : 0);
    var why = $("#fb-why");
    why.innerHTML = nt < total ? "<b>" + nt + "</b><span>of " + total + " tables</span>" : "<b>" + total + "</b><span>table" + (total === 1 ? "" : "s") + "</span>";
    var alg = S.cfg.layout.algorithm;
    $("#display-sub").textContent = "· " + alg + (alg === "layered" ? " " + (DIRS[S.cfg.layout.direction] || "") : "") +
      (st.column_mode !== "auto" && st.column_mode !== "all" ? " · " + (st.column_mode === "none" ? "headers" : st.column_mode) : "");
    var notice = (st.notices || []).join("\n");
    if (notice && notice !== S.lastNotice) toast("⚠ " + notice, 4000);
    S.lastNotice = notice;
  }

  function changedObjectCount(d) {
    return d.tables.length + (d.enums || []).length + (d.views || []).length + (d.functions || []).length + (d.triggers || []).length + (d.extensions || []).length;
  }
  function pillsHtml(d) {
    var s = d.summary;
    if (!d.tables.length) return "<span class=\"pill pill--none\">" + (s.other_changes ? "no table changes" : "no changes") + "</span>";
    return (s.tables_added ? "<span class=\"pill pill--add\">+" + s.tables_added + "</span>" : "") +
      (s.tables_removed ? "<span class=\"pill pill--del\">−" + s.tables_removed + "</span>" : "") +
      (s.tables_modified ? "<span class=\"pill pill--mod\">~" + s.tables_modified + "</span>" : "") + "<span class=\"lbl\">tables</span>";
  }
  function renderDiffSummary() {
    var box = $("#diff-pills"), d = S.diff;
    var active = d && (S.base || S.design);
    var n = active ? changedObjectCount(d) : 0;
    $("#changes-count").textContent = n && !S.design ? String(n) : "";
    if (!box) return;
    box.innerHTML = active ? pillsHtml(d) : "";
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
      $$("button", seg).forEach(function (b) { b.setAttribute("aria-pressed", b.getAttribute("data-v") === String(v) ? "true" : "false"); });
    });
    $$("[data-out]").forEach(function (o) { o.textContent = getPath(S.cfg, o.getAttribute("data-out")); });
    $$("[data-show-if]").forEach(function (el) {
      var c = el.getAttribute("data-show-if").split("=");
      var v = getPath(S.cfg, c[0]);
      el.hidden = c.length > 1 ? String(v) !== c[1] : !(Array.isArray(v) ? v.length : v);
    });
    var gc = $("#group-custom-opt");
    if (gc) gc.hidden = !(S.cfg.groups && S.cfg.groups.length) && S.cfg.layout.group_by !== "custom";
    var gn = $("#groups-n");
    if (gn) gn.textContent = S.cfg.groups && S.cfg.groups.length ? "· " + S.cfg.groups.length : "";
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
    $("#reset-positions").onclick = function () { resetPositions(); toast(filterActive() ? "Positions reset for this filter" : "Positions reset"); };
    $("#reset-config").onclick = function () {
      S.cfg = merge(clone(S.defaults), (S.projectCfg && S.projectCfg.default) || {});
      syncControls();
      render({ fit: true });
      toast("Display settings reset");
    };
    $$("#modeswitch button").forEach(function (b) {
      b.onclick = function () { setMode(b.getAttribute("data-mode")); };
    });
    // display settings live in a popover, available in every mode
    var dp = $("#display-pop");
    $("#display-btn").onclick = function (e) {
      e.stopPropagation();
      dp.hidden = !dp.hidden;
      if (!dp.hidden) placePop(dp, $("#display-btn"));
    };
    document.addEventListener("click", function (e) { if (!e.target.closest("#display-pop,#display-btn")) dp.hidden = true; });
  }

  /** Put a fixed-position popover under `anchor`, kept inside the window. */
  function placePop(pop, anchor, o) {
    o = o || {};
    var r = anchor.getBoundingClientRect();
    pop.style.top = (r.bottom + 6) + "px";
    var left = o.alignRight ? r.right - pop.offsetWidth : r.left;
    pop.style.left = Math.max(8, Math.min(left, innerWidth - pop.offsetWidth - 8)) + "px";
    var over = r.bottom + 6 + pop.offsetHeight - (innerHeight - 8);
    if (over > 0) pop.style.top = Math.max(8, r.bottom + 6 - over) + "px";
  }

  // ---- modes: browse / compare / design ---------------------------------------
  function showPanel(mode) {
    $$("#modeswitch button").forEach(function (x) { x.setAttribute("aria-pressed", x.getAttribute("data-mode") === mode ? "true" : "false"); });
    $$(".panel").forEach(function (p) { p.classList.toggle("active", p.getAttribute("data-panel") === mode); });
    document.body.setAttribute("data-mode", mode);
    $("#design-foot").hidden = !(mode === "design" && S.design);
    renderSource();
  }

  // ---- source card: the first block of the sidebar in every mode -------------
  // Browse: which version is viewed. Compare: what is compared (presets, base,
  // compare, totals, fetch). Design: the design's name, intent, base and state.
  function refDetail(r) {
    if (STATIC) return r === BASEFILE ? "base file" : "your file";
    var s = S.server || {};
    if (r === WORKTREE) return (s.branch ? s.branch : "") + (s.dirty ? " · uncommitted changes" : s.branch ? "" : "checked-out files");
    if (r === INDEX) return "staged changes";
    if (r === BASEFILE) return "file, not in git";
    if (!r) return "";
    var m = /^([0-9a-f]{7,40})(\^?)$/.exec(r);
    if (m) {
      var c = S.log.find(function (x) { return x.sha === m[1]; });
      return c ? (m[2] ? "parent of: " : "") + c.subject : "commit";
    }
    if (S.refs.branches.indexOf(r) >= 0) return r.indexOf("origin/") === 0 ? "remote branch" : "branch";
    if (S.refs.tags.indexOf(r) >= 0) return "tag";
    return "git ref";
  }
  function refRow(which, ref, side) {
    var unset = !ref;
    return "<button class=\"ref\" id=\"" + which + "-select\" title=\"Click to search branches, tags and commits, or type any ref\">" +
      (side ? "<span class=\"ref__side\">" + side + "</span>" : "") +
      "<span class=\"ref__main\"><code class=\"" + (unset ? "unset" : "") + "\">" + esc(unset ? "pick a version…" : refLabel(ref)) + "</code>" +
      (unset ? "" : "<small>" + esc(refDetail(ref)) + "</small>") + "</span></button>";
  }
  function renderSource() {
    var box = $("#source");
    if (!box || !S.cfg) return;
    var s = S.server || {}, git = !STATIC && s.is_git, h = "";
    if (S.mode === "browse") {
      if (STATIC) {
        h = "<div class=\"source__head\"><span class=\"source__label\">Viewing</span><span class=\"source__state\">files never leave your browser</span></div>" +
          "<div class=\"refs\"><div class=\"ref\" style=\"cursor:default\"><span class=\"ref__main\"><code>" + esc(S.playground.name) + "</code><small>" + (S.playground.base ? "compared with " + esc(S.playground.baseName) : "playground") + "</small></span></div></div>" +
          "<div class=\"source__foot\"><button class=\"btn btn--sm\" id=\"pg-open\">Open schema…</button>" +
          ((STATIC.examples || []).length ? "<select class=\"input input--sm\" id=\"pg-examples\"><option value=\"\">Examples…</option>" + STATIC.examples.map(function (x, i) { return "<option value=\"" + i + "\">" + esc(x.name) + "</option>"; }).join("") + "</select>" : "") + "</div>";
      } else {
        h = "<div class=\"source__head\"><span class=\"source__label\">Viewing</span>" + (git ? "<span class=\"source__state\">nothing is checked out</span>" : "") + "</div>" +
          (git ? "<div class=\"refs\">" + refRow("view", S.compare) + "</div>"
            : "<div class=\"refs\"><div class=\"ref\" style=\"cursor:default\"><span class=\"ref__main\"><code>" + esc(refLabel(S.compare)) + "</code><small>not in git</small></span></div></div>");
      }
    } else if (S.mode === "compare") {
      if (STATIC) {
        h = "<div class=\"source__head\"><span class=\"source__label\">Comparing</span></div>" +
          "<div class=\"refs\"><div class=\"ref\" style=\"cursor:default\"><span class=\"ref__side\">base</span><span class=\"ref__main\"><code" + (S.playground.base ? "" : " class=\"unset\"") + ">" + esc(S.playground.base ? S.playground.baseName : "no base file yet") + "</code></span></div>" +
          "<div class=\"ref\" style=\"cursor:default\"><span class=\"ref__side\">compare</span><span class=\"ref__main\"><code>" + esc(S.playground.name) + "</code></span></div></div>" +
          "<div class=\"source__foot\"><span class=\"pills\" id=\"diff-pills\"></span><span class=\"spacer\"></span><button class=\"btn btn--sm\" id=\"pg-base\">Compare with…</button>" +
          (S.playground.base ? "<button class=\"icon-btn\" id=\"pg-clear-base\" title=\"Remove the base file\">×</button>" : "") + "</div>";
      } else {
        var comparable = git || !!s.base_file;
        h = "<div class=\"source__head\"><span class=\"source__label\">Comparing</span>" +
          (comparable ? "<button class=\"btn btn--ghost btn--sm caret\" id=\"preset-btn\" style=\"color:var(--fg-muted)\" title=\"Common comparisons\">" + esc(presetLabel()) + "</button>" : "") + "</div>" +
          (comparable ? "<div class=\"refs\">" + refRow("base", S.base, "base") + refRow("compare", S.compare, "compare") +
            "<button class=\"refs__swap\" id=\"swap-btn\" title=\"Swap base and compare\"" + (S.base ? "" : " disabled") + ">⇅</button></div>"
            : "<p>This file is not in a git repository. Start with <code>--base-file old.sql</code> to compare two files.</p>") +
          "<div class=\"source__foot\"><span class=\"pills\" id=\"diff-pills\"></span><span class=\"spacer\"></span>" +
          (git ? "<button class=\"btn btn--ghost btn--sm\" id=\"fetch-btn\" style=\"color:var(--fg-muted)\" title=\"git fetch: pick up branches your colleagues pushed\">↻ Fetch</button>" : "") + "</div>";
      }
    } else if (S.mode === "design") {
      var d = S.design;
      if (!d) {
        h = "<div class=\"source__head\"><span class=\"source__label\">Design</span></div>" +
          "<p><b>Sketch schema changes</b> on top of the loaded schema: tables, columns, foreign keys, indexes. They show up as a diff and export as a spec an agent implements with migrations.</p>" +
          "<input type=\"text\" class=\"input\" id=\"design-new-name\" placeholder=\"Name the design, e.g. Card payments\">" +
          "<button class=\"btn btn--primary\" id=\"design-start\">Start designing</button>";
      } else {
        var saved = S.designSlug && !S.designDirty;
        h = "<div class=\"source__head\"><span class=\"source__label\">Design</span><span class=\"source__state\" id=\"design-state\">" +
          "<span class=\"dot " + (saved ? "dot--add" : "dot--mod") + "\"></span>" + (saved ? "saved" : S.designSlug ? "unsaved changes" : "not saved yet") +
          "<button class=\"icon-btn\" id=\"design-menu-btn\" title=\"Rename, close…\" style=\"width:22px;height:20px\">⋯</button></span></div>" +
          "<input type=\"text\" class=\"source__title-input\" id=\"design-name\" value=\"" + esc(d.name) + "\" placeholder=\"Name\" spellcheck=\"false\">" +
          "<textarea class=\"source__desc\" id=\"design-desc\" rows=\"2\" placeholder=\"Goal of the change, context for the implementer\">" + esc(d.description || "") + "</textarea>" +
          "<span class=\"source__meta\">on " + esc(refLabel((d.source && d.source.ref) || S.compare)) + (d.source && d.source.commit ? " · " + esc(String(d.source.commit).slice(0, 7)) : "") + "</span>";
      }
    }
    box.innerHTML = h;
    renderDiffSummary();
    var dn = $("#design-name");
    if (dn) {
      dn.oninput = function (e) { S.design.name = e.target.value; markDesignDirty(); };
      var dd = $("#design-desc");
      dd.oninput = function (e) { S.design.description = e.target.value; markDesignDirty(); };
      var grow = function () { dd.style.height = "auto"; dd.style.height = Math.min(120, dd.scrollHeight) + "px"; };
      dd.addEventListener("input", grow);
      grow();
    }
  }
  function markDesignDirty() {
    S.designDirty = true;
    saveDesignDraft();
    var st = $("#design-state");
    if (st) st.innerHTML = "<span class=\"dot dot--mod\"></span>" + (S.designSlug ? "unsaved changes" : "not saved yet") + "<button class=\"icon-btn\" id=\"design-menu-btn\" title=\"Rename, close…\" style=\"width:22px;height:20px\">⋯</button>";
  }

  /** Common comparisons: [label, base, compare, description]. */
  function presetItems() {
    var s = S.server || {}, items = [];
    if (STATIC || !s.is_git) return items;
    if (s.dirty) items.push(["Uncommitted changes", "HEAD", WORKTREE, "HEAD → working tree"]);
    items.push(["Staged changes", "HEAD", INDEX, "HEAD → index"]);
    items.push(["Last commit", "HEAD~1", "HEAD", "HEAD~1 → HEAD"]);
    ["main", "master", "develop"].forEach(function (m) {
      if (S.refs.branches.indexOf(m) >= 0 && s.branch !== m) items.push(["This branch vs " + m, m + "...HEAD", WORKTREE, "from where it split off " + m]);
    });
    items.push(["A colleague's branch…", "__branch__", null, "origin/…, from where it left main"]);
    if (s.base_file) items.push(["Base file vs working tree", BASEFILE, WORKTREE, s.base_file.split("/").pop()]);
    return items;
  }
  function presetLabel() {
    if (!S.base) return "Choose…";
    var hit = presetItems().find(function (it) { return it[1] === S.base && it[2] === S.compare; });
    if (hit) return hit[0];
    if (S.mergeBaseOf && S.mergeBaseOf.sha === S.base) return S.mergeBaseOf.label;
    if (/^[0-9a-f]{40}\^$/.test(S.base) && S.compare === S.base.slice(0, 40)) return "One commit";
    return "Custom";
  }
  function applyPreset(it) {
    if (it[1] === "__branch__") { S.pendingBranchReview = true; openRefPicker("compare"); return; }
    setMode("compare", { force: true, comparison: { base: it[1], compare: it[2] }, presetLabel: it[0] });
  }

  function bindSource() {
    var box = $("#source");
    box.addEventListener("click", function (e) {
      var b = e.target.closest("button,select");
      if (!b) return;
      var id = b.id;
      if (id === "view-select") { e.stopPropagation(); openRefPicker("view"); }
      else if (id === "base-select") { e.stopPropagation(); openRefPicker("base"); }
      else if (id === "compare-select") { e.stopPropagation(); openRefPicker("compare"); }
      else if (id === "preset-btn") { e.stopPropagation(); openRefPicker(S.base ? "base" : "compare", { presets: true }); }
      else if (id === "swap-btn") { if (S.base) setComparison(S.compare, S.base); }
      else if (id === "fetch-btn") fetchOrigin(b);
      else if (id === "pg-open") { S.pgTarget = "current"; $("#pg-file").click(); }
      else if (id === "pg-base") { S.pgTarget = "base"; $("#pg-file").click(); }
      else if (id === "pg-clear-base") { S.playground.base = null; S.base = null; refreshPlayground(); }
      else if (id === "design-start") {
        var n = $("#design-new-name").value.trim();
        if (!n) { $("#design-new-name").focus(); toast("Name the design first", 1500); return; }
        newDesign(n);
      } else if (id === "design-menu-btn") { e.stopPropagation(); designMenu(b); }
    });
    box.addEventListener("keydown", function (e) {
      if (e.target.id === "design-new-name" && e.key === "Enter") $("#design-start").click();
      if (e.target.id === "design-name" && e.key === "Enter") e.target.blur();
    });
    box.addEventListener("change", function (e) {
      if (e.target.id === "pg-examples") {
        var x = STATIC.examples[Number(e.target.value)];
        e.target.value = "";
        if (x) loadExample(x);
      }
    });
  }
  function fetchOrigin(b) {
    if (b) { b.disabled = true; b.textContent = "↻ fetching…"; }
    return api("api/git/fetch", { method: "POST" }).then(function (r) {
      S.lastFetch = Date.now();
      return loadGit().then(function () { updateCompareUI(); toast(r.summary || "Fetched"); });
    }, function (e) { toast("Fetch failed: " + e.message, 5000); }).then(function () { if (b) { b.disabled = false; b.textContent = "↻ Fetch"; } });
  }
  function designMenu(anchor) {
    var m = $("#context-menu");
    var items = [
      ["Rename", function () { var n = $("#design-name"); if (n) { n.focus(); n.select(); } }],
      ["Re-layout the diagram", function () { $("#design-relayout").click(); }],
      ["-"],
      ["Close design", function () { closeDesign(false); }],
    ];
    if (S.designSlug) {
      items.push(["Delete design…", function () {
        deleteDesign(S.designSlug, S.design.name).then(function (ok) { if (ok) { S.designDirty = false; closeDesign(false); } });
      }, "danger"]);
    }
    m.innerHTML = "<div class=\"menu__head\">" + esc(S.design.name || "design") + "</div>" + items.map(function (it, i) {
      if (it[0] === "-") return "<hr class=\"menu__sep\">";
      return "<button class=\"menu__item" + (it[2] ? " menu__item--" + it[2] : "") + "\" data-i=\"" + i + "\"><span>" + esc(it[0]) + "</span></button>";
    }).join("");
    m.hidden = false;
    placePop(m, anchor, { alignRight: true });
    $$("button", m).forEach(function (b) { b.onclick = function () { m.hidden = true; items[Number(b.getAttribute("data-i"))][1](); }; });
  }

  /** Default comparison when entering Compare mode without one. */
  function defaultComparison() {
    var s = S.server || {};
    if (STATIC) return S.playground.base ? { base: BASEFILE, compare: WORKTREE } : null;
    if (s.base_file) return { base: BASEFILE, compare: WORKTREE };
    if (!s.is_git) return null;
    if (s.dirty) return { base: "HEAD", compare: WORKTREE };
    var main = ["main", "master"].find(function (m) { return S.refs.branches.indexOf(m) >= 0; });
    if (main && s.branch !== main) return { base: main + "...HEAD", compare: WORKTREE };
    return { base: "HEAD~1", compare: "HEAD" };
  }

  function setMode(mode, o) {
    o = o || {};
    if (mode === S.mode && !o.force) { showPanel(mode); return; }
    if (S.mode === "design" && mode !== "design" && S.design) {
      // leaving Design: ask about unsaved work first, then come back here
      closeDesign(true).then(function (ok) { if (ok) setMode(mode, o); });
      return;
    }
    S.mergeBaseOf = null;
    var prev = S.mode;
    S.mode = mode;
    showPanel(mode);
    if (mode === "browse") {
      S.lens = null;
      var ref = S.browseRef || (prev === "compare" ? S.compare : S.compare) || WORKTREE;
      if (ref === INDEX) ref = WORKTREE;
      S.browseRef = ref;
      if (S.base || S.compare !== ref) setComparison(null, ref, { fit: true });
      else { syncControls(); render({ fit: !!o.fit, preserve: !o.fit }); updateCompareUI(); }
    } else if (mode === "compare") {
      var c = o.comparison || (S.base ? { base: S.base, compare: S.compare } : defaultComparison());
      if (!c) { toast(STATIC ? "Load a second file with Compare with… to compare" : "This file is not in git — start with --base-file to compare two files", 4000); S.mode = prev; showPanel(prev); return; }
      if (c.base && c.base.indexOf("...") > 0) {
        var parts = c.base.split("...");
        var label = o.presetLabel || "This branch vs " + parts[0];
        api("api/git/merge-base?a=" + encodeURIComponent(parts[0]) + "&b=" + encodeURIComponent(parts[1])).then(function (r) { S.mergeBaseOf = { sha: r.sha, label: label }; setComparison(r.sha, c.compare, { lens: { kind: "diff", label: "Changed tables" } }); }, function () { setComparison("HEAD", c.compare, { lens: { kind: "diff", label: "Changed tables" } }); });
      } else setComparison(c.base, c.compare, { lens: { kind: "diff", label: "Changed tables" } });
    } else if (mode === "design") {
      S.prevMode = prev === "design" ? "browse" : prev;
      S.lens = null;
      if (S.base) setComparison(null, S.compare === INDEX ? WORKTREE : S.compare, { fit: true });
      else { renderDesignPanel(); updateCompareUI(); }
    }
    saveState();
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
    S.selectedObj = null;
    viewer.select(id);
    if (o.center && viewer.nodes.has(id)) viewer.centerOn(id);
    if (S.editor) return; // the docked editor keeps its slot until it is closed
    renderDetails(id);
  }
  function closeDetails() {
    S.selected = null;
    S.selectedObj = null;
    viewer.select(null);
    $("#details").hidden = true;
  }

  var BADGE = { added: "<span class=\"badge badge--add\">new</span>", removed: "<span class=\"badge badge--del\">dropped</span>", modified: "<span class=\"badge badge--mod\">changed</span>" };
  var SIGN = { added: "+", removed: "−", modified: "~" };
  var SIGN_CLS = { added: "sign--add", removed: "sign--del", modified: "sign--mod" };
  function signCell(st) { return "<td class=\"sign\"><span class=\"sign " + (SIGN_CLS[st] || "") + "\">" + (SIGN[st] || "") + "</span></td>"; }

  function colFlags(t, c) {
    if (t.primary_key && t.primary_key.columns.indexOf(c) >= 0) return "<b class=\"key key--pk\">PK</b>";
    if ((t.foreign_keys || []).some(function (x) { return x.columns.indexOf(c) >= 0; })) return "<b class=\"key key--fk\">FK</b>";
    var uq = (t.uniques || []).some(function (u) { return u.columns.length === 1 && u.columns[0] === c; }) ||
      (t.indexes || []).some(function (i) { return i.unique && !i.predicate && i.columns.length === 1 && i.columns[0] === c; });
    return uq ? "<b class=\"key key--uq\">UQ</b>" : "";
  }

  function detailsHead(id, o) {
    var dot = id.indexOf("."), schema = dot > 0 ? id.slice(0, dot) : "public", name = dot > 0 ? id.slice(dot + 1) : id;
    return "<div class=\"details__head\"><h2 class=\"details__title\"><span class=\"name\">" + (schema !== "public" ? "<span class=\"schema\">" + esc(schema) + ".</span>" : "") + esc(name) + "</span>" +
      (o.kind ? "<span class=\"badge badge--muted\">" + o.kind + "</span>" : "") + (BADGE[o.status] || "") +
      "<button class=\"icon-btn close\" title=\"Close (Esc)\">×</button></h2>" +
      (o.comment ? "<p class=\"details__comment\">" + esc(o.comment) + "</p>" : "") +
      (o.tools ? "<div class=\"details__tools\">" + o.tools + "</div>" : "") + "</div>";
  }

  function renderDetails(id) {
    var d = JSON.parse(viz.table(id));
    var box = $("#details");
    var t = d.table, v = d.view, en = d.enum;
    if (!t && !v && !en) { box.hidden = true; return; }
    if (!t && !v && en) { renderEnumDetails(id, en); return; }
    if (S.editor) return; // the docked editor owns the right panel
    box.hidden = false;
    var diff = d.diff || { columns: [], foreign_keys: [], indexes: [], constraints: [], properties: [] };
    var colEnums = d.column_enums || {};
    var status = d.status;
    var ov = S.cfg.tables[id] || {};
    var visible = viewer.nodes.has(id);
    var tools =
      (S.design && t && status !== "removed" ? "<button class=\"btn btn--sm btn--primary\" data-act=\"edit\">Edit table <span class=\"sc\">dbl-click</span></button>" : "") +
      (patternFor(id) ? "<button class=\"btn btn--sm\" data-act=\"unfocus\" title=\"Remove from the filter\">In filter <span class=\"sc\">×</span></button>"
        : "<button class=\"btn btn--sm\" data-act=\"focus\" title=\"Show only this table and its neighbours\">Focus" + (S.design ? "" : " <span class=\"sc\">dbl-click</span>") + "</button>" +
          (S.cfg.focus.length ? "<button class=\"btn btn--sm\" data-act=\"addfocus\" title=\"Add to the current filter\">+ Add to filter</button>" : "")) +
      (visible ? "<button class=\"btn btn--sm\" data-act=\"hide\">Hide</button>" : "<button class=\"btn btn--sm\" data-act=\"show\">Show</button>") +
      (t ? "<select class=\"btn btn--sm\" data-act=\"colmode\" title=\"Columns shown for this table\">" +
        [["", "columns: default"], ["all", "all columns"], ["keys", "keys only"], ["relations", "PK/FK only"], ["referenced", "referenced only"], ["changed", "changed only"], ["none", "collapsed"]].map(function (o) {
          var cur = ov.collapsed ? "none" : ov.columns || "";
          return "<option value=\"" + o[0] + "\"" + (cur === o[0] ? " selected" : "") + ">" + o[1] + "</option>";
        }).join("") + "</select>" : "") +
      "<input type=\"color\" data-act=\"color\" title=\"Header colour\" value=\"" + esc(ov.color || "#4f6bed") + "\">" +
      (ov.color ? "<button class=\"icon-btn\" data-act=\"nocolor\" title=\"Remove colour\">×</button>" : "");
    var h = detailsHead(id, { status: status, comment: (t && t.comment) || (v && v.comment), tools: tools, kind: v ? (v.materialized ? "mview" : "view") : "" });
    h += "<div class=\"details__body\">";

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
      var hiddenCount = cols.filter(function (x) { return node && x.st !== "removed" && !node.querySelector("[data-col=\"" + CSS.escape(x.c.name) + "\"]"); }).length;
      var nDel = cols.filter(function (x) { return x.st === "removed"; }).length, nAdd = cols.filter(function (x) { return x.st === "added"; }).length, nMod = cols.filter(function (x) { return x.st === "modified"; }).length;
      var note = status === "modified" ? [nMod ? "~" + nMod : "", nAdd ? "+" + nAdd : "", nDel ? "−" + nDel : ""].filter(Boolean).join(" ") + (nMod || nAdd || nDel ? " column" + (nMod + nAdd + nDel > 1 ? "s" : "") : "") : hiddenCount ? hiddenCount + " hidden in diagram" : "";
      // indexes a column is part of: an IX tag in the row, the details in its tooltip
      var idxOf = function (owner, name) { return (owner.indexes || []).filter(function (i) { return i.columns.indexOf(name) >= 0; }); };
      var colTip = function (owner, c) {
        var lines = [];
        if (c.comment) lines.push(c.comment);
        (owner.foreign_keys || []).filter(function (f) { return f.columns.indexOf(c.name) >= 0; }).forEach(function (f) { lines.push("→ " + display(f.ref_table) + "(" + f.ref_columns.join(", ") + ")" + (f.on_delete ? " on delete " + f.on_delete.toLowerCase() : "")); });
        idxOf(owner, c.name).forEach(function (i) { lines.push((i.unique ? "unique index " : "index ") + i.name + " (" + i.columns.join(", ") + ")" + (i.predicate ? " where " + i.predicate : "")); });
        if (c.default) lines.push("default: " + c.default);
        return lines.join("\n");
      };
      var wideKeys = cols.some(function (x) { var o = x.st === "removed" && d.base ? d.base : t; return idxOf(o, x.c.name).length && (o.is_pk ? false : (colFlags(o, x.c.name).indexOf("key--pk") >= 0 || colFlags(o, x.c.name).indexOf("key--fk") >= 0)); });
      h += "<h3 class=\"sh\">Columns <span>" + t.columns.length + "</span><em>" + esc(note) + "</em></h3><table class=\"cols\"><colgroup><col class=\"c-sign\"><col class=\"c-key\"" + (wideKeys ? " style=\"width:44px\"" : "") + "><col><col class=\"c-type\"><col class=\"c-eye\"></colgroup>" + cols.map(function (x) {
        var c = x.c, shown = node ? !!node.querySelector("[data-col=\"" + CSS.escape(c.name) + "\"]") : true;
        var owner = x.st === "removed" && d.base ? d.base : t;
        var flags = colFlags(owner, c.name);
        var inIdx = idxOf(owner, c.name);
        // IX sits next to PK / FK in the key column (a UQ badge already implies an index)
        var ix = inIdx.length && flags.indexOf("key--uq") < 0 ? "<b class=\"key key--ix\" title=\"" + esc(inIdx.map(function (i) { return i.name + " (" + i.columns.join(", ") + ")" + (i.predicate ? " where " + i.predicate : ""); }).join("\n")) + "\">IX</b>" : "";
        var chg = "";
        if (x.st === "modified") chg = (byName[c.name].changes || []).map(function (f) { return "<span class=\"chg\">" + esc(f.field) + ": " + esc(f.old || "∅") + " → " + esc(f.new || "∅") + "</span>"; }).join("");
        return "<tr class=\"" + x.st + (shown ? "" : " hidden-col") + "\" title=\"" + esc(colTip(owner, c)) + "\">" + signCell(x.st) +
          "<td class=\"flags\">" + flags + ix + "</td>" +
          "<td class=\"name\">" + esc(c.name) + (c.nullable ? "<span class=\"nul\"> ?</span>" : "") + chg + (c.default ? "<span class=\"dflt\">= " + esc(c.default) + "</span>" : "") + "</td>" +
          "<td class=\"type\" title=\"" + esc(c.data_type) + (colEnums[c.name] ? " — enum, click for values" : "") + "\">" +
            (colEnums[c.name] ? "<a class=\"link\" data-enum=\"" + esc(colEnums[c.name]) + "\">" + esc(shortType(c.data_type)) + "</a>" : esc(shortType(c.data_type))) + "</td>" +
          "<td>" + (visible && x.st !== "removed" ? "<button class=\"eye\" data-col=\"" + esc(c.name) + "\" title=\"" + (shown ? "Hide in diagram" : "Show in diagram") + "\">" + (shown ? "👁" : "◌") + "</button>" : "") + "</td></tr>";
      }).join("") + "</table>";

      var idxSt = {};
      (diff.indexes || []).forEach(function (i) { idxSt[i.name] = i.status; });
      var idx = (t.indexes || []).map(function (i) {
        return "<li class=\"" + (idxSt[i.name] || "") + "\" title=\"" + esc(i.definition) + "\">" + (i.unique ? "<b class=\"key key--uq\">UQ</b>" : "<b class=\"key key--ix\">IX</b>") + esc(i.name) + " <span class=\"muted\">(" + esc(i.columns.join(", ")) + ")" + (i.predicate ? " WHERE " + esc(i.predicate) : "") + "</span></li>";
      });
      (diff.indexes || []).filter(function (i) { return i.status === "removed" || (i.status === "modified" && i.name.indexOf("→") >= 0); }).forEach(function (i) {
        idx.push("<li class=\"" + i.status + "\"><b class=\"key key--ix\">IX</b>" + esc(i.name) + " <span class=\"muted\">" + esc(i.old || "") + "</span></li>");
      });
      var cons = [];
      if (t.primary_key) cons.push("<li><b class=\"key key--pk\">PK</b>(" + esc(t.primary_key.columns.join(", ")) + ")</li>");
      (t.uniques || []).forEach(function (u) { cons.push("<li><b class=\"key key--uq\">UQ</b>(" + esc(u.columns.join(", ")) + ")</li>"); });
      (t.checks || []).forEach(function (c) { cons.push("<li><b class=\"key key--ix\">CK</b>" + esc(c.name ? c.name + ": " : "") + esc(c.expression) + "</li>"); });
      (diff.constraints || []).forEach(function (c) { cons.push("<li class=\"" + c.status + "\">" + esc(c.name) + ": " + esc(c.new || c.old) + "</li>"); });
      if (idx.length || cons.length) h += "<h3 class=\"sh\">Indexes & constraints <span>" + (idx.length + cons.length) + "</span></h3><ul class=\"list\">" + idx.join("") + cons.join("") + "</ul>";

      var fkSt = {}, fkRen = {};
      (diff.foreign_keys || []).forEach(function (f) {
        fkSt[f.name] = f.status;
        var m = f.name.split(" → ");
        if (m.length === 2) { fkSt[m[1]] = "modified"; fkRen[m[1]] = m[0]; }
      });
      var rels = (t.foreign_keys || []).map(function (f) {
        var key = f.name || "";
        return "<li class=\"" + (fkSt[key] || "") + "\"><span class=\"dir\">→</span><a class=\"link\" data-goto=\"" + esc(f.ref_table) + "\">" + esc(display(f.ref_table)) + "</a> <span class=\"muted\">" + esc(f.columns.join(", ")) +
          (f.on_delete ? " · on delete " + esc(f.on_delete.toLowerCase()) : "") + (fkRen[key] ? " · renamed from " + esc(fkRen[key]) : "") + "</span></li>";
      });
      (diff.foreign_keys || []).filter(function (f) { return f.status === "removed" && status === "modified"; }).forEach(function (f) { rels.push("<li class=\"removed\"><span class=\"dir\">→</span>" + esc(f.old) + "</li>"); });
      d.referenced_by.forEach(function (r) {
        rels.push("<li><span class=\"dir\">←</span><a class=\"link\" data-goto=\"" + esc(r.table) + "\">" + esc(display(r.table)) + "</a> <span class=\"muted\">" + esc(r.columns.join(", ")) + (r.on_delete ? " · on delete " + esc(r.on_delete.toLowerCase()) : "") + "</span></li>");
      });
      if (rels.length) h += "<h3 class=\"sh\">Relations <span>" + rels.length + "</span><em>click to jump</em></h3><ul class=\"list\">" + rels.join("") + "</ul>";
      (diff.properties || []).forEach(function (p) {
        h += "<h3 class=\"sh\">" + esc(p.field) + " changed</h3><ul class=\"list\"><li class=\"removed\">" + esc(p.old || "∅") + "</li><li class=\"added\">" + esc(p.new || "∅") + "</li></ul>";
      });
      if (t.partition_by) h += "<h3 class=\"sh\">Partitioned by</h3><ul class=\"list\"><li>" + esc(t.partition_by) + "</li></ul>";
      if (d.triggers.length) h += "<h3 class=\"sh\">Triggers <span>" + d.triggers.length + "</span></h3><ul class=\"list\">" + d.triggers.map(function (tr) { return "<li title=\"" + esc(tr.definition) + "\">" + esc(tr.name) + "</li>"; }).join("") + "</ul>";
      if (d.used_by_views.length) h += "<h3 class=\"sh\">Used by views</h3><ul class=\"list\">" + d.used_by_views.map(function (vid) { return "<li><a class=\"link\" data-goto=\"" + esc(vid) + "\">" + esc(display(vid)) + "</a></li>"; }).join("") + "</ul>";
    }
    if (v) {
      h += "<h3 class=\"sh\">Reads</h3><ul class=\"list\">" + v.depends_on.map(function (dep) { return "<li><span class=\"dir\">→</span><a class=\"link\" data-goto=\"" + esc(dep) + "\">" + esc(display(dep)) + "</a></li>"; }).join("") + "</ul>";
      h += "<h3 class=\"sh\">Definition</h3><pre class=\"def\">" + esc(v.definition) + "</pre>";
    }
    h += "</div>";
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
    if (S.editor) return; // the docked editor owns the right panel
    box.hidden = false;
    var status = en.status || "unchanged";
    var base = en.base_values || null;
    var rows = en.values.map(function (v) { return { v: v, st: base && status === "modified" && base.indexOf(v) < 0 ? "added" : "" }; });
    if (base && status === "modified") base.forEach(function (v, i) { if (en.values.indexOf(v) < 0) rows.splice(Math.min(i, rows.length), 0, { v: v, st: "removed" }); });
    var h = detailsHead(id, { status: status, kind: "enum" }) + "<div class=\"details__body\">" +
      "<h3 class=\"sh\">Values <span>" + en.values.length + "</span></h3><ul class=\"list\">" + rows.map(function (r) { return "<li class=\"" + r.st + "\"><span class=\"sign " + (SIGN_CLS[r.st] || "") + "\">" + (SIGN[r.st] || "") + "</span> " + esc(r.v) + "</li>"; }).join("") + "</ul>";
    if (en.used_by && en.used_by.length) {
      h += "<h3 class=\"sh\">Used by <span>" + en.used_by.length + "</span></h3><ul class=\"list\">" + en.used_by.map(function (u) {
        return "<li><span class=\"dir\">←</span><a class=\"link\" data-goto=\"" + esc(u.table) + "\">" + esc(display(u.table)) + "</a><span class=\"muted\">." + esc(u.column) + "</span></li>";
      }).join("") + "</ul>";
    }
    h += "</div>";
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
  /** "~1 +2 −1 · −1 idx": what changed in a table, before you click it. */
  function changeSummary(t) {
    if (t.status !== "modified") {
      var n = (t.columns || []).length;
      return n ? (t.status === "added" ? "+" : "−") + n : "";
    }
    var c = { added: 0, removed: 0, modified: 0 };
    (t.columns || []).forEach(function (x) { c[x.status] = (c[x.status] || 0) + 1; });
    var parts = [c.modified ? "~" + c.modified : "", c.added ? "+" + c.added : "", c.removed ? "−" + c.removed : ""].filter(Boolean);
    var idx = (t.indexes || []).length, fk = (t.foreign_keys || []).length + (t.constraints || []).length;
    if (idx) parts.push((parts.length ? "· " : "") + idx + " idx");
    if (fk) parts.push((parts.length ? "· " : "") + fk + " fk");
    if (!parts.length && (t.properties || []).length) parts.push("props");
    return parts.join(" ");
  }
  function renderChanges() {
    var box = $("#changes");
    var d = S.diff;
    if (!d || (!S.base && !S.design)) {
      box.innerHTML = "<div class=\"list-empty\"><b>Nothing compared yet.</b><br>" +
        (STATIC ? "Load a second file with <b>Compare with…</b> above to see what changed."
          : S.server && S.server.is_git ? "Pick a preset or a base and compare version above, or click a commit in the history below."
            : "This file is not in a git repository. Start with <code>--base-file old.sql</code> to compare two files.") + "</div>";
      $("#changes-count").textContent = "";
      return;
    }
    // The diagram shows the actual changes; this panel only lists what
    // changed so you can jump to it.
    var h = "";
    // every changed object: tables and enums (drawn), then views, functions,
    // triggers and extensions (their definitions diff in the details panel)
    var entries = d.tables.map(function (t) { return { id: t.id, status: t.status, kind: "table", sum: changeSummary(t) }; })
      .concat((d.enums || []).map(function (e) { return { id: e.name, status: e.status, kind: "enum", sum: "" }; }));
    OBJECT_KINDS.forEach(function (k) {
      (d[k.key] || []).forEach(function (it) { entries.push({ id: it.name, status: it.status, kind: k.kind, group: k.key, sum: k.key === "triggers" ? triggerTable(it.name).label : "" }); });
    });
    var counts = [["table", d.tables.length], ["enum", (d.enums || []).length]].concat(OBJECT_KINDS.map(function (k) { return [k.label, (d[k.key] || []).length]; }))
      .filter(function (c) { return c[1]; }).map(function (c) { return c[1] + " " + c[0] + (c[1] === 1 ? "" : "s"); });
    h += "<h3 class=\"sh\">Changes<em>" + counts.join(" · ") + "</em></h3>";
    if (!entries.length) h += "<div class=\"list-empty\">No schema changes between these versions.</div>";
    if (entries.length) {
      var order = { added: 0, modified: 1, removed: 2 };
      var kindOrder = { table: 0, enum: 1, view: 2, func: 3, trig: 4, ext: 5 };
      entries.sort(function (a, b) { return kindOrder[a.kind] - kindOrder[b.kind] || order[a.status] - order[b.status] || display(a.id).localeCompare(display(b.id)); });
      h += "<ul class=\"changed-list\">" + entries.map(function (t) {
        var drawable = t.kind === "table" || t.kind === "enum" || t.kind === "view";
        var visible = drawable && viewer.nodes.has(t.id);
        var selected = drawable ? t.id === S.selected : !!(S.selectedObj && S.selectedObj.group === t.group && S.selectedObj.name === t.id);
        var title = !drawable ? "Show the definition and what changed" : visible ? "Show in the diagram"
          : t.kind === "enum" ? "Not drawn — click to enable enum types and show it" : t.kind === "view" ? "Not drawn — click to enable views and show it" : "Hidden by the current filter — click to show";
        return "<li data-goto=\"" + esc(t.id) + "\"" + (t.group ? " data-group=\"" + t.group + "\"" : "") + " class=\"lrow lrow--change" + (drawable && !visible ? " is-out" : "") + "\" aria-selected=\"" + selected + "\" title=\"" + title + "\">" +
          "<span class=\"lrow__kind\">" + t.kind + "</span><span class=\"lrow__name" + (t.status === "removed" ? " del" : "") + "\">" + esc(objectLabel(t.kind, t.id)) + "</span>" +
          (t.sum ? "<span class=\"lrow__sum\">" + esc(t.sum) + "</span>" : "") + BADGE[t.status] + "</li>";
      }).join("") + "</ul>";
    }
    box.innerHTML = h;
    $$("[data-goto]", box).forEach(function (hd) {
      hd.onclick = function () {
        var id = hd.getAttribute("data-goto"), group = hd.getAttribute("data-group"), kind = hd.querySelector(".lrow__kind").textContent;
        if (group && group !== "views") { showObject(group, id); return; }
        if (viewer.nodes.has(id)) selectTable(id, { center: true });
        else if (kind === "enum") { S.cfg.enums = "all"; syncControls(); render({ preserve: true }); setTimeout(function () { selectTable(id, { center: true }); }, 120); }
        else if (kind === "view") { S.cfg.show_views = true; syncControls(); render({ preserve: true }); setTimeout(function () { if (viewer.nodes.has(id)) selectTable(id, { center: true }); else showObject("views", id); }, 120); }
        else { showTable(id); setTimeout(function () { selectTable(id, { center: true }); }, 80); }
      };
    });
  }

  // ---- non-table objects: views, functions, triggers, extensions ---------------
  var OBJECT_KINDS = [
    { key: "views", kind: "view", label: "view" },
    { key: "functions", kind: "func", label: "function" },
    { key: "triggers", kind: "trig", label: "trigger" },
    { key: "extensions", kind: "ext", label: "extension" },
  ];
  /** "trg_x ON public.accounts" → the table it fires on. */
  function triggerTable(name) {
    var m = / ON (\S+)$/.exec(name);
    return m ? { id: m[1], label: "on " + display(m[1]) } : { id: null, label: "" };
  }
  function objectLabel(kind, id) {
    if (kind === "trig") return id.replace(/ ON \S+$/, "");
    if (kind === "func") return display(id).replace(/\(.*\)$/, "()").replace("()", "");
    return display(id);
  }
  /** Line diff of two definitions: [[" " | "+" | "-", line], …] (LCS). */
  function lineDiff(a, b) {
    var A = a ? a.split("\n") : [], B = b ? b.split("\n") : [], out = [];
    if (A.length * B.length > 250000) { // too big to align: show as removed + added
      A.forEach(function (l) { out.push(["-", l]); });
      B.forEach(function (l) { out.push(["+", l]); });
      return out;
    }
    var n = A.length, m = B.length, L = [];
    for (var i = 0; i <= n; i++) { L.push(new Uint16Array(m + 1)); }
    for (i = n - 1; i >= 0; i--) for (var j = m - 1; j >= 0; j--) L[i][j] = A[i] === B[j] ? L[i + 1][j + 1] + 1 : Math.max(L[i + 1][j], L[i][j + 1]);
    i = 0; j = 0;
    while (i < n && j < m) {
      if (A[i] === B[j]) { out.push([" ", A[i]]); i++; j++; }
      else if (L[i + 1][j] >= L[i][j + 1]) { out.push(["-", A[i]]); i++; }
      else { out.push(["+", B[j]]); j++; }
    }
    while (i < n) out.push(["-", A[i++]]);
    while (j < m) out.push(["+", B[j++]]);
    return out;
  }
  function showObject(group, name) {
    var it = ((S.diff || {})[group] || []).find(function (x) { return x.name === name; });
    if (!it) return;
    closeDetails();
    S.selectedObj = { group: group, name: name };
    renderObjectDetails(group, it);
    renderChanges();
  }
  function renderObjectDetails(group, it) {
    var box = $("#details");
    if (S.editor) return;
    box.hidden = false;
    var k = OBJECT_KINDS.find(function (x) { return x.key === group; });
    var id = group === "triggers" ? it.name.replace(/ ON \S+$/, "") : it.name;
    var trig = group === "triggers" ? triggerTable(it.name) : null;
    var h = detailsHead(id, { status: it.status, kind: k.label }) + "<div class=\"details__body\">";
    if (trig && trig.id) h += "<h3 class=\"sh\">Fires on</h3><ul class=\"list\"><li><span class=\"dir\">→</span><a class=\"link\" data-goto=\"" + esc(trig.id) + "\">" + esc(display(trig.id)) + "</a></li></ul>";
    var diff = it.status === "modified" ? lineDiff(it.old || "", it.new || "")
      : (it.status === "added" ? (it.new || "").split("\n").map(function (l) { return ["+", l]; }) : (it.old || "").split("\n").map(function (l) { return ["-", l]; }));
    var na = diff.filter(function (x) { return x[0] === "+"; }).length, nd = diff.filter(function (x) { return x[0] === "-"; }).length;
    h += "<h3 class=\"sh\">Definition<span>" + (it.status === "modified" ? "" : it.status) + "</span><em>" + (it.status === "modified" ? "+" + na + " −" + nd + " lines" : diff.length + " lines") + "</em></h3>" +
      "<pre class=\"def ddiff\">" + diff.map(function (x) {
        var cls = x[0] === "+" ? "add" : x[0] === "-" ? "del" : "";
        return "<span class=\"" + cls + "\"><i>" + (x[0] === " " ? " " : x[0]) + "</i>" + esc(x[1]) + "\n</span>";
      }).join("") + "</pre></div>";
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

  function renderHistory() {
    var ul = $("#history");
    if (STATIC || !S.server || !S.server.is_git || !S.log.length) { ul.innerHTML = ""; $("#history-title").hidden = true; return; }
    $("#history-title").hidden = false;
    var items = [];
    if (S.server.dirty) items.push("<li class=\"crow\" data-base=\"HEAD\" data-compare=\"WORKTREE\" aria-selected=\"" + (S.base === "HEAD" && S.compare === WORKTREE) + "\"><span class=\"crow__sha\">work</span><span class=\"crow__subj\">Uncommitted changes</span><span class=\"crow__meta\">HEAD → working tree</span></li>");
    S.log.forEach(function (c) {
      var active = S.compare === c.sha && S.base === c.sha + "^";
      items.push("<li class=\"crow\" data-base=\"" + c.sha + "^\" data-compare=\"" + c.sha + "\" aria-selected=\"" + active + "\" title=\"" + esc(c.subject) + "\"><span class=\"crow__sha\">" + esc(c.short) + "</span><span class=\"crow__subj\">" + esc(c.subject) + "</span><span class=\"crow__meta\">" + esc(c.author) + " · " + ago(c.date) + "</span></li>");
    });
    ul.innerHTML = items.join("");
    $$("li", ul).forEach(function (li) {
      li.onclick = function () {
        var sha = li.getAttribute("data-compare"), c = S.log.find(function (x) { return x.sha === sha; });
        S.mergeBaseOf = null;
        setComparison(li.getAttribute("data-base"), sha, { lens: { kind: "commit", label: c ? c.short : "Uncommitted changes" } });
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
    showPanel(S.mode);
    renderHistory();
    if (S.mode === "browse") renderBrowseList();
  }

  // ---- browse: table list -------------------------------------------------------
  function hiddenByConfig(id) {
    return S.cfg.exclude.some(function (p) { return p === id || fbMatch(p, id); });
  }
  function renderBrowseList() {
    var ul = $("#browse-list");
    if (!ul || !S.tables) return;
    var q = ($("#browse-filter").value || "").trim().toLowerCase();
    var all = S.tables.filter(function (t) { return t.kind === "table" && t.status !== "removed"; });
    var list = all.filter(function (t) { return !q || t.label.toLowerCase().indexOf(q) >= 0; });
    $("#browse-total").textContent = q ? list.length + " of " + all.length : String(all.length);
    var groups = {}, names = [];
    list.forEach(function (t) { if (!groups[t.schema]) { groups[t.schema] = []; names.push(t.schema); } groups[t.schema].push(t); });
    var multi = names.length > 1;
    var row = function (t) {
      var hidden = !t.visible && hiddenByConfig(t.id);
      return "<li data-id=\"" + esc(t.id) + "\" class=\"lrow" + (multi ? " lrow--indent" : "") + (t.visible ? "" : hidden ? " is-hidden" : " is-out") + "\" aria-selected=\"" + (t.id === S.selected) + "\" title=\"" + esc(t.comment || (t.visible ? "" : hidden ? "hidden by a filter rule — click to show" : "outside the current view — click to show")) + "\">" +
        "<span class=\"lrow__name\">" + esc(multi ? t.label.replace(t.schema + ".", "") : t.label) + "</span>" + (hidden ? "<span class=\"lrow__tag\">hidden</span>" : "") +
        "<span class=\"lrow__n\" title=\"columns\">" + t.columns + "</span><span class=\"lrow__n\" title=\"relations\">" + (t.fk_in + t.fk_out) + "</span></li>";
    };
    ul.innerHTML = (multi ? names.map(function (s) {
      return "<li class=\"gh\"><span class=\"tri\">▾</span><code>" + esc(s) + "</code><span>" + groups[s].length + "</span></li>" + groups[s].map(row).join("");
    }).join("") : list.map(row).join("")) || "<li class=\"list-empty\">no tables</li>";
  }
  function bindBrowseList() {
    $("#browse-filter").addEventListener("input", renderBrowseList);
    $("#browse-list").addEventListener("click", function (e) {
      var li = e.target.closest("li[data-id]");
      if (!li) return;
      var id = li.getAttribute("data-id");
      if (viewer.nodes.has(id)) selectTable(id, { center: true });
      else { showTable(id); setTimeout(function () { selectTable(id, { center: true }); }, 80); }
    });
    $("#browse-list").addEventListener("mouseover", function (e) { var li = e.target.closest("li[data-id]"); viewer.highlight(li ? li.getAttribute("data-id") : null); });
    $("#browse-list").addEventListener("mouseleave", function () { viewer.highlight(null); });
  }
  function setComparison(base, compare, o) {
    o = o || {};
    S.base = base || null;
    if (S.base && S.mode !== "compare") { S.mode = "compare"; showPanel("compare"); }
    if (!S.base && S.mode === "compare" && !S.design) { S.mode = "browse"; showPanel("browse"); }
    S.compare = compare || WORKTREE;
    $("#loading").hidden = false;
    loadSources().then(function () {
      if (o.lens && S.diff && !S.diff.tables.length && !(S.diff.enums || []).length) {
        // nothing to narrow down to: show the whole schema instead of an empty view
        S.lens = null;
        var firstObj = null;
        OBJECT_KINDS.forEach(function (k) { if (!firstObj && (S.diff[k.key] || []).length) firstObj = { group: k.key, name: S.diff[k.key][0].name }; });
        if (firstObj) {
          // only functions / triggers / views changed: open the first one's definition diff
          toast("No table changes between " + refLabel(S.base) + " and " + refLabel(S.compare) + " — the changed objects are listed in the sidebar", 4000);
          S.selected = null;
          S.selectedObj = firstObj;
        } else toast("No schema changes between " + refLabel(S.base) + " and " + refLabel(S.compare), 4000);
      } else if (o.lens) startLens(o.lens);
      else if (!S.base) S.lens = null;
      if (S.mode === "browse") S.browseRef = S.compare;
      render(o.lens || o.fit ? { fit: true } : { fit: false, preserve: true });
      saveState();
    }, function (e) {
      toast("Could not load " + refLabel(S.compare) + ": " + e.message, 5000);
      $("#loading").hidden = true;
    });
  }
  // ---- ref picker: one popover for presets, base / compare and fetch -----------
  var pick = null; // { which, groups, pos, q, sides }
  function refItems(which) {
    var quick = [];
    if (which !== "base") quick.push({ v: WORKTREE, l: "working tree", d: which === "view" ? "your checked-out files" : "uncommitted changes" });
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
  function openRefPicker(which, o) {
    o = o || {};
    var pop = $("#ref-pop"), anchor = $("#" + which + "-select") || $("#preset-btn") || $("#source");
    if (!pop.hidden && pick && pick.which === which && !o.presets) { closeRefPicker(); return; }
    pick = { which: which, groups: refItems(which), pos: 0, q: "", sides: which !== "view" };
    var git = S.server && S.server.is_git;
    pop.innerHTML = "<div class=\"refpick__top\">" +
      (pick.sides ? "<div class=\"seg refpick__sides\" id=\"ref-sides\"></div>" : "") +
      "<input class=\"input input--mono\" id=\"ref-q\" placeholder=\"" + (pick.sides ? "branch, tag, sha or any ref" : "branch, tag, sha or any ref to view") + "\" autocomplete=\"off\" spellcheck=\"false\"></div>" +
      "<div class=\"popover__body\" id=\"ref-list\"></div>" +
      (git ? "<div class=\"popover__foot\"><b id=\"ref-fetch\" title=\"git fetch: pick up branches your colleagues pushed\">↻ Fetch origin</b><span class=\"muted\">" + (S.lastFetch ? "fetched " + ago(new Date(S.lastFetch).toISOString()) : "") + "</span>" +
        "<span class=\"spacer\"></span><span class=\"muted\" style=\"font-size:11px\">↑↓ ↵" + (pick.sides ? " · ⇥ switch side" : "") + "</span></div>" : "");
    pop.hidden = false;
    placePop(pop, anchor);
    renderRefSides();
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
      else if (e.key === "Tab" && pick.sides) { e.preventDefault(); switchRefSide(pick.which === "base" ? "compare" : "base"); }
      else if (/^[1-9]$/.test(e.key) && !inp.value && pick.sides) {
        var p = (pick.flat || []).filter(function (it) { return it.preset; })[Number(e.key) - 1];
        if (p) { e.preventDefault(); chooseRef(p); }
      }
    });
    var f = $("#ref-fetch");
    if (f) f.onclick = function () { f.textContent = "↻ fetching…"; fetchOrigin().then(function () { if (pick) { pick.groups = refItems(pick.which); renderRefList(); var fb = $("#ref-fetch"); if (fb) fb.textContent = "↻ Fetch origin"; } }); };
  }
  function switchRefSide(which) {
    pick.which = which;
    pick.groups = refItems(which);
    pick.pos = 0;
    renderRefSides();
    renderRefList();
    $("#ref-q").focus();
  }
  function renderRefSides() {
    var el = $("#ref-sides");
    if (!el) return;
    el.innerHTML = ["base", "compare"].map(function (s) {
      var v = s === "base" ? S.base : S.compare;
      return "<button data-side=\"" + s + "\" aria-pressed=\"" + (pick.which === s) + "\"><span>" + s + "</span><code>" + esc(v ? refLabel(v) : "…") + "</code></button>";
    }).join("") + "<kbd>⇥</kbd>";
    $$("button", el).forEach(function (b) { b.onclick = function () { switchRefSide(b.getAttribute("data-side")); }; });
  }
  function closeRefPicker() { $("#ref-pop").hidden = true; pick = null; }
  function renderRefList() {
    var list = $("#ref-list"), q = pick.q.trim().toLowerCase(), flat = [], sections = [];
    var match = function (it) { return !q || it.l.toLowerCase().indexOf(q) >= 0 || (it.d || "").toLowerCase().indexOf(q) >= 0 || it.v.toLowerCase().indexOf(q) >= 0; };
    if (pick.sides) {
      // presets set both sides; they lead when nothing is typed
      var presets = presetItems().filter(function (it) { return !q || it[0].toLowerCase().indexOf(q) >= 0; })
        .map(function (it, i) { return { v: it[1] + "\u0000" + it[2], l: it[0], d: it[3], preset: it, key: i + 1 }; });
      if (presets.length) sections.push({ title: "Presets", sub: "set both sides", items: presets, total: presets.length, presets: true });
    }
    var exact = false;
    pick.groups.forEach(function (g) {
      var all = g[1].filter(match);
      all.forEach(function (it) { if (it.l.toLowerCase() === q || it.v.toLowerCase() === q) exact = true; });
      if (all.length) sections.push({ title: g[0] || (pick.sides ? "Versions" : ""), sub: pick.sides ? "sets " + pick.which : "", items: all.slice(0, 40), total: all.length });
    });
    // anything typed that isn't in the lists can be used as a ref (HEAD~3, a sha,
    // origin/x); it comes after the matches so Enter picks the best match first
    if (q && !exact && /^[\w./~^@{}+-]+$/.test(pick.q.trim())) {
      sections.push({ title: sections.length ? "" : "", items: [{ v: pick.q.trim(), l: "Use “" + pick.q.trim() + "” as a ref", d: "", custom: true }], total: 1 });
    }
    var current = pick.which === "base" ? (S.base || "") : S.compare, h = "";
    if (pick.which === "compare" && S.pendingBranchReview && !q) h = "<div class=\"ref-empty\">Pick the branch to review (usually origin/…)</div>";
    sections.forEach(function (sec) {
      if (sec.title) h += "<div class=\"menu__head\">" + esc(sec.title) + (sec.sub ? " <span>— " + esc(sec.sub) + "</span>" : "") + (sec.total > sec.items.length ? " <span>" + sec.items.length + " of " + sec.total + "</span>" : "") + "</div>";
      sec.items.forEach(function (it) {
        var i = flat.push(it) - 1;
        if (it.preset) {
          var on = it.preset[1] === S.base && it.preset[2] === S.compare || (S.mergeBaseOf && S.mergeBaseOf.label === it.l && S.mergeBaseOf.sha === S.base);
          h += "<button class=\"ref-item preset" + (i === pick.pos ? " active" : "") + "\" aria-checked=\"" + !!on + "\" data-i=\"" + i + "\"><span class=\"chk\">" + (on ? "✓" : "") + "</span><span class=\"l\">" + esc(it.l) + "</span><span class=\"d\">" + esc(it.d) + "</span><span class=\"sc k\">" + it.key + "</span></button>";
        } else {
          h += "<button class=\"ref-item" + (i === pick.pos ? " active" : "") + (it.v === current ? " current" : "") + (it.custom ? " custom" : "") + "\" data-i=\"" + i + "\">" +
            (pick.sides ? "<span class=\"chk\"></span>" : "") + "<span class=\"l\">" + esc(it.l) + "</span>" + (it.d ? "<span class=\"d\">" + esc(it.d) + "</span>" : "") + "</button>";
        }
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
    if (it.preset) { applyPreset(it.preset); return; }
    // a new comparison opens on what changed, like one started from the CLI
    var apply = function (v) {
      if (which === "view") { S.browseRef = v; setComparison(null, v, { fit: true }); return; }
      var base = which === "base" ? (v || null) : S.base, compare = which === "base" ? S.compare : v;
      if (which === "compare" && S.pendingBranchReview) {
        // reviewing a colleague's branch: compare it from where it split off main
        S.pendingBranchReview = false;
        var main = ["main", "master"].find(function (m) { return S.refs.branches.indexOf(m) >= 0; }) || "main";
        api("api/git/merge-base?a=" + encodeURIComponent(main) + "&b=" + encodeURIComponent(v)).then(function (r) {
          setComparison(r.sha, v, { lens: { kind: "diff", label: "Changed tables" } });
        }, function () { setComparison(main, v, { lens: { kind: "diff", label: "Changed tables" } }); });
        return;
      }
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
  function bindRefPicker() {
    document.addEventListener("click", function (e) { if (pick && !e.target.closest("#ref-pop")) closeRefPicker(); });
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
    hintUsed("ctx");
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
      items.push(["Show with its neighbours", function () { focusOn(id, false, null); }, S.design ? "" : "dbl-click"]);
      if (S.cfg.focus.length && !fp) items.push(["Add to filter", function () { focusOn(id, true, null); }]);
      items.push(["Hide table", function () { hideTable(id); }]);
      // custom groups: hand-pick this table into one
      items.push(["-"]);
      var grp = groupOf(id), groups = S.cfg.groups || [];
      if (grp && groupHasExact(grp, id)) items.push(["Remove from group “" + grp.name + "”", function () { removeFromGroup(grp, id); }]);
      else if (grp) items.push(["In group “" + grp.name + "” by pattern", function () { openGroupsPop(null, grp); }]);
      groups.filter(function (g) { return g !== grp; }).slice(0, 6).forEach(function (g) {
        items.push(["Add to group “" + g.name + "”", function () { addToGroup(g, id); }, "", g.color || colorFor(g.name)]);
      });
      items.push(["New group with this table…", function () { newGroup(null, id); }]);
      if (groups.length) items.push(["Manage groups…", function () { openGroupsPop(); }]);
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
      items.push([(S.cfg.groups || []).length ? "Manage groups…" : "New group…", function () { if ((S.cfg.groups || []).length) openGroupsPop(); else newGroup(null, null); }]);
      if (userFiltersActive()) items.push(["Show all tables (clear filters)", function () { clearFilters(); }]);
      if (Object.keys(positionStore(false)).length) items.push(["Reset dragged positions", function () { resetPositions(); }]);
    }
    m.innerHTML = items.map(function (it, i) {
      if (it[0] === "-") return "<hr class=\"menu__sep\">";
      if (it[0] === "title") return "<div class=\"menu__head\">" + esc(it[1]) + "</div>";
      var danger = /^(Drop table|Remove table)/.test(it[0]);
      return "<button class=\"menu__item" + (danger ? " menu__item--danger" : "") + "\" data-i=\"" + i + "\">" + (it[3] ? "<span class=\"swatch\" style=\"background:" + esc(it[3]) + "\"></span>" : "") + "<span>" + esc(it[0]) + "</span>" + (it[2] ? "<span class=\"sc\">" + esc(it[2]) + "</span>" : "") + "</button>";
    }).join("");
    m.hidden = false;
    var x = Math.min(e.clientX, innerWidth - m.offsetWidth - 8), y = Math.min(e.clientY, innerHeight - m.offsetHeight - 8);
    m.style.left = x + "px";
    m.style.top = y + "px";
    $$("button", m).forEach(function (b) {
      b.onclick = function () { m.hidden = true; items[Number(b.getAttribute("data-i"))][1](); };
    });
  }

  // ---- custom groups: create a group, hand-pick its tables, keep it in .schema.json ----
  // Groups live in cfg.groups as {name, tables: [names or patterns], color?}; the
  // layout draws them with group_by = "custom" (same data the CLI and config use).
  var PALETTE = ["#4f6bed", "#1a9b5b", "#d9822b", "#c2418c", "#7c4dde", "#0e8fa8", "#c9423a", "#6d8f1f", "#b58a00", "#2f7f76"];
  /** Same FNV-1a hash the renderer uses, so the editor shows the colour the diagram draws. */
  function colorFor(key) {
    var h = 2166136261;
    for (var i = 0; i < key.length; i++) { h ^= key.charCodeAt(i); h = Math.imul(h, 16777619) >>> 0; }
    return PALETTE[h % PALETTE.length];
  }
  function groupOf(id) {
    return (S.cfg.groups || []).find(function (g) { return (g.tables || []).some(function (p) { return p === id || p === display(id) || fbMatch(p, id); }); }) || null;
  }
  function groupHasExact(g, id) { return g.tables.indexOf(display(id)) >= 0 || g.tables.indexOf(id) >= 0; }
  function groupsChanged(o) {
    if (S.cfg.groups.length && S.cfg.layout.group_by !== "custom" && !(o && o.keepMode)) S.cfg.layout.group_by = "custom";
    if (!S.cfg.groups.length && S.cfg.layout.group_by === "custom") S.cfg.layout.group_by = "none";
    syncControls();
    render(o && o.preserve ? { preserve: true } : { fit: true });
    if (!$("#groups-pop").hidden && !(o && o.noPop)) renderGroupsPop();
  }
  function addToGroup(g, id) {
    var p = display(id);
    (S.cfg.groups || []).forEach(function (o) { if (o !== g) o.tables = o.tables.filter(function (x) { return x !== p && x !== id; }); });
    if (g.tables.indexOf(p) < 0) g.tables.push(p);
    groupsChanged();
    toast(display(id) + " → group “" + g.name + "”", 1600);
  }
  function removeFromGroup(g, id) {
    g.tables = g.tables.filter(function (p) { return p !== display(id) && p !== id; });
    groupsChanged();
  }
  function newGroup(name, id) {
    S.cfg.groups = S.cfg.groups || [];
    var n = S.cfg.groups.length + 1;
    var g = { name: name || "Group " + n, tables: id ? [display(id)] : [] };
    S.cfg.groups.push(g);
    groupsChanged({ preserve: !id });
    openGroupsPop(null, g, { focusName: true });
  }
  function openGroupsPop(anchor, focusGroup, o) {
    o = o || {};
    var pop = $("#groups-pop");
    $("#display-pop").hidden = true;
    $("#context-menu").hidden = true;
    // opened from a menu click: show on the next tick so that click's bubble
    // (which closes popovers) has already passed
    setTimeout(function () {
      renderGroupsPop();
      pop.hidden = false;
      placePop(pop, anchor || $("#display-btn"), { alignRight: true });
      if (focusGroup) {
        var i = S.cfg.groups.indexOf(focusGroup);
        var el = pop.querySelector(".grp[data-i=\"" + i + "\"]");
        if (el) {
          var f = el.querySelector(o.focusName ? ".grp__name" : ".chip-input");
          if (f) { f.focus(); if (o.focusName) f.select(); }
        }
      }
    }, 0);
  }
  function renderGroupsPop() {
    var pop = $("#groups-pop"), groups = S.cfg.groups || [];
    var drawn = S.cfg.layout.group_by === "custom";
    var h = "<div class=\"popover__head\">Groups <span>" + groups.length + "</span><span class=\"spacer\"></span>" +
      "<label class=\"check\" style=\"margin:0\" title=\"Draw the groups around their tables (Group by: custom groups)\"><input type=\"checkbox\" data-grp-show" + (drawn ? " checked" : "") + "> draw</label></div>" +
      "<div class=\"popover__body\">";
    if (!groups.length) h += "<p class=\"muted\" style=\"margin:0;font-size:12px\">No groups yet. Make one, then hand-pick tables here or with <b>right-click → Add to group</b> on the diagram.</p>";
    groups.forEach(function (g, i) {
      var members = S.tables ? S.tables.filter(function (t) { return t.kind === "table" && (g.tables.indexOf(t.label) >= 0 || g.tables.indexOf(t.id) >= 0 || g.tables.some(function (p) { return fbMatch(p, t.id); })); }).length : null;
      h += "<div class=\"grp\" data-i=\"" + i + "\"><div class=\"grp__head\">" +
        "<input type=\"color\" data-grp-color=\"" + i + "\" value=\"" + esc(g.color || colorFor(g.name)) + "\" title=\"Group colour\">" +
        "<input class=\"input input--sm grp__name\" data-grp-name=\"" + i + "\" value=\"" + esc(g.name) + "\" placeholder=\"Group name\" spellcheck=\"false\">" +
        "<button class=\"icon-btn\" data-grp-del=\"" + i + "\" title=\"Delete group\">×</button></div>" +
        "<div class=\"grp__tables\">" + g.tables.map(function (p) {
          var glob = p.indexOf("*") >= 0 || p.indexOf("?") >= 0;
          return "<span class=\"chip" + (glob ? " chip--rule" : "") + "\" title=\"" + (glob ? "pattern" : "table") + "\"><b>" + esc(p) + "</b><button class=\"chip__x\" data-grp-rm=\"" + i + "\" data-p=\"" + esc(p) + "\" title=\"Remove from group\">×</button></span>";
        }).join("") +
        "<input class=\"chip-input\" list=\"table-names\" data-grp-add=\"" + i + "\" placeholder=\"" + (g.tables.length ? "add table or pattern…" : "table or pattern, e.g. billing.*") + "\" autocomplete=\"off\" spellcheck=\"false\"></div>" +
        (members != null && g.tables.length ? "<span class=\"grp__meta\">" + members + " table" + (members === 1 ? "" : "s") + "</span>" : "") + "</div>";
    });
    h += "</div><div class=\"popover__foot\"><button class=\"btn btn--sm\" data-grp-new>+ New group</button><span class=\"spacer\"></span>" +
      (STATIC ? "<span class=\"muted\" style=\"font-size:11px\">kept in this browser</span>" : "<button class=\"btn btn--sm\" data-grp-save title=\"Write the groups to .schema.json so everyone on the project gets them\">Save to .schema.json</button>") + "</div>";
    pop.innerHTML = h;
  }
  function bindGroups() {
    var pop = $("#groups-pop");
    $("#groups-btn").onclick = function (e) { e.stopPropagation(); openGroupsPop($("#display-btn")); };
    document.addEventListener("click", function (e) { if (!e.target.closest("#groups-pop,#groups-btn")) pop.hidden = true; });
    var nameT;
    pop.addEventListener("input", function (e) {
      var el = e.target;
      if (el.hasAttribute("data-grp-name")) {
        S.cfg.groups[Number(el.getAttribute("data-grp-name"))].name = el.value;
        clearTimeout(nameT);
        nameT = setTimeout(function () { groupsChanged({ preserve: true, noPop: true, keepMode: true }); }, 300);
      }
    });
    pop.addEventListener("change", function (e) {
      var el = e.target;
      if (el.hasAttribute("data-grp-color")) { S.cfg.groups[Number(el.getAttribute("data-grp-color"))].color = el.value; groupsChanged({ preserve: true, noPop: true, keepMode: true }); }
      else if (el.hasAttribute("data-grp-show")) { S.cfg.layout.group_by = el.checked ? "custom" : "none"; syncControls(); render({ fit: true }); }
    });
    pop.addEventListener("keydown", function (e) {
      var el = e.target;
      if (!el.hasAttribute("data-grp-add")) return;
      if (e.key === "Enter") {
        var p = patternFromInput(el.value), i = Number(el.getAttribute("data-grp-add"));
        if (!p) return;
        var g = S.cfg.groups[i];
        if (g.tables.indexOf(p) < 0) g.tables.push(p);
        groupsChanged();
        var again = pop.querySelector("[data-grp-add=\"" + i + "\"]");
        if (again) again.focus();
      } else if (e.key === "Escape") { pop.hidden = true; }
    });
    pop.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (!b) return;
      if (b.hasAttribute("data-grp-rm")) {
        var g = S.cfg.groups[Number(b.getAttribute("data-grp-rm"))], p = b.getAttribute("data-p");
        g.tables = g.tables.filter(function (x) { return x !== p; });
        groupsChanged();
      } else if (b.hasAttribute("data-grp-del")) {
        var i = Number(b.getAttribute("data-grp-del")), gone = S.cfg.groups[i];
        S.cfg.groups.splice(i, 1);
        groupsChanged();
        toast("Deleted group “" + gone.name + "”", 1600);
      } else if (b.hasAttribute("data-grp-new")) {
        newGroup(null, null);
      } else if (b.hasAttribute("data-grp-save")) {
        var pc = clone(S.projectCfg || {});
        pc.default = pc.default || {};
        pc.default.groups = clone(S.cfg.groups);
        pc.default.layout = pc.default.layout || {};
        if (S.cfg.layout.group_by === "custom") pc.default.layout.group_by = "custom";
        api("api/config", { method: "PUT", body: JSON.stringify(pc) }).then(function (r) {
          S.projectCfg = pc;
          toast("Groups saved to " + r.path.split("/").pop());
        }, function (err) { toast("Save failed: " + err.message, 4000); });
      }
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
    var menu = $("#export-menu"), vmenu = $("#views-menu");
    $("#export-btn").onclick = function (e) { e.stopPropagation(); vmenu.hidden = true; menu.hidden = !menu.hidden; };
    $("#views-btn").onclick = function (e) { e.stopPropagation(); menu.hidden = true; vmenu.hidden = !vmenu.hidden; };
    document.addEventListener("click", function (e) {
      if (!e.target.closest(".menu-wrap")) { menu.hidden = true; vmenu.hidden = true; }
      if (!e.target.closest(".context-menu")) $("#context-menu").hidden = true;
    });
    var base = function () { return (S.server ? S.server.name : S.playground.name).split("/").pop().replace(/\.(sql|rb)$/, ""); };
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
    });
    vmenu.addEventListener("click", function (e) {
      var b = e.target.closest("button");
      if (!b) return;
      vmenu.hidden = true;
      if (b.hasAttribute("data-view")) {
        var v = b.getAttribute("data-view");
        var cfg = merge(clone(S.defaults), (S.projectCfg && S.projectCfg.default) || {});
        if (v && S.projectCfg.views && S.projectCfg.views[v]) merge(cfg, S.projectCfg.views[v]);
        S.cfg = cfg;
        S.viewName = v;
        renderViews(v);
        syncControls();
        render({ fit: true });
        toast(v ? "View “" + v + "”" : "Project default view");
        return;
      }
      var what = b.hasAttribute("data-save-view") ? "save-view" : "save-default";
      var name = what === "save-default" ? null : window.prompt("Name for this view (saved to .schema.json)", S.viewName || "");
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
    });
  }
  function renderViews(selected) {
    var btn = $("#views-btn"), menu = $("#views-menu");
    if (STATIC) { btn.hidden = true; $("#export-foot").hidden = true; return; }
    btn.hidden = false;
    if (selected != null) S.viewName = selected;
    var names = Object.keys((S.projectCfg && S.projectCfg.views) || {});
    $("#views-name").textContent = S.viewName || "default";
    var item = function (v, label, desc) {
      return "<button class=\"menu__item\" data-view=\"" + esc(v) + "\" aria-checked=\"" + ((S.viewName || "") === v) + "\"><span>" + esc(label) + "</span>" + (desc ? "<span class=\"menu__desc\">" + esc(desc) + "</span>" : "") + "</button>";
    };
    menu.innerHTML = "<div class=\"menu__head\">Views <span>· .schema.json</span></div>" +
      item("", "default", S.projectCfg && S.projectCfg.default ? "project default" : "built-in") +
      names.map(function (n) { return item(n, n, ""); }).join("") +
      "<hr class=\"menu__sep\"><button class=\"menu__item\" data-save-view><span>Save current as…</span></button><button class=\"menu__item\" data-save-default><span>Make project default</span><span class=\"menu__desc\">what schema opens with</span></button>";
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
      if (e.key === "Escape" && S.editor && !(pick)) { e.preventDefault(); closeEditor(); return; }
      if (tag === "input" || tag === "select" || tag === "textarea" || e.metaKey || e.ctrlKey || e.altKey) {
        if ((e.metaKey || e.ctrlKey) && e.key === "k") { e.preventDefault(); $("#search").focus(); }
        if (S.editor && (e.metaKey || e.ctrlKey) && e.key === "s") { e.preventDefault(); saveEditor(); }
        if (S.design && (e.metaKey || e.ctrlKey) && e.key === "z" && !e.shiftKey && tag !== "input" && tag !== "textarea" && !S.editor) { e.preventDefault(); designUndo(); }
        return;
      }
      var k = e.key;
      if (e.shiftKey && (k === "B" || k === "C" || k === "D")) { e.preventDefault(); setMode({ B: "browse", C: "compare", D: "design" }[k]); return; }
      if (S.design && k === "n") { e.preventDefault(); openTableEditor(null); return; }
      if (k === "/") { e.preventDefault(); $("#search").focus(); }
      else if (k === "f") viewer.fit();
      else if (k === "+" || k === "=") viewer.zoomBy(1.25);
      else if (k === "-") viewer.zoomBy(0.8);
      else if (k === "0") viewer.setZoom(1);
      else if (k >= "1" && k <= "5") { S.cfg.layout.algorithm = ALGS[Number(k) - 1]; syncControls(); render({ fit: true }); hintUsed("k"); }
      else if (k === "c") { toggleChangesLens(!S.lens); hintUsed("c"); }
      else if (k === "[" || k === "]") { lensDepth(k === "]" ? 1 : -1); hintUsed("c"); }
      else if (k === "k") { cycle(COLS, "columns"); hintUsed("k"); }
      else if (k === "e") { cycle(EDGES, "edges.style"); hintUsed("k"); }
      else if (k === "?") $("#help").showModal();
      else if (k === "Escape") {
        // an open menu or popover takes the first Escape
        var open = $$("#context-menu,#display-pop,#fb-pop,#groups-pop,#export-menu,#views-menu,#search-results").filter(function (el) { return !el.hidden; });
        if (open.length) { open.forEach(function (el) { el.hidden = true; }); return; }
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

  // the running build, in the help dialog: "v0.3.0 · 2aa810f" (server) or the WASM bundle's version (static)
  function renderVersion() {
    var el = $("#help-version"), s = S.server;
    if (!el) return;
    var v = s && s.version ? s.version : wb ? wb.version() : "";
    if (!v) return;
    var build = s && s.build ? s.build + (s.build_dirty ? " (local changes)" : "") : "";
    el.textContent = "v" + v + (build ? " · " + build : "");
    el.title = "schema " + v + (build ? " built from " + build : "");
  }

  function renderFileInfo() {
    renderVersion();
    if (STATIC) return;
    var s = S.server;
    $("#file-name").textContent = s.name;
    document.title = s.name.split("/").pop() + " — schema";
    var meta = [];
    if (s.repo_name) meta.push(s.repo_name);
    if (s.branch) meta.push(s.branch);
    if (s.head) meta.push(s.head);
    if (s.dirty) meta.push("uncommitted changes");
    if (!s.is_git) meta.push("not in git");
    $("#file-meta").textContent = meta.join(" · ");
  }

  // ---- playground (no server) ---------------------------------------------------
  function setupPlayground() {
    var fileInput = $("#pg-file");
    fileInput.onchange = function () {
      var f = fileInput.files[0];
      if (!f) return;
      f.text().then(function (t) { loadText(S.pgTarget || "current", t, f.name); });
      fileInput.value = "";
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
    S.mode = S.playground.base ? "compare" : (S.mode === "design" ? "design" : "browse");
    showPanel(S.mode);
    $("#file-name").textContent = S.playground.name;
    $("#file-meta").textContent = S.playground.base ? "compared with " + S.playground.baseName : "playground · files never leave your browser";
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
    S.lens = null;
    syncControls();
    render({ fit: true });
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
  /** More or fewer neighbours around the changed tables ([ and ]). */
  function lensDepth(delta) {
    if (!S.lens) { if (delta > 0) toggleChangesLens(true); return; }
    S.lens.context = Math.max(0, (S.lens.context || 0) + delta);
    applyFilter();
  }
  function lensLabel(L) { return L.kind === "commit" ? L.label : S.design ? "Design changes" : "Changes"; }

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

  // The canvas bar: "Show" + the lens (Compare / Design) + filter chips + input.
  function renderFilterBar() {
    var bar = $("#filter-bar");
    if (!bar || !S.cfg) return;
    var st = (S.result && S.result.stats) || {};
    var parts = ["<span class=\"canvasbar__label\">Show</span>"];
    var canLens = S.mode !== "browse" && !!(S.diff && (S.base || S.design) && (S.diff.tables.length || (S.diff.enums || []).length));
    var paused = !!(S.lens && !S.lens.combine);
    var x = function (attr, title) { return "<button class=\"chip__x\" " + attr + " title=\"" + (title || "Remove") + "\">×</button>"; };
    var rule = function (label, value, attr, title) {
      return "<span class=\"chip chip--rule\" title=\"" + esc(title || "") + "\"><i>" + label + "</i>" + value + x(attr) + "</span>";
    };
    if (canLens) {
      var on = !!S.lens, L = S.lens || {}, c = on ? L.context || 0 : 0;
      var n = S.diff.tables.length + (S.diff.enums || []).length;
      var total = (st.tables_total || 0) + (S.cfg.show_views ? st.views_total || 0 : 0);
      parts.push("<div class=\"seg lens\">" +
        "<button data-lens-on aria-pressed=\"" + on + "\" title=\"Only the tables this comparison changed\">" + esc(on ? lensLabel(L) : S.design ? "Design changes" : "Changes") + " <span class=\"n\">" + n + "</span>" +
        (on ? "<span class=\"stepper\" title=\"Neighbours of the changed tables ([ and ])\"><span role=\"button\" data-lens-dec" + (c ? "" : " aria-disabled=\"true\"") + ">−</span><output>" + (c ? "+" + c : "0") + "</output><span role=\"button\" data-lens-inc>+</span></span>" : "") +
        "</button><button data-lens-off aria-pressed=\"" + !on + "\" title=\"Every table, with the changes highlighted (c)\">All tables <span class=\"n\">" + total + "</span></button></div>");
      if (on) parts.push("<span class=\"lens-hint\">" + (c ? "changed tables + " + c + " hop" + (c > 1 ? "s" : "") : "only changed tables") + "</span>");
      parts.push("<span class=\"vsep\"></span>");
    }
    var chips = 0;
    if (paused && userFiltersActive()) {
      // the user's own filters wait outside the lens; clicking one applies it too
      parts.push("<span class=\"paused-lbl\" title=\"Your filters are kept; click one to narrow the changes with it\">paused:</span>");
      S.cfg.focus.forEach(function (p) { parts.push("<button class=\"chip chip--paused\" data-resume title=\"Apply this filter inside the changes too\">" + esc(display(p)) + "</button>"); chips++; });
      S.cfg.include.forEach(function (p) { parts.push("<button class=\"chip chip--paused\" data-resume>only " + esc(p) + "</button>"); chips++; });
      manualExcludes().slice(0, 2).forEach(function (p) { parts.push("<button class=\"chip chip--paused\" data-resume>hide " + esc(display(p)) + "</button>"); chips++; });
      if (S.cfg.schemas.length) { parts.push("<button class=\"chip chip--paused\" data-resume>schemas " + esc(S.cfg.schemas.join(", ")) + "</button>"); chips++; }
    } else {
      S.cfg.focus.forEach(function (p) {
        var d = fdepth(p), m = (st.focus_matches || {})[p];
        var multi = m != null && (p.indexOf("*") >= 0 || p.indexOf("?") >= 0);
        parts.push("<span class=\"chip\" title=\"" + esc("Showing " + display(p) + (d ? " and tables up to " + d + " relation" + (d > 1 ? "s" : "") + " away" : " only")) + "\">" +
          "<button class=\"chip__lbl\" data-center=\"" + esc(p) + "\" title=\"Show on the diagram\">" + esc(display(p)) + "</button>" +
          (multi ? "<span class=\"chip__n\">" + m + "</span>" : m === 0 ? "<span class=\"chip__n warn\" title=\"matches no table\">0</span>" : "") +
          "<span class=\"chip__depth\"><button data-dec=\"" + esc(p) + "\" title=\"Fewer neighbours\"" + (d ? "" : " disabled") + ">−</button>" +
          "<span title=\"Neighbours: tables up to this many relations away" + (S.cfg.focus_depths && S.cfg.focus_depths[p] != null ? "" : " (default depth, set in Options)") + "\">" + (d ? d + " hop" + (d > 1 ? "s" : "") : "only") + "</span>" +
          "<button data-inc=\"" + esc(p) + "\" title=\"More neighbours\">+</button></span>" + x("data-rm-focus=\"" + esc(p) + "\"") + "</span>");
        chips++;
      });
      if (S.cfg.focus.length && S.cfg.focus_direction !== "both") { parts.push(rule("neighbours", S.cfg.focus_direction === "outgoing" ? "referenced only" : "referencing only", "data-rm=\"direction\"")); chips++; }
      S.cfg.include.forEach(function (p, i) { parts.push(rule("only", esc(p), "data-rm-inc=\"" + i + "\"", "Only tables matching " + p)); chips++; });
      var ex = manualExcludes();
      var shown = S.fbAllHidden ? ex : ex.slice(0, 3);
      shown.forEach(function (p) { parts.push(rule("hide", esc(display(p)), "data-rm-exc=\"" + esc(p) + "\"", "Hidden: " + p)); chips++; });
      if (ex.length > shown.length) parts.push("<button class=\"chip-more\" data-all-hidden>+" + (ex.length - shown.length) + " hidden</button>");
      if (S.cfg.schemas.length) { parts.push(rule("schemas", esc(S.cfg.schemas.join(", ")), "data-rm=\"schemas\"")); chips++; }
      if (!S.cfg.show_isolated) { parts.push(rule("", "no isolated tables", "data-rm=\"isolated\"")); chips++; }
      if (S.cfg.hide_columns.length) { parts.push(rule("hide cols", esc(S.cfg.hide_columns.join(", ")), "data-rm=\"hidecols\"", "Columns hidden in every table (Display ▾ → Columns)")); chips++; }
    }
    var placeholder = S.lens ? "narrow further…" : chips ? "add table or pattern…" : "table or pattern, e.g. card*";
    bar.innerHTML = parts.join("") +
      "<input id=\"fb-input\" class=\"chip-input\" list=\"table-names\" autocomplete=\"off\" spellcheck=\"false\" placeholder=\"" + placeholder + "\">";
    $("#fb-clear").hidden = !(userFiltersActive() && !paused);
    bar.scrollLeft = 0;
    updateFbExpand();
  }

  /** Many chips: offer to expand the bar so they wrap instead of being clipped. */
  function updateFbExpand() {
    var bar = $("#filter-bar"), btn = $("#fb-expand"), cb = $("#canvasbar");
    cb.classList.toggle("expanded", !!S.fbExpanded);
    if (S.fbExpanded) { btn.hidden = false; btn.textContent = "less ▴"; return; }
    var right = bar.getBoundingClientRect().right;
    var hiddenChips = $$(".chip, .seg, .chip-more", bar).filter(function (el) { return el.getBoundingClientRect().right > right + 1; }).length;
    var over = bar.scrollWidth > bar.clientWidth + 1;
    btn.hidden = !over;
    btn.textContent = hiddenChips ? hiddenChips + " more ▾" : "more ▾";
  }

  /** A chip typed while a lens is on narrows the lens instead of leaving it. */
  function addFocusChip(p) {
    if (S.lens && !S.lens.combine) S.lens.combine = true;
    setFocus(p, null, true);
    hintUsed("filter");
    applyFilter();
    // keep the bar anchored at its first chip; a clipped tail is what "N more" is for
    setTimeout(function () { var i = $("#fb-input"); if (i) i.focus({ preventScroll: true }); }, 30);
  }

  function bindFilterBar() {
    var bar = $("#canvasbar");
    bar.addEventListener("keydown", function (e) {
      if (e.target.id !== "fb-input") return;
      if (e.key === "Enter") {
        var p = patternFromInput(e.target.value);
        if (p) addFocusChip(p);
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
      if (S.index.some(function (t) { return t.label === v; })) addFocusChip(v);
    });
    bar.addEventListener("click", function (e) {
      var step = e.target.closest("[data-lens-dec],[data-lens-inc]");
      if (step) {
        e.stopPropagation();
        if (step.hasAttribute("data-lens-inc")) lensDepth(1);
        else if (step.getAttribute("aria-disabled") !== "true") lensDepth(-1);
        return;
      }
      var b = e.target.closest("button");
      if (!b) return;
      var a = function (n) { return b.getAttribute(n); };
      if (b.hasAttribute("data-inc")) { S.cfg.focus_depths[a("data-inc")] = fdepth(a("data-inc")) + 1; applyFilter(); }
      else if (b.hasAttribute("data-dec")) { S.cfg.focus_depths[a("data-dec")] = Math.max(0, fdepth(a("data-dec")) - 1); applyFilter(); }
      else if (b.hasAttribute("data-rm-focus")) { removeFocus(a("data-rm-focus")); applyFilter(); }
      else if (b.hasAttribute("data-rm-inc")) { S.cfg.include.splice(Number(a("data-rm-inc")), 1); applyFilter(); }
      else if (b.hasAttribute("data-rm-exc")) { S.cfg.exclude = S.cfg.exclude.filter(function (x) { return x !== a("data-rm-exc"); }); applyFilter({ preserve: true }); }
      else if (b.hasAttribute("data-all-hidden")) { S.fbAllHidden = true; renderFilterBar(); }
      else if (b.hasAttribute("data-resume")) { S.lens.combine = true; applyFilter(); }
      else if (b.hasAttribute("data-center")) {
        var p = a("data-center");
        var hit = S.result.nodes.find(function (n) { return n.id === qid(p) || n.label === p; }) ||
          S.result.nodes.find(function (n) { return fbMatch(p, n.id); });
        if (hit) selectTable(hit.id, { center: true });
      } else if (a("data-rm") === "direction") { S.cfg.focus_direction = "both"; applyFilter(); }
      else if (a("data-rm") === "schemas") { S.cfg.schemas = []; applyFilter(); }
      else if (a("data-rm") === "isolated") { S.cfg.show_isolated = true; applyFilter(); }
      else if (a("data-rm") === "hidecols") { S.cfg.hide_columns = []; syncControls(); render({ preserve: true }); }
      else if (b.hasAttribute("data-lens-on")) { if (!S.lens) toggleChangesLens(true); }
      else if (b.hasAttribute("data-lens-off")) { if (S.lens) exitLens(); }
      else if (b.id === "fb-clear") { clearFilters(); }
      else if (b.id === "fb-options") { togglePop("options", b); }
      else if (b.id === "fb-why") { togglePop("why", b); }
      else if (b.id === "fb-expand") { S.fbExpanded = !S.fbExpanded; updateFbExpand(); }
    });
    if (window.ResizeObserver) new ResizeObserver(function () { if (S.cfg) updateFbExpand(); }).observe($("#filter-bar"));
    document.addEventListener("click", function (e) {
      if (!e.target.closest("#fb-pop,#fb-options,#fb-why")) $("#fb-pop").hidden = true;
    });
    $("#fb-pop").addEventListener("click", function (e) {
      var b = e.target.closest("[data-unhide],[data-why]");
      if (!b) return;
      if (b.hasAttribute("data-unhide")) { S.cfg.exclude = S.cfg.exclude.filter(function (x) { return x !== b.getAttribute("data-unhide"); }); applyFilter({ preserve: true }); renderPop(); return; }
      var w = b.getAttribute("data-why");
      if (w === "hop") { S.cfg.focus.forEach(function (p) { S.cfg.focus_depths[p] = fdepth(p) + 1; }); applyFilter(); renderPop(); }
      else if (w === "lens-hop") { lensDepth(1); renderPop(); }
      else if (w === "views") { S.cfg.show_views = true; syncControls(); render({ preserve: true }); renderPop(); }
      else if (w === "all") { $("#fb-pop").hidden = true; if (S.lens) S.lens = null; clearFilters(); }
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
    placePop(pop, anchor, { alignRight: kind === "why" });
  }

  function renderPop() {
    var pop = $("#fb-pop"), st = (S.result && S.result.stats) || {}, h = "";
    if (S.popKind === "why") {
      var hd = st.hidden || {};
      var nt = (st.nodes_visible || 0) - (st.enums_visible || 0), total = (st.tables_total || 0) + (S.cfg.show_views ? st.views_total || 0 : 0);
      var act = function (w, label) { return "<button class=\"btn btn--ghost btn--sm\" data-why=\"" + w + "\">" + label + "</button>"; };
      var rows = [
        [hd.outside_focus, "beyond " + (S.cfg.focus.length === 1 ? fdepth(S.cfg.focus[0]) + " hop" + (fdepth(S.cfg.focus[0]) === 1 ? "" : "s") + " of <code>" + esc(display(S.cfg.focus[0])) + "</code>" : "the filtered tables' neighbours"), S.cfg.focus.length ? act("hop", "+1 hop") : ""],
        [hd.unchanged, "unchanged, not part of " + (S.lens ? esc(lensLabel(S.lens).toLowerCase()) : "the changes"), S.lens ? act("lens-hop", "+1 hop") : ""],
        [hd.exclude, "hidden by name" + (S.defaults.exclude.length ? " (incl. " + S.defaults.exclude.length + " bookkeeping tables by default)" : ""), ""],
        [hd.include, "don't match the “only” patterns", ""],
        [hd.schema, "in other schemas", ""],
        [hd.isolated, "without relations", ""],
        [hd.partitions, "partitions folded into their parent table", ""],
      ].filter(function (r) { return r[0]; });
      if (!S.cfg.show_views && st.views_total) rows.push([st.views_total, "views", act("views", "show")]);
      h = "<div class=\"popover__head\">" + (nt < total ? nt + " of " + total : total) + " tables shown<span>· " + (st.edges_visible || 0) + " relation" + (st.edges_visible === 1 ? "" : "s") + "</span></div><div class=\"popover__body\">" +
        (rows.length ? "<ul class=\"why\">" + rows.map(function (r) { return "<li><b>" + r[0] + "</b><span>" + r[1] + "</span>" + r[2] + "</li>"; }).join("") + "</ul>" : "<p class=\"muted\">Nothing is hidden.</p>");
      var ex = manualExcludes();
      if (ex.length) {
        h += "<h4>Hidden tables</h4><ul class=\"why\">" + ex.map(function (x) { return "<li><span><code>" + esc(display(x)) + "</code></span><button class=\"btn btn--ghost btn--sm\" data-unhide=\"" + esc(x) + "\">show</button></li>"; }).join("") + "</ul>";
      }
      h += "</div>" + (filterActive() ? "<div class=\"popover__foot\"><button class=\"btn btn--ghost btn--sm\" data-why=\"all\" style=\"color:var(--fg-muted);padding:0\">Show everything</button><span class=\"spacer\"></span><kbd>esc</kbd></div>" : "");
    } else {
      h = "<div class=\"popover__body\"><h4>Neighbours of filtered tables</h4>" +
        "<label class=\"row\" title=\"How many relations away to show, for tables in the filter without their own depth (use − / + on a chip to override)\"><span>Depth</span><select data-pop=\"focus_depth\">" +
        [0, 1, 2, 3, 4, 5].map(function (n) { return "<option value=\"" + n + "\"" + (S.cfg.focus_depth === n ? " selected" : "") + ">" + (n ? n + " hop" + (n > 1 ? "s" : "") : "none") + "</option>"; }).join("") + "</select></label>" +
        "<label class=\"row\"><span>Direction</span><select data-pop=\"focus_direction\"><option value=\"both\">both ways</option><option value=\"outgoing\">tables they reference</option><option value=\"incoming\">tables referencing them</option></select></label>" +
        "<h4>Patterns</h4>" +
        "<label class=\"row col\"><span>Only tables matching</span><input type=\"text\" data-pop=\"include\" placeholder=\"billing.*, user*\" value=\"" + esc(S.cfg.include.join(", ")) + "\"></label>" +
        "<label class=\"row col\"><span>Hide tables matching</span><input type=\"text\" data-pop=\"exclude\" placeholder=\"audit_*\" value=\"" + esc(S.cfg.exclude.join(", ")) + "\"></label>" +
        ((S.schemaNames || []).length > 1 ? "<h4>Schemas</h4><div class=\"schema-checks\">" + S.schemaNames.map(function (s) {
          return "<label class=\"check\"><input type=\"checkbox\" data-pop-schema=\"" + esc(s) + "\"" + (!S.cfg.schemas.length || S.cfg.schemas.indexOf(s) >= 0 ? " checked" : "") + "> " + esc(s) + "</label>";
        }).join("") + "</div>" : "") +
        "<h4>More</h4>" +
        "<label class=\"check\"><input type=\"checkbox\" data-pop=\"hide_isolated\"" + (S.cfg.show_isolated ? "" : " checked") + "> Hide tables without relations</label></div>";
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
    if (S.mode !== "design") { S.prevMode = S.mode; S.mode = "design"; }
    showPanel("design");
    updateCompareUI();
    saveState();
  }

  function newDesign(name) {
    startDesign({
      version: 1, name: name, description: "",
      source: { file: S.server ? (S.server.rel || S.server.name) : S.playground.name, ref: STATIC ? null : S.compare, commit: S.server ? S.server.head : null },
      ops: [], notes: {}, positions: {}, created: new Date().toISOString(),
    }, { dirty: true });
  }

  /** Close the design. Unsaved work asks first: discard, keep the browser
   *  draft (it resumes on the next launch), or save. Resolves true when closed. */
  function closeDesign(leaving) {
    var finish = function (keepDraft) {
      if (S.editor) closeEditor();
      var draft = keepDraft ? localStorage.getItem(designKey()) : null;
      S.design = null;
      S.designSlug = null;
      viz.clear_design();
      saveDesignDraft();
      if (draft) { try { localStorage.setItem(designKey(), draft); } catch (e) { /* ignore */ } }
      buildIndex();
      S.diff = JSON.parse(viz.diff());
      renderChanges();
      renderDesignPanel();
      updateCompareUI();
      render({ preserve: true });
      if (!leaving) setMode(S.prevMode || "browse", { force: true });
      return true;
    };
    if (!S.designDirty) return Promise.resolve(finish(false));
    var n = S.design.ops.length;
    return confirmDialog({
      title: "Close “" + (S.design.name || "design") + "”?",
      text: (n ? n + " operation" + (n === 1 ? " is" : "s are") : "The design is") + " not saved to " + (STATIC ? "this browser" : "the repo") + ". A draft stays in this browser until you start another design.",
      buttons: [["Discard", "discard", "btn--ghost btn--danger"], ["Keep draft", "keep", ""], [STATIC ? "Save in browser" : "Save to repo", "save", "btn--primary"]],
    }).then(function (choice) {
      if (!choice) return false;
      if (choice === "save") return saveDesign().then(function () { return finish(false); }, function (e) { toast("Save failed: " + e.message, 4000); return false; });
      return finish(choice === "keep");
    });
  }

  /** A small confirm dialog; resolves with the chosen button's value, or null. */
  function confirmDialog(o) {
    var dlg = $("#confirm");
    dlg.innerHTML = "<div class=\"dialog__body\"><h3>" + esc(o.title) + "</h3><p>" + esc(o.text) + "</p></div>" +
      "<div class=\"dialog__foot\">" + o.buttons.map(function (b) { return "<button class=\"btn " + b[2] + "\" data-choice=\"" + b[1] + "\">" + esc(b[0]) + "</button>"; }).join("") + "</div>";
    return new Promise(function (resolve) {
      var done = false;
      $$("[data-choice]", dlg).forEach(function (b) { b.onclick = function () { done = true; dlg.close(); resolve(b.getAttribute("data-choice")); }; });
      dlg.onclose = function () { if (!done) resolve(null); };
      dlg.showModal();
      var primary = dlg.querySelector(".btn--primary") || dlg.querySelector("[data-choice]");
      if (primary) primary.focus();
    });
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
    // the editor docks into the right panel, where the details were
    $("#details").hidden = true;
    $("#table-editor").hidden = false;
    var f = $("#table-editor [data-focus]") || $("#table-editor [data-f=name]");
    if (f) f.focus();
  }
  function closeEditor() {
    S.editor = null;
    $("#table-editor").hidden = true;
    if (S.selected) renderDetails(S.selected);
  }

  function drawEditor() {
    var ed = S.editor, st = ed.st, box = $("#table-editor");
    var enumTypes = (S.enums || []).map(display);
    var cell = function (sec, i, f, val, attrs, isNew) {
      return "<input class=\"input" + (isNew ? " te-new" : "") + "\" data-sec=\"" + sec + "\" data-i=\"" + i + "\" data-f=\"" + f + "\" value=\"" + esc(val) + "\" " + (attrs || "") + ">";
    };
    var check = function (sec, i, f, on, title) {
      return "<input type=\"checkbox\" class=\"c\" data-sec=\"" + sec + "\" data-i=\"" + i + "\" data-f=\"" + f + "\"" + (on ? " checked" : "") + " title=\"" + title + "\">";
    };
    var del = function (sec, i) { return "<button type=\"button\" class=\"te-del\" data-del=\"" + sec + "\" data-i=\"" + i + "\" title=\"Remove\">×</button>"; };
    var section = function (title, n, add, label) { return "<div class=\"te-section\"><h3 class=\"sh\">" + title + (n ? " <span>" + n + "</span>" : "") + "</h3><button type=\"button\" class=\"btn btn--ghost btn--sm\" data-add=\"" + add + "\">+ " + label + "</button></div>"; };
    var h = "<div class=\"details__head\"><div class=\"te-head-row\"><span class=\"lbl\">" + (ed.id ? "Edit table" : "New table") + "</span>" +
      (ed.created ? "<span class=\"badge badge--add\">new</span>" : "<span class=\"badge badge--mod\">changed</span>") +
      "<span class=\"hint\">" + (ed.id && !ed.created ? "edits are recorded as operations" : "applies to the diagram on Apply") + "</span>" +
      "<button type=\"button\" class=\"icon-btn close\" data-act=\"cancel\" title=\"Close (Esc)\">×</button></div>" +
      "<div class=\"te-top\"><label class=\"field\"><span>Table name</span><input class=\"input\" data-f=\"name\" value=\"" + esc(st.name) + "\" placeholder=\"cards or billing.cards\" spellcheck=\"false\"></label>" +
      "<label class=\"field\"><span>Comment</span><input class=\"input comment\" data-f=\"comment\" value=\"" + esc(st.comment) + "\" placeholder=\"What is stored here\"></label></div></div>" +
      "<div class=\"details__body\">" +
      section("Columns", st.columns.length, "columns", "Column") +
      "<div class=\"te-grid\"><span class=\"h\">Name</span><span class=\"h\">Type</span><span class=\"h c\" title=\"Nullable\">Null</span><span class=\"h\">Default</span><span class=\"h c\" title=\"Primary key\">PK</span><span></span>" +
      st.columns.map(function (c, i) {
        return cell("columns", i, "name", c.name, "spellcheck=\"false\" placeholder=\"column_name\"" + (c.focus ? " data-focus" : ""), !c.orig) +
          cell("columns", i, "type", c.type, "list=\"te-types\" spellcheck=\"false\"") +
          check("columns", i, "nullable", c.nullable, "Nullable") +
          cell("columns", i, "default", c.default, "spellcheck=\"false\" placeholder=\"—\"") +
          check("columns", i, "pk", c.pk, "Primary key") + del("columns", i);
      }).join("") + "</div>" +
      section("Foreign keys", st.fks.length, "fks", "Foreign key") +
      (st.fks.length ? "<div class=\"te-grid te-grid--fk\"><span class=\"h\">Column(s)</span><span class=\"h\">References</span><span class=\"h\">Column(s)</span><span class=\"h\">On delete</span><span></span>" +
      st.fks.map(function (f, i) {
        return cell("fks", i, "columns", f.columns, "list=\"te-cols\" spellcheck=\"false\" placeholder=\"user_id\"", !f.orig) +
          cell("fks", i, "ref", f.ref, "list=\"table-names\" spellcheck=\"false\" placeholder=\"users\"") +
          cell("fks", i, "ref_columns", f.ref_columns, "spellcheck=\"false\" placeholder=\"id\"") +
          "<select class=\"input\" data-sec=\"fks\" data-i=\"" + i + "\" data-f=\"on_delete\">" + ON_DELETE.map(function (x) { return "<option value=\"" + x + "\"" + (x === (f.on_delete || "") ? " selected" : "") + ">" + (x ? x.toLowerCase() : "—") + "</option>"; }).join("") + "</select>" +
          del("fks", i);
      }).join("") + "</div>" : "") +
      section("Indexes", st.indexes.length, "indexes", "Index") +
      (st.indexes.length ? "<div class=\"te-grid te-grid--ix\"><span class=\"h\">Column(s)</span><span class=\"h c\">Unique</span><span class=\"h\">Where (partial)</span><span></span>" +
      st.indexes.map(function (x, i) {
        return cell("indexes", i, "columns", x.columns, "list=\"te-cols\" spellcheck=\"false\" placeholder=\"account_id, created_at\"", !x.orig) +
          check("indexes", i, "unique", x.unique, "Unique") +
          cell("indexes", i, "where", x.where, "spellcheck=\"false\" placeholder=\"deleted_at IS NULL\"") + del("indexes", i);
      }).join("") + "</div>" : "") +
      "<label class=\"field te-note\"><span>Note for the implementer</span><textarea class=\"input\" data-f=\"note\" rows=\"2\" placeholder=\"Intent, constraints, backfill or data-migration hints\">" + esc(st.note) + "</textarea></label>" +
      "<div class=\"te-err\"" + (ed.error ? "" : " hidden") + ">" + esc(ed.error) + "</div></div>" +
      "<div class=\"details__foot\">" + (ed.id ? "<button type=\"button\" class=\"btn btn--ghost btn--danger\" data-act=\"drop\">" + (ed.created ? "Remove table" : "Drop table") + "</button>" : "") +
      "<span class=\"spacer\"></span><button type=\"button\" class=\"btn\" data-act=\"cancel\">Cancel <span class=\"sc\">esc</span></button><button type=\"button\" class=\"btn btn--primary\" data-act=\"save\">" + (ed.id ? "Apply" : "Create table") + " <span class=\"sc\">⌘S</span></button></div>" +
      "<datalist id=\"te-types\">" + TYPES.concat(enumTypes).map(function (x) { return "<option value=\"" + esc(x) + "\">"; }).join("") + "</datalist>" +
      "<datalist id=\"te-cols\">" + st.columns.filter(function (c) { return c.name; }).map(function (c) { return "<option value=\"" + esc(c.name) + "\">"; }).join("") + "</datalist>";
    box.innerHTML = h;
  }

  function bindEditor() {
    var dlg = $("#table-editor");
    dlg.addEventListener("input", function (e) {
      var el = e.target, f = el.getAttribute("data-f");
      if (!f || !S.editor) return;
      var st = S.editor.st, sec = el.getAttribute("data-sec");
      var target = sec ? st[sec][Number(el.getAttribute("data-i"))] : st;
      if (el.type === "checkbox") {
        target[f] = el.checked;
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
      } else if (b.getAttribute("data-act") === "cancel") {
        closeEditor();
      } else if (b.getAttribute("data-act") === "drop") {
        var id = S.editor.id;
        closeEditor();
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
    if (!r.ops.length && !noteChanged) { closeEditor(); toast("No changes", 1200); return; }
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
    closeEditor();
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
  /** Delete a saved design after confirming. Resolves true when it is gone. */
  function deleteDesign(slug, name) {
    var open = S.design && S.designSlug === slug;
    return confirmDialog({
      title: "Delete “" + (name || slug) + "”?",
      text: (STATIC ? "Removes the design from this browser." : "Removes .schema/designs/" + slug + ".json and " + slug + ".md from the repo.") +
        (open && S.designDirty ? " Unsaved changes are discarded too." : "") + " This cannot be undone.",
      buttons: [["Cancel", "cancel", ""], ["Delete", "delete", "btn--danger btn--solid"]],
    }).then(function (choice) {
      if (choice !== "delete") return false;
      var p;
      if (STATIC) {
        var all = {};
        try { all = JSON.parse(localStorage.getItem("schema:designs:playground") || "{}"); } catch (e) { /* ignore */ }
        delete all[slug];
        localStorage.setItem("schema:designs:playground", JSON.stringify(all));
        p = Promise.resolve();
      } else p = api("api/designs/" + encodeURIComponent(slug), { method: "DELETE" });
      return p.then(function () { toast("Deleted design “" + (name || slug) + "”"); return true; },
        function (e) { toast("Delete failed: " + e.message, 4000); return false; });
    });
  }

  /** One operation as a row: sign, what, and the table it touches. */
  function opRow(op, label) {
    var kind = op.op || "", sign = /^(create|add)_/.test(kind) || kind === "set_primary_key" ? "added" : /^drop_/.test(kind) ? "removed" : "modified";
    var text = String(label || kind).replace(/`/g, "");
    var t = display(qid(op.table));
    if (kind === "create_table") text = "create table " + t;
    else if (kind === "drop_table") text = "drop table " + t;
    else if (kind === "rename_table") text = "rename table " + t + " → " + op.to;
    else text = t + ": " + text;
    return { sign: sign, text: text };
  }

  function renderDesignPanel() {
    var box = $("#design-panel"), foot = $("#design-foot");
    if (!box) return;
    $("#design-count").textContent = S.design ? String(S.design.ops.length) : "";
    if (!S.design) {
      foot.hidden = true;
      box.innerHTML = "<h3 class=\"sh\">Saved designs</h3><ul class=\"design-list\" id=\"design-list\"><li class=\"list-empty\">loading…</li></ul>";
      listDesigns().then(function (list) {
        var ul = $("#design-list");
        if (!ul) return;
        if (!list.length) { ul.innerHTML = "<li class=\"list-empty\">none yet</li>"; return; }
        ul.innerHTML = list.map(function (d) {
          return "<li data-slug=\"" + esc(d.slug) + "\" title=\"Open this design\"><span class=\"name\">" + esc(d.name || d.slug) + "</span><span class=\"meta\">" + d.ops + " op" + (d.ops === 1 ? "" : "s") + (d.updated ? " · " + ago(d.updated) : "") + "</span>" +
            "<button class=\"x\" data-del-design=\"" + esc(d.slug) + "\" data-name=\"" + esc(d.name || d.slug) + "\" title=\"Delete this design\">×</button></li>";
        }).join("");
        $$("li[data-slug]", ul).forEach(function (li) {
          li.onclick = function (e) {
            var slug = li.getAttribute("data-slug");
            var del = e.target.closest("[data-del-design]");
            if (del) { deleteDesign(slug, del.getAttribute("data-name")).then(function (ok) { if (ok) renderDesignPanel(); }); return; }
            loadDesign(slug).then(function (d) { startDesign(d, { slug: slug }); toast("Opened design " + (d.name || slug)); }, function (e) { toast("Could not open: " + e.message); });
          };
        });
      }, function () { var ul = $("#design-list"); if (ul) ul.innerHTML = "<li class=\"list-empty\">could not list designs</li>"; });
      return;
    }
    var d = S.design, st = S.designState || { ops: [], errors: [] };
    var errs = {};
    (st.errors || []).forEach(function (e) { errs[e.op] = (errs[e.op] ? errs[e.op] + "; " : "") + e.message; });
    box.innerHTML =
      "<div class=\"toolbar\" style=\"margin:-8px -6px 8px\"><button class=\"btn btn--sm\" id=\"design-new-table\" title=\"Or right-click the canvas\">+ Table <span class=\"sc\">N</span></button>" +
      "<button class=\"btn btn--sm\" id=\"design-undo\"" + (S.designUndo.length ? "" : " disabled") + ">Undo <span class=\"sc\">⌘Z</span></button>" +
      "<button class=\"btn btn--sm\" id=\"design-relayout\" title=\"Lay the diagram out again\">Re-layout</button></div>" +
      "<h3 class=\"sh\">Operations <span>" + d.ops.length + "</span>" + (d.ops.length ? "<em>in order applied</em>" : "") + "</h3>" +
      (d.ops.length ? "<ol class=\"op-list\" style=\"list-style:none;margin:0;padding:0\">" + d.ops.map(function (op, i) {
        var r = opRow(op, (st.ops || [])[i]);
        return "<li class=\"orow\"" + (errs[i] ? " data-status=\"err\"" : "") + "><span class=\"orow__i\">" + (i + 1) + "</span><span class=\"sign " + SIGN_CLS[r.sign] + "\">" + SIGN[r.sign] + "</span>" +
          "<div><code>" + esc(r.text) + "</code>" + (errs[i] ? "<small>" + esc(errs[i]) + "</small>" : "") + "</div>" +
          "<button class=\"x\" data-del-op=\"" + i + "\" title=\"Remove this operation\">×</button></li>";
      }).join("") + "</ol>" : "<div class=\"list-empty\">No changes yet. Add a table (<kbd>n</kbd>), or double-click a table to edit it.</div>");
    var slug = S.designSlug || (wb.slugify ? wb.slugify(d.name) : d.name.toLowerCase().replace(/[^a-z0-9]+/g, "-"));
    foot.hidden = S.mode !== "design";
    foot.innerHTML = "<h3 class=\"sh\" style=\"padding:0\">Hand off to an agent<em>spec · SQL · JSON</em></h3>" +
      "<button class=\"btn btn--primary btn--lg btn--block\" data-dexp=\"prompt\" title=\"Saves, then copies a self-contained prompt: instructions plus the full spec\">Copy agent prompt</button>" +
      "<div class=\"row2\"><button class=\"btn\" id=\"design-save\">" + (STATIC ? "Save in browser" : "Save to repo") + "</button><button class=\"btn caret\" id=\"design-formats\">Other formats</button></div>" +
      (!STATIC ? "<div class=\"paths\"><span title=\"" + esc(S.designPath || "") + "\">" + (S.designPath ? "→ " + esc(S.designPath.replace(/^.*\/(\.schema\/)/, "$1")) : "saves to .schema/designs/" + esc(slug) + ".json") + "</span>" +
        "<span>verify: <b>schema design check " + esc(slug) + "</b></span></div>" : "");
    $("#design-new-table").onclick = function () { openTableEditor(null); };
    $("#design-undo").onclick = designUndo;
    $("#design-relayout").onclick = function () {
      designEdit(function () { d.positions = {}; d.filter_positions = {}; });
      setTimeout(function () { if (!filterActive()) { snapshotPositions(); saveDesignDraft(); } }, 200);
    };
    $("#design-save").onclick = function () { saveDesign().catch(function (e) { toast("Save failed: " + e.message, 4000); }); };
    $$("[data-del-op]", box).forEach(function (b) {
      b.onclick = function () { var i = Number(b.getAttribute("data-del-op")); designEdit(function () { d.ops.splice(i, 1); }); };
    });
    $$("[data-dexp]", foot).forEach(function (b) { b.onclick = function () { designExport(b.getAttribute("data-dexp")); }; });
    $("#design-formats").onclick = function (e) {
      e.stopPropagation();
      var m = $("#context-menu");
      var items = [["Copy spec (Markdown)", "md-copy"], ["Copy SQL", "sql-copy"], ["-"], ["Download .md", "md"], ["Download .sql", "sql"], ["Download .json", "json"]];
      m.innerHTML = items.map(function (it) { return it[0] === "-" ? "<hr class=\"menu__sep\">" : "<button class=\"menu__item\" data-dexp=\"" + it[1] + "\"><span>" + esc(it[0]) + "</span></button>"; }).join("");
      m.hidden = false;
      placePop(m, e.currentTarget, { alignRight: true });
      $$("[data-dexp]", m).forEach(function (b) { b.onclick = function () { m.hidden = true; designExport(b.getAttribute("data-dexp")); }; });
    };
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

  // ---- hints: one line about a feature you haven't used yet ---------------------
  var HINTS = [
    { id: "filter", html: "<b>Type a table name</b> in the bar above the diagram to show just it and its neighbours" },
    { id: "dbl", html: "<b>Double-click</b> a table to focus it with its neighbours" },
    { id: "c", html: "<b>Press <kbd>c</kbd></b> to switch between changed tables and all tables · <kbd>[</kbd> <kbd>]</kbd> add neighbours", when: function () { return !!(S.diff && S.base); } },
    { id: "ctx", html: "<b>Right-click</b> a table for filters, columns and design actions" },
    { id: "k", html: "<kbd>k</kbd> cycles column modes · <kbd>e</kbd> edge styles · <kbd>1</kbd>–<kbd>5</kbd> layouts" },
  ];
  function hintsDone() {
    try {
      var d = JSON.parse(localStorage.getItem("schema:hints") || "{}");
      if (localStorage.getItem("schema:tip")) d.filter = 1; // the old one-time tip
      return d;
    } catch (e) { return {}; }
  }
  function hintUsed(id) {
    var d = hintsDone();
    if (d[id]) return;
    d[id] = 1;
    try { localStorage.setItem("schema:hints", JSON.stringify(d)); } catch (e) { /* ignore */ }
    if (S.hintShown === id) $("#hint").hidden = true;
  }
  function showHint() {
    var done = hintsDone(), el = $("#hint");
    var h = HINTS.find(function (x) { return !done[x.id] && (!x.when || x.when()); });
    if (!h) { el.hidden = true; return; }
    S.hintShown = h.id;
    el.innerHTML = "<span>" + h.html + "</span><span class=\"muted\">·</span><span>all shortcuts</span><kbd>?</kbd><button class=\"icon-btn\" data-dismiss title=\"Got it\">×</button>";
    el.hidden = false;
    el.querySelector("[data-dismiss]").onclick = function () { hintUsed(h.id); el.hidden = true; };
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
        hintUsed("dbl");
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
  // ---- resizable panels -----------------------------------------------------------
  // The sidebar and the right panel (details or the docked editor) can be dragged;
  // widths are remembered for every file. Double-click a handle to reset.
  var PANELS = { sidebar: { v: "--sidebar-w", min: 200, max: 520 }, details: { v: "--details-w", min: 260, max: 760 }, editor: { v: "--editor-w", min: 360, max: 900 } };
  function panelWidths() { try { return JSON.parse(localStorage.getItem("schema:panels") || "{}"); } catch (e) { return {}; } }
  function applyPanelWidths() {
    var w = panelWidths(), root = document.documentElement.style;
    Object.keys(PANELS).forEach(function (k) { if (w[k]) root.setProperty(PANELS[k].v, w[k] + "px"); else root.removeProperty(PANELS[k].v); });
  }
  var panelDrag = null; // the drag in progress: { k, w, end }
  function endPanelDrag() {
    var d = panelDrag;
    if (!d) { document.body.classList.remove("resizing"); return; }
    panelDrag = null;
    window.removeEventListener("pointermove", d.move, true);
    window.removeEventListener("pointerup", d.end, true);
    window.removeEventListener("pointercancel", d.end, true);
    window.removeEventListener("mouseup", d.end, true);
    window.removeEventListener("blur", d.end);
    d.h.classList.remove("active");
    document.body.classList.remove("resizing");
    var saved = panelWidths();
    saved[d.k] = d.w;
    try { localStorage.setItem("schema:panels", JSON.stringify(saved)); } catch (err) { /* ignore */ }
  }
  function bindResizers() {
    applyPanelWidths();
    var rightPanel = function () { return S.editor ? "editor" : "details"; };
    [["#rz-left", function () { return "sidebar"; }, 1], ["#rz-right", rightPanel, -1]].forEach(function (spec) {
      var h = $(spec[0]), which = spec[1], dir = spec[2];
      h.addEventListener("pointerdown", function (e) {
        if (e.button !== 0 && e.pointerType === "mouse") return;
        if (panelDrag) endPanelDrag();
        var k = which(), p = PANELS[k];
        var el = k === "sidebar" ? $("#sidebar") : k === "editor" ? $("#table-editor") : $("#details");
        var start = e.clientX, w0 = el.getBoundingClientRect().width;
        var d = panelDrag = { k: k, w: w0, h: h };
        // listen on the window (capture) so the drag ends wherever the button
        // comes up — outside the window, over the canvas, after a Cmd-Tab…
        d.move = function (ev) {
          if (ev.pointerType === "mouse" && ev.buttons === 0) { endPanelDrag(); return; } // the up was missed
          d.w = Math.round(Math.max(p.min, Math.min(p.max, w0 + (ev.clientX - start) * dir)));
          document.documentElement.style.setProperty(p.v, d.w + "px");
        };
        d.end = function () { endPanelDrag(); };
        window.addEventListener("pointermove", d.move, true);
        window.addEventListener("pointerup", d.end, true);
        window.addEventListener("pointercancel", d.end, true);
        window.addEventListener("mouseup", d.end, true);
        window.addEventListener("blur", d.end);
        try { h.setPointerCapture(e.pointerId); } catch (err) { /* fine without capture: the window listeners cover it */ }
        h.classList.add("active");
        document.body.classList.add("resizing");
        e.preventDefault();
      });
      h.addEventListener("dblclick", function () {
        var k = which(), saved = panelWidths();
        delete saved[k];
        try { localStorage.setItem("schema:panels", JSON.stringify(saved)); } catch (err) { /* ignore */ }
        applyPanelWidths();
      });
    });
    // safety net: a stuck "resizing" state clears on the next press or Escape
    document.addEventListener("pointerdown", function (e) { if (document.body.classList.contains("resizing") && !e.target.closest(".resizer")) endPanelDrag(); }, true);
    document.addEventListener("keydown", function (e) { if (e.key === "Escape" && panelDrag) endPanelDrag(); }, true);
    document.addEventListener("visibilitychange", function () { if (document.hidden && panelDrag) endPanelDrag(); });
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
      renderVersion();
      viz = new wb.Schema();
      S.defaults = JSON.parse(wb.default_config());
      initViewer();
      bindControls();
      bindRefPicker();
      bindSearch();
      bindExport();
      bindKeys();
      bindEditor();
      bindFilterBar();
      bindSource();
      bindBrowseList();
      bindResizers();
      bindGroups();
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
      else if (stored && stored.compare) { S.base = stored.base; S.compare = stored.compare; S.mergeBaseOf = stored.mergeBaseOf || null; }
      else { S.base = S.server.initial.base; S.compare = S.server.initial.compare; }
      S.mode = params.get("mode") || (S.designParam ? "design" : S.base ? "compare" : (stored && stored.mode && stored.mode !== "design") ? stored.mode : "browse");
      if (S.mode === "compare" && !S.base) S.mode = "browse";
      if (S.mode === "browse") { S.base = null; S.browseRef = S.compare; }
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
      showPanel(S.mode);
      render({ fit: true });
      updateCompareUI();
      poll();
      setTimeout(showHint, 800);
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
    showPanel(S.mode);
    setTimeout(showHint, 800);
    if (pick) return loadExample(pick).then(restoreDesign);
    $("#loading").textContent = "Open or drop a structure.sql or schema.rb file";
  }

  boot();
})();
