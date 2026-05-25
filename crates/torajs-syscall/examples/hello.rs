//! v0.7-A1 hello — closure of the v0.7-A1 trigger:
//! `cargo run -p torajs-syscall --example hello` outputs "hello\n"
//! to stdout via raw syscall, with NO libc involvement.
//!
//! Verify with `otool -L target/release/examples/hello` — should
//! show only the dyld + libsystem-stdc/dyld stubs that Rust's
//! lifetime-of-main code currently pulls in; the `write` symbol
//! itself comes from torajs-syscall, not from `_libc_write`.
//! (Step v0.7-Z final audit verifies the user binary `tr build`
//! produces a 0-libSystem output.)

use torajs_syscall::sysno::STDOUT_FD;
use torajs_syscall::{exit, write};

fn main() {
    let msg = b"hello via raw aarch64 svc #0x80 (no libc)\n";
    match unsafe { write(STDOUT_FD, msg) } {
        Ok(n) => {
            if n != msg.len() {
                exit(1);
            }
        }
        Err(_) => exit(2),
    }
    exit(0);
}
