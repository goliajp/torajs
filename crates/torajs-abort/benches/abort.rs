//! Criterion bench for `torajs-abort`.
//!
//! `abort_with` is noreturn — we can't bench it directly. What this
//! bench DOES measure is the **cold-path call setup** that the
//! compiler emits at the call site: the `bl __torajs_abort_with`
//! plus the argument materialization for the byte-slice pointer +
//! length. That's the only cost paid on the happy path because
//! `abort_with` is marked `#[cold] + #[inline(never)]`.
//!
//! Concretely: we measure the time to compute a small `prepare()`
//! function that materializes a byte-slice address + length and
//! does NOT call abort, vs. a no-op baseline. The delta is the
//! call-site setup cost which is what staticlib callers actually
//! pay on the success path (since abort never fires).
//!
//! This is informational — not a perf gate. There's no `BUDGETS.md`
//! entry for it because the call-setup cost is a single instruction
//! pair at the AOT site; the bench just lets future polish work
//! check that `#[cold]` / `#[inline(never)]` continue to keep the
//! happy-path inlined tight.

use core::hint::black_box;
use criterion::{Criterion, criterion_group, criterion_main};

/// Mimics the happy-path call-site shape: caller has a `&[u8]`,
/// would `abort_with(msg)` on a guard miss but the guard succeeds.
/// We measure the materialization + branch overhead only.
#[inline(never)]
fn maybe_abort(msg: &[u8], cond: bool) -> usize {
    if cond {
        // unreachable on the happy path — this branch is what the
        // compiler emits as `cmp + b.gt fail; success path; fail: bl
        // __torajs_abort_with`. We never hit it here.
        unsafe { torajs_abort::__torajs_abort_with(msg.as_ptr(), msg.len()) }
    }
    msg.len()
}

fn bench_happy_path(c: &mut Criterion) {
    let msg = b"never-fires";
    c.bench_function("abort_with-happy-path-100k", |b| {
        b.iter(|| {
            let mut acc = 0usize;
            for _ in 0..100_000 {
                acc = acc.wrapping_add(black_box(maybe_abort(black_box(msg), black_box(false))));
            }
            acc
        });
    });
}

criterion_group!(benches, bench_happy_path);
criterion_main!(benches);
