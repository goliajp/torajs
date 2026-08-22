//! Micro-harness for the concat kernel alone — the `acc = acc + piece`
//! string-builder loop that `multibyte-concat-100k` runs 1000 times
//! (rotation 471). Each round starts from an empty accumulator and
//! appends a 4-code-unit UTF-16 piece 100 times, so the accumulator
//! walks 0 → 400 code units and the loop pays one alloc + one
//! full-length copy + one free per append.
//!
//! Not a gate: an ablation loop. Edit `concat.rs` / `block.rs` /
//! `layout.rs`, rebuild this example (seconds), `hyperfine` it, and
//! read the delta; the profiler folds the whole append into two
//! symbols (`memcpy` + `__torajs_str_concat`), so this is how its
//! inside gets priced. Run:
//! `cargo run --release -p torajs-str --example concat_micro -- 30000 [append]`
//!
//! The second argument picks the shape: the default runs
//! `concat` + `drop` (what the lowering emitted before the
//! `str_append` peephole), `append` runs the ownership-transfer
//! kernel. Both answer the same total, so the pair is a same-binary
//! ablation of the append path alone.

use torajs_str::{__torajs_str_append, __torajs_str_concat, __torajs_str_drop, StrBlock};

// rc_dec's hit-zero hook lives in torajs-weak, which this binary does
// not link; the kernel under test never reaches it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut std::ffi::c_void) {}

/// UTF-16 LE cell holding `units`, refcount 1.
fn make_utf16(units: &[u16]) -> *mut u8 {
    let length = units.len() as u32;
    let mut b = StrBlock::alloc_with_encoding(length, false);
    let dst = unsafe { b.as_bytes_mut(length * 2) };
    for (i, &u) in units.iter().enumerate() {
        dst[i * 2..i * 2 + 2].copy_from_slice(&u.to_le_bytes());
    }
    b.into_raw()
}

fn main() {
    let rounds: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(30_000);
    // "中文测试" — the bench's piece, 4 BMP CJK code units.
    let piece = make_utf16(&[0x4E2D, 0x6587, 0x6D4B, 0x8BD5]);
    let use_append = std::env::args().nth(2).as_deref() == Some("append");
    let mut total: u64 = 0;
    for _ in 0..rounds {
        let mut acc = make_utf16(&[]);
        for _ in 0..100 {
            acc = if use_append {
                unsafe { __torajs_str_append(acc, piece) }
            } else {
                let next = unsafe { __torajs_str_concat(acc, piece) };
                unsafe { __torajs_str_drop(acc) };
                next
            };
        }
        total += unsafe { (acc.add(8) as *const u32).read() } as u64;
        unsafe { __torajs_str_drop(acc) };
    }
    println!("{total}");
}
