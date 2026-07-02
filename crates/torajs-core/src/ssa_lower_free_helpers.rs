//! Free helpers extracted from `ssa_lower.rs` chunk 396 —
//! Path A.3-batch17.
//!
//! Module-level pure helpers (not on `LowerCtx`, no SSA context)
//! that operate on raw source strings:
//!
//! - `count_capture_groups(pattern)` — count regex capture groups
//!   in a raw pattern string per ES §22.2.1, used by the
//!   `s.replace(re, fn)` dispatch to size the callback's arity.
//! - `decode_env_ann(ann)` — decode the `__env(name1|name2|...)`
//!   annotation that `lift_arrow_fns` puts on a capturing
//!   closure's hidden first param.
//!
//! Re-exported from `ssa_lower` as `pub(crate) use` for
//! backward-compatible call sites.

/// P9.5-A1.1 — count capture groups in a regex literal pattern. Used at
/// ssa-lower time by the `s.replace(re, fn)` dispatch to determine the
/// callback's expected arity (one match arg + N capture args). Mirrors
/// the runtime parser's group counter but operates on the raw source-
/// level pattern string (Expr::Regex.pattern) before tora's regex
/// compiler runs.
///
/// Counting rules per ES spec §22.2.1:
///   - `(` opens a capture group → +1
///   - `(?:` `(?=` `(?!` `(?<=` `(?<!` open non-capturing constructs → 0
///   - `(?<name>` is a named capture → +1 (rule for `<` not followed by `=`/`!`)
///   - `\(` is a literal paren → 0
///   - `[...]` char class: parens inside don't count
///   - `\\` followed by any char escapes that char
pub(crate) fn count_capture_groups(pattern: &str) -> usize {
    let bytes = pattern.as_bytes();
    let mut n = 0usize;
    let mut in_class = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            i += 2;
            continue;
        }
        if in_class {
            if b == b']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if b == b'[' {
            in_class = true;
            i += 1;
            continue;
        }
        if b == b'(' {
            // (?:, (?=, (?!, (?<=, (?<! → non-capturing
            // (?<name> → capturing named group
            if i + 2 < bytes.len() && bytes[i + 1] == b'?' {
                let c = bytes[i + 2];
                if c == b':' || c == b'=' || c == b'!' {
                    i += 3;
                    continue;
                }
                if c == b'<' && i + 3 < bytes.len() {
                    let d = bytes[i + 3];
                    if d == b'=' || d == b'!' {
                        i += 4;
                        continue;
                    }
                    // (?<name>... — capturing named group, fall through to +1
                }
            }
            n += 1;
        }
        i += 1;
    }
    n
}

/// Decode the `__env(name1|name2|...)` annotation lift_arrow_fns put on
/// a capturing closure's hidden first param. Returns the ordered capture
/// names. Returns `None` for anything that doesn't match the form.
pub(crate) fn decode_env_ann(ann: &str) -> Option<Vec<String>> {
    let inner = ann.strip_prefix("__env(")?.strip_suffix(')')?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    Some(inner.split('|').map(|s| s.to_string()).collect())
}

#[cfg(test)]
mod count_capture_groups_tests {
    use super::count_capture_groups;
    #[test]
    fn plain() {
        assert_eq!(count_capture_groups("foo"), 0);
        assert_eq!(count_capture_groups(""), 0);
    }
    #[test]
    fn one_group() {
        assert_eq!(count_capture_groups("(a)"), 1);
    }
    #[test]
    fn nested_groups() {
        assert_eq!(count_capture_groups("(a(b))"), 2);
        assert_eq!(count_capture_groups("((a))"), 2);
    }
    #[test]
    fn non_capturing() {
        assert_eq!(count_capture_groups("(?:a)"), 0);
        assert_eq!(count_capture_groups("(?=a)b"), 0);
        assert_eq!(count_capture_groups("(?!a)b"), 0);
        assert_eq!(count_capture_groups("(?<=a)b"), 0);
        assert_eq!(count_capture_groups("(?<!a)b"), 0);
    }
    #[test]
    fn named_capture() {
        assert_eq!(count_capture_groups("(?<n>a)"), 1);
        assert_eq!(count_capture_groups("(?<first>\\w+) (?<last>\\w+)"), 2);
    }
    #[test]
    fn mixed() {
        assert_eq!(count_capture_groups("(a)(?:b)(c)"), 2);
        assert_eq!(count_capture_groups("(\\w+) (\\w+)"), 2);
        assert_eq!(count_capture_groups("(a)(b)(c)"), 3);
    }
    #[test]
    fn char_class_parens() {
        assert_eq!(count_capture_groups("[(]"), 0);
        assert_eq!(count_capture_groups("[(ab)]"), 0);
        assert_eq!(count_capture_groups("([(a)])"), 1);
    }
    #[test]
    fn escaped_parens() {
        assert_eq!(count_capture_groups("\\("), 0);
        assert_eq!(count_capture_groups("\\(a\\)"), 0);
        assert_eq!(count_capture_groups("(a)\\("), 1);
    }
    #[test]
    fn complex() {
        // The common bun idiom: (\w+) (\w+) — 2 groups.
        assert_eq!(count_capture_groups("(\\w+) (\\w+)"), 2);
        // Mix: 1 named + 1 positional + 1 non-capturing.
        assert_eq!(count_capture_groups("(?<key>\\w+)(?:=)(\\w+)"), 2);
    }
}
