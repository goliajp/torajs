# torajs-abort performance + size budgets

`torajs-abort` ships **cold path** code only — `abort_with` is `#[cold]
+ #[inline(never)] + -> !`, so latency at the call point is irrelevant
(by definition the caller is about to die). The meaningful budgets are
**call-site setup cost** (a single `bl + 2 args` on the happy path)
and **binary-size contribution** (the reason this crate exists).

## Size budget (the actual metric)

The whole point of `torajs-abort` is to keep the user binary small by
replacing `expect()` / `panic!()` / `assert!()` call sites with
`abort_with(b"msg")`. That swap eliminates the transitive dep on:

- `std::panicking` (panic-handler dispatch)
- `std::backtrace_rs` (frame capture)
- `gimli` + `addr2line` (DWARF decoders)
- `rustc_demangle` (symbol demangler)
- `std::io::Error` + `std::thread::Thread` (paths these touch)

| Path | Budget | Measured | Notes |
| --- | ---: | ---: | --- |
| `libtorajs_abort.a` artifact size | ≤ 8 KB | ~2 KB | The whole staticlib body is the 2-fn extern wrapper + 1 inline path; release-mode build is mostly relocation metadata. |
| Per-call-site code at the AOT site | ≤ 16 bytes | ~12 bytes | `adr x0, .Lmsg + mov x1, #len + bl __torajs_abort_with` — three aarch64 instructions, exactly the same shape as a plain `bl panic!` minus the per-site format string materialization that `panic!` requires. |
| Cumulative user-binary delta vs `expect()`-everywhere build | ≥ -100 KB | ~-150 KB | Across the torajs bench corpus, post-A3 ship was -35 KB on `fib40` user binary alone; combined with A4.1 (build-std + panic=abort) the cumulative win on a 26-case median is ~-150 KB per binary. See `bench/results/2026-05-24-mini-d62b4c1.json`. |

## Latency budget (informational only)

`benches/abort.rs` measures the happy-path call-site setup (where the
abort branch is `cond = false` and never fires). Reported median on a
dev machine (M3 / `aarch64-apple-darwin`):

| Case | Iter count | Median |
| --- | ---: | ---: |
| `abort_with-happy-path-100k` | 100 000 | ~50 µs total (~0.5 ns / iter) |

That's basically the cost of an `&[u8]` pointer + length materialization +
a conditional branch — i.e. `#[cold]` + `#[inline(never)]` is doing its
job and the compiler keeps the success path tight.

## What's NOT budgeted

- **Failure-path latency**: irrelevant — by definition the process is
  about to die. `abort_with` calls `write(2) + write(2) + abort()`; the
  three-syscall cost (≤ 10 µs typical) doesn't matter at all.
- **Memory footprint**: no allocations, no static storage — the crate
  has zero runtime memory cost beyond the 12 bytes per call site.
- **Concurrency**: `abort()` is async-signal-safe and process-wide.
  Concurrent callers all reach `abort` and the first one's signal
  delivery decides; no ordering or fairness contract.
