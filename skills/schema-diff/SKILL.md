---
name: schema-diff
description: Review Postgres schema changes (structure.sql or Rails schema.rb) between git refs or files — summarise added/removed/changed tables, columns, foreign keys and indexes, flag risky migrations, and open a visual diff. Use for reviewing migrations, PRs or branches that touch the database schema.
---

# schema-diff

## Steps

1. Pick the comparison:
   - uncommitted changes: no refs (HEAD vs working tree when the file is dirty) or `work`
   - staged only: `staged`
   - branch review: `main...HEAD` (merge-base, what a PR would merge) or `main..feature`
   - last commit(s): `HEAD~1`, `HEAD~3`
   - two files without git: `--base-file old.sql`
2. Get the machine-readable diff:
   ```bash
   schema diff db/structure.sql main...HEAD          # Markdown summary (db/schema.rb works too)
   schema diff db/structure.sql main...HEAD --json   # full structured diff
   ```
   If it reports "nothing to compare", ask which refs to use.
3. Review the changes. Call out:
   - dropped tables / columns (data loss), type changes that rewrite tables or narrow types
   - `NOT NULL` added without a default on existing tables
   - new foreign keys without a supporting index on the referencing columns
     (check the index list for the FK columns), missing `ON DELETE` behaviour
   - unique indexes/constraints added to populated tables, removed indexes still used by FKs
   - enum values removed or reordered (`schema diff` lists +/− values; the viewer draws changed enums
     linked to the columns that use them), view/function definitions changed
   For details on any table use `schema inspect db/structure.sql --table NAME`.
4. Open the visual diff focused on what changed, in the background:
   ```bash
   schema db/structure.sql main...HEAD --detach
   ```
   Comparisons open on the changed tables only. Add `--context 1` (or more) to include
   neighbours, which then show only the columns the relations use;
   `--all-tables` for the whole schema, or `--all-columns` to expand the neighbours.
   Added tables/columns are green, removed red (dashed), modified amber with `old → new` types.
5. Report: a short summary table of changes, then the risks with concrete suggestions, then the viewer URL.

## Sharing

For a PR comment or design doc, `schema html db/structure.sql main...HEAD --changes-only -o pr-diff.html`
creates a single self-contained HTML file (embedded WASM viewer) — see the `schema-embed` skill.
