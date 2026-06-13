//! Per-case bug-corpus dump for root-cause clustering.
//!
//! The runner's stdout `--report-bugs N` only shows the head of the bug
//! list; the summary `--json` only carries bucket counts. Neither is
//! enough to systematically classify the bug corpus by root-cause
//! substrate gap. `--bugs-ndjson PATH` writes EVERY bug case as one JSON
//! object per line so a downstream cluster pass can group by `kind`
//! (exit code — 138/139 = silent crash, 1 = loud fail, stdout-mismatch)
//! and `msg` (stderr first line — the substrate-gap signal).
//!
//! Zero-dep, like the rest of the runner: JSON is hand-escaped.

use std::path::{Path, PathBuf};

/// Escape a string for inclusion inside a JSON string literal. Handles
/// the structural characters plus all C0 control codes (the stderr
/// `msg` lines can carry tabs / embedded newlines).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Write the full bug list to `out_path` as ndjson. Paths are made
/// repo-relative against `root` (the test262 corpus root) so the
/// feature directory is the leading path component.
pub fn write_bugs_ndjson(
    out_path: &Path,
    root: &Path,
    bugs: &[(PathBuf, String, String)],
) -> Result<(), String> {
    let mut body = String::new();
    for (p, kind, msg) in bugs {
        let rel = p.strip_prefix(root).unwrap_or(p);
        body.push_str(&format!(
            "{{\"path\":\"{}\",\"kind\":\"{}\",\"msg\":\"{}\"}}\n",
            json_escape(&rel.display().to_string()),
            json_escape(kind),
            json_escape(msg),
        ));
    }
    std::fs::write(out_path, body).map_err(|e| format!("write {}: {e}", out_path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_structural_and_control_chars() {
        assert_eq!(json_escape("a\"b\\c"), "a\\\"b\\\\c");
        assert_eq!(json_escape("line1\nline2\t!"), "line1\\nline2\\t!");
        assert_eq!(json_escape("\u{0007}"), "\\u0007");
    }

    #[test]
    fn ndjson_line_per_bug_relative_paths() {
        let root = Path::new("vendor/test262");
        let bugs = vec![
            (
                PathBuf::from("vendor/test262/test/built-ins/Array/fill/x.js"),
                "exit 139".to_string(),
                "(no stderr)".to_string(),
            ),
            (
                PathBuf::from("vendor/test262/test/annexB/RegExp/y.js"),
                "exit 1".to_string(),
                "Test262Error: nope".to_string(),
            ),
        ];
        let tmp = std::env::temp_dir().join("torajs-bugdump-test.ndjson");
        write_bugs_ndjson(&tmp, root, &bugs).unwrap();
        let got = std::fs::read_to_string(&tmp).unwrap();
        let lines: Vec<&str> = got.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            "{\"path\":\"test/built-ins/Array/fill/x.js\",\"kind\":\"exit 139\",\"msg\":\"(no stderr)\"}"
        );
        assert_eq!(
            lines[1],
            "{\"path\":\"test/annexB/RegExp/y.js\",\"kind\":\"exit 1\",\"msg\":\"Test262Error: nope\"}"
        );
        let _ = std::fs::remove_file(&tmp);
    }
}
