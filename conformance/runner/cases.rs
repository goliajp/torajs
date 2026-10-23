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
}

/// The marker a case writes in its leading `//` comment block to
/// declare a non-zero exit status, e.g. `// exit-code: 1`.
const EXIT_MARKER: &str = "exit-code:";

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
        let expected_exit = match std::fs::read_to_string(&src) {
            Ok(text) => match declared_exit(&text) {
                Ok(code) => code,
                Err(e) => die(&format!("{}: {e}", src.display())),
            },
            Err(e) => die(&format!("read {}: {e}", src.display())),
        };
        let expected = src.with_extension("expected");
        out.push(Case {
            name,
            expected_override: expected.is_file().then_some(expected),
            src,
            expected_exit,
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
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(comment) = line.strip_prefix("//") else {
            break;
        };
        if let Some(rest) = comment.trim().strip_prefix(EXIT_MARKER) {
            return match rest.trim().parse::<i32>() {
                Ok(code) if (0..=255).contains(&code) => Ok(code),
                _ => Err(format!(
                    "`{EXIT_MARKER}` expects 0..=255, got `{}`",
                    rest.trim()
                )),
            };
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::declared_exit;

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
}
