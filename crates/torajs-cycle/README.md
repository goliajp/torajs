# torajs-cycle

[![Crates.io](https://img.shields.io/crates/v/torajs-cycle?style=flat-square&logo=rust)](https://crates.io/crates/torajs-cycle)
[![docs.rs](https://img.shields.io/docsrs/torajs-cycle?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-cycle)
[![License](https://img.shields.io/crates/l/torajs-cycle?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-cycle?style=flat-square)](https://crates.io/crates/torajs-cycle)

Bacon & Rajan trial-deletion cycle collector for the [torajs] AOT
TypeScript runtime. Detects + reclaims reference cycles that pure
refcounting (`torajs-rc`) can't free. 0 Cargo deps.

Extracted from `runtime_cycle.c` (510 LOC of C) as **P4.4** (commit
`eab2435`, 2026-05-24) — closed Phase 1's P4 (Array / Object / Map /
Set / Weak / Cycle substrate).

## Algorithm

[Bacon & Rajan 2001 "Concurrent Cycle Collection in Reference Counted
Systems"](https://citeseerx.ist.psu.edu/document?repid=rep1&type=pdf&doi=ee1cb38e21a7c61fb1e8a9b8dadcdce8c4571767):

1. **Buffer**: rc_dec hooks call `__torajs_cycle_buffer_push(p)` on
   non-zero-ref decs that touch potential cycle roots (class instances
   + arrays).
2. **Threshold trigger**: buffer fill > N or main exit → `collect()`.
3. **Mark phase** — recursive walk from each buffered root, painting
   reachable nodes GRAY.
4. **Scan phase** — recursive walk again, painting nodes WHITE if
   their refcount == count-of-internal-references (i.e. only the
   cycle reaches them), or BLACK if they have an external ref.
5. **Collect phase** — free every WHITE node + recurse to its child
   references.

## Module layout

| Module | LOC | Purpose |
| --- | ---: | --- |
| `lib.rs` | ~200 | Top-level API + colors + buffer push/unpush hooks |
| `layout.rs` | ~80 | Header layout (cycle color bits in flags) + child-walk dispatch |
| `buffer.rs` | ~200 | Global trial-root buffer with thresholded auto-collect |
| `collect.rs` | ~260 | Mark / scan / collect trial-deletion loop |
| `arr.rs` | ~110 | Array<*>'s child-walk impl |

## Cross-tier deps

| extern | Provider |
| --- | --- |
| `__torajs_class_layouts` | ssa_inkwell-emitted at codegen — per-class child-offset table |
| `__torajs_n_class_layouts` | ssa_inkwell-emitted at codegen — count |
| `__torajs_rc_dec` | torajs-rc |
| Per-tag `_drop` | torajs-value-drop |

## When does it run

1. **Auto**: when the trial buffer exceeds threshold (`AUTO_COLLECT_THRESHOLD`
   = 8192 candidates today).
2. **Manual**: `gc()` from user code.
3. **Main exit**: drained from the `main()` post-return hook.

## What's NOT in scope

- **Generational collection**: single generation. Cycle detection
  doesn't need it.
- **Incremental / concurrent collection**: single-threaded; the
  collector runs to completion synchronously.
- **Weak reference integration**: handled by `torajs-weak`'s separate
  observer registry — out of this crate's scope.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
