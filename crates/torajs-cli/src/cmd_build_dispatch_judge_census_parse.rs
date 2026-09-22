//! Reading the runtime crates' Rust sources into a name-level call
//! graph: which functions a file defines, and which names each body
//! calls. Split out of [`super::cmd_build_dispatch_judge_census`] in
//! rotation 693 when that file reached the 500-line limit. The line
//! is: this file answers "what does the source say", the census
//! answers "what does that mean for the table".
//!
//! None of it is a Rust parser. It blanks out comments and literals
//! so their braces and identifiers do not count, then matches braces
//! and parens. That is enough for a call graph over names and wrong
//! in ways that only widen it (a macro body reads as calls), which is
//! the safe direction for what the census does with it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Blank out comments, string / byte-string / raw-string literals and
/// char literals, so braces and identifiers inside them don't count.
pub(crate) fn strip(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while i < b.len() {
        let prev_ident = i > 0 && ident(b[i - 1]);
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < b.len() {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if !prev_ident && raw_string_len(&b[i..]).is_some() {
            i += raw_string_len(&b[i..]).unwrap_or(1);
            out.extend_from_slice(b"\"\"");
        } else if b[i] == b'"' || (!prev_ident && b[i..].starts_with(b"b\"")) {
            i += if b[i] == b'b' { 2 } else { 1 };
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            out.extend_from_slice(b"\"\"");
        } else if b[i] == b'\'' {
            i += char_literal_len(&b[i..]).unwrap_or_else(|| {
                out.push(b'\'');
                1
            });
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Length of a raw (byte) string literal starting at `b`, if any.
fn raw_string_len(b: &[u8]) -> Option<usize> {
    let mut i = usize::from(b.first() == Some(&b'b'));
    if b.get(i) != Some(&b'r') {
        return None;
    }
    i += 1;
    let hashes = b[i..].iter().take_while(|&&c| c == b'#').count();
    i += hashes;
    if b.get(i) != Some(&b'"') {
        return None;
    }
    let close: Vec<u8> = std::iter::once(b'"')
        .chain(std::iter::repeat_n(b'#', hashes))
        .collect();
    let body = &b[i + 1..];
    let end = body
        .windows(close.len())
        .position(|w| w == close.as_slice())?;
    Some(i + 1 + end + close.len())
}

/// Length of a char literal at `b` (`'x'`, `'\n'`, `'é'`), or `None`
/// for a lifetime / label.
fn char_literal_len(b: &[u8]) -> Option<usize> {
    if b.get(1) == Some(&b'\\') {
        let end = b[2..].iter().position(|&c| c == b'\'')?;
        return Some(end + 3);
    }
    let width = match b.get(1)? {
        c if *c < 0x80 => 1,
        c if *c >= 0xF0 => 4,
        c if *c >= 0xE0 => 3,
        _ => 2,
    };
    (b.get(1 + width) == Some(&b'\'')).then_some(width + 2)
}

/// Cut every `#[cfg(test)] mod name { … }` block.
pub(crate) fn drop_test_mods(mut s: String) -> String {
    while let Some(at) = s.find("#[cfg(test)]") {
        let rest = &s[at + "#[cfg(test)]".len()..];
        let head = rest.trim_start();
        let head = head
            .strip_prefix("pub(crate)")
            .or_else(|| head.strip_prefix("pub"))
            .unwrap_or(head)
            .trim_start();
        let Some(open_rel) = head
            .strip_prefix("mod ")
            .and_then(|m| m.find(['{', ';']).filter(|&p| m.as_bytes()[p] == b'{'))
        else {
            // a cfg(test) fn / use — keep it (a test twin of a real fn
            // only widens the census), but step past the attribute.
            s.replace_range(at..at + "#[cfg(test)]".len(), "");
            continue;
        };
        let open = s.len() - head.len() + "mod ".len() + open_rel;
        let close = matching_brace(s.as_bytes(), open);
        s.replace_range(at..close, "");
    }
    s
}

/// Index one past the brace matching the `{` at `open`.
fn matching_brace(b: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    b.len()
}

/// Every `fn name(…) … { body }` in a stripped source: `(name, body)`.
pub(crate) fn fns(s: &str) -> Vec<(String, String)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = s[from..].find("fn ") {
        let at = from + rel;
        from = at + 3;
        if at > 0 && (b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_') {
            continue;
        }
        let name: String = s[from..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let Some(paren) = s[from..].find('(').map(|p| from + p) else {
            break;
        };
        let after = close_paren(b, paren);
        let Some(open) = s[after..].find(['{', ';']).map(|p| after + p) else {
            break;
        };
        if name.is_empty() || b[open] == b';' {
            continue;
        }
        let close = matching_brace(b, open);
        out.push((name, s[open..close].to_string()));
    }
    out
}

/// Index one past the `)` matching the `(` at `open`.
fn close_paren(b: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    b.len()
}

/// The names a body CALLS as free / path functions: an identifier not
/// preceded by `.` (a method on a std receiver would resolve by name to
/// an unrelated crate fn) and followed by `(` or a turbofish.
pub(crate) fn calls(body: &str) -> HashSet<&str> {
    let b = body.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = HashSet::new();
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_alphabetic() || b[i] == b'_') || (i > 0 && ident(b[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && ident(b[i]) {
            i += 1;
        }
        if start > 0 && b[start - 1] == b'.' {
            continue;
        }
        let rest = body[i..].trim_start();
        let rest = match rest.strip_prefix("::<") {
            Some(r) => r.find('>').map_or(r, |p| r[p + 1..].trim_start()),
            None => rest,
        };
        if rest.starts_with('(') {
            out.insert(&body[start..i]);
        }
    }
    out
}

pub(crate) fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).expect("crate src dir is readable") {
        let p = e.expect("dir entry is readable").path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Read as bytes: a stray NUL must not hide a file (rotation 678).
pub(crate) fn read_lossy(p: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(p).expect("source is readable")).into_owned()
}
