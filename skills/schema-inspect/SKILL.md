---
name: schema-inspect
description: Answer questions about a Postgres database schema (tables, columns, relations, indexes) by querying structure.sql with schema inspect instead of reading the whole dump. Use when you need to understand data models, find where a column lives, or trace relationships between tables.
---

# schema-inspect

`structure.sql` files are large; `schema inspect` gives compact, exact answers.

## Commands

```bash
schema inspect db/structure.sql                      # one line per table: columns count, → references, ← referenced by
schema inspect db/structure.sql --table users        # columns, types, nullability, defaults, PK/FK, indexes, checks, referenced-by, triggers, views
schema inspect db/structure.sql --table users --depth 2   # plus the tables within 2 FK hops
schema inspect db/structure.sql --table 'billing.*'  # glob patterns work
schema inspect db/structure.sql --search email       # tables/columns matching (substring or glob)
schema inspect db/structure.sql HEAD~5 --table users # the schema as of a git ref
```

Add `--json` to any of them for structured output.

## Workflow

1. Start with the summary (or `--search`) to locate the relevant tables.
2. Drill into specific tables with `--table` (use `--depth` to discover join paths).
3. Answer precisely: quote column names/types and the FK path (e.g. `comments.task_id → tasks.id → projects.id`).
4. If a picture helps, open a focused diagram in the background:
   `schema db/structure.sql --focus users,orders --depth 1 --detach` (see the `schema-view` skill),
   or embed one in a document (see `schema-embed`).
