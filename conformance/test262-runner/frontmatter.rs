//! test262 frontmatter parsing — the `/*--- ... ---*/` YAML block.
//!
//! Zero-dep line scanner (no YAML library, per the in-house pillar):
//! extracts exactly the keys the runner's verdict logic needs —
//! `negative:` (phase + type) and `includes:`. Everything else in
//! the block is ignored.
//!
//! Why this exists (takagi 2026-06-13): the runner previously used
//! bun as the only oracle and discarded every case bun itself failed
//! (`bun-skip` — 24,860 of 53,174 cases at the 2026-06-10 baseline).
//! But a test262 case carries its own pass semantics: `negative:`
//! cases EXPECT an error of a given phase, and positive cases
//! self-validate through the assert harness (exit 0 = pass). Parsing
//! the frontmatter lets the runner judge those cases directly instead
//! of hiding behind the oracle.

/// Parsed subset of a test262 case's frontmatter.
#[derive(Debug, Default, Clone)]
pub struct Frontmatter {
    /// `negative.phase` — `parse` / `early` / `resolution` / `runtime`.
    pub negative_phase: Option<String>,
    /// `negative.type` — expected error constructor name (recorded
    /// for audit; the verdict only phase-matches today).
    pub negative_type: Option<String>,
    /// `includes:` harness files beyond the implicit assert.js +
    /// sta.js (which the typed harness already covers).
    pub includes: Vec<String>,
    /// `flags:` list — `async` drives the `$DONE` completion-marker
    /// judgment (an async case exiting 0 without printing
    /// `Test262:AsyncTestComplete` is NOT a pass).
    pub flags: Vec<String>,
}

impl Frontmatter {
    pub fn is_async(&self) -> bool {
        self.flags.iter().any(|f| f == "async")
    }

    /// RFC 20260810-sloppy-goal-arguments S1 — `flags: [noStrict]`
    /// cases run under the sloppy goal only (official test262 host
    /// convention). The runner stages them as `case.cts`, which maps
    /// to the CommonJS sloppy goal for bun and tr alike.
    pub fn is_no_strict(&self) -> bool {
        self.flags.iter().any(|f| f == "noStrict")
    }
}

/// Parse the `/*--- ... ---*/` block out of `src`. Missing block or
/// missing keys → defaults (not an error: plenty of cases have no
/// frontmatter keys we care about).
pub fn parse(src: &str) -> Frontmatter {
    let mut fm = Frontmatter::default();
    let Some(start) = src.find("/*---") else {
        return fm;
    };
    let Some(end_rel) = src[start..].find("---*/") else {
        return fm;
    };
    let block = &src[start + 5..start + end_rel];

    let mut in_negative = false;
    let mut in_includes_list = false;
    let mut in_flags_list = false;
    for raw in block.lines() {
        let line = raw.trim_end();
        let trimmed = line.trim_start();
        let indented = line.len() > trimmed.len();

        // Top-level key closes any open sub-block.
        if !indented && !trimmed.is_empty() && !trimmed.starts_with('-') {
            in_negative = false;
            in_includes_list = false;
            in_flags_list = false;
        }

        if let Some(rest) = trimmed.strip_prefix("negative:") {
            // Inline form `negative: SyntaxError` (legacy) — phase
            // defaults to parse in old-style frontmatter.
            let inline = rest.trim();
            if !inline.is_empty() {
                fm.negative_type = Some(inline.to_string());
                fm.negative_phase = Some("parse".to_string());
            } else {
                in_negative = true;
            }
            continue;
        }
        if in_negative && indented {
            if let Some(v) = trimmed.strip_prefix("phase:") {
                fm.negative_phase = Some(v.trim().to_string());
                continue;
            }
            if let Some(v) = trimmed.strip_prefix("type:") {
                fm.negative_type = Some(v.trim().to_string());
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("includes:") {
            let inline = rest.trim();
            if let Some(list) = inline.strip_prefix('[') {
                // Inline form `includes: [a.js, b.js]`.
                let list = list.strip_suffix(']').unwrap_or(list);
                for item in list.split(',') {
                    push_include(&mut fm.includes, item);
                }
            } else if inline.is_empty() {
                in_includes_list = true;
            }
            continue;
        }
        if in_includes_list && indented {
            if let Some(item) = trimmed.strip_prefix('-') {
                push_include(&mut fm.includes, item);
                continue;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("flags:") {
            let inline = rest.trim();
            if let Some(list) = inline.strip_prefix('[') {
                // Inline form `flags: [generated, async]`.
                let list = list.strip_suffix(']').unwrap_or(list);
                for item in list.split(',') {
                    push_flag(&mut fm.flags, item);
                }
            } else if inline.is_empty() {
                in_flags_list = true;
            }
            continue;
        }
        if in_flags_list
            && indented
            && let Some(item) = trimmed.strip_prefix('-')
        {
            push_flag(&mut fm.flags, item);
            continue;
        }
    }
    fm
}

fn push_flag(out: &mut Vec<String>, raw: &str) {
    let name = raw.trim().trim_matches('"').trim_matches('\'');
    if !name.is_empty() {
        out.push(name.to_string());
    }
}

/// The typed harness already provides assert.js + sta.js equivalents,
/// so only record includes beyond those two.
fn push_include(out: &mut Vec<String>, raw: &str) {
    let name = raw.trim().trim_matches('"').trim_matches('\'');
    if name.is_empty() || name == "assert.js" || name == "sta.js" {
        return;
    }
    out.push(name.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_block_and_includes_list() {
        let src = r#"// preamble
/*---
esid: sec-foo
description: >
  something
negative:
  phase: parse
  type: SyntaxError
includes:
  - propertyHelper.js
  - assert.js
flags: [onlyStrict]
---*/
var x;"#;
        let fm = parse(src);
        assert_eq!(fm.negative_phase.as_deref(), Some("parse"));
        assert_eq!(fm.negative_type.as_deref(), Some("SyntaxError"));
        assert_eq!(fm.includes, vec!["propertyHelper.js".to_string()]);
    }

    #[test]
    fn inline_includes_and_no_negative() {
        let src = "/*---\nincludes: [compareArray.js, sta.js]\n---*/";
        let fm = parse(src);
        assert!(fm.negative_phase.is_none());
        assert_eq!(fm.includes, vec!["compareArray.js".to_string()]);
    }

    #[test]
    fn runtime_negative() {
        let src = "/*---\nnegative:\n  phase: runtime\n  type: TypeError\n---*/";
        let fm = parse(src);
        assert_eq!(fm.negative_phase.as_deref(), Some("runtime"));
        assert_eq!(fm.negative_type.as_deref(), Some("TypeError"));
    }

    #[test]
    fn no_frontmatter() {
        let fm = parse("var x = 1;");
        assert!(fm.negative_phase.is_none());
        assert!(fm.includes.is_empty());
        assert!(!fm.is_async());
    }

    #[test]
    fn inline_flags_async() {
        let fm = parse("/*---\nflags: [generated, async]\n---*/");
        assert_eq!(fm.flags, vec!["generated".to_string(), "async".to_string()]);
        assert!(fm.is_async());
    }

    #[test]
    fn block_flags_list() {
        let fm = parse("/*---\nflags:\n  - noStrict\n  - async\ndescription: x\n---*/");
        assert_eq!(fm.flags, vec!["noStrict".to_string(), "async".to_string()]);
        assert!(fm.is_async());
    }

    #[test]
    fn non_async_flags() {
        let fm = parse("/*---\nflags: [onlyStrict]\n---*/");
        assert!(!fm.is_async());
    }
}
