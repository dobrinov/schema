<img src="web/favicon.svg" width="72" alt="schema logo">

# schema

Interactive ER diagrams and visual git diffs for Postgres `structure.sql` files.
It's written in Rust and compiled to WebAssembly: one engine runs in the CLI, the browser viewer and embeddable HTML diagrams.

```bash
schema db/structure.sql            # opens your browser; shows uncommitted changes if any
schema main...HEAD --changes-only  # what this branch changes, and nothing else
schema --focus orders --depth 2    # just the tables around `orders`
```

- **Git-aware.** Compare the working tree, the index, commits, branches or tags, using familiar git ref syntax. You can also pick any version from the file's history in the UI. Added tables and columns are green, removed ones are red and dashed, and changed ones are amber with `old → new`.
- **Configurable.** Focus on tables and their neighbours, include or exclude tables with globs, and hide columns globally (`*_at`) or per table. Tables can show keys only or collapse to their header. Positions are draggable, and named views are saved to `.schema.json`.
- **Layouts:** layered (Sugiyama, in any direction), force-directed, grid, circular and radial. When you focus on tables, the layered layout arranges the neighbourhood around them: tables they reference on one side, tables that reference them on the other, further hops further out, ordered so relations cross as little as possible. Tables can be grouped by schema, by name prefix or into custom groups.
- **Edges:** curved, orthogonal or straight. They attach to the FK and PK column rows or to table borders. They show crow's-foot cardinality and can optionally include inferred `*_id` relations and view dependencies.
- **Postgres-native parser** for `pg_dump --schema-only` output and hand-written DDL. It handles schemas, enums, partitions, identity and generated columns, partial and expression indexes, checks, views, materialized views, functions, triggers and comments.
- **Shareable output:** SVG, PNG, or a single self-contained HTML file with the WASM viewer inlined. There's also an embed bundle for your own pages.
- **Design mode.** Sketch schema changes on top of your real schema (new tables, columns, foreign keys, indexes, renames, drops), see them as a diff, and export a spec an agent implements with migrations. `schema design check` verifies the result.
- **Agent skills.** They let Claude Code and other agents open diagrams, review migrations, answer schema questions, implement designs and generate HTML docs with diagrams embedded.

## Install

Requirements: Rust with the `wasm32-unknown-unknown` target, and `wasm-pack`.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
make install        # builds web/pkg (WASM), then `cargo install --path crates/cli`
```

This installs the `schema` binary to `~/.cargo/bin`. If your shell reports `schema not found`, that directory isn't on your `PATH`. Add it once:

```bash
echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc   # or ~/.bashrc
source ~/.zshrc
```

Plain `make` only builds into `target/`. Run the binary from there with `./target/debug/schema`, or use `make install` as above.

A plain `cargo build` / `cargo install --path crates/cli` also works. The CLI's `build.rs` runs wasm-pack automatically whenever the WASM bundle is missing or out of date. Set `SCHEMA_SKIP_WASM=1` to skip that step.

## Quick start

Run `schema` from the root of the repository that contains your schema, not from this repo:

```bash
cd ~/code/my-rails-app

