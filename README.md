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
- **Layouts:** layered (Sugiyama, in any direction), force-directed, grid, circular and radial. Tables can be grouped by schema, by name prefix or into custom groups.
- **Edges:** curved, orthogonal or straight. They attach to the FK and PK column rows or to table borders. They show crow's-foot cardinality and can optionally include inferred `*_id` relations and view dependencies.
- **Postgres-native parser** for `pg_dump --schema-only` output and hand-written DDL. It handles schemas, enums, partitions, identity and generated columns, partial and expression indexes, checks, views, materialized views, functions, triggers and comments.
- **Shareable output:** SVG, PNG, or a single self-contained HTML file with the WASM viewer inlined. There's also an embed bundle for your own pages.
- **Agent skills.** They let Claude Code and other agents open diagrams, review migrations, answer schema questions and generate HTML docs with diagrams embedded.

## Install

Requirements: Rust with the `wasm32-unknown-unknown` target, and `wasm-pack`.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
make install        # builds web/pkg (WASM), then `cargo install --path crates/cli`
```

A plain `cargo build` / `cargo install --path crates/cli` also works. The CLI's `build.rs` runs wasm-pack automatically whenever the WASM bundle is missing or out of date. Set `SCHEMA_SKIP_WASM=1` to skip that step.

## Usage

```
schema [FILE] [REFS...] [OPTIONS]      open the interactive viewer
schema html    [FILE] [REFS...] -o out.html   standalone HTML (embedded WASM)
schema svg     [FILE] [REFS...] -o out.svg    static SVG
schema diff    [FILE] [REFS...] [--json]      Markdown / JSON schema diff
schema inspect [FILE] [--table T [--depth N]] [--search Q] [--json]
schema embed   [-o schema.embed.js]        embeddable bundle
schema list | stop [FILE|--all]               running instances
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

Common view options: `--focus a,b --depth N --direction in|out|both`, `--changes-only --context N`, `--layout layered|force|grid|circular|radial`, `--rankdir LR|TB|RL|BT`, `--edges curved|orthogonal|straight|hidden`, `--anchor column|table`, `--columns auto|all|keys|relations|changed|none`, `--hide-columns created_at,users.encrypted_*`, `--include`, `--exclude`, `--schemas`, `--group-by schema|prefix|custom`, `--views`, `--partitions`, `--inferred`, `--labels`, `--view NAME`, `--config FILE|JSON`, `--dark`.

Every file gets its own port, starting from 5491. Running the command again for the same file reuses the existing server; `--new` restarts it. Other server options are `--no-open`, `--port`, and `-d/--detach` to run in the background (useful for agents). The viewer live-reloads when the file, the git index or HEAD changes.

### In the browser

- **Display tab:** layout, relations, columns and filters. Every setting is remembered per file.
- **Tables tab:** show or hide individual tables, filter the list, and focus on a table.
- **Changes tab:** a structured diff and the file's git history. Click a commit to see what it changed.
- **Diagram:** drag tables around, and scroll or pinch to pan and zoom. Click a table for details, where you can also hide individual columns. Double-click a table to focus on it, and right-click for more actions.
- **Keyboard:** `/` search, `f` fit, `1`–`5` switch layouts, `c` toggle changes only, `k` cycle column modes, `e` cycle edge styles, `Esc` clear the selection or focus.
- **Export menu:** SVG, PNG, standalone HTML, the diff as Markdown, the config JSON, or the equivalent CLI command. You can also save the current view as a named view or as the project default in `.schema.json`.

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

## Agent skills

```bash
schema skills install          # → ./.claude/skills (repo root)
schema skills install --global # → ~/.claude/skills
```

| Skill | What it does |
|---|---|
| `schema-view` | opens a focused diagram (in the background) for what you're working on |
| `schema-diff` | reviews schema changes between refs, flags risky migrations and opens the visual diff |
| `schema-inspect` | answers schema questions with `schema inspect` instead of reading the dump |
| `schema-embed` | generates HTML documents with interactive embedded diagrams |

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
