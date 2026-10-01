//! schema-core: parse Postgres `structure.sql` (or Rails `schema.rb`), diff schemas, lay out and
//! render ER diagrams as SVG. Shared by the WASM module and the CLI.
pub mod config;
pub mod design;
pub mod diff;
pub mod glob;
pub mod graph;
pub mod layout;
pub mod lexer;
pub mod model;
pub mod ortho;
pub mod parser;
pub mod rails;
pub mod render;
pub mod route;
pub mod session;

pub use config::ViewConfig;
pub use diff::{diff, SchemaDiff, Status};
pub use model::Schema;
pub use parser::parse;
pub use session::Session;

/// One-shot helper: SQL (+ optional base SQL) + config → SVG string.
pub fn render_sql(sql: &str, base_sql: Option<&str>, cfg: &ViewConfig) -> String {
    let mut s = Session::new();
    s.set_sql(sql);
    if base_sql.is_some() {
        s.set_base_sql(base_sql);
    }
    s.view(cfg.clone()).svg
}
