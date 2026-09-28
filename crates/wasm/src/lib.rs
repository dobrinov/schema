//! WebAssembly bindings. Everything crosses the boundary as JSON strings to
//! keep the JS glue tiny and dependency free.
use schema_core::{Session, ViewConfig};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Schema {
    inner: Session,
}

fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|e| format!("{{\"error\":{:?}}}", e.to_string()))
}

#[wasm_bindgen]
impl Schema {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Schema {
        Schema { inner: Session::new() }
    }

    /// Parse the current (compare) schema. Returns a JSON summary.
    pub fn set_sql(&mut self, sql: &str) -> String {
        to_json(&self.inner.set_sql(sql))
    }

    /// Parse the base schema to diff against; pass `undefined` to clear.
    pub fn set_base_sql(&mut self, sql: Option<String>) -> String {
        to_json(&self.inner.set_base_sql(sql.as_deref()))
    }

    /// Render with a (partial) JSON view config. Returns
    /// `{svg, width, height, nodes, edges, stats}` or `{error}`.
    pub fn view(&mut self, config_json: &str) -> String {
        match ViewConfig::from_json(config_json) {
            Ok(cfg) => to_json(&self.inner.view(cfg)),
            Err(e) => to_json(&serde_json::json!({ "error": e })),
        }
    }

    /// Re-route the edges of a node moved to (x, y). Returns `[{id, d}]`.
    pub fn move_node(&mut self, id: &str, x: f64, y: f64) -> String {
        to_json(&self.inner.move_node(id, x, y))
    }

    pub fn tables(&self) -> String {
        to_json(&self.inner.tables())
    }

    pub fn table(&self, id: &str) -> String {
        to_json(&self.inner.table(id))
    }

    pub fn diff(&self) -> String {
        to_json(&self.inner.diff_json())
    }

    pub fn diff_markdown(&self) -> String {
        self.inner.diff_markdown()
    }

    pub fn schema(&self) -> String {
        to_json(&self.inner.schema_json())
    }

    pub fn summary(&self) -> String {
        to_json(&self.inner.summary())
    }
}

impl Default for Schema {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
pub fn default_config() -> String {
    to_json(&ViewConfig::default())
}

#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
