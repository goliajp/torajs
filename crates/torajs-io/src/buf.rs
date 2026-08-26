//! Line-buffered byte ring for stdout/stderr. v0.7-A3 scope is
//! single-threaded user binary; the buffer is process-global
//! (not TLS) because torajs has no thread-spawn surface yet
//! (microtask runs on the main thread). Cross-thread io is a
//! v0.8+ concern; when introduced, the buffer state moves to
//! `#[thread_local]` (already supported by Rust's std on every
//! tier-1 target).
//!
//! ## Design
//!
//! - Fixed 4 KB `[u8; 4096]` buffer per fd. Buffer-full triggers
//!   a flush before continuing to push.
//! - Newline (`\n`) triggers a flush after pushing the newline.
//!   Mirrors libc stdio's line-buffered default for tty stdout.
//!   This gives interleaved-friendly output without per-byte
//!   syscalls.
//! - Flush is one `torajs_syscall::write(fd, &buf[0..len])`. On
//!   short-write, retries from the new offset. On EINTR, retries.
//!   Other errors are swallowed (matches libc's `_IO_putc` —
//!   stdio errors set a flag on the FILE* but the call returns
//!   the byte regardless).
//!
//! ## Why no allocator
//!
//! torajs-io must NOT depend on `alloc` so the alloc family can
//! itself depend on torajs-io for `panic!` / `eprintln!`-style
//! emergency writes in the future. Fixed-size stack/static
//! buffer keeps the dependency direction strictly downward.

use core::cell::UnsafeCell;

/// Line buffer size — 4 KB matches libc's `stdout` default and
/// fits in one syscall page. A typical `console.log(...)` line
/// is < 200 B; 4 KB holds ~20 lines worst case before a
/// non-newline-driven flush.
pub const BUF_CAP: usize = 4096;

/// Single-fd line buffer. Process-global state per fd via
/// [`STDOUT`] / (future) `STDERR` statics.
pub struct LineBuf {
    /// SAFETY: protected by torajs's single-threaded execution
    /// model (v0.7 scope). `UnsafeCell` lets `&'static
    /// LineBuf` callers mutate without going through a Mutex.
    /// When v0.8 adds thread spawn, this becomes
    /// `#[thread_local]` + plain fields.
    inner: UnsafeCell<Inner>,
}

struct Inner {
    buf: [u8; BUF_CAP],
    len: usize,
}

/// The two sinks' file descriptors — carried by the caller into
/// every `push` / `write` / `flush`, never stored in the buffer.
/// r505: an `fd` field made the two 4 KB statics NON-zero, so both
/// lived in `__DATA,__data` and every program paid 8,224 B of file
/// for two buffers that start empty; all-zero statics are `.bss` —
/// vmsize only, no file backing.
pub const STDOUT_FD: i32 = 1;
pub const STDERR_FD: i32 = 2;

/// SAFETY: `LineBuf` is single-threaded by v0.7 scope. Sync is
/// vacuously satisfied since there's no concurrent access path
/// in any current torajs user binary. Asserting `Sync` lets us
/// place a `LineBuf` in a `static` — required because
/// `extern "C"` entry points have no `self`.
unsafe impl Sync for LineBuf {}

impl LineBuf {
    /// Construct a fresh empty line buffer bound to `fd` (1 for
    /// stdout, 2 for stderr). `const fn` so the global static
    /// can be zero-initialized at program start.
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(Inner {
                buf: [0u8; BUF_CAP],
                len: 0,
            }),
        }
    }

    /// Push one byte. If `c == b'\n'`, flush after push (line-
    /// buffered semantics). If buffer would overflow, flush first
    /// then push.
    ///
    /// # Safety
    ///
    /// Single-threaded access only. Caller (the extern entry
    /// point) is responsible for ensuring no concurrent reborrow
    /// of `&self`'s inner state.
    pub unsafe fn push(&self, fd: i32, c: u8) {
        let inner = unsafe { &mut *self.inner.get() };
        if inner.len == BUF_CAP {
            Self::flush_inner(inner, fd);
        }
        // r503 — `get_mut`, not `[]`: the flush above keeps `len` in
        // range by construction, but not provably so, and the
        // bounds-check panic was the io crate's only edge into the
        // core::fmt renderer (4 KB in every program). A miss drops
        // the byte — unreachable, and loud nowhere is the honest
        // price of the renderer's absence.
        if let Some(slot) = inner.buf.get_mut(inner.len) {
            *slot = c;
            inner.len += 1;
        }
        if c == b'\n' {
            Self::flush_inner(inner, fd);
        }
    }

    /// Push a slice in one go. Splits across multiple flushes if
    /// the slice exceeds [`BUF_CAP`]. Flushes after each
    /// newline-containing chunk to preserve line-buffered
    /// semantics.
    ///
    /// # Safety
    ///
    /// Same as [`push`](LineBuf::push).
    pub unsafe fn write(&self, fd: i32, s: &[u8]) {
        for &b in s {
            unsafe { self.push(fd, b) };
        }
    }

    /// Explicit flush — drains any buffered bytes to `fd` via a
    /// single `torajs_syscall::write` (with EINTR / short-write
    /// retry).
    ///
    /// # Safety
    ///
    /// Same as [`push`](LineBuf::push).
    pub unsafe fn flush(&self, fd: i32) {
        let inner = unsafe { &mut *self.inner.get() };
        Self::flush_inner(inner, fd);
    }

    fn flush_inner(inner: &mut Inner, fd: i32) {
        let mut off = 0;
        while off < inner.len {
            // r503 — `get`, same reason as `push`: `off < len <=
            // BUF_CAP` holds by construction, not by type.
            let Some(slice) = inner.buf.get(off..inner.len) else {
                break;
            };
            // SAFETY: slice is a live `&[u8]`; torajs_syscall::write
            // requires `len` bytes valid which is enforced by the
            // slice itself.
            let r = unsafe { torajs_syscall::write(fd, slice) };
            match r {
                Ok(0) => break,
                Ok(n) => off += n,
                Err(_e) => break,
            }
        }
        inner.len = 0;
    }
}

