---
name: schema-design
description: Implement a schema design created in the schema viewer (migrations from a .schema/designs spec, verified with `schema design check`), or draft a design of database changes for the user to review visually. Use when the user mentions a schema design, asks to implement designed tables/columns, or wants to plan database changes before writing migrations.
---

# schema-design

A *design* is a list of schema operations (create table, add column, add foreign key, …) made on
top of the current `structure.sql`, usually in the viewer's **Design** tab. Designs are saved in
the repo as `.schema/designs/<name>.json` (source of truth) and `<name>.md` (the spec).

## Implement a design

1. Find it: `schema design list`
2. Read the spec: `schema design show NAME` (Markdown with intent, per-table changes, SQL and the
   operations as JSON). Use `--format sql` for just the DDL, `--format json` for the raw design.
   Honour the notes and ⚠️ warnings (backfills before `NOT NULL`, data loss on drops).
3. Implement it with the project's migration tooling — for Rails, `bin/rails generate migration …`
   and edit the migration; don't edit `db/structure.sql` / `db/schema.rb` by hand. Keep table, column and index
   names; constraint names may follow the project's conventions.
4. Run the migrations so the schema dump is regenerated (Rails: `bin/rails db:migrate`).
5. Verify: `schema design check NAME`. It compares the design with the regenerated
   `structure.sql` or `schema.rb` (types are compared loosely: `varchar(255)` = `character varying(255)`,
   `timestamp` = `timestamp(6)`) and exits non-zero until every table is ✓. Fix and repeat.
6. Report what you implemented and anything you intentionally did differently.

## Draft a design for the user

When the user wants to plan changes first, write `.schema/designs/<slug>.json` and open it:

```json
{
  "version": 1,
  "name": "Card payments",
  "description": "Why this change exists and what it enables.",
  "ops": [
    {"op": "create_table", "table": "cards", "primary_key": ["id"], "columns": [
      {"name": "id", "type": "bigserial", "nullable": false},
      {"name": "user_id", "type": "bigint", "nullable": false},
      {"name": "last4", "type": "varchar(4)"},
      {"name": "created_at", "type": "timestamp(6)", "nullable": false}]},
    {"op": "add_foreign_key", "table": "cards", "columns": ["user_id"], "references": "users", "on_delete": "cascade"},
    {"op": "add_index", "table": "cards", "columns": ["user_id", "last4"], "unique": true},
    {"op": "add_column", "table": "users", "column": {"name": "phone", "type": "varchar(20)"}}
  ],
  "notes": {"public.cards": "Store only the last 4 digits, never the card number."}
}
```

Other ops: `drop_table`, `rename_table {to}`, `drop_column {column}`, `rename_column {column, to}`,
`alter_column {column, type?, nullable?, default?, drop_default?}`, `set_primary_key {columns}`,
`drop_foreign_key {name}`, `drop_index {name}`, `set_comment {column?, comment}`.
Tables are `name` or `schema.name`; `nullable` defaults to true; `ref_columns` default to the
referenced primary key.

Then: `schema design show <slug>` to validate (warnings list invalid ops), and
`schema --design <slug> -d` to open it in the viewer, where the user can review and edit it
(changes show as a diff: new = green, changed = amber, dropped = red) and save it back.
