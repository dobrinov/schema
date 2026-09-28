//! A forgiving SQL tokenizer tuned for `pg_dump` output.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Unquoted identifier or keyword, lower-cased.
    Word,
    /// `"Quoted"` identifier, unescaped, case preserved.
    QuotedIdent,
    /// String literal (standard, escape or dollar-quoted), unescaped.
    Str,
    Number,
    Op,
    Punct,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: Kind,
    pub text: String,
    pub start: usize,
    pub end: usize,
}

impl Token {
    pub fn is_word(&self, w: &str) -> bool {
        self.kind == Kind::Word && self.text == w
    }
    pub fn is_punct(&self, c: char) -> bool {
        self.kind == Kind::Punct && self.text.len() == 1 && self.text.starts_with(c)
    }
    pub fn is_ident(&self) -> bool {
        matches!(self.kind, Kind::Word | Kind::QuotedIdent)
    }
}

fn is_ident_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}
fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80
}
fn is_op_char(b: u8) -> bool {
    b"+-*/<>=~!@#%^&|`?".contains(&b)
}

pub fn tokenize(src: &str) -> Vec<Token> {
    let b = src.as_bytes();
    let n = b.len();
    let mut i = 0;
    let mut out = Vec::new();
    let mut line_start = true;
    while i < n {
        let c = b[i];
        if c == b'\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        // psql meta-commands such as `\restrict` emitted by newer pg_dump.
        if c == b'\\' && line_start {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        line_start = false;
        // comments
        if c == b'-' && i + 1 < n && b[i + 1] == b'-' {
            while i < n && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && i + 1 < n && b[i + 1] == b'*' {
            let mut depth = 1;
            i += 2;
            while i < n && depth > 0 {
                if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if b[i] == b'*' && i + 1 < n && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        let start = i;
        // escape string E'...'
        if (c == b'e' || c == b'E') && i + 1 < n && b[i + 1] == b'\'' {
            let (text, end) = read_string(src, i + 1, true);
            out.push(Token { kind: Kind::Str, text, start, end });
            i = end;
            continue;
        }
        if c == b'\'' {
            let (text, end) = read_string(src, i, false);
            out.push(Token { kind: Kind::Str, text, start, end });
            i = end;
            continue;
        }
        if c == b'"' {
            let mut j = i + 1;
            let mut text = String::new();
            let mut seg = j;
            while j < n {
                if b[j] == b'"' {
                    if j + 1 < n && b[j + 1] == b'"' {
                        text.push_str(&src[seg..j + 1]);
                        j += 2;
                        seg = j;
                        continue;
                    }
                    break;
                }
                j += 1;
            }
            text.push_str(&src[seg..j.min(n)]);
            let end = (j + 1).min(n);
            out.push(Token { kind: Kind::QuotedIdent, text, start, end });
            i = end;
            continue;
        }
        if c == b'$' {
            // positional parameter
            if i + 1 < n && b[i + 1].is_ascii_digit() {
                let mut j = i + 1;
                while j < n && b[j].is_ascii_digit() {
                    j += 1;
                }
                out.push(Token { kind: Kind::Number, text: src[i..j].to_string(), start, end: j });
                i = j;
                continue;
            }
            // dollar quoting
            let mut j = i + 1;
            while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            if j < n && b[j] == b'$' {
                let tag = &src[i..=j];
                let body_start = j + 1;
                let (body_end, end) = match src[body_start..].find(tag) {
                    Some(p) => (body_start + p, body_start + p + tag.len()),
                    None => (n, n),
                };
                out.push(Token { kind: Kind::Str, text: src[body_start..body_end].to_string(), start, end });
                i = end;
                continue;
            }
            out.push(Token { kind: Kind::Punct, text: "$".into(), start, end: i + 1 });
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == b'.' && i + 1 < n && b[i + 1].is_ascii_digit()) {
            let mut j = i;
            while j < n && (b[j].is_ascii_digit() || b[j] == b'.' || b[j] == b'_') {
                // don't swallow `..`
                if b[j] == b'.' && j + 1 < n && b[j + 1] == b'.' {
                    break;
                }
                j += 1;
            }
            if j < n && (b[j] == b'e' || b[j] == b'E') {
                let mut k = j + 1;
                if k < n && (b[k] == b'+' || b[k] == b'-') {
                    k += 1;
                }
                if k < n && b[k].is_ascii_digit() {
                    j = k;
                    while j < n && b[j].is_ascii_digit() {
                        j += 1;
                    }
                }
            }
            out.push(Token { kind: Kind::Number, text: src[i..j].to_string(), start, end: j });
            i = j;
            continue;
        }
        if is_ident_start(c) {
            let mut j = i;
            while j < n && is_ident_char(b[j]) {
                j += 1;
            }
            out.push(Token { kind: Kind::Word, text: src[i..j].to_lowercase(), start, end: j });
            i = j;
            continue;
        }
        if c == b':' {
            if i + 1 < n && b[i + 1] == b':' {
                out.push(Token { kind: Kind::Op, text: "::".into(), start, end: i + 2 });
                i += 2;
            } else {
                out.push(Token { kind: Kind::Op, text: ":".into(), start, end: i + 1 });
                i += 1;
            }
            continue;
        }
        if is_op_char(c) {
            let mut j = i;
            while j < n && is_op_char(b[j]) {
                if j > i && ((b[j] == b'-' && j + 1 < n && b[j + 1] == b'-') || (b[j] == b'/' && j + 1 < n && b[j + 1] == b'*')) {
                    break;
                }
                j += 1;
            }
            out.push(Token { kind: Kind::Op, text: src[i..j].to_string(), start, end: j });
            i = j;
            continue;
        }
        // single-char punctuation; step over a whole UTF-8 char
        let ch = src[i..].chars().next().unwrap();
        let len = ch.len_utf8();
        out.push(Token { kind: Kind::Punct, text: ch.to_string(), start, end: i + len });
        i += len;
    }
    out
}

/// Read a single-quoted string starting at the opening quote. Returns the
/// unescaped content and the byte offset just past the closing quote.
fn read_string(src: &str, open: usize, backslash_escapes: bool) -> (String, usize) {
    let b = src.as_bytes();
    let n = b.len();
    let mut j = open + 1;
    let mut text = String::new();
    let mut seg = j;
    while j < n {
        if backslash_escapes && b[j] == b'\\' && j + 1 < n {
            text.push_str(&src[seg..j]);
            let e = b[j + 1];
            match e {
                b'n' => text.push('\n'),
                b't' => text.push('\t'),
                b'r' => text.push('\r'),
                _ => {
                    let ch = src[j + 1..].chars().next().unwrap();
                    text.push(ch);
                    j += ch.len_utf8() - 1;
                }
            }
            j += 2;
            seg = j;
            continue;
        }
        if b[j] == b'\'' {
            if j + 1 < n && b[j + 1] == b'\'' {
                text.push_str(&src[seg..j + 1]);
                j += 2;
                seg = j;
                continue;
            }
            break;
        }
        j += 1;
    }
    text.push_str(&src[seg..j.min(n)]);
    (text, (j + 1).min(n))
}

/// Collapse runs of whitespace outside of string literals.
pub fn normalize_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut pending_space = false;
    for ch in s.chars() {
        if in_str {
            out.push(ch);
            if ch == '\'' {
                in_str = false;
            }
            continue;
        }
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            let prev = out.chars().last().unwrap_or(' ');
            if !(prev == '(' || ch == ')' || ch == ',') {
                out.push(' ');
            }
            pending_space = false;
        }
        if ch == '\'' {
            in_str = true;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_dump_constructs() {
        let t = tokenize("CREATE TABLE \"Foo\"\"x\" (a int DEFAULT 'it''s'::text); -- hi\n$f$ body; $f$ E'a\\'b' $1 3.5e2 ::");
        let texts: Vec<_> = t.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, vec!["create", "table", "Foo\"x", "(", "a", "int", "default", "it's", "::", "text", ")", ";", " body; ", "a'b", "$1", "3.5e2", "::"]);
    }

    #[test]
    fn skips_meta_commands() {
        let t = tokenize("\\restrict abc\nSET x = 1;\n");
        assert_eq!(t[0].text, "set");
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_ws("  SELECT a,\n   b  FROM ( x )  WHERE s = '  a  '"), "SELECT a, b FROM (x) WHERE s = '  a  '");
    }
}
