//! Per-syscall counters and the census line writer, present only under
//! `--cfg torajs_census`.
//!
//! The counters answer "how many times did this program enter the
//! kernel, and through which door" — the number the design principle's
//! I/O budget is written in, and one no wall clock gives. Lines go to
//! fd 2 as `census <name>=<value>`, formatted on the stack, through the
//! uncounted trampoline so the dump does not count itself.

use core::sync::atomic::{AtomicU64, Ordering};

use crate::arch_aarch64_macos::syscall6_raw;
use crate::sysno::SYS_WRITE;

const SLOTS: usize = 1024;
const ZERO: AtomicU64 = AtomicU64::new(0);
static COUNTS: [AtomicU64; SLOTS] = [ZERO; SLOTS];

#[inline]
pub fn tick(sysno: u32) {
    if let Some(c) = COUNTS.get(sysno as usize) {
        c.fetch_add(1, Ordering::Relaxed);
    }
}

fn put_u64(buf: &mut [u8], at: &mut usize, mut v: u64) {
    let mut digits = [0u8; 20];
    let mut n = 0;
    loop {
        digits[n] = b'0' + (v % 10) as u8;
        n += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    while n > 0 {
        n -= 1;
        buf[*at] = digits[n];
        *at += 1;
    }
}

fn put(buf: &mut [u8], at: &mut usize, s: &[u8]) {
    buf[*at..*at + s.len()].copy_from_slice(s);
    *at += s.len();
}

/// `census <name>=<value>\n` on fd 2.
pub fn emit(name: &[u8], value: u64) {
    let mut buf = [0u8; 128];
    let mut at = 0;
    put(&mut buf, &mut at, b"census ");
    put(&mut buf, &mut at, &name[..name.len().min(90)]);
    put(&mut buf, &mut at, b"=");
    put_u64(&mut buf, &mut at, value);
    put(&mut buf, &mut at, b"\n");
    unsafe { syscall6_raw(SYS_WRITE, 2, buf.as_ptr() as i64, at as i64, 0, 0, 0) };
}

/// The same line writer for crates that do not depend on this one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_census_emit(name: *const u8, len: usize, value: u64) {
    let name = unsafe { core::slice::from_raw_parts(name, len) };
    emit(name, value);
}

/// One line per syscall number that was entered, plus the total.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_syscall_census_dump() {
    let mut total = 0u64;
    for (n, c) in COUNTS.iter().enumerate() {
        let v = c.load(Ordering::Relaxed);
        if v == 0 {
            continue;
        }
        total += v;
        let mut name = [0u8; 16];
        let mut at = 0;
        put(&mut name, &mut at, b"syscall.");
        put_u64(&mut name, &mut at, n as u64);
        emit(&name[..at], v);
    }
    emit(b"syscall.total", total);
}
