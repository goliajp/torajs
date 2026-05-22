# torajs-str

Layer-2 substrate of the torajs AOT TypeScript runtime — owns the
`Str` and `Substr` heap types plus their pool-aware allocation and
free paths. Pure Rust (`std`-only), 0 runtime deps.

```text
Str    = [header:8][len:8][bytes:N]           prefix 16
Substr = [header:8][len:8][parent:8][off:8]   prefix 32 (Phase 3.1-b)
```

Sub-crate of the architecture rewrite (`docs/architecture-rewrite.md`
P3 Layer 2). Replaces the small-Str pool + alloc/free helpers that
previously lived in `crates/torajs-runtime/src/runtime_str.c`.

## Modules

- [`layout`] — ABI constants (`STR_HDR_SIZE` / `STR_LEN_OFF` /
  `STR_DATA_OFF`) + packed header init + `block_size`.
- [`pool`] — Small-Str LIFO pool (32 slots × ≤16-byte payload
  blocks) backed by per-slot atomics for the Rust safety story.
  Single-threaded runtime; `Relaxed` ordering compiles to identical
  asm under `Ordering::Relaxed`.
- [`alloc`] — Pool-aware `alloc` + `free_pool_aware` + the two
  `extern "C"` wrappers (`__torajs_str_alloc_pooled` /
  `__torajs_str_free`) that ssa_inkwell-emitted IR and remaining C
  helpers in `runtime_str.c` call into.

## Design — Rust idiomatic, not a C transcription

Per the project's pure-Rust pillar, the API is Rust-first:

- **`StrBlock`** newtype around `NonNull<u8>` for layout-aware
  pointer ops, instead of bare `*mut c_void`.
- **`Pool` methods** (`pop()` / `push()`) over the global static
  rather than free fns that read raw `static mut` state.
- **`extern "C"` wrappers** are thin (≤ 10 lines each) —
  null-check + transmute + delegate to the idiomatic core.

## ABI invariants (must not change)

- `STR_HDR_SIZE` = 16. `STR_LEN_OFF` = 8. `STR_DATA_OFF` = 16.
- Packed header init = `1u64 | ((Tag::Str as u64) << 32)` — one
  8-byte store sets `refcount=1`, `type_tag=0` (Str), `flags=0`.
- `STR_POOL_PAYLOAD` = 16 bytes. `STR_POOL_SLOTS` = 32. Blocks at
  the pool size class share `header(16) + payload(16)` = 32-byte
  total; larger allocs go straight to `malloc`.
