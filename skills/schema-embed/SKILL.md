---
name: schema-embed
description: Generate HTML documents (design docs, PR descriptions, reports, onboarding pages) that embed an interactive ER diagram of a Postgres schema using the schema WASM bundle. Use when the user wants a shareable or self-contained schema diagram, or when writing HTML that should include a database diagram.
---

# schema-embed

schema ships its Rust renderer as a WASM module that can be inlined into any HTML file.
The result works offline, from `file://`, and on static hosting.

## Option A — whole page (simplest)

```bash
schema html db/structure.sql -o schema.html --focus users --depth 1 --title "User model"
schema html db/structure.sql main...HEAD --changes-only -o schema-diff.html   # visual diff
schema html db/structure.sql --static -o schema.html   # no WASM: pre-rendered SVG with pan/zoom only (small)
```

All view flags from `schema --help` apply. The page contains a pre-rendered SVG (visible even
if scripts are blocked) that becomes interactive once the WASM loads.

## Option B — embed diagrams inside your own HTML

1. Write the bundle next to the HTML (or inline its contents in a `<script>` tag):
   ```bash
   schema embed -o schema.embed.js     # ~1 MB, defines window.Schema
   ```
2. Declarative embedding — put the SQL inside the element:
   ```html
   <script src="schema.embed.js"></script>
   <div data-schema data-title="Orders" style="height:520px"
        data-config='{"focus":["orders"],"focus_depth":1,"columns":"keys"}'>
     <script type="application/sql">
       CREATE TABLE users (id bigint PRIMARY KEY, email text NOT NULL);
       CREATE TABLE orders (id bigint PRIMARY KEY, user_id bigint NOT NULL REFERENCES users(id));
     </script>
   </div>
   ```
   Add `<script type="application/sql" data-base-sql>…old DDL…</script>` inside the same element to
   render a diff, or use `data-src="structure.sql"` / `data-base-src` to fetch files instead.
3. Programmatic embedding:
   ```js
   Schema.mount(document.getElementById("erd"), { sql, baseSql, config: { layout: { algorithm: "force" } }, title: "Schema" });
   const svg = await Schema.render(sql, { columns: "keys" });       // SVG string
   const md = await Schema.diffMarkdown(oldSql, newSql);            // Markdown diff
   ```

## Config reference (partial JSON is fine; everything has defaults)

`focus` [patterns], `focus_depth`, `focus_direction` (both|outgoing|incoming), `include`, `exclude`, `schemas`,
`changes_only`, `changes_context`, `columns` (auto|all|keys|relations|changed|none), `hide_columns` (e.g. `"*_at"`,
`"users.encrypted_*"`), `max_columns`, `show_types`, `show_defaults`, `indexes` (none|changed|all),
`show_views`, `show_partitions`, `show_isolated`,
`layout` {`algorithm` layered|force|grid|circular|radial, `direction` LR|TB|RL|BT, `node_spacing`, `rank_spacing`, `group_by` none|schema|prefix|custom},
`edges` {`style` curved|orthogonal|straight|hidden, `anchor` column|table, `labels`, `cardinality`, `inferred`},
`tables` {"users": {`columns`, `hide_columns`, `show_columns`, `collapsed`, `color`, `note`}},
`groups` [{`name`, `tables`, `color`}] (with group_by custom), `theme` light|dark, `title`.

## Tips

- Keep diagrams focused: 5–25 tables read best. Use `focus` + `columns: "keys"` for big schemas.
- For generated docs, prefer one diagram per topic rather than the whole schema.
- Inline SQL only needs the relevant `CREATE TABLE` / `ALTER TABLE ... FOREIGN KEY` statements.
