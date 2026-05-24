# torajs-promise

[![Crates.io](https://img.shields.io/crates/v/torajs-promise?style=flat-square&logo=rust)](https://crates.io/crates/torajs-promise)
[![docs.rs](https://img.shields.io/docsrs/torajs-promise?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-promise)
[![License](https://img.shields.io/crates/l/torajs-promise?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-promise?style=flat-square)](https://crates.io/crates/torajs-promise)

`Promise<T>` substrate for the [torajs] AOT TypeScript runtime. Full
spec surface (state machine + `.then` / `.catch` / `.finally` + 4 sync
combinators + thenable absorption + microtask integration). 0 Cargo
deps; paired with [`torajs-microtask`] for queue dispatch.

Extracted from `runtime_promise.c` (~1071 LOC of C) as **P6.1**
(commit `b6f2a71`, 2026-05-24). 7 modules, ~1.3 KLOC.

## State machine

```text
       PENDING  →  FULFILLED   (via resolve(v))
          ↓
          ↓     →  REJECTED    (via reject(reason))
          ↓
       (callback chain attached pre-resolve fires on transition)
```

Heap layout: 32 B (universal header + state byte + ... + callback head).

## Combinators

- `Promise.all(iterable)` — fulfilled when every input resolves; first
  rejection bubbles up.
- `Promise.allSettled(iterable)` — never rejects; always returns array
  of `{status, value}` / `{status, reason}` records.
- `Promise.race(iterable)` — fulfilled / rejected by the first
  settled input.
- `Promise.any(iterable)` — fulfilled by the first fulfillment; rejects
  with `AggregateError` if all inputs reject.

## Performance

The crate ships with a bounded free-list **pool** (32 slots, P-PERF.A6
ship commit `8f754ca` baseline) that recycles Promise heap blocks.
Measured win on the workspace's promise-heavy bench corpus:

| Case | Before pool | After pool | Delta |
| --- | ---: | ---: | ---: |
| promise-await-100k | ~3.0 ms | 1.94 ms | **-35%** |
| promise-then-100k | ~5.8 ms | 4.38 ms | **-25%** |
| async-fn-call-100k | ~3.0 ms | 1.92 ms | **-36%** |

(Per Phase 1 closed baseline at `bench/results/2026-05-24-mini-9b7740c.json`.)

## Modules (7 files, ~1.3 KLOC)

| Module | Purpose |
| --- | --- |
| `lib.rs` | Re-exports + extern boundary |
| `layout.rs` | Heap layout (header + state + value + cb head) |
| `alloc.rs` | Promise construction + pool + drop |
| `state.rs` | resolve / reject + state transitions + cb-chain dispatch |
| `then.rs` | .then / .catch / .finally (simple + closure variants) |
| `combinator.rs` | .all / .allSettled / .race / .any |
| `thenable.rs` | Thenable absorption |
| `queue_microtask.rs` | queueMicrotask global |

## What's NOT in scope

- **Promise subclassing**: `Promise.prototype.constructor` follows
  the platform's PromiseSubclass invariant but custom subclasses
  not yet fully exercised.
- **Promise rejection tracking / unhandled rejection hook**:
  available as a runtime debugger feature, not yet surfaced.

## License

Dual-licensed: Apache-2.0 / MIT — see [LICENSE-APACHE](LICENSE-APACHE)
+ [LICENSE-MIT](LICENSE-MIT).

[torajs]: https://torajs.com
[`torajs-microtask`]: https://crates.io/crates/torajs-microtask
