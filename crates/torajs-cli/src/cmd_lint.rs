//! `tr lint <file> [--deny]` — runs the 5 starter rules.
//!
//! T-06 (v0.3.0). Rules: unused-let / dead-code-after-return /
//! unreachable-catch / shadowed-let / unused-import. Default exit
//! 0 even with warnings; `--deny` makes any warning exit non-zero
//! (CI gate shape).

use std::process::ExitCode;

use crate::util::{byte_to_line_col, read_source};

pub(crate) fn run_lint(args: &[String]) -> ExitCode {
    let mut input: Option<&str> = None;
    let mut deny = false;
    for a in args {
        match a.as_str() {
            "--deny" | "-D" => deny = true,
            "--help" | "-h" => {
                println!("tr lint — surface lint warnings");
                println!();
                println!("USAGE: tr lint <file|-> [--deny]");
                println!();
                println!("Rules:");
                println!(
                    "  unused-let               top-level let / const declared but never read"
                );
                println!("  dead-code-after-return   stmt after return / throw / break / continue");
                println!("  unreachable-catch        catch on a try whose body cannot throw");
                println!(
                    "  shadowed-let             inner-scope let shadows enclosing-scope binding"
                );
                println!("  unused-import            import binding never referenced");
                println!();
                println!("  --deny, -D    exit non-zero if any warning is reported");
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
    match torajs_core::linter::lint(&src) {
        Ok(diags) => {
            if diags.is_empty() {
                return ExitCode::SUCCESS;
            }
            for d in &diags {
                let (line, col) = byte_to_line_col(&src, d.span.start);
                let severity = match d.severity {
                    torajs_core::check::Severity::Warning => "warning",
                    torajs_core::check::Severity::Error => "error",
                };
                eprintln!("{path}:{line}:{col}: {severity}: {}", d.message);
            }
            if deny {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(1)
        }
    }
}
