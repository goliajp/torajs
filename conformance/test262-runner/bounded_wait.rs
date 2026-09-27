//! Waiting on a child process with a deadline, without polling it.
//!
//! The caller's thread blocks in `wait_with_output` (which drains both
//! pipes while it waits); a watchdog thread waits out the deadline and
//! SIGKILLs the target unless the wait reports the exit first. The
//! polling loops this replaced noticed an exit half a backoff step
//! late, and read the pipes only after the exit — so a case writing
//! more than a pipe buffer blocked on the write until its timeout and
//! was judged a timeout. The watchdog can fire in the instant between
//! the reap and the done signal, against a pid macOS will not have
//! handed out again that soon (pids increment).

use std::process::{Child, Output};
use std::sync::mpsc;
use std::time::Duration;

/// `kill(2)` — sent with a negated pid to signal a whole process group.
/// tr AOT-compiles each case to a native binary it then spawns
/// (`torajs-run-new-*`); killing only the direct child (`Child::kill`)
/// orphaned that grandchild to init, where an infinite-loop or
/// runaway-allocation case kept a core pegged and memory growing forever.
/// On a shared runner that starved the host into a watchdog-timeout panic
/// and a reboot loop (mini, 2026-07-23). `torajs-test262` is a dev-only
/// harness; declaring the one libc call inline keeps its zero-dep build.
const SIGKILL: i32 = 9;
unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

/// What SIGKILL targets when the deadline passes.
pub enum KillTarget {
    /// The child alone.
    Process,
    /// The child's whole process group (it must lead one).
    Group,
}

/// `Some(output)` when the child exits within `timeout`; `None` when
/// the deadline passed and the target was SIGKILLed (the child is
/// reaped either way).
pub fn wait_output_within(
    child: Child,
    target: KillTarget,
    timeout: Duration,
) -> Option<std::io::Result<Output>> {
    let pid = child.id() as i32;
    let victim = match target {
        KillTarget::Process => pid,
        KillTarget::Group => -pid,
    };
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        let fired = done_rx.recv_timeout(timeout).is_err();
        if fired {
            unsafe { kill(victim, SIGKILL) };
        }
        fired
    });
    let out = child.wait_with_output();
    let _ = done_tx.send(());
    if watchdog.join().unwrap_or(false) {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    fn spawn(script: &str) -> Child {
        Command::new("sh")
            .args(["-c", script])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .expect("spawn sh")
    }

    #[test]
    fn a_quick_child_answers_its_output() {
        let out = wait_output_within(spawn("echo hi"), KillTarget::Group, Duration::from_secs(5))
            .expect("no timeout")
            .expect("output");
        assert_eq!(out.stdout, b"hi\n");
    }

    #[test]
    fn a_child_writing_past_the_pipe_buffer_is_not_a_timeout() {
        let out = wait_output_within(
            spawn("head -c 1000000 /dev/zero"),
            KillTarget::Group,
            Duration::from_secs(5),
        )
        .expect("a large write must not block until the deadline")
        .expect("output");
        assert_eq!(out.stdout.len(), 1_000_000);
    }

    #[test]
    fn a_hung_group_is_killed_at_the_deadline() {
        let start = Instant::now();
        let got = wait_output_within(
            spawn("sleep 30 & wait"),
            KillTarget::Group,
            Duration::from_millis(200),
        );
        assert!(got.is_none());
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "took {:?}",
            start.elapsed()
        );
    }
}
