//! `tr run` — AOT-with-cache. Mirrors `cmd_build`'s codegen + obj +
//! link path, materializes the binary to a unique temp file (pid +
//! nanos), `exec`s it, and cleans up. Equivalent to
//! `tr build X.ts -o /tmp/X && /tmp/X` with cleanup; the conformance
//! gate drives every fixture through this path.
//!
//! Stdin/stdout/stderr inherit through to the user; child exit code
//! (or 128 + signal for signal-killed runs) flows out as `ExitCode`.

use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

use crate::cmd_build::{build_link_config, lower_to_ssa};
use crate::util::rand_suffix;
use torajs_link::archive_emit::link_to_exec_with_archives;

pub(crate) fn run(file_arg: Option<&String>) -> ExitCode {
    let path = match file_arg {
        Some(p) => p.as_str(),
        None => {
            eprintln!("error: missing file argument (use `-` for stdin)");
            return ExitCode::from(2);
        }
    };

    let ssa_module = match lower_to_ssa(path) {
        Ok(m) => m,
        Err(code) => return code,
    };

    // Phase 0 step 8b — egraph mid-end pass. Honors TORAJS_EGRAPH_OFF=1.
    let ssa_module = torajs_egraph::transform_module(ssa_module);

    let cfg = build_link_config(&ssa_module, false, false);

    let bytes = match link_to_exec_with_archives(&cfg) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("link error: {e:?}");
            return ExitCode::from(1);
        }
    };

    let target_path: PathBuf = std::env::temp_dir().join(format!(
        "torajs-run-new-{}-{}",
        std::process::id(),
        rand_suffix()
    ));

    if let Err(e) = std::fs::write(&target_path, &bytes) {
        eprintln!("write error: {e}");
        return ExitCode::from(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = match std::fs::metadata(&target_path) {
            Ok(m) => m.permissions(),
            Err(e) => {
                eprintln!("stat error: {e}");
                let _ = std::fs::remove_file(&target_path);
                return ExitCode::from(1);
            }
        };
        perms.set_mode(0o755);
        if let Err(e) = std::fs::set_permissions(&target_path, perms) {
            eprintln!("chmod error: {e}");
            let _ = std::fs::remove_file(&target_path);
            return ExitCode::from(1);
        }
    }

    let status = Command::new(&target_path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();

    let _ = std::fs::remove_file(&target_path);

    match status {
        Ok(s) => {
            if let Some(code) = s.code() {
                ExitCode::from(code.clamp(0, 255) as u8)
            } else {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    if let Some(sig) = s.signal() {
                        return ExitCode::from((128 + sig).clamp(0, 255) as u8);
                    }
                }
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("exec {}: {e}", target_path.display());
            ExitCode::from(1)
        }
    }
}
