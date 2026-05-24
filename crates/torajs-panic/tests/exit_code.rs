//! Fork-based smoke test for `__torajs_panic` — like `torajs-abort`,
//! `__torajs_panic` is `-> !` and exits the test process, so we
//! fork() + redirect stderr + waitpid() to verify exit code +
//! captured message.
//!
//! Unix-only; `torajs-panic` itself targets Unix-shaped runtime
//! deployments.

#![cfg(unix)]

use std::ffi::{CString, c_int};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

unsafe extern "C" {
    fn fork() -> c_int;
    fn waitpid(pid: c_int, status: *mut c_int, options: c_int) -> c_int;
    fn pipe(fds: *mut c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
    fn dup2(oldfd: c_int, newfd: c_int) -> c_int;
    fn read(fd: c_int, buf: *mut u8, n: usize) -> isize;
}

const STDERR_FILENO: c_int = 2;
// `__torajs_panic` exits with code 1 (matches the C-runtime version's
// behavior pre-port). 101 is Rust's panic default; this crate is
// extern "C" + libc `exit(1)`, intentionally not 101.
const PANIC_EXIT_CODE: c_int = 1;

fn wifexited(status: c_int) -> bool {
    (status & 0x7f) == 0
}
fn wexitstatus(status: c_int) -> c_int {
    (status >> 8) & 0xff
}

#[test]
fn panic_exits_with_code_101_and_writes_message() {
    let mut fds: [c_int; 2] = [0; 2];
    let rc = unsafe { pipe(fds.as_mut_ptr()) };
    assert_eq!(rc, 0, "pipe() failed");
    let read_end = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write_end = unsafe { OwnedFd::from_raw_fd(fds[1]) };

    let pid = unsafe { fork() };
    assert!(pid >= 0, "fork() failed");

    if pid == 0 {
        // Child: redirect stderr, then panic.
        unsafe {
            dup2(write_end.as_raw_fd(), STDERR_FILENO);
            close(read_end.as_raw_fd());
            let msg = CString::new("smoke-test-panic-marker").unwrap();
            torajs_panic::__torajs_panic(msg.as_ptr() as *const u8);
        }
        // unreachable
    }
    drop(write_end);

    // Drain stderr until EOF / child exits.
    let mut buf = [0u8; 4096];
    let mut captured = Vec::new();
    loop {
        let n = unsafe { read(read_end.as_raw_fd(), buf.as_mut_ptr(), buf.len()) };
        if n <= 0 {
            break;
        }
        captured.extend_from_slice(&buf[..n as usize]);
    }

    // Verify exit code = 101.
    let mut status: c_int = 0;
    let waited = unsafe { waitpid(pid, &mut status, 0) };
    assert_eq!(waited, pid);
    assert!(
        wifexited(status),
        "child died via signal; expected normal exit (status = {status:#x})"
    );
    assert_eq!(
        wexitstatus(status),
        PANIC_EXIT_CODE,
        "child exit code mismatch (got {}, expected {PANIC_EXIT_CODE})",
        wexitstatus(status)
    );

    // The captured stderr should include the message marker. We
    // don't assert exact bytes because the backtrace section follows
    // and varies per platform / build mode.
    let captured_str = String::from_utf8_lossy(&captured);
    assert!(
        captured_str.contains("smoke-test-panic-marker"),
        "stderr capture missing message marker; got {captured_str:?}"
    );
}
