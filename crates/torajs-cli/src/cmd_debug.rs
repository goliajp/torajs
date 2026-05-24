//! `tr debug <file>` — compile + drop into lldb.
//!
//! V3-15 — V3-16 era. Compiles the input to a debug-info-rich
//! binary (DWARF) and execs lldb on it. The DWARF emission shipped
//! with v0.3 #4 already maps every IR instruction back to a
//! source-byte span (`util::byte_to_line_col`), so source-level
//! breakpoints + stepping + frame-variable work out of the box.
//! Any installed `lldb` (Apple's macOS, llvm.org's tarball, distro
//! package) is fine — we don't depend on a specific version.
//!
//! VS Code DAP integration is a separate follow-up: the planned
//! shape is `tr debug --dap` that speaks DAP over stdio + a thin
//! VS Code extension that launches it. Shipping the CLI-driven
//! path first lets users debug today without waiting for the
//! marketplace publish.

use std::process::ExitCode;

use crate::util::rand_suffix;

pub(crate) fn run_debug(args: &[String]) -> ExitCode {
    let mut input: Option<&str> = None;
    let mut extra: Vec<&String> = Vec::new();
    let mut after_dashdash = false;
    for a in args {
        if after_dashdash {
            extra.push(a);
            continue;
        }
        match a.as_str() {
            "--help" | "-h" => {
                println!("tr debug — compile with DWARF + launch lldb");
                println!();
                println!("USAGE: tr debug <file> [-- <lldb-args>]");
                println!();
                println!("Examples:");
                println!("  tr debug demo.ts                       # interactive lldb shell");
                println!("  tr debug demo.ts -- -o 'b _main' -o run -o 'frame variable'");
                println!();
                println!("Source-level breakpoints + step + frame-variable work via the");
                println!("v0.3 #4 DWARF emission. Pass any extra lldb args after `--`.");
                return ExitCode::SUCCESS;
            }
            "--" => after_dashdash = true,
            other if input.is_none() && !other.starts_with("--") => {
                input = Some(other);
            }
            other => {
                eprintln!("error: unexpected argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(input) = input else {
        eprintln!("error: missing input file");
        return ExitCode::from(2);
    };
    let bin_path = std::env::temp_dir().join(format!(
        "torajs-debug-{}-{}",
        std::process::id(),
        rand_suffix()
    ));
    // Build via `tr build`-equivalent path. We shell out to the
    // current `tr` binary's `build` subcommand so the debug binary
    // gets the exact same toolchain, optimization defaults, and
    // DWARF setup as a normal `tr build`. `--opt O0` keeps source
    // <-> instruction mapping clean for stepping (O3 inlines past
    // most function boundaries).
    let self_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: locating tr binary: {e}");
            return ExitCode::from(1);
        }
    };
    let build_status = std::process::Command::new(&self_exe)
        .arg("build")
        .arg(input)
        .arg("-o")
        .arg(&bin_path)
        .arg("--opt")
        .arg("O0")
        .status();
    match build_status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("tr build exited {s}");
            return ExitCode::from(s.code().unwrap_or(1) as u8);
        }
        Err(e) => {
            eprintln!("error: spawning tr build: {e}");
            return ExitCode::from(1);
        }
    }
    let mut lldb = std::process::Command::new("lldb");
    for a in &extra {
        lldb.arg(a);
    }
    lldb.arg(&bin_path);
    use std::os::unix::process::CommandExt;
    let err = lldb.exec();
    eprintln!("error: exec lldb: {err}");
    ExitCode::from(1)
}
