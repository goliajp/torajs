# torajs-panic performance + size budgets

`torajs-panic` runs **once per process** at most — by the time
`__torajs_panic` fires the process is about to exit. Latency is
irrelevant; the only meaningful budgets are:

1. **Static memory footprint** — what the linked binary spends to
   hold the backtrace decode path.
2. **Binary-size contribution** — the staticlib's emitted code +
   any libc dependencies it pulls in.

Per-op latency is documented for awareness only (no perf-gate test
because the metric doesn't matter on the fatal path).

## Static memory budget

| Item | Bytes | Notes |
| --- | ---: | --- |
| `BACKTRACE_BUFFER` (libc backtrace addresses, up to 64 frames × 8 B) | 512 | Stack-allocated only during a `__torajs_panic` call — not a persistent static. |
| macOS executable-path scratch buffer | 1024 | Also stack-only, only lives across the `_NSGetExecutablePath` call. |
| Linux symbol-table strings | varies | Allocated by `backtrace_symbols(...)` from `libc` — not budgeted because `libc` owns the lifetime. |
| Static globals | 0 | The crate has no `static` storage of its own. |

Process-wide static cost: ~0 bytes for this crate. The libc backtrace
machinery's amortized footprint lives in `libc`, shared across the
entire process — not attributable to `torajs-panic`.

## Binary-size budget

| Path | Budget | Measured |
| --- | ---: | ---: |
| `libtorajs_panic.a` artifact | ≤ 16 KB | ~6 KB |
| Per-call code at the AOT site | ≤ 16 bytes | ~12 bytes (`adr x0, msg + bl __torajs_panic`) |
| Linked binary cost over the libc baseline | ≤ 4 KB | ~2 KB (the libc `backtrace` / `backtrace_symbols` symbols are dynamically linked on most platforms) |

## Latency (informational only)

| Path | Approx | Notes |
| --- | ---: | --- |
| `__torajs_panic` write to stderr | ~10 µs | One `fputs(msg, stderr)`. |
| Backtrace frame capture | ~50 µs | libc `backtrace(...)` — fast on aarch64 + frame pointers. |
| Backtrace symbolication (macOS `atos`) | ~50 ms | One-shot `system("atos -o <exe> <addrs>")` round trip. |
| Backtrace symbolication (Linux `backtrace_symbols`) | ~1 ms | In-process; doesn't shell out. |
| `exit(101)` | ~10 µs | Stdlib teardown + kernel `_exit` syscall. |

Worst case end-to-end: ~50 ms on macOS, ~5 ms on Linux. Irrelevant —
the process is dying.

## What's NOT budgeted

- **Repeated `__torajs_panic` calls.** By the time `__torajs_panic`
  is called the process is exiting; nothing prevents calls from
  happening (e.g. a panicking destructor as cleanup runs), but the
  cost is per-process not amortized.
- **Stack space for backtrace decode.** ~1.5 KB at worst — well
  within any reasonable stack budget.
- **Backtrace fidelity / correctness.** Best-effort. On stripped
  release binaries macOS atos prints `?? at <bin>+<offset>`; on
  Linux you get the same plus symbol where available. Symbol
  demangling is libc / atos's job; we don't add a `gimli` /
  `rustc_demangle` dependency.
