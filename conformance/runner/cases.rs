//! Discovering conformance cases and what each one expects.

use crate::die;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct Case {
    pub(crate) name: String,
    pub(crate) src: PathBuf,
    pub(crate) expected_override: Option<PathBuf>,
    /// the exit status bun, `tr run` and the AOT binary must all end
    /// with; a case declares a non-zero one in its leading comment
    pub(crate) expected_exit: i32,
    /// the spec clause a case cites for ending with a status bun does
    /// not; without one, bun ending otherwise than declared is a failure
    pub(crate) bun_differs: Option<String>,
}

/// The marker a case writes in its leading `//` comment block to
/// declare a non-zero exit status, e.g. `// exit-code: 1`.
const EXIT_MARKER: &str = "exit-code:";

/// The marker a case writes in the same block when it ends on purpose
/// with a status bun does not, followed by the spec clause it rests on,
/// e.g. `// bun-differs: ECMA-262 §16.1.7 early errors`.
const BUN_DIFFERS_MARKER: &str = "bun-differs:";

pub(crate) fn collect_cases(dir: &Path) -> Vec<Case> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => die(&format!("read_dir {}: {e}", dir.display())),
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|x| x.path())).collect();
    paths.sort();
    let mut out = Vec::new();
    for p in paths {
        // A directory is one multi-file case named after it: bun and
        // tr both run `<dir>/main.ts`, which imports its siblings, and
        // its override lives at `<dir>/main.expected`.
        let (name, src) = if p.is_dir() {
            let main_ts = p.join("main.ts");
            if !main_ts.is_file() {
                continue;
            }
            (file_name(&p, |p| p.file_name()), main_ts)
        } else if p.extension().is_some_and(|e| e == "ts" || e == "cts") {
            // `.cts` rides the sloppy script goal on both sides
            (file_name(&p, |p| p.file_stem()), p)
        } else {
            continue;
        };
        let text = match std::fs::read_to_string(&src) {
            Ok(text) => text,
            Err(e) => die(&format!("read {}: {e}", src.display())),
        };
        let (expected_exit, bun_differs) = match (declared_exit(&text), declared_bun_differs(&text))
        {
            (Ok(code), Ok(clause)) => (code, clause),
            (Err(e), _) | (_, Err(e)) => die(&format!("{}: {e}", src.display())),
        };
        let expected = src.with_extension("expected");
        out.push(Case {
            name,
            expected_override: expected.is_file().then_some(expected),
            src,
            expected_exit,
            bun_differs,
        });
    }
    out
}

fn file_name(p: &Path, part: fn(&Path) -> Option<&std::ffi::OsStr>) -> String {
    part(p).and_then(|s| s.to_str()).unwrap_or("?").to_string()
}

/// The exit status declared in the case's leading comment block, 0
/// when it declares none. Only the block of `//` lines the file starts
/// with is read, so a program that merely prints the marker does not
/// declare anything; a marker that does not parse is an error rather
/// than a silent 0.
fn declared_exit(text: &str) -> Result<i32, String> {
    if let Some(rest) = leading_marker(text, EXIT_MARKER) {
        return match rest.trim().parse::<i32>() {
            Ok(code) if (0..=255).contains(&code) => Ok(code),
            _ => Err(format!(
                "`{EXIT_MARKER}` expects 0..=255, got `{}`",
                rest.trim()
            )),
        };
    }
    Ok(0)
}

/// The spec clause after `bun-differs:` in the leading comment block.
/// The clause must name the spec (`§`, `sec-` or `ECMA-262`): the
/// marker turns a disagreement with bun into a skip, and one that
/// cites nothing would hide a wrong declaration the same way a skip
/// on every mismatch did.
fn declared_bun_differs(text: &str) -> Result<Option<String>, String> {
    let Some(rest) = leading_marker(text, BUN_DIFFERS_MARKER) else {
        return Ok(None);
    };
    let clause = rest.trim();
    if ["§", "sec-", "ECMA-262"].iter().any(|k| clause.contains(k)) {
        Ok(Some(clause.to_string()))
    } else {
        Err(format!(
            "`{BUN_DIFFERS_MARKER}` must cite a spec clause (§, sec- or ECMA-262), got `{clause}`"
        ))
    }
}

/// The text after `marker` on a line of the `//` block the file starts
/// with, so a program that merely prints a marker declares nothing.
fn leading_marker<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let comment = line.strip_prefix("//")?;
        if let Some(rest) = comment.trim().strip_prefix(marker) {
            return Some(rest);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{declared_bun_differs, declared_exit};

    #[test]
    fn reads_the_marker_from_the_leading_comment_block() {
        let src =
            "// the reporter prints and the process ends with 1\n//\n// exit-code: 1\nthrow 1;\n";
        assert_eq!(declared_exit(src), Ok(1));
    }

    #[test]
    fn defaults_to_zero_and_ignores_markers_after_code() {
        assert_eq!(declared_exit("// a case\nconsole.log(1);\n"), Ok(0));
        assert_eq!(declared_exit("console.log(1);\n// exit-code: 3\n"), Ok(0));
        assert_eq!(declared_exit(""), Ok(0));
    }

    #[test]
    fn rejects_a_marker_that_does_not_parse() {
        assert!(declared_exit("// exit-code: one\n").is_err());
        assert!(declared_exit("// exit-code: 256\n").is_err());
    }

    #[test]
    fn a_bun_differs_marker_must_cite_the_spec() {
        let src = "// exit-code: 1\n// bun-differs: ECMA-262 §16.1.7\nthrow 1;\n";
        assert_eq!(
            declared_bun_differs(src),
            Ok(Some("ECMA-262 §16.1.7".to_string()))
        );
        assert_eq!(declared_bun_differs("// a case\n"), Ok(None));
        assert!(declared_bun_differs("// bun-differs: bun is wrong\n").is_err());
    }
}
