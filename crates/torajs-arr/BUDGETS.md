# torajs-arr performance budgets

Hot path. Per-op budgets per the bench corpus + B-series IR-restore
recovery work (commits 8f3f39f / 838cc5b / fede277).

| Path | Hot | Per-op (observed) |
| --- | --- | ---: |
| `arr.push` | **Yes** | ~5-10 ns (IR alwaysinline) |
| `arr.shift` (deque) | **Yes** | ~3-5 ns (head++ + len--) |
| `arr.push_unchecked` | **Yes** | ~3 ns (5-instr IR) |
| `arr[i]` index | **Yes** | ~1-3 ns (load only) |
| `arr.map` per element | Hot | ~10 ns + closure cost |
| `arr.filter` per element | Hot | ~10 ns + closure cost |
| `arr.slice` | Warm | ~50-100 ns + memcpy |
| `arr.sort` | Cold (per call) | O(n log n) |

## Cross-TU IR alwaysinline (critical)

Three fns must remain in **inkwell-emitted alwaysinline IR** to avoid
the `bl + ret` cross-TU overhead that dominates hot push/pop/shift
loops:

- `__torajs_arr_push` (187 LOC IR — B1b 2026-05-24)
- `__torajs_arr_shift` (4-memory-op — B4-shift 2026-05-24)
- `__torajs_arr_push_unchecked` (5-instr — B4-push-unchecked 2026-05-24)

These ALSO exist as `extern "C"` in this crate's staticlib for cross-
sub-crate Rust callers (fs / process / promise / regex). The
linkage = Internal for the IR-emitted variant means LLVM picks the
local definition for user-code call sites — eliminating the call
boundary.

See `crates/torajs-core/src/ssa_inkwell.rs` for the IR builders.

## What's NOT budgeted

- **Sort algorithm** is delegated to `core::sort_unstable` for
  comparable elements; comparator-driven sorts use a hand-written
  quicksort. Sort latency depends on input distribution.
- **Array<Any> tag-aware ops**: each slot is 16 bytes (tag + value);
  ~2× the per-slot cost of Array<T>. Bench corpus dominated by
  Array<T>.
