# Changelog

Notable changes to schema. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [semantic versioning](https://semver.org/).

Add entries under **Unreleased** as you go. `scripts/release.sh` turns that section into the new version, and the release notes and the website's changelog are built from this file.

## [Unreleased]

## [0.5.0] - 2026-10-02

### Added

- `schema png` renders the diagram as a PNG image (`--scale N`, default 2; `--dark`), e.g. `schema png a1b2c3^! -o schema.png` for what one commit changed.
- `REV^!` compares a commit with its parent (git's notation for "just this commit"), in every command and the viewer.
- Select and hand tools, in a tool bar at the bottom of the diagram (`V` / `H`). With the select tool, drag a box around tables or shift-click them to select several, then drag one to move them all; hold `Space` to pan. With the hand tool, dragging anywhere pans. The tool is remembered.

### Changed

- A new version is announced in a thin banner across the top of the viewer instead of a button in the top bar. **What's new** lists the changelog entries since your version (and, for builds from a source clone, the commits on main), and the banner gives the command that fits how schema was installed: `brew upgrade dobrinov/tap/schema` for Homebrew, `schema update` for a source clone, the release page for a downloaded binary. Dismissing it hides it until the next version.
- `schema update` runs `brew upgrade` for any Homebrew install.
- A table's right-click menu no longer has both *Show its neighbours too* and *Add to filter*, which did nearly the same thing; *Add to filter, with its neighbours* keeps your chips and adds the table with its neighbours.
- The mode tabs no longer show the ⇧B / ⇧C / ⇧D badges (the shortcuts are still in the tooltips and the help), and the shortcuts also work on non-Latin keyboard layouts.

### Fixed

- In the Changes view, a table chip's hops did nothing: the chip only picked which changed tables to show. Its hops now add the neighbours of the changed tables it matches.
- **Show with its neighbours** (and double-clicking a table) showed no neighbours when the default depth in Options was 0; it now always shows at least one hop, and no longer opens the details panel.
- A chip's **+** is disabled once more hops would add nothing (a table without relations, or every connected table already shown), and an empty Changes view explains when no changed table is within reach of the chips.
- `schema update` and the update banner suggest `brew upgrade` only for Homebrew release installs; a source build installed under a Homebrew prefix updates from its clone again.
- **What's new** for a source build no longer lists unreleased changelog entries the build already has.
- Keyboard shortcuts follow the typed letter on Latin layouts (Dvorak, …) and the physical key on non-Latin ones (Cyrillic, …).
- Holding Space pans only when the diagram has focus or the pointer is over it, so Space still presses a focused button; a click on a table while Space is held selects it.
- Toasts no longer cover the hint and the tool bar; *Fit* leaves room for the tool bar; the hint and minimap make way on a narrow diagram, and the top bar fits at tablet widths.
- The tables list highlights the table whose details are open, also when it was picked on the diagram.
- *Diff as Markdown* is disabled when nothing is compared; column defaults stay on one line in the details panel; a new table's primary key is listed once; the ref picker no longer lists `origin` (the remote's HEAD) as a branch; starting a design without a name marks the name field.

## [0.4.0] - 2026-10-01

### Added

- The help dialog (`?`) shows the running version and the commit it was built from (noting local changes), and `/api/health` reports the version.

## [0.3.0] - 2026-10-01

### Added

- Rails `db/schema.rb` support. The Ruby schema DSL is read as the Postgres DDL it describes: tables, primary keys (`id: :uuid`, `id: :serial`, composite and custom keys, `id: false`), column types, limits, precision, defaults and `array: true`, enums, virtual columns, indexes (expression, partial, ordered, `using:`, `include:`), foreign keys (including Rails' default `<singular>_id` columns), check and unique constraints, comments, `t.references` and `t.timestamps`, and `scenic` views. Diffs, git history, design mode and every export work the same as with `structure.sql`.
- `db/schema.rb` and `schema.rb` are auto-detected after the `structure.sql` / `schema.sql` candidates, and the playground opens `.rb` files.
- This changelog, also published on the website.

### Changed

- The release workflow builds the WASM bundle once, cross-compiles the Intel macOS binary and publishes the Homebrew formula through the GitHub contents API.
- Workflows use the Node 24 action majors.
- Website: install options up front, with a Homebrew one-liner in the hero.

## [0.2.0] - 2026-10-01

The first tagged release.

### Added

- Interactive ER diagrams for Postgres `structure.sql` files, served locally and opened in the browser, with one engine (Rust compiled to WebAssembly) behind the CLI, the viewer and embeds.
- Git-aware comparisons using git ref syntax (`main`, `HEAD~3`, `main..feature`, `main...feature`, `work`, `staged`, `unstaged`, `--base-file`), with a searchable picker over the file's history. Comparisons open on the changed tables only.
- Enum changes in diffs: enum nodes with added and removed values, linked to the affected columns.
- Browse, Compare and Design modes. Design mode sketches schema changes on top of the real schema and exports a spec for an agent to implement; `schema design check` verifies the result.
- Focus with per-table neighbour depth, include / exclude globs, hidden columns, column modes, and named views saved to `.schema.json`.
- Layered, force-directed, grid, circular and radial layouts; a focus layout that arranges neighbours around the focused tables; table groups by schema, prefix or hand-picked sets.
- Orthogonal edges by default, routed globally with lanes and hops at crossings.
- Column tooltips with key and index details, and an IX tag on indexed columns.
- Exports: SVG, PNG, self-contained HTML, and an embeddable JS bundle.
- `schema diff` (Markdown / JSON) and `schema inspect` for terminals and agents.
- Agent skills: `schema-view`, `schema-diff`, `schema-inspect`, `schema-embed` and `schema-design`.
- Startup update check and `schema update`.
- Release builds for macOS (Apple silicon and Intel), Linux (x86_64 and arm64) and Windows, and a Homebrew tap.

[Unreleased]: https://github.com/dobrinov/schema/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/dobrinov/schema/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/dobrinov/schema/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/dobrinov/schema/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/dobrinov/schema/releases/tag/v0.2.0
