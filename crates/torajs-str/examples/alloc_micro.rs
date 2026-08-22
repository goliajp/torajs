//! Micro-harness for `__torajs_str_alloc` alone — the entry every
//! helper that builds a Str out of a UTF-8 byte buffer at runtime
//! goes through. The profiler folds the whole thing into one symbol,
//! so this is how its inside gets priced.
//!
//! Not a gate: an ablation loop. Edit `block_ffi.rs`, rebuild this
//! example (seconds), run it, read the delta. Run:
//! `cargo run --release -p torajs-str --example alloc_micro -- 2000000`
//!
//! The payloads are what `regex-dfa-*-100k` hands over per iteration:
//! a whole matched line, the slice a match returns, and the digits of
//! a loop counter — all ASCII. The two non-ASCII rows are there so a
//! change that speeds the ASCII answer up at their expense shows.

use std::time::Instant;
use torajs_str::{__torajs_str_alloc, __torajs_str_drop};

// rc_dec's hit-zero hook lives in torajs-weak, which this binary does
// not link; the kernel under test never reaches it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_weakref_target_dying(_target: *mut std::ffi::c_void) {}

const CASES: &[(&str, &str)] = &[
    ("line-ascii", "pre a\nmiddle\nc post 50000"),
    ("match-ascii", "a\nmiddle\nc"),
    ("digits", "50000"),
    ("empty", ""),
    (
        "long-ascii",
        "the quick brown fox jumps over the lazy dog, and then does it again 50000",
    ),
    ("latin1", "café crème brûlée naïve façade"),
    ("utf16", "日本語のテキスト 50000"),
];

fn main() {
    let iters: u64 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(2_000_000);

    println!("{:<13} {:>5} {:>9}", "case", "bytes", "ns/iter");
    for (name, payload) in CASES {
        let src = payload.as_bytes();
        let run = || {
            for _ in 0..iters {
                let p = unsafe { __torajs_str_alloc(src.as_ptr(), src.len() as i64) };
                std::hint::black_box(p);
                unsafe { __torajs_str_drop(p) };
            }
        };
        for _ in 0..(iters / 10).max(1) {
            let p = unsafe { __torajs_str_alloc(src.as_ptr(), src.len() as i64) };
            std::hint::black_box(p);
            unsafe { __torajs_str_drop(p) };
        }
        let t = Instant::now();
        run();
        let ns = t.elapsed().as_nanos() as f64 / iters as f64;
        println!("{:<13} {:>5} {:>9.2}", name, src.len(), ns);
    }
}
