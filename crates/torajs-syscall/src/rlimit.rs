//! `getrlimit` for the stack limit.

use crate::safe::{Errno, decode};
use crate::sysno::SYS_GETRLIMIT;

/// `RLIMIT_STACK` in `<sys/resource.h>`.
const RLIMIT_STACK: i64 = 3;

/// `struct rlimit`.
#[repr(C)]
struct Rlimit {
    cur: u64,
    max: u64,
}

/// The soft stack limit in bytes: the size the kernel gives the main
/// thread's stack at exec.
pub fn stack_limit() -> Result<u64, Errno> {
    let mut r = Rlimit { cur: 0, max: 0 };
    let raw: i64;
    // The trap clobbers what a call may (x0-x17, NZCV, the
    // caller-saved vector registers) and keeps v8-v15 as a call does.
    // `clobber_abi("C")` would name v8-v15 whole, which costs the caller
    // a save of d8-d15 for nothing: this one call sits on the release
    // path that every artifact links.
    unsafe {
        core::arch::asm!(
            "svc #0x80",
            "b.cc 1f",
            "neg x0, x0",
            "1:",
            inlateout("x16") SYS_GETRLIMIT as i64 => _,
            inlateout("x0") RLIMIT_STACK => raw,
            inlateout("x1") &mut r as *mut Rlimit as i64 => _,
            lateout("x2") _, lateout("x3") _, lateout("x4") _, lateout("x5") _,
            lateout("x6") _, lateout("x7") _, lateout("x8") _, lateout("x9") _,
            lateout("x10") _, lateout("x11") _, lateout("x12") _, lateout("x13") _,
            lateout("x14") _, lateout("x15") _, lateout("x17") _,
            lateout("v0") _, lateout("v1") _, lateout("v2") _, lateout("v3") _,
            lateout("v4") _, lateout("v5") _, lateout("v6") _, lateout("v7") _,
            lateout("v16") _, lateout("v17") _, lateout("v18") _, lateout("v19") _,
            lateout("v20") _, lateout("v21") _, lateout("v22") _, lateout("v23") _,
            lateout("v24") _, lateout("v25") _, lateout("v26") _, lateout("v27") _,
            lateout("v28") _, lateout("v29") _, lateout("v30") _, lateout("v31") _,
            options(nostack),
        );
    }
    decode(raw).map(|_| r.cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agrees_with_libc() {
        unsafe extern "C" {
            fn getrlimit(resource: i32, rlp: *mut Rlimit) -> i32;
        }
        let mut r = Rlimit { cur: 0, max: 0 };
        assert_eq!(unsafe { getrlimit(RLIMIT_STACK as i32, &mut r) }, 0);
        assert_eq!(stack_limit(), Ok(r.cur));
        assert!(r.cur <= r.max);
    }
}
