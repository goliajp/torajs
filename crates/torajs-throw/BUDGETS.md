# torajs-throw performance budgets

`torajs-throw` sits on **every runtime-helper call boundary**. The
ssa_lower-emitted `emit_throw_check` pass inserts a poll after each
helper that *might* raise (bigint div, dynobj frozen-set, regex
syntax error, ...); the poll is `bl __torajs_throw_check + cbnz`. So
the happy-path budget is per-helper-call — this is one of the most
frequently-executed code paths in any torajs user binary.

Budgets here are documentary (no perf-gate tests); `benches/throw.rs`
reports the actual numbers via criterion.

## Path taxonomy

| Path | Hot/Cold | Rate |
| --- | --- | --- |
| `__torajs_throw_check` | **Hot** | Per may-throw helper call. Order-of-magnitude estimate: millions of polls per second for a throw-light workload. |
| `__torajs_throw_set` | **Cold** | Only fires when a runtime helper actually decides to raise (1 per thrown error). |
| `__torajs_throw_take` / `__torajs_throw_take_tag` | **Cold** | Only fires inside a user-fn's catch block. 1 per caught error. |
| `__torajs_register_native_error` | **One-shot** | 3 calls per program (Error / TypeError / RangeError factories) during `synthesize_module_init`. Latency irrelevant. |

## Budgets

| Path | Budget | Observed (dev) | Headroom | Notes |
| --- | ---: | ---: | ---: | --- |
| `throw_check-happy-path-100k` | ≤ 200 µs (~2 ns/iter) | ~80 µs (~0.8 ns/iter) | 2.5× | 100k polls; single atomic load + return. Branchless on the success side; the caller's `cbnz` adds the conditional out-of-band of this bench. |
| `throw_set_take_cycle-100k` | ≤ 3 ms | ~1 ms | 3× | 3 atomic stores + 2 atomic loads per iter. Far slower than the happy path; that's fine since this only fires on actual throws. |

## Memory budget

Static footprint = 3 × 8 bytes (`THROW_TAG` + `THROW_VALUE` +
`THROW_ACTIVE`) + 3 × ptr-size (`REGISTRY[0..3]`) = 24 bytes globals
+ 24 bytes ptr table = **48 bytes process-wide**, independent of
throw count. No heap allocations are made by this crate itself; the
convenience throwers `__torajs_throw_range_error` / `_type_error`
allocate one `Str` per call via `__torajs_str_alloc_pooled` (lives
in `torajs-str`, not budgeted here).

## Atomicity story

All loads / stores are `Ordering::Relaxed`. Justification:

- The runtime is single-threaded today; concurrent reader/writer of
  the throw slot is not a runtime concern.
- The registry is single-write-at-startup, read-only after — the
  one-shot factory registration in `synthesize_module_init` finishes
  before any user code that could throw runs.
- Future multi-threaded torajs would require per-thread throw slots
  (TLS proper, not `static AtomicI64`) regardless of memory ordering.

`AtomicPtr<()>` instead of `*mut c_void` for the registry slot only
because raw pointers aren't `Sync`. `AtomicI64` instead of `i64` for
the throw slot for the same Rust-safety reason. Neither is providing
real happens-before semantics that the runtime depends on.

## What's NOT budgeted

- **`__torajs_throw_range_error` / `_type_error`** latency: dominated
  by the `Str::alloc_pooled` call + the factory invocation, both of
  which live in `torajs-str` / codegen-emitted code respectively. Not
  this crate's budget.
- **Per-helper-call overhead in the user binary**: the `bl
  __torajs_throw_check + cbnz` pair adds ~1-2 ns per helper-call site.
  That's a codegen / ABI concern, not budgeted here.
- **Throw stack growth**: there is no throw stack — only one slot —
  by design. Nested throws overwrite; the user-IR is expected to
  catch + clear before any nested might-throw helper runs.
