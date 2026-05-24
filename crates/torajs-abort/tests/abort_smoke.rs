//! Black-box smoke test for `abort_with` — abort is noreturn + kills the
//! test process, so we fork() a child, have the child call abort_with,
//! then waitpid() in the parent to assert non-zero exit + signal.
//!
//! Why not `std::process::Command`: spawning a separate binary would
//! require a dedicated test bin in `Cargo.toml` (`[[bin]]` + helper
//! crate dep), inflating the polish surface. Inline fork is 10 lines
//! of unsafe libc that exercises the actual abort path.
//!
//! Why this is correctness-meaningful: a wrong implementation that
//! returned normally (or wrote to stdout instead of stderr) would
//! pass casual eyeballing of `lib.rs` but fail here.
//!
//! Unix-only; the abort crate itself targets unix-shaped staticlibs.

#![cfg(unix)]

use std::ffi::c_int;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::ExitCode;

unsafe extern "C" {
    fn fork() -> c_int;
    fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    fn pipe(fds: *mut c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn dup2(oldfd: c_int, newfd: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut u8, n: usize) -> isize;
}

const STDERR_FILENO: c_int = 2;

/// Macros expanding `WIFSIGNALED` / `WTERMSIG` per the POSIX wait status
/// bit layout (low 7 bits = signal that killed; bit 7 = core dumped;
/// bits 8..15 = exit code if normally exited). `abort()` raises `SIGABRT`
/// (signal 6) so `WIFSIGNALED && WTERMSIG == 6` is the success criterion.
fn wifsignaled(status: c_int) -> bool {
    (status & 0x7f) != 0 && (status & 0x7f) != 0x7f
}
fn wtermsig(status: c_int) -> c_int {
    status & 0x7f
}

const SIGABRT: c_int = 6;

#[test]
fn abort_with_kills_with_sigabrt_and_writes_stderr() {
    // Set up a pipe to capture the child's stderr.
    let mut fds: [c_int; 2] = [0; 2];
    let rc = unsafe { pipe(fds.as_mut_ptr()) };
    assert_eq!(rc, 0, "pipe() failed");
    let read_end = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write_end = unsafe { OwnedFd::from_raw_fd(fds[1]) };

    let pid = unsafe { fork() };
    assert!(pid >= 0, "fork() failed");

    if pid == 0 {
        // Child: redirect stderr to the pipe write end + call abort.
        unsafe {
            dup2(write_end.as_raw_fd(), STDERR_FILENO);
            // Close the parent-side read end so the pipe collapses
            // cleanly when the child exits.
            close(read_end.as_raw_fd());
            torajs_abort::abort_with(b"smoke-test-abort-message");
        }
        // unreachable — abort_with is noreturn
    }

    // Parent: close write end so EOF propagates after child exits.
    drop(write_end);

    // Read child stderr into a Vec.
    let mut buf = [0u8; 256];
    let mut captured = Vec::new();
    loop {
        let n = unsafe { read(read_end.as_raw_fd(), buf.as_mut_ptr(), buf.len()) };
        if n <= 0 {
            break;
        }
        captured.extend_from_slice(&buf[..n as usize]);
    }

    // Wait for the child to exit and verify it died via SIGABRT.
    let mut status: c_int = 0;
    let waited = unsafe { waitpid(pid, &mut status, 0) };
    assert_eq!(waited, pid, "waitpid returned wrong pid");
    assert!(
        wifsignaled(status),
        "child exited normally; expected SIGABRT termination (status = {status:#x})"
    );
    assert_eq!(
        wtermsig(status),
        SIGABRT,
        "child died on signal {}, expected SIGABRT ({})",
        wtermsig(status),
        SIGABRT
    );

    // Verify the message + trailing newline landed on stderr.
    assert_eq!(
        captured.as_slice(),
        b"smoke-test-abort-message\n",
        "stderr capture mismatch: {captured:?}"
    );
}

#[test]
fn abort_with_empty_message_still_writes_newline() {
    let mut fds: [c_int; 2] = [0; 2];
    let rc = unsafe { pipe(fds.as_mut_ptr()) };
    assert_eq!(rc, 0, "pipe() failed");
    let read_end = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write_end = unsafe { OwnedFd::from_raw_fd(fds[1]) };

    let pid = unsafe { fork() };
    assert!(pid >= 0, "fork() failed");

    if pid == 0 {
        unsafe {
            dup2(write_end.as_raw_fd(), STDERR_FILENO);
            close(read_end.as_raw_fd());
            torajs_abort::abort_with(b"");
        }
    }
    drop(write_end);

    let mut buf = [0u8; 16];
    let mut captured = Vec::new();
    loop {
        let n = unsafe { read(read_end.as_raw_fd(), buf.as_mut_ptr(), buf.len()) };
        if n <= 0 {
            break;
        }
        captured.extend_from_slice(&buf[..n as usize]);
    }

    let mut status: c_int = 0;
    let _ = unsafe { waitpid(pid, &mut status, 0) };
    assert!(wifsignaled(status) && wtermsig(status) == SIGABRT);
    assert_eq!(
        captured.as_slice(),
        b"\n",
        "empty-msg path must still flush \\n"
    );
}

// Pacifier — keeps Cargo happy when the file is compiled (the actual
// tests are gated behind #[cfg(unix)] above).
#[allow(dead_code)]
fn _exit_code_stub() -> ExitCode {
    ExitCode::SUCCESS
}