schema                                  # db/structure.sql; shows uncommitted changes if any
schema main...my-branch                 # visual diff: the changed tables + their neighbours
schema main...my-branch --all-tables    # the whole schema, changes highlighted
schema main...my-branch --all-columns   # neighbours show all their columns too
schema diff main...my-branch            # Markdown summary in the terminal
schema -d main...my-branch              # run the server in the background
```

`FILE` is optional: `db/structure.sql` is found automatically. Pass a path only if the schema lives somewhere else.

A comparison opens on what changed: only changed tables plus one hop of neighbours (`--context N` for more). That keeps a few new tables findable in a 400-table schema. Toggle **only changes** in the viewer, or pass `--all-tables`, to see everything with the changes highlighted.

With three dots (`main...my-branch`), the comparison starts from where the branch split off `main`, so you only see that branch's changes. With two dots (`main..my-branch`), it compares the two branch tips directly.

## Usage

```
schema [FILE] [REFS...] [OPTIONS]      open the interactive viewer
schema html    [FILE] [REFS...] -o out.html   standalone HTML (embedded WASM)
schema svg     [FILE] [REFS...] -o out.svg    static SVG
schema diff    [FILE] [REFS...] [--json]      Markdown / JSON schema diff
schema inspect [FILE] [--table T [--depth N]] [--search Q] [--json]
schema embed   [-o schema.embed.js]        embeddable bundle
schema list | stop [FILE|--all]               running instances
schema design  list | show NAME [--format md|sql|json] | check NAME [--json]
schema skills  list | show NAME | install [--global] [--dir PATH]
```

`FILE` defaults to `db/structure.sql`, `structure.sql`, `db/schema.sql` or `schema.sql`. schema looks in the current directory first, then in the repo root.

| Refs | Compares |
|---|---|
| *(none)* | `HEAD` vs working tree, if the file has uncommitted changes |
| `main` / `HEAD~3` / `v1.2.0` | that ref vs working tree |
| `main..feature` or `main feature` | `main` vs `feature` |
| `main...feature` | merge-base vs `feature` (what a PR merges) |
| `work` · `staged` · `unstaged` | HEAD→worktree · HEAD→index · index→worktree |
| `--base REF --compare REF` | explicit; `WORKTREE` and `INDEX` are pseudo-refs |
| `--base-file old.sql` | two files, no git needed |

Common view options: `--focus a,b --depth N --direction in|out|both` (per-table depth: `--focus users:2,cards:0`), `--changes-only --context N`, `--layout layered|force|grid|circular|radial`, `--rankdir LR|TB|RL|BT`, `--edges curved|orthogonal|straight|hidden`, `--anchor column|table`, `--columns auto|all|keys|relations|referenced|changed|none`, `--unchanged-columns MODE`, `--hide-columns created_at,users.encrypted_*`, `--include`, `--exclude`, `--schemas`, `--group-by schema|prefix|custom`, `--views`, `--partitions`, `--inferred`, `--labels`, `--view NAME`, `--config FILE|JSON`, `--dark`.

Every file gets its own port, starting from 5491. Running the command again for the same file reuses the existing server; `--new` restarts it. Other server options are `--no-open`, `--port`, and `-d/--detach` to run in the background (useful for agents). The viewer live-reloads when the file, the git index or HEAD changes.

### In the browser

- **Filter bar** (over the diagram): type a table or pattern (`users`, `card*`) to show only those tables. Each entry is a chip with its own neighbour depth (− / +), so you can show `users` with 2 hops and `cards` alone. Include/exclude patterns, schemas, "changes only" and hidden tables appear as chips too. When a filter is active the bar is highlighted and shows "N of M tables"; click that for what is hidden and why. *Options* has direction, patterns, schemas and more; *Clear* removes everything. Right-click a table for *Show only this table*, *Show its neighbours too*, *More/Fewer neighbours* and *Remove from filter*.
- **Display tab:** layout, relations, columns and which objects appear (views, partitions). Every setting is remembered per file.
- **Tables tab:** show or hide individual tables, search the list, and focus on a table.
- **Changes tab:** a structured diff and the file's git history. Click a commit to see what it changed: the view switches to that commit, filters to the changed tables plus their direct neighbours, and zooms to them.
- **Diagram:** drag tables around, and scroll or pinch to pan and zoom. A filtered view is laid out for just the tables it shows. Dragged positions belong to the view you made them in: the unfiltered diagram and each filter keep their own arrangement. Click a table for details, where you can also hide individual columns. Double-click a table to focus on it, and right-click for more actions.
- **Keyboard:** `/` search, `f` fit, `1`–`5` switch layouts, `c` toggle changes only, `k` cycle column modes, `e` cycle edge styles, `Esc` clear the selection or focus.
- **Export menu:** SVG, PNG, standalone HTML, the diff as Markdown, the config JSON, or the equivalent CLI command. You can also save the current view as a named view or as the project default in `.schema.json`.

### Columns in a diff

When you compare versions, the tables that didn't change are there only for context. By default they show just the columns the diagram's relations use: FK columns pointing out, and the columns other visible tables reference. The rest collapse into a "… N more columns" row you can click. Changed tables keep their normal columns, and per-table column settings still win.

To change it, use **Columns → Unchanged tables** in the Display tab, `--unchanged-columns MODE` (`--all-columns` shows everything), or `"unchanged_columns"` in config (`null` = same as the other tables).

The same `referenced` mode is available as the main column mode (`--columns referenced`) for any diagram.

### `.schema.json`

Put this file at the repo root:

```json
{
  "default": { "exclude": ["schema_migrations", "ar_internal_metadata", "active_storage_*"], "hide_columns": ["created_at", "updated_at"] },
  "views": {
    "billing": { "focus": ["billing.*"], "focus_depth": 1, "columns": "keys", "layout": { "group_by": "schema" } }
  }
}
```

Then run `schema --view billing`. The full config reference is in [`crates/core/src/config.rs`](crates/core/src/config.rs). Every field is optional.

## Embedding

```html
<script src="schema.embed.js"></script>   <!-- from `schema embed` -->
<div data-schema data-config='{"focus":["orders"]}' style="height:500px">
  <script type="application/sql">CREATE TABLE ...</script>
  <!-- optional: <script type="application/sql" data-base-sql>old DDL</script> -->
