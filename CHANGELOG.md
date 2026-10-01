# Changelog

Notable changes to schema. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [semantic versioning](https://semver.org/).

Add entries under **Unreleased** as you go. `scripts/release.sh` turns that section into the new version, and the release notes and the website's changelog are built from this file.

## [Unreleased]

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

[Unreleased]: https://github.com/dobrinov/schema/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/dobrinov/schema/releases/tag/v0.2.0
