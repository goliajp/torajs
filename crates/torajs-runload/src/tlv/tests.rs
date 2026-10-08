use std::arch::asm;
use std::sync::OnceLock;

use super::{BindError, TlvBinding, TlvDescriptor, TlvTemplate, bind};

static INIT: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const INIT_AT: usize = 8;
const BLOCK_LEN: usize = 64;

fn binding() -> TlvBinding {
    static B: OnceLock<TlvBinding> = OnceLock::new();
    *B.get_or_init(|| {
        bind(TlvTemplate {
            init: &INIT,
            init_at: INIT_AT,
            block_len: BLOCK_LEN,
        })
        .expect("bind")
    })
}

fn descriptor(offset: usize) -> TlvDescriptor {
    let b = binding();
    TlvDescriptor {
        thunk: b.thunk,
        key: b.key,
        offset,
    }
}

/// The access sequence compiled code uses: x0 = descriptor, call the
/// descriptor's thunk.
fn access(d: &TlvDescriptor) -> *mut u8 {
    let out: *mut u8;
    unsafe {
        asm!(
            "ldr x8, [x0]",
            "blr x8",
            inout("x0") d as *const TlvDescriptor => out,
            out("x8") _,
            out("x16") _,
            out("x17") _,
            out("lr") _,
        );
    }
    out
}

#[test]
fn first_access_builds_the_block_from_the_template() {
    let d = descriptor(INIT_AT);
    let at = access(&d);
    let block = unsafe { std::slice::from_raw_parts(at.sub(INIT_AT), BLOCK_LEN) };
    assert_eq!(&block[..INIT_AT], &[0; INIT_AT]);
    assert_eq!(&block[INIT_AT..INIT_AT + INIT.len()], &INIT);
    assert!(block[INIT_AT + INIT.len()..].iter().all(|b| *b == 0));
    assert_eq!(access(&d), at, "the second access reuses the block");
    let other = descriptor(40);
    assert_eq!(access(&other), unsafe { at.sub(INIT_AT).add(40) });
}

#[test]
fn a_template_whose_init_does_not_fit_is_refused() {
    let t = |init_at, block_len| TlvTemplate {
        init: &INIT,
        init_at,
        block_len,
    };
    assert_eq!(
        bind(t(BLOCK_LEN - 8, BLOCK_LEN)),
        Err(BindError::InitOutsideBlock)
    );
    assert_eq!(
        bind(t(usize::MAX, BLOCK_LEN)),
        Err(BindError::InitOutsideBlock)
    );
    assert_eq!(bind(t(0, usize::MAX)), Err(BindError::BlockTooLarge));
}

#[test]
fn a_second_bind_is_refused() {
    binding();
    let again = bind(TlvTemplate {
        init: &INIT,
        init_at: 0,
        block_len: 2 * BLOCK_LEN,
    });
    assert_eq!(again, Err(BindError::AlreadyBound));
}

#[test]
fn each_thread_gets_its_own_block() {
    let d = descriptor(INIT_AT);
    let here = access(&d);
    unsafe { *here = 0xAA };
    let there = std::thread::spawn(move || {
        let at = access(&descriptor(INIT_AT));
        (at as usize, unsafe { *at })
    })
    .join()
    .expect("thread");
    assert_ne!(there.0, here as usize);
    assert_eq!(there.1, INIT[0], "a new thread starts from the template");
    assert_eq!(unsafe { *here }, 0xAA);
}

/// x1-x15 after the call, then q0-q31.
#[repr(C, align(16))]
struct Saved([u64; 16], [[u8; 16]; 32]);

/// Seed x1-x15 and q0-q31, access through the thunk, store them back.
fn access_and_save(d: &TlvDescriptor) -> Saved {
    let mut s = Saved([0; 16], [[0; 16]; 32]);
    let buf = &raw mut s;
    unsafe {
        asm!(
            "mov x1, #0x101", "mov x2, #0x102", "mov x3, #0x103", "mov x4, #0x104",
            "mov x5, #0x105", "mov x6, #0x106", "mov x7, #0x107", "mov x8, #0x108",
            "mov x9, #0x109", "mov x10, #0x10a", "mov x11, #0x10b", "mov x12, #0x10c",
            "mov x13, #0x10d", "mov x14, #0x10e", "mov x15, #0x10f",
            "movi v0.16b, #0x40", "movi v1.16b, #0x41", "movi v2.16b, #0x42",
            "movi v3.16b, #0x43", "movi v4.16b, #0x44", "movi v5.16b, #0x45",
            "movi v6.16b, #0x46", "movi v7.16b, #0x47", "movi v8.16b, #0x48",
            "movi v9.16b, #0x49", "movi v10.16b, #0x4a", "movi v11.16b, #0x4b",
            "movi v12.16b, #0x4c", "movi v13.16b, #0x4d", "movi v14.16b, #0x4e",
            "movi v15.16b, #0x4f", "movi v16.16b, #0x50", "movi v17.16b, #0x51",
            "movi v18.16b, #0x52", "movi v19.16b, #0x53", "movi v20.16b, #0x54",
            "movi v21.16b, #0x55", "movi v22.16b, #0x56", "movi v23.16b, #0x57",
            "movi v24.16b, #0x58", "movi v25.16b, #0x59", "movi v26.16b, #0x5a",
            "movi v27.16b, #0x5b", "movi v28.16b, #0x5c", "movi v29.16b, #0x5d",
            "movi v30.16b, #0x5e", "movi v31.16b, #0x5f",
            "ldr x16, [x0]",
            "blr x16",
            "stp x1, x2, [x20, #0]", "stp x3, x4, [x20, #16]", "stp x5, x6, [x20, #32]",
            "stp x7, x8, [x20, #48]", "stp x9, x10, [x20, #64]", "stp x11, x12, [x20, #80]",
            "stp x13, x14, [x20, #96]", "str x15, [x20, #112]",
            "stp q0, q1, [x20, #128]", "stp q2, q3, [x20, #160]", "stp q4, q5, [x20, #192]",
            "stp q6, q7, [x20, #224]", "stp q8, q9, [x20, #256]", "stp q10, q11, [x20, #288]",
            "stp q12, q13, [x20, #320]", "stp q14, q15, [x20, #352]",
            "stp q16, q17, [x20, #384]", "stp q18, q19, [x20, #416]",
            "stp q20, q21, [x20, #448]", "stp q22, q23, [x20, #480]",
            "stp q24, q25, [x20, #512]", "stp q26, q27, [x20, #544]",
            "stp q28, q29, [x20, #576]", "stp q30, q31, [x20, #608]",
            in("x20") buf,
            inout("x0") d as *const TlvDescriptor => _,
            clobber_abi("C"),
        );
    }
    s
}

fn assert_preserved(s: &Saved, path: &str) {
    for (i, x) in s.0[..15].iter().enumerate() {
        assert_eq!(*x, 0x101 + i as u64, "{path}: x{}", i + 1);
    }
    for (i, q) in s.1.iter().enumerate() {
        assert_eq!(*q, [0x40 + i as u8; 16], "{path}: q{i}");
    }
}

#[test]
fn both_paths_preserve_every_register_but_x0_x16_x17() {
    let d = descriptor(INIT_AT);
    std::thread::spawn(move || {
        assert_preserved(&access_and_save(&d), "slow path");
        assert_preserved(&access_and_save(&d), "fast path");
    })
    .join()
    .expect("thread");
}
