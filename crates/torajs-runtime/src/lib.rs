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

/* runtime_regex.c deleted entirely at P6.2-e (2026-05-24). The
 * full ECMAScript regex engine (parser / Thompson NFA compiler /
 * Pike VM matcher + extern API: compile / drop / get_source /
 * test / find / get/set_last_index / str_match_regex /
 * regex_exec / str_match_all_regex / str_replace_regex /
 * str_replace_all_regex / str_replace_regex_fn /
 * str_replace_all_regex_fn / str_split_regex) now lives in pure-
 * Rust `torajs-regex` (libtorajs_regex.a). attach_groups for named
 * captures lives there too and calls back into torajs-dynobj /
 * torajs-arr at link time. */

/// v0.2 #2 — Date class implementation.
pub const RUNTIME_DATE_C: &str = include_str!("runtime_date.c");

/* runtime_promise.c deleted entirely at P6.1 (2026-05-24). The
 * Promise surface (alloc + pool + drop + resolve/reject +
 * .then/.catch/.finally + .all/.allSettled/.race/.any +
 * queueMicrotask) now lives in pure-Rust `torajs-promise`
 * (libtorajs_promise.a). The orthogonal capture-box helpers (3 fns,
 * ~40 LOC) carved out to runtime_capture_box.c — they serve
 * codegen's escape-captured-let promotion, not Promise, and stay
 * C-side until a future cleanup phase folds them into a refcount
 * sub-crate. */

/// v0.5 T-15.g.5 — capture-box ARC for Copy escape-captured lets.
/// Carved out of runtime_promise.c at P6.1 (2026-05-24) — 3 fns
/// (alloc/inc/drop) handling refcount on the 16B heap box used by
/// closures sharing a captured `let` slot. Orthogonal to Promise.
pub const RUNTIME_CAPTURE_BOX_C: &str = include_str!("runtime_capture_box.c");

/* runtime_fetch.c deleted entirely at P6.3 (2026-05-24). The sync
 * `fetch(url)` MVP (libcurl-easy wrapper + Response heap object +
 * response_drop) now lives in pure-Rust `torajs-fetch`
 * (libtorajs_fetch.a). Native target links against the system
 * libcurl via `#[link(name = "curl")]`; wasm32-wasi degrades to
 * the same empty-body Response shape (T-21.b will route through
 * the browser fetch API). */

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

/* runtime_cycle.c deleted entirely at P4.4 (2026-05-24). The full
 * Bacon-Rajan trial-deletion cycle collector (mark/scan/collect
 * phases + buffer + buffer/unbuffer hooks + main-exit drain) now
 * lives in pure-Rust `torajs-cycle` (libtorajs_cycle.a).
 * `__torajs_class_layouts` / `__torajs_n_class_layouts` are still
 * emitted by ssa_inkwell at codegen — torajs-cycle reads them via
 * `extern "C"` at link time. */

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
    ("runtime_date.c", RUNTIME_DATE_C),
    ("runtime_libc_bridge.c", RUNTIME_LIBC_BRIDGE_C),
    ("runtime_capture_box.c", RUNTIME_CAPTURE_BOX_C),
];
