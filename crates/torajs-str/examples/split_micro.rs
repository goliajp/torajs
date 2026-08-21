//! Micro-harness for the split kernel alone — `__torajs_str_split`
//! on a static-literal parent, the product handed straight back to
//! the split pool, the way `split-only-100k` runs in steady state
//! once the release walk is out of the way (rotation 469).
//!
//! Not a gate: an ablation loop. Edit `split/ops.rs` or
//! `split/pool.rs`, rebuild this example (seconds), `hyperfine` it,
//! and read the delta; the profiler sees the kernel as one symbol, so
//! this is how its inside gets priced. Run:
//! `cargo run --release -p torajs-str --example split_micro -- 20000000`

use torajs_rc::FLAG_STATIC_LITERAL;
use torajs_str::{__torajs_split_block_free_push, __torajs_str_split, StrBlock};

// rc_dec's hit-zero hook lives in torajs-weak, which this binary does
// not link; the kernel under test never reaches it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut std::ffi::c_void) {}

fn make_literal(payload: &[u8]) -> *mut u8 {
    let mut b = StrBlock::alloc(payload.len() as u32);
    let dst = unsafe { b.as_bytes_mut(payload.len() as u32) };
    dst.copy_from_slice(payload);
    let p = b.into_raw();
    // flags word is the header's last u16 (offset 6): mark the cell a
    // literal so the kernel takes the `.rodata`-parent shape.
    unsafe {
        let flags = p.add(6) as *mut u16;
        *flags |= FLAG_STATIC_LITERAL;
    }
    p
}

fn main() {
    let n: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(20_000_000);
    let s = make_literal(b"3 4 + 2 * 5 +");
    let sep = make_literal(b" ");
    let mut total: u64 = 0;
    for _ in 0..n {
        let p = unsafe { __torajs_str_split(s, sep) };
        total += unsafe { *(p.add(8) as *const u64) };
        unsafe { __torajs_split_block_free_push(p) };
    }
    println!("{total}");
}
