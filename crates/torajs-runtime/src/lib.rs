//! v0.3 #6 Graduation — torajs-runtime crate.
//!
//! Holds the C source files that get embedded into every `tr build`
//! artifact. The compiler crate (`torajs-core`) consumes [`SOURCES`]
//! to write them into the per-build temp dir and `cc -c` each one,
//! linking the resulting objects into the final binary.
//!
//! Splitting the runtime into its own crate locks the C-side ABI
//! (universal heap header, refcount intrinsics, string/array
//! helpers, regex/Date engines) so the compiler can evolve without
//! accidentally breaking what's been embedded into already-shipped
//! binaries. Future room: a `build.rs` that pre-compiles into a
//! static `libtorajs_runtime.a` for cross-compile and link-time
//! deduplication; for now we keep the compile-on-emit shape that
//! `tr build` already had.

/// String / Array / Object / Number / JSON helpers + universal heap
/// header + ARC intrinsics (`__torajs_rc_inc` / `__torajs_rc_dec`)
/// + the small-Str and Array LIFO pools + libc panic backtrace.
pub const RUNTIME_STR_C: &str = include_str!("runtime_str.c");

/// v0.2 #1 — regex matching engine.
pub const RUNTIME_REGEX_C: &str = include_str!("runtime_regex.c");

/// v0.2 #2 — Date class implementation.
pub const RUNTIME_DATE_C: &str = include_str!("runtime_date.c");

/// v0.5 T-15 — Promise heap layout + microtask queue + executor.
/// T-15.a ships only the heap layout; subsequent sub-steps wire the
/// microtask queue (T-15.c), .then chaining (T-15.d), and main-exit
/// auto-drain (T-15.e).
pub const RUNTIME_PROMISE_C: &str = include_str!("runtime_promise.c");

/// v0.6 T-21 — `fetch(url)` HTTP client (sync MVP via libcurl).
/// Native target only; wasm32-wasi gates the whole TU on
/// `#ifndef __wasi__` and routes through the browser's fetch
/// API instead (T-21.b).
pub const RUNTIME_FETCH_C: &str = include_str!("runtime_fetch.c");

/// v0.6 T-20.b — wasm32-wasi libc ABI bridge. The whole TU is
/// gated on `#ifdef __wasi__` so the native object is empty;
/// on wasm it provides `__torajs_libc_*` wrappers that take
/// `int64_t` (matching tora SSA's Type::I64 size) and pass
/// through to libc with an implicit truncation to size_t (i32
/// on wasm32). ssa_inkwell switches its libc declares to these
/// names when target=Wasm32Wasi; native keeps calling raw libc.
pub const RUNTIME_LIBC_BRIDGE_C: &str = include_str!("runtime_libc_bridge.c");

/// BigInt substrate fully ported to `crates/torajs-bigint/` (P3.3
/// closed, 2026-05-23). Sign-magnitude / u64-limb / schoolbook +
/// Karatsuba mul / two's-complement bitwise / shift floor — all in
/// Rust now. Cross-tier calls resolved at link time against
/// libtorajs_bigint.a.
///
/* Weak family (runtime_weakref.c + runtime_weakmap.c +
 * runtime_weakset.c) deleted entirely across P4.3'-a..P4.3'-d
 * (2026-05-24):
 *   - P4.3'-a: WeakRef owner-side ops (create/deref/drop) → Rust
 *   - P4.3'-b: shared observer registry → Rust; runtime_weakref.c nuked
 *   - P4.3'-c: WeakMap surface → Rust; runtime_weakmap.c nuked
 *   - P4.3'-d: WeakSet surface → Rust; runtime_weakset.c nuked
 * The entire `weak` family lives in pure-Rust `torajs-weak`
 * (libtorajs_weak.a) — 0 C runtime files remain for it. */

/// v0.7 T-26 (slice C) — Bacon-Rajan trial-deletion cycle
/// collector for class instances. Manual `gc()` trigger.
pub const RUNTIME_CYCLE_C: &str = include_str!("runtime_cycle.c");

/* runtime_map.c deleted entirely at P4.3-g (2026-05-24). The full
 * Map/Set surface + MapIter + ArrIter now lives in pure-Rust crates:
 *   - torajs-collections: Map/Set + MapIter family
 *   - torajs-arr::iter   : ArrIter family (was misplaced in
 *                          runtime_map.c when MapIter graduated) */

/// All C runtime translation units in (filename, contents) form, in
/// the order they should be written + cc'd. Filename is the basename
/// the compiler should write into the per-build temp directory.
pub const SOURCES: &[(&str, &str)] = &[
    ("runtime_str.c", RUNTIME_STR_C),
    ("runtime_regex.c", RUNTIME_REGEX_C),
    ("runtime_date.c", RUNTIME_DATE_C),
    ("runtime_promise.c", RUNTIME_PROMISE_C),
    ("runtime_fetch.c", RUNTIME_FETCH_C),
    ("runtime_libc_bridge.c", RUNTIME_LIBC_BRIDGE_C),
    ("runtime_cycle.c", RUNTIME_CYCLE_C),
];
