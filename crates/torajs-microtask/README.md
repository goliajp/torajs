# torajs-microtask

[![Crates.io](https://img.shields.io/crates/v/torajs-microtask?style=flat-square&logo=rust)](https://crates.io/crates/torajs-microtask)
[![docs.rs](https://img.shields.io/docsrs/torajs-microtask?style=flat-square&logo=docs.rs)](https://docs.rs/torajs-microtask)
[![License](https://img.shields.io/crates/l/torajs-microtask?style=flat-square)](#license)
[![Downloads](https://img.shields.io/crates/d/torajs-microtask?style=flat-square)](https://crates.io/crates/torajs-microtask)

Microtask queue for the [torajs] AOT TypeScript runtime: FIFO queue of
`{fn_ptr, arg}` records with head-cursor pop, grow-by-doubling backing
array, and compaction. Drains to empty (including tasks enqueued during
the drain itself) — matches JS spec's microtask semantics. 0 Cargo
deps. Auto-called from `main()` exit hook.

Extracted from `runtime_promise.c`'s microtask section as **P5**
(commit `011936e`, 2026-05-24). Powers `queueMicrotask` + Promise
callback dispatch (`.then` / `.catch` / `.finally` continuations) for
the rest of the torajs runtime.

## Why microtasks

Per [JS spec](https://html.spec.whatwg.org/multipage/webappapis.html#perform-a-microtask-checkpoint):

- Promise reactions, `queueMicrotask`, `MutationObserver` callbacks
  all run on the microtask queue.
- The microtask queue is drained **completely** after the current
  task — including any microtasks scheduled during the drain.
- This is what makes `await` semantics work: the continuation after
  an `await` is a microtask, queued at the moment the awaited promise
  resolves.

## Single global queue (single-threaded)

torajs is single-threaded today; one global queue suffices. The queue
backing is a `Vec<{fn: fn_ptr, arg: i64}>` with head cursor + tail
push. Pop advances head; compaction kicks in when head/cap > 0.5 so
the queue doesn't grow unbounded for long-lived programs.

When threading lands post-v1.0, the queue becomes per-worker.

## ABI

```rust
// Task fn signature: a 1-arg extern "C" fn taking an i64-sized
// argument (caller's choice — refcounted *mut c_void / promise
// pointer / wrapped index / etc.).
pub type MicrotaskFn = unsafe extern "C" fn(arg: i64);

// Enqueue: caller transfers ownership of arg.
pub unsafe extern "C" fn __torajs_microtask_enqueue(
    fn_: Option<MicrotaskFn>, arg: i64,
);

// Drain the queue. Runs FIFO; tasks enqueued during the drain run
// after the current head (matches spec). Returns when the queue
// is empty.
pub unsafe extern "C" fn __torajs_microtask_run_until_idle();

// Inspect pending count — used by Promise spec edge cases (e.g.
// the resolve-or-reject double-call check).
pub unsafe extern "C" fn __torajs_microtask_pending_count() -> usize;
```

## Drain semantics

```text
loop:
  if head == tail: break  // queue empty
  task = queue[head]; head += 1
  task.fn(task.arg)       // task may itself enqueue more
  if head >= queue.len() / 2: compact  // bound head-cursor advance
```

Tasks enqueued during a `fn(arg)` call land at the tail and run in
the same drain. The drain only exits when head catches up to tail.

## License

Dual-licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

[torajs]: https://torajs.com
