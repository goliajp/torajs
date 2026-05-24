//! `tr fmt <file> [--write]` — deterministic source reformatter.
//!
//! T-05 (v0.3.0). Reads the source, walks it through
//! `torajs_core::formatter::format`, emits to stdout (default) or
//! rewrites the file in place (`--write`). Comment-bearing source
//! is rejected with a clear "v0.4 follow-up" message — no silent
//! comment loss.

use std::process::ExitCode;

use crate::util::read_source;

pub(crate) fn run_fmt(args: &[String]) -> ExitCode {
    let mut input: Option<&str> = None;
    let mut write_in_place = false;
    for a in args {
        match a.as_str() {
            "--write" | "-w" => write_in_place = true,
            "--help" | "-h" => {
                println!("tr fmt — deterministic source reformatter");
                println!();
                println!("USAGE: tr fmt <file|-> [--write]");
                println!();
                println!("  --write, -w   rewrite the file in place (default: stdout)");
                println!();
                println!("Style: 2-space indent, single quotes, no trailing semicolons.");
                println!(
                    "Comment-bearing source is rejected (comment-aware fmt is a v0.4 follow-up)."
                );
                return ExitCode::SUCCESS;
            }
            other if input.is_none() && !other.starts_with("--") => {
                input = Some(other);
            }
            other => {
                eprintln!("error: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(path) = input else {
        eprintln!("error: missing file argument (use `-` for stdin)");
        return ExitCode::from(2);
    };
    let src = match read_source(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    match torajs_core::formatter::format(&src) {
        Ok(out) => {
            if write_in_place {
                if path == "-" {
                    eprintln!("error: --write incompatible with stdin input");
                    return ExitCode::from(2);
                }
                if let Err(e) = std::fs::write(path, &out) {
                    eprintln!("error: writing {path}: {e}");
                    return ExitCode::from(1);
                }
            } else {
                print!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
