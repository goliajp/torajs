//! Peak thread count of a run step: one extra, untimed run whose task
//! info is polled every 100 µs until it exits. A thread that lives
//! shorter than the poll interval can be missed, so the reading is a
//! lower bound; a run that ends before the first poll gives `None`.
//! Only the spawned process is counted, not processes it starts.

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
pub(crate) fn peak_threads(cmd: &str, timeout: Duration) -> Option<u32> {
    use crate::rusage_darwin::{PROC_PIDTASKINFO, ProcTaskInfo, proc_pidinfo};
    let argv = crate::runner::split_cmd(cmd);
    let mut child = Command::new(argv.first()?)
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let pid = child.id() as i32;
    let size = std::mem::size_of::<ProcTaskInfo>() as i32;
    let start = Instant::now();
    let mut peak: Option<u32> = None;
    loop {
        let mut info = ProcTaskInfo::default();
        let n = unsafe {
            proc_pidinfo(
                pid,
                PROC_PIDTASKINFO,
                0,
                (&mut info as *mut ProcTaskInfo).cast(),
                size,
            )
        };
        if n == size && info.pti_threadnum > 0 {
            peak = peak.max(Some(info.pti_threadnum as u32));
        }
        match child.try_wait() {
            Ok(Some(status)) => return status.success().then_some(peak).flatten(),
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_micros(100)),
            _ => {
                unsafe { kill(-pid, 9) };
                let _ = child.wait();
                return None;
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn peak_threads(_cmd: &str, _timeout: Duration) -> Option<u32> {
    None
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn a_sleeping_shell_has_one_thread() {
        let n = peak_threads("/bin/sleep 0.05", Duration::from_secs(5));
        assert_eq!(n, Some(1));
    }

    #[test]
    fn a_failing_run_has_no_reading() {
        assert_eq!(
            peak_threads("/bin/sh -c exit_1_please", Duration::from_secs(5)),
            None
        );
    }
}
