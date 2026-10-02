<img src="web/favicon.svg" width="72" alt="schema logo">

# schema

Interactive ER diagrams and visual git diffs for Postgres `structure.sql` and Rails `schema.rb` files.
It's written in Rust and compiled to WebAssembly: one engine runs in the CLI, the browser viewer and embeddable HTML diagrams.

```bash
schema db/structure.sql            # opens your browser; shows uncommitted changes if any
schema main...HEAD --changes-only  # what this branch changes, and nothing else
schema --focus orders --depth 2    # just the tables around `orders`
```

- **Git-aware.** Compare the working tree, the index, commits, branches or tags, using familiar git ref syntax. You can also pick any version from the file's history in the UI. Added tables and columns are green, removed ones are red and dashed, and changed ones are amber with `old → new`.
- **Configurable.** Focus on tables and their neighbours, include or exclude tables with globs, and hide columns globally (`*_at`) or per table. Tables can show keys only or collapse to their header. Positions are draggable, and named views are saved to `.schema.json`.
- **Layouts:** layered (Sugiyama, in any direction), force-directed, grid, circular and radial. When you focus on tables, the layered layout arranges the neighbourhood around them: tables they reference on one side, tables that reference them on the other, further hops further out, ordered so relations cross as little as possible. Tables can be grouped by schema, by name prefix or into custom groups.
- **Edges:** orthogonal by default: straight runs with rounded corners, each edge in its own lane so parallel edges never overlap, ordered to avoid crossings, and a small hop where two lines must cross. Curved and straight styles are available too. Edges attach to the FK and PK column rows or to table borders. They show crow's-foot cardinality and can optionally include inferred `*_id` relations and view dependencies.
- **Enums in diffs.** A changed enum type is drawn as a node with its added / removed values, linked to every column that uses it; those columns and their tables are marked as affected.
- **Postgres-native parser** for `pg_dump --schema-only` output and hand-written DDL. It handles schemas, enums, partitions, identity and generated columns, partial and expression indexes, checks, views, materialized views, functions, triggers and comments.
- **Rails `schema.rb` too.** The Ruby schema DSL is read as the Postgres DDL it stands for: tables, primary keys (`id: :uuid`, composite keys, `id: false`), column types and defaults, enums, indexes, foreign keys (with Rails' default `<table>_id` columns), check and unique constraints, comments, and `scenic` views. Everything else, from git diffs to design mode, works the same.
- **Shareable output:** SVG, PNG, or a single self-contained HTML file with the WASM viewer inlined. There's also an embed bundle for your own pages.
- **Design mode.** Sketch schema changes on top of your real schema (new tables, columns, foreign keys, indexes, renames, drops), see them as a diff, and export a spec an agent implements with migrations. `schema design check` verifies the result.
- **Agent skills.** They let Claude Code and other agents open diagrams, review migrations, answer schema questions, implement designs and generate HTML docs with diagrams embedded.

## Install

**Homebrew** (macOS and Linux):

```bash
brew install dobrinov/tap/schema
```

**Binaries** for macOS (Apple silicon and Intel), Linux (x86_64 and arm64) and Windows are on the [Releases page](https://github.com/dobrinov/schema/releases/latest). Unpack the archive and put `schema` (or `schema.exe`) on your `PATH`. The viewer needs `git` on the `PATH` for anything git-related.

**From source.** Requirements: Rust with the `wasm32-unknown-unknown` target, and `wasm-pack`.

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
schema main...my-branch                 # visual diff: just the changed tables (+ on the chip adds neighbours)
schema main...my-branch --all-tables    # the whole schema, changes highlighted
schema main...my-branch --all-columns   # neighbours show all their columns too
schema diff main...my-branch            # Markdown summary in the terminal
schema -d main...my-branch              # run the server in the background
```

`FILE` is optional: `db/structure.sql` (or `db/schema.rb` for Rails apps on the Ruby schema format) is found automatically. Pass a path only if the schema lives somewhere else.

A comparison opens on what changed: only the changed tables. Press **+** on the *Changes* control above the diagram (or pass `--context N`) to add neighbours. That keeps a few new tables findable in a 400-table schema. Switch to **All tables** in the viewer, or pass `--all-tables`, to see everything with the changes highlighted.

With three dots (`main...my-branch`), the comparison starts from where the branch split off `main`, so you only see that branch's changes. With two dots (`main..my-branch`), it compares the two branch tips directly.

## Usage

```
schema [FILE] [REFS...] [OPTIONS]      open the interactive viewer
schema html    [FILE] [REFS...] -o out.html   standalone HTML (embedded WASM)
schema svg     [FILE] [REFS...] -o out.svg    static SVG
schema png     [FILE] [REFS...] -o out.png    PNG image (--scale N, default 2)
schema diff    [FILE] [REFS...] [--json]      Markdown / JSON schema diff
schema inspect [FILE] [--table T [--depth N]] [--search Q] [--json]
schema embed   [-o schema.embed.js]        embeddable bundle
schema list | stop [FILE|--all]               running instances
schema design  list | show NAME [--format md|sql|json] | check NAME [--json]
schema skills  list | show NAME | install [--global] [--dir PATH]
```

`FILE` defaults to `db/structure.sql`, `structure.sql`, `db/schema.sql`, `schema.sql`, `db/schema.rb` or `schema.rb`, in that order. schema looks in the current directory first, then in the repo root.

| Refs | Compares |
|---|---|
| *(none)* | `HEAD` vs working tree, if the file has uncommitted changes |
| `main` / `HEAD~3` / `v1.2.0` | that ref vs working tree |
| `main..feature` or `main feature` | `main` vs `feature` |
| `main...feature` | merge-base vs `feature` (what a PR merges) |
| `a1b2c3^!` | the changes made by one commit (its parent vs the commit) |
| `work` · `staged` · `unstaged` | HEAD→worktree · HEAD→index · index→worktree |
| `--base REF --compare REF` | explicit; `WORKTREE` and `INDEX` are pseudo-refs |
| `--base-file old.sql` | two files, no git needed |

Common view options: `--focus a,b --depth N --direction in|out|both` (per-table depth: `--focus users:2,cards:0`), `--changes-only --context N`, `--layout layered|force|grid|circular|radial`, `--rankdir LR|TB|RL|BT`, `--edges curved|orthogonal|straight|hidden`, `--anchor column|table`, `--columns auto|all|keys|relations|referenced|changed|none`, `--unchanged-columns MODE`, `--hide-columns created_at,users.encrypted_*`, `--include`, `--exclude`, `--schemas`, `--group-by schema|prefix|custom`, `--views`, `--enums none|changed|all`, `--partitions`, `--inferred`, `--labels`, `--view NAME`, `--config FILE|JSON`, `--dark`.

To get a picture of what a commit changed, for a PR or a chat thread: `schema png a1b2c3^! -o schema.png`. Changed tables are shown by default; `--context 1` adds their neighbours, `--dark` uses the dark theme and `--scale 1` makes a smaller image. `-o -` writes the PNG to stdout.

Every file gets its own port, starting from 5491. Running the command again for the same file reuses the existing server; `--new` restarts it, and it restarts by itself when the `schema` binary has been rebuilt since the instance started. Other server options are `--no-open`, `--port`, and `-d/--detach` to run in the background (useful for agents). The viewer live-reloads when the file, the git index or HEAD changes.

### In the browser

The viewer has three **modes**, switched in the top bar (`⇧B`, `⇧C`, `⇧D`). Every mode reads git refs directly (`git show ref:path`), so no branch is ever checked out. Each mode answers the same three questions in the same places: *which version* at the top of the sidebar, *which part of the schema* in the bar above the diagram, and *what about this table* in the panel on the right.

- **Browse** — view the schema and filter it. The **Viewing** card picks the version: your working tree, or any branch, tag or commit (`origin/feature` included). Below it, the tables grouped by schema, with column and relation counts.
- **Compare** — what changed between two versions. The **Comparing** card holds the **base** and **compare** pickers; clicking either (or the preset name) opens one popover with the common cases — uncommitted changes, staged changes, last commit, this branch vs main (from where it split off), a colleague's branch (pick `origin/…`, it is compared from where it left main) — plus a search over branches, tags and commits, and **↻ Fetch** for branches your colleagues pushed. The sidebar lists the changed tables and enums with what changed in each (`~1 +2`), and the file's history: click a commit to see the changes it introduced. The diagram opens on the changed tables only.
- **Design** — sketch changes on top of the current schema and hand them to an agent (see *Designing schema changes*). Leaving Design returns to the mode you came from.

Shared everywhere:

- **Canvas bar** (above the diagram): type a table or pattern (`users`, `card*`) to show only those tables. Each chip shows its neighbours up to the default depth (*Options → Depth*, 1 hop to start); use − / + on a chip to give that table its own depth. Include/exclude patterns, schemas and hidden columns appear as chips too. When there are more chips than fit, **N more ▾** expands the bar so they wrap and can be edited. **N of M tables** on the right says what is hidden and why.
- **Groups:** besides grouping by schema or name prefix, you can hand-pick tables into named groups. Right-click a table → *New group with this table…* or *Add to group*, or open *Display ▾ → Custom groups → Manage* to name groups, pick a colour and add tables or patterns (`billing.*`). Groups are remembered per file; **Save to .schema.json** makes them the project default for everyone. In Compare and Design, **Changes** limits the view to what changed (− / + adds neighbours, `[` and `]` on the keyboard) and **All tables** shows everything with the changes highlighted (`c` toggles); chips you add while Changes is on narrow the changes to the ones they match (a chip's hops add the neighbours of those changed tables), and your earlier filters wait as paused chips until you click one.
- **Display ▾** (canvas bar): layout, columns, relations and which objects appear; the rarely needed settings sit under *Advanced*. Every setting is remembered per file.
- **Diagram:** drag tables around, and scroll or pinch to pan and zoom. A filtered view is laid out for just the tables it shows; dragged positions belong to the view you made them in. Click a table for details, where you can also hide individual columns; click a column typed with an enum to see the enum's values and where else it is used. Double-click a table to show only it and its neighbours; right-click for more actions. Colour on the canvas means status: green added, red removed, amber changed, blue selected.
- **Keyboard:** `/` search, `f` fit, `1`–`5` switch layouts, `c` changes / all tables, `[` `]` fewer / more neighbours, `k` cycle column modes, `e` cycle edge styles, `Esc` close the panel, show everything, or clear the filter. Press `?` for the full list.
- **View ▾** (top bar): switch between the views saved in `.schema.json`, save the current one under a name, or make it the project default. **Export ▾** downloads the diagram (SVG, PNG, standalone HTML) or copies the diff as Markdown, the CLI command that reproduces the view, or its config.

The CLI maps onto the modes: `schema` browses the working tree, `schema --at origin/feature` browses a branch, `schema main...feature` / `schema HEAD~1` / `schema --base-file old.sql` compare, `schema --design NAME` designs.

### Columns in a diff

When you compare versions, the tables that didn't change are there only for context. By default they show just the columns the diagram's relations use: FK columns pointing out, and the columns other visible tables reference. The rest collapse into a "… N more columns" row you can click. Changed tables keep their normal columns, and per-table column settings still win.

To change it, use **Display ▾ → Advanced → Unchanged tables**, `--unchanged-columns MODE` (`--all-columns` shows everything), or `"unchanged_columns"` in config (`null` = same as the other tables).

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

Switch to **Design** in the top bar, name the design and start editing. Your loaded schema is the starting point, and every change is recorded as an operation, shown as a diff: new tables are green, changed ones amber, dropped ones red.

- **+ Table** (or `n`, or right-click the canvas → *New table here*) opens the table editor in the right panel, so the diagram stays visible. It covers columns (type, nullable, default, primary key), foreign keys, indexes (unique, partial) and a note for whoever implements it; `⌘S` applies.
- **Existing tables:** double-click a table, or right-click → *Edit table*, *Add column*, *Drop table*. Renames, type changes and dropped columns are recorded as explicit operations.
- **Undo and cleanup:** `⌘Z` undoes, the numbered operation list lets you remove any single step, and invalid steps are flagged inline. The layout is frozen while you design; **Re-layout** starts it fresh.
- **Saving:** *Save to repo* writes `.schema/designs/<name>.json` and a Markdown spec `<name>.md`. Unsaved work survives reloads as a browser draft; closing a design asks whether to discard, keep the draft or save. Reopen a saved design from Design mode or with `schema --design NAME`.

For the agent that implements it:

```bash
schema design show card-payments              # the spec: intent, per-table changes, SQL, operations JSON
schema design show card-payments --format sql # just the PostgreSQL DDL
schema design check card-payments             # after migrating: ✓ / ✗ per table, exit 1 until done
```

*Copy agent prompt* in Design mode saves the design and copies a self-contained prompt: the implementation rules and steps plus the whole spec inline (changes, warnings, SQL, operations), so it works pasted into any agent. `schema design show NAME --format prompt` prints the same text. You can also copy or download the spec, SQL or JSON directly. In the web playground, designs are saved in your browser instead.

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

- a landing page, with a changelog section rendered from `CHANGELOG.md`
- a gallery of embedded examples
- the full app as a serverless **playground** (drop in any `.sql` or `schema.rb` file; nothing leaves the browser)
- standalone HTML exports

To host it, either enable GitHub Pages from the `docs/` folder, or use the included workflow (`.github/workflows/pages.yml`). The workflow tests, builds and deploys the site on every push to `main`. For that route, set **Settings → Pages → Source** to "GitHub Actions".

## Updating

On launch, `schema` checks in the background for a newer version. A release build (Homebrew or a downloaded binary) looks for a newer `vX.Y.Z` tag; a build of a source clone looks at whether `main` has moved past the commit it was built from. If so, the terminal says so and the viewer shows a banner across the top with the update command for your install and a *What's new* list (the changelog entries since your version; for a source build, also the commits on `main`). `schema update` runs `brew upgrade` for Homebrew installs, pulls and reinstalls a source clone, and otherwise points at the Releases page; `schema update check` only checks. `schema --version` shows the version and built commit. Set `SCHEMA_NO_UPDATE_CHECK=1` to disable the check.

## Releasing

Releases are cut from `main` with one command:

```bash
scripts/release.sh patch        # or minor, major, or an explicit X.Y.Z
```

Add changes to the *Unreleased* section of [CHANGELOG.md](CHANGELOG.md) as you go; the script refuses to release while it is empty. It dates that section as the new version, bumps the workspace version in `Cargo.toml` (every crate inherits it), refreshes `Cargo.lock`, runs the tests, commits `Release vX.Y.Z`, tags `vX.Y.Z` and pushes both. The tag triggers the [Release workflow](.github/workflows/release.yml), which

1. builds the CLI for macOS arm64 and x86_64, Linux x86_64 and arm64, and Windows x86_64, each as a `.tar.gz` (`.zip` on Windows) with a `SHA256SUMS` file;
2. publishes a GitHub Release for the tag with those assets, using the version's CHANGELOG.md section (plus generated notes) as the description;
3. updates `Formula/schema.rb` in the [`dobrinov/homebrew-tap`](https://github.com/dobrinov/homebrew-tap) repository so `brew upgrade` picks the new version up.

Step 3 needs a `HOMEBREW_TAP_TOKEN` repository secret: a fine-grained personal access token with *Contents: read and write* on the tap repository. Without it the workflow still publishes the release and only logs a warning. To rebuild an existing tag, run the workflow manually from the Actions tab with the tag name.

## Reporting bugs

Click the bug button in the viewer's top bar (or press <kbd>Shift</kbd>+<kbd>R</kbd>), then **Start recording**, do what goes wrong and **Stop and describe**. schema records your clicks and keys, how the view changed after each one and any errors, and turns them into a report with instructions for an AI agent. **Open GitHub issue** files it with the [bug report template](.github/ISSUE_TEMPLATE/bug_report.yml) filled in. Table and column names are replaced with placeholders unless you untick *Hide table and column names*, and home directories never appear in the report. A recording survives a page reload.

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
