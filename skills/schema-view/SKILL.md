---
name: schema-view
description: Open an interactive ER diagram of a Postgres structure.sql / schema.sql or Rails schema.rb in the browser with schema, optionally focused on specific tables or showing git changes. Use when the user wants to see, explore or visualise the database schema.
---

# schema-view

Open the schema viewer for a Postgres schema dump (Rails `db/structure.sql` or `db/schema.rb`,
`pg_dump --schema-only` output or hand-written DDL).

## Steps

1. Find the schema file. `schema` auto-detects `db/structure.sql`, `structure.sql`,
   `db/schema.sql`, `schema.sql` and `db/schema.rb`; pass a path if it lives elsewhere.
   The examples below use `db/structure.sql`; a Rails `db/schema.rb` works the same way.
2. Translate the request into flags (all optional):
   - tables of interest → `--focus users,orders --depth 1` (`--direction in|out|both`);
     per-table neighbour depth: `--focus users:2,orders:0`
   - git comparison → refs: `main`, `HEAD~1`, `main..feature`, `main...feature`, `staged`, `work`
   - noise reduction → `--exclude 'audit_*,active_storage_*'`, `--hide-columns created_at,updated_at`,
     `--columns keys` (PK/FK/unique only), `--columns referenced` (only columns used by drawn relations),
     `--all-columns` (in a diff: also expand unchanged tables), `--no-isolated`
   - presentation → `--layout layered|force|grid|circular|radial`, `--rankdir LR|TB`,
     `--edges orthogonal|curved|straight`, `--group-by schema|prefix`, `--views`
   - saved views from `.schema.json` → `--view NAME`
3. Run it **in the background** so the command returns:
   ```bash
   schema db/structure.sql --focus users --depth 2 --detach
   ```
   It prints the URL and opens the browser. If an instance for that file already runs,
   it is reused (add `--new` to restart it).
4. Tell the user the URL and what they are looking at (focus, comparison, filters).

## Useful follow-ups

- `schema list` shows running instances, `schema stop db/structure.sql` stops one.
- Everything is adjustable in the UI afterwards (Browse / Compare / Design modes, Display ▾, the filter bar, right-click menus).
- To save a reusable view, write it to `.schema.json` at the repo root:
  ```json
  { "default": { "exclude": ["schema_migrations", "ar_internal_metadata", "active_storage_*"] },
    "views": { "billing": { "focus": ["billing.*"], "focus_depth": 1, "columns": "keys" } } }
  ```
  then `schema --view billing --detach`.

## Notes

- Use `--no-open` when running in an environment without a browser; share the printed URL instead.
- The viewer live-reloads when the file or git HEAD changes.