/// Process-global stdout buffer. Initialized at program start
/// (const init); first push lazy-fills.
pub static STDOUT: LineBuf = LineBuf::new();

/// Process-global stderr buffer — the `console.error` /
/// `console.warn` sink behind [`crate::sink`]'s current-sink
/// switch. Line-buffered like STDOUT; the sink-switch primitives
/// drain both buffers at every crossing so `2>&1` redirection
/// preserves caller-order interleaving (the generalization of the
/// legacy "flush stdout before a raw stderr write(2)" convention).
pub static STDERR: LineBuf = LineBuf::new();

#[cfg(test)]
mod tests {
    /// Sentinel fd for the buffer-state tests: write() on it errors
    /// (EBADF), which buf swallows.
    const FD: i32 = 99;

    use super::*;

    /// Sanity: const init shapes the buffer as empty + fd=1.
    /// We don't call write() from tests (would clobber test
    /// runner's stdout); just exercise the buffer state.
    #[test]
    fn sink_fds_are_the_posix_ones() {
        assert_eq!(STDOUT_FD, 1);
        assert_eq!(STDERR_FD, 2);
        // The statics themselves are all-zero by construction (no
        // fd inside) — that is what keeps them out of `__data`.
        let inner = unsafe { &*STDERR.inner.get() };
        assert_eq!(inner.len, 0);
    }

    /// Push without newline does not flush. Verify by checking
    /// the buffer length grew.
    #[test]
    fn push_no_newline_buffers() {
        let lb = LineBuf::new(); // sentinel fd; write() on it
        // would error (EBADF) which buf swallows.
        unsafe {
            lb.push(FD, b'a');
            lb.push(FD, b'b');
            lb.push(FD, b'c');
        }
        let inner = unsafe { &*lb.inner.get() };
        assert_eq!(inner.len, 3);
        assert_eq!(&inner.buf[0..3], b"abc");
    }

    /// Push newline triggers a flush — buf len resets to 0.
    /// fd=99 returns EBADF on write but the buffer state still
    /// resets (matches libc behavior — stdio errors don't
    /// preserve buffered bytes; they go to a sink).
    #[test]
    fn push_newline_flushes() {
        let lb = LineBuf::new();
        unsafe {
            lb.push(FD, b'h');
            lb.push(FD, b'i');
            lb.push(FD, b'\n');
        }
        let inner = unsafe { &*lb.inner.get() };
        assert_eq!(inner.len, 0, "newline must flush buffer");
    }

    /// Buffer-full triggers a pre-push flush. Push BUF_CAP+1
    /// bytes without newlines (use a constant non-`\n` byte so
    /// newline-driven flushes don't fire mid-fill); first
    /// BUF_CAP fill, +1 forces a flush + writes the new byte
    /// into the (now empty) buf at position 0.
    #[test]
    fn buffer_full_pre_flushes() {
        let lb = LineBuf::new();
        unsafe {
            for _ in 0..BUF_CAP {
                lb.push(FD, b'x'); // any non-`\n` byte
            }
        }
        let inner_pre = unsafe { &*lb.inner.get() };
        assert_eq!(
            inner_pre.len, BUF_CAP,
            "buffer should be full before next push"
        );
        unsafe {
            lb.push(FD, 0xAA);
        }
        let inner_post = unsafe { &*lb.inner.get() };
        // After buffer-full pre-flush + 1 byte push, len = 1.
        // (Even though the flush write(99, ...) errors, the
        // buf::flush_inner resets len unconditionally — matches
        // libc behavior.)
        assert_eq!(inner_post.len, 1);
        assert_eq!(inner_post.buf[0], 0xAA);
    }

    /// write([bytes]) is push-equivalent — verify a chunk
    /// containing a newline flushes mid-chunk.
    #[test]
    fn write_chunk_with_newline_flushes_midchunk() {
        let lb = LineBuf::new();
        unsafe { lb.write(FD, b"foo\nbar") };
        let inner = unsafe { &*lb.inner.get() };
        // "foo\n" flushed; "bar" remains buffered.
        assert_eq!(inner.len, 3);
        assert_eq!(&inner.buf[0..3], b"bar");
    }

    /// Explicit flush drains arbitrary buffered bytes.
    #[test]
    fn explicit_flush_drains() {
        let lb = LineBuf::new();
        unsafe {
            lb.push(FD, b'x');
            lb.push(FD, b'y');
            lb.flush(FD);
        }
        let inner = unsafe { &*lb.inner.get() };
        assert_eq!(inner.len, 0);
    }
}