</div>
<!-- or data-src="structure.sql" data-base-src="old.sql" -->
```

```js
Schema.mount(el, { sql, baseSql, config, title });
await Schema.render(sql, config);       // → SVG string
await Schema.diffMarkdown(oldSql, newSql);
```

## Designing schema changes

Open the **Design** tab, name the design and start editing. Your loaded schema is the starting point, and every change is recorded as an operation, shown as a diff: new tables are green, changed ones amber, dropped ones red.

- **+ New table** (or `n`, or right-click the canvas → *New table here*) opens the table editor. It covers columns (type, NOT NULL, default, primary key), foreign keys, indexes (unique, partial) and a note for whoever implements it.
- **Existing tables:** double-click a table, or right-click → *Edit table*, *Add column*, *Drop table*. Renames, type changes and dropped columns are recorded as explicit operations.
- **Undo and cleanup:** `⌘Z` undoes, the operation list lets you remove any single step, and invalid steps are flagged. The layout is frozen while you design; **Re-layout** starts it fresh.
- **Saving:** *Save to repo* writes `.schema/designs/<name>.json` and a Markdown spec `<name>.md`. Unsaved work survives reloads as a browser draft. Reopen a saved design from the Design tab or with `schema --design NAME`.

For the agent that implements it:

```bash
schema design show card-payments              # the spec: intent, per-table changes, SQL, operations JSON
schema design show card-payments --format sql # just the PostgreSQL DDL
schema design check card-payments             # after migrating: ✓ / ✗ per table, exit 1 until done
```

*Copy agent prompt* in the Design tab saves the design and copies a self-contained prompt: the implementation rules and steps plus the whole spec inline (changes, warnings, SQL, operations), so it works pasted into any agent. `schema design show NAME --format prompt` prints the same text. You can also copy or download the spec, SQL or JSON directly. In the web playground, designs are saved in your browser instead.

## Agent skills

The skills are instructions for Claude Code (and other coding agents), not shell commands. There is no `schema-diff` binary; in a terminal you run `schema diff`.

Install them in the repository whose schema you want to work with:

```bash
cd ~/code/my-rails-app
schema skills install          # → .claude/skills/ in that repo
schema skills install --global # → ~/.claude/skills (all repos)
```

Then, inside Claude Code in that repo:

```
/schema-diff main...my-branch
```

Claude runs `schema diff`, reviews the migration for risks (dropped columns, FKs without indexes, `NOT NULL` without defaults, …), and opens the visual diff in your browser.

| In Claude Code | What it does |
|---|---|
| `/schema-view` | opens a focused diagram (in the background) for what you're working on |
| `/schema-diff` | reviews schema changes between refs, flags risky migrations and opens the visual diff |
| `/schema-inspect` | answers schema questions with `schema inspect` instead of reading the dump |
| `/schema-embed` | generates HTML documents with interactive embedded diagrams |
| `/schema-design` | implements a design from `.schema/designs` with migrations and verifies it, or drafts a design for you to review |

The skill sources live in [`skills/`](skills).

## Website / GitHub Pages

`make site` builds `docs/`, which contains:

- a landing page
- a gallery of embedded examples
- the full app as a serverless **playground** (drop in any `.sql` file; nothing leaves the browser)
- standalone HTML exports

To host it, either enable GitHub Pages from the `docs/` folder, or use the included workflow (`.github/workflows/pages.yml`). The workflow tests, builds and deploys the site on every push to `main`. For that route, set **Settings → Pages → Source** to "GitHub Actions".

## Development

```
crates/core   parser, diff, filtering/graph, layout engines, edge routing, SVG renderer (pure Rust)
crates/wasm   wasm-bindgen bindings (JSON in / JSON out)
crates/cli    CLI, local HTTP server, git integration, exports, skills (assets compiled in)
web/          app (index.html, app.js, app.css), shared viewer.js, embed.js/css
site/         landing + examples sources for the Pages site
skills/       agent skills
examples/     sample structure.sql (+ a changed version for diffs)
```

```bash
make test                 # cargo test --workspace
make dev                  # serve web/ from disk: edit JS/CSS and just reload
SCHEMA_WEB_DIR=web schema ...   # same, for any invocation
```

On macOS, a Homebrew `rustc` on your PATH can shadow the rustup toolchain, and only the rustup toolchain has the wasm32 target. The Makefile and `build.rs` prefer the rustup toolchain automatically.
