//! Tiny case-insensitive glob matcher (`*` and `?`).

pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Match a table pattern against a qualified id (`schema.table`). Patterns
/// without a dot match the bare table name in any schema.
pub fn table_matches(pattern: &str, id: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern.contains('.') {
        glob_match(pattern, id)
    } else {
        let name = crate::model::split_id(id).1;
        glob_match(pattern, name)
    }
}

pub fn any_table_matches(patterns: &[String], id: &str) -> bool {
    patterns.iter().any(|p| table_matches(p, id))
}

/// Match a column pattern: `col`, `*_at`, or `table.col` / `schema.table.col`.
pub fn column_matches(pattern: &str, table_id: &str, column: &str) -> bool {
    let pattern = pattern.trim();
    match pattern.rfind('.') {
        Some(i) => table_matches(&pattern[..i], table_id) && glob_match(&pattern[i + 1..], column),
        None => glob_match(pattern, column),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("user*", "users"));
        assert!(glob_match("*_at", "created_at"));
        assert!(!glob_match("*_at", "created_by"));
        assert!(glob_match("a?c", "ABC"));
        assert!(table_matches("users", "public.users"));
        assert!(table_matches("billing.*", "billing.invoices"));
        assert!(!table_matches("billing.*", "public.invoices"));
        assert!(column_matches("users.encrypted_*", "public.users", "encrypted_password"));
        assert!(!column_matches("posts.encrypted_*", "public.users", "encrypted_password"));
    }
}
