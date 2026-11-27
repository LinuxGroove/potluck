//! Reading Luau on the web: a small tokenizer for colours (and for finding
//! the strings players see), and unified diffs between versions.

use maud::{html, Markup};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Keyword,
    Str,
    Comment,
    Number,
    Name,
    Call,
    Other,
}

pub struct Token<'a> {
    pub kind: Kind,
    pub text: &'a str,
}

const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while", "continue", "export", "type", "typeof",
];

/// `[[`, `[=[` and so on: the length of the opening, and its level.
fn long_open(b: &[u8], i: usize) -> Option<(usize, usize)> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then_some((j + 1 - i, j - i - 1))
}

fn long_close(s: &str, from: usize, level: usize) -> usize {
    let close = format!("]{}]", "=".repeat(level));
    s[from..].find(&close).map(|k| from + k + close.len()).unwrap_or(s.len())
}

pub fn tokens(s: &str) -> Vec<Token<'_>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut last_word = "";
    while i < b.len() {
        let start = i;
        let c = b[i];
        let kind;
        if c == b'-' && b.get(i + 1) == Some(&b'-') {
            kind = Kind::Comment;
            if let Some((open, level)) = long_open(b, i + 2) {
                i = long_close(s, i + 2 + open, level);
            } else {
                i = s[i..].find('\n').map(|k| i + k).unwrap_or(b.len());
            }
        } else if let Some((open, level)) = long_open(b, i) {
            kind = Kind::Str;
            i = long_close(s, i + open, level);
        } else if c == b'"' || c == b'\'' || c == b'`' {
            kind = Kind::Str;
            i += 1;
            while i < b.len() && b[i] != c && b[i] != b'\n' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(b.len());
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            kind = Kind::Number;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let w = &s[start..i];
            kind = if KEYWORDS.contains(&w) {
                Kind::Keyword
            } else if last_word == "function" || s[i..].trim_start().starts_with('(') {
                Kind::Call
            } else {
                Kind::Name
            };
            last_word = w;
            out.push(Token { kind, text: w });
            continue;
        } else {
            kind = Kind::Other;
            i += s[i..].chars().next().map(|ch| ch.len_utf8()).unwrap_or(1);
            if !c.is_ascii_whitespace() && c != b':' && c != b'.' {
                last_word = "";
            }
        }
        // keep a token on whole characters even if the code is cut short
        while i < b.len() && !s.is_char_boundary(i) {
            i += 1;
        }
        out.push(Token { kind, text: &s[start..i.min(b.len())] });
    }
    out
}

fn class(k: Kind) -> &'static str {
    match k {
        Kind::Keyword => "k",
        Kind::Str => "s",
        Kind::Comment => "c",
        Kind::Number => "n",
        Kind::Call => "f",
        Kind::Name | Kind::Other => "",
    }
}

/// A file as numbered, coloured lines.
pub fn code(path: &str, text: &str) -> Markup {
    let lua = path.ends_with(".luau") || path.ends_with(".lua");
    // Split tokens into lines so every line is its own row.
    let mut lines: Vec<Vec<(Kind, String)>> = vec![Vec::new()];
    let toks: Vec<(Kind, &str)> = if lua { tokens(text).into_iter().map(|t| (t.kind, t.text)).collect() } else { vec![(Kind::Other, text)] };
    for (k, t) in toks {
        let mut parts = t.split('\n');
        if let Some(first) = parts.next() {
            if !first.is_empty() {
                lines.last_mut().unwrap().push((k, first.to_string()));
            }
        }
        for p in parts {
            lines.push(Vec::new());
            if !p.is_empty() {
                lines.last_mut().unwrap().push((k, p.to_string()));
            }
        }
    }
    if lines.len() > 1 && lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    html! {
        table.code {
            @for (n, line) in lines.iter().enumerate() {
                tr id={ "L" (n + 1) } {
                    td.ln { (n + 1) }
                    td.tx {
                        @for (k, t) in line {
                            @let c = class(*k);
                            @if c.is_empty() { (t) } @else { span class=(c) { (t) } }
                        }
                    }
                }
            }
        }
    }
}

/// One file's change between two versions, as a unified diff.
pub fn diff_text(path: &str, old: &str, new: &str) -> String {
    let d = similar::TextDiff::from_lines(old, new);
    d.unified_diff().context_radius(3).header(&format!("a/{path}"), &format!("b/{path}")).to_string()
}

/// Lines added and taken away.
pub fn diff_counts(old: &str, new: &str) -> (usize, usize) {
    let d = similar::TextDiff::from_lines(old, new);
    let mut add = 0;
    let mut del = 0;
    for c in d.iter_all_changes() {
        match c.tag() {
            similar::ChangeTag::Insert => add += 1,
            similar::ChangeTag::Delete => del += 1,
            similar::ChangeTag::Equal => {}
        }
    }
    (add, del)
}

/// A unified diff (one or many files) drawn with added and removed lines.
pub fn diff(text: &str) -> Markup {
    html! {
        pre.diff {
            @for line in text.lines() {
                @let c = if line.starts_with("+++") || line.starts_with("---") || line.starts_with("diff ") || line.starts_with("index ") { "h" }
                    else if line.starts_with("@@") { "at" }
                    else if line.starts_with('+') { "add" }
                    else if line.starts_with('-') { "del" }
                    else { "" };
                span class=(c) { (line) "\n" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_cover_luau() {
        let src = "local x = \"a\" -- note\n--[[ long\ncomment ]] function Mossback:tick(dt) return [[s]] .. `t{1}` end";
        let t = tokens(src);
        let joined: String = t.iter().map(|t| t.text).collect();
        assert_eq!(joined, src);
        let kinds: Vec<(Kind, &str)> = t.iter().filter(|t| t.kind != Kind::Other).map(|t| (t.kind, t.text)).collect();
        assert!(kinds.contains(&(Kind::Keyword, "local")));
        assert!(kinds.contains(&(Kind::Str, "\"a\"")));
        assert!(kinds.contains(&(Kind::Comment, "-- note")));
        assert!(kinds.contains(&(Kind::Comment, "--[[ long\ncomment ]]")));
        assert!(kinds.contains(&(Kind::Call, "tick")));
        assert!(kinds.contains(&(Kind::Str, "[[s]]")));
        assert!(kinds.contains(&(Kind::Str, "`t{1}`")));
    }

    #[test]
    fn code_escapes_html() {
        let m = code("main.luau", "print(\"<script>alert(1)</script>\")\n").into_string();
        assert!(!m.contains("<script>"));
        assert!(m.contains("&lt;script&gt;"));
    }

    #[test]
    fn diffs() {
        let t = diff_text("main.luau", "a\nb\nc\n", "a\nB\nc\nd\n");
        assert!(t.contains("-b") && t.contains("+B") && t.contains("+d"), "{t}");
        assert_eq!(diff_counts("a\nb\n", "a\nc\nd\n"), (2, 1));
    }
}
