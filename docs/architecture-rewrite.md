# torajs runtime — architecture rewrite plan

C runtime (~6 kLoC across `runtime_*.c` files) is **rewritten to Rust**,
restructured **by concern (layered architecture)** not by source-file
organization, and each resulting layer module published as an
**independent crates.io crate under `torajs-*`** — mirroring mailrs's
working method.

This file is the **canonical design document** for that rewrite. Before
landing the first commit of a new crate, the design here is the
agreement; deviations require a doc update commit first.

## Motivation

| Driver | Why |
|---|---|
| Perf-first, zero loss | `[profile.release]` already `lto = "fat" + codegen-units = 1`. Rust+C 在 fat-LTO 之下跨 crate / 跨 source language inline 等价；rewrite 本身不引入 perf cost. **Pre-condition: every crate ship must hold geomean ≥ current 4.41× vs bun-aot + 26/26 wins**. |
| Polish velocity ↑ | criterion (statistical micro-bench) / iai (instruction-count, zero-thermal-noise) / miri (UB detection) / cargo-show-asm / flamegraph — Rust 工具链显著比 C 直接，micro-iteration 从分钟级降到秒级. |
| crates.io publish | 每个 layer 模块独立发到 `crates.io/torajs-<name>`，开源社区可用，docs.rs 自动文档. |
| Architecture clarity | 当前 `runtime_str.c` 3000+ 行混了 str / arr / dynobj / heap header / rc / drop dispatch — 单文件多 concern. Rewrite 按 layered architecture 重新组织，单 crate 单 concern. |

**Non-goals**:
- ❌ Perf single-shot jump (例如 4.41× → 6×). Rust rewrite 在 LLVM 上限内 ±1%, 大幅 push 仍要靠 PGO / 算法 / inline tuning 等独立工作.
- ❌ API stable v1.0 一上来. 重写期 sub-crate 设 v0.1.x semver, API 可 break (commit 注明).
- ❌ ABI / FFI 边界引入 (内部不暴露 `extern "C"` 不必要; fat-LTO 之下 Rust pub fn = C extern fn 等价 inline behavior).

## Layered architecture

```
┌─────────────────────────────────────────────────┐
│ Layer 5 surface                                 │
│   torajs-promise   torajs-fetch   torajs-bigint │
│   torajs-regex (surface methods)                │
├─────────────────────────────────────────────────┤
│ Layer 4 dispatch                                │
│   torajs-microtask   (regex VM internal mod)   │
├─────────────────────────────────────────────────┤
│ Layer 3 containers                              │
│   torajs-arr   torajs-dynobj   torajs-collections│
│   torajs-cycle                                  │
├─────────────────────────────────────────────────┤
│ Layer 2 primitives                              │
│   torajs-str   torajs-num                       │
├─────────────────────────────────────────────────┤
│ Layer 1 foundation                              │
│   torajs-rc   torajs-anyvalue   torajs-throw    │
│   torajs-ucd                                    │
├─────────────────────────────────────────────────┤
│ Layer 0 allocator                               │
│   torajs-pool                                   │
└─────────────────────────────────────────────────┘
```

**Dependency rule**: a Layer-N crate may depend only on Layer-(N-1) or
lower. No circular deps. Cross-layer dep goes through a re-export
shim, not direct.

## Crate inventory

15 published crates. Each has the standard mailrs-style layout (see
"Per-crate template" below).

### Layer 0 — allocator

**`torajs-pool`**
- Generic fixed-size + size-class memory pool.
- API: `FixedPool<T, const CAP: usize>`, `SizeClassPool` (variable
  size with size-class buckets).
- Source: A6 `runtime_promise.c::promise_pool_head_/count_` 泛化 + 加
  size-class 支持给 closure env / capture box / dynobj.
- Keywords: `["allocator", "pool", "memory", "torajs"]`
- Categories: `["memory-management", "data-structures", "no-std"]`

### Layer 1 — foundation

**`torajs-rc`**
- Universal heap-header layout + refcount inc/dec + drop dispatch by
  type tag.
- API: `HeapHeader` (8-byte `{rc:u32, type_tag:u16, flags:u16}`),
  `unsafe fn rc_inc(p: *mut HeapHeader)`, `unsafe fn rc_dec(p: *mut
  HeapHeader) -> bool` (true = caller should free), `RcDrop` trait
  (per-type-tag drop fn pointer registry).
- Source: `runtime_str.c` rc / heap header 部分 + `value_drop_heap`
  tag-dispatch table.
- Keywords: `["refcount", "rc", "torajs", "heap"]`
- Categories: `["memory-management"]`

**`torajs-anyvalue`**
- Any-box tag encoding + unbox helpers + ToString tag-dispatch.
- API: `AnyTag` (enum-like `repr(u8)` with NULL / UNDEF / BOOL / I64 /
  F64 / HEAP / SUBSTR variants), `unbox_tag(any) -> AnyTag`,
  `unbox_value(any) -> i64`, `to_str(tag, value) -> *Str`.
- Source: `runtime_str.c` any_box / any_unbox / any_to_str.
- Depends on: `torajs-rc` (for HEAP variant drop).

**`torajs-throw`**
- Catchable throw-slot machinery + native-error registry (TypeError /
  RangeError / Error).
- API: `throw_set(tag: AnyTag, value: i64)`, `throw_check() -> bool`
  (true if active), `throw_take() -> AnyValue`, `register_native_error
  (slot: u8, factory: fn(*Str) -> *Obj)`.
- Source: `runtime_str.c::torajs_throw_native` etc. + `__torajs_throw_*`
  exported wrappers.
- Depends on: `torajs-anyvalue`.

**`torajs-ucd`**
- Unicode Character Database subset — Letter / Number / ASCII /
  Script-property tables for regex `\p{L}`, `\p{N}` etc.
- API: `is_letter_cp(cp: u32) -> bool`, `is_number_cp(cp: u32) -> bool`,
  property-by-name lookup.
- Source: `runtime_regex.c` UCD tables (hand-curated subset; v1+ may
  auto-import from `UnicodeData.txt`).
- No deps (pure data + lookup).
- Categories: `["text-processing", "no-std"]`

### Layer 2 — primitives

**`torajs-str`**
- Str + Substr heap types + pooled alloc + string operations (concat /
  slice / split / trim / starts_with / index_of / char_code_at /
  replace / replaceAll).
- API: `Str` (pointer-stable, refcount, heap-pooled), `Substr` (zero-
  copy view), per-op fn.
- Source: `runtime_str.c` 主体 (~2000 lines).
- Depends on: `torajs-pool` (str pool), `torajs-rc`.
- Categories: `["text-processing", "data-structures"]`

**`torajs-num`**
- Math namespace fns + Number coercion (`ToNumber` per spec).
- API: `to_number(any: AnyValue) -> f64`, `Math` namespace (`sqrt`,
  `pow`, etc.).
- Source: scattered Math intrinsic implementations in `ssa_inkwell` +
  any cross-cutting num coerce in `runtime_str.c`.

**`torajs-bigint`**
- Arbitrary-precision integer (BigInt) arith with libc-malloc backing.
- API: `BigInt` heap type, add/sub/mul/div/mod/shift/cmp.
- Source: `runtime_bigint.c`.
- Depends on: `torajs-rc`, `torajs-throw` (RangeError on div-by-zero).
- Keywords: `["bigint", "torajs", "arbitrary-precision"]`

### Layer 3 — containers

**`torajs-arr`**
- `Arr<T>` head-aware deque (ring buffer for amortized O(1) `shift`)
  + push / pop / shift / unshift / iter methods + `arrprops` side-
  table for `arr.<unknown-prop>` access.
- API: `Arr<T>`, `arr_alloc`, `arr_push_unchecked`, `arr_shift`,
  `arr_drop`, `arrprops_get/set`.
- Source: `runtime_str.c` arr* fns.
- Depends on: `torajs-pool`, `torajs-rc`.
- Keywords: `["array", "deque", "ringbuffer", "torajs"]`
- Categories: `["data-structures"]`

**`torajs-dynobj`**
- Hashtable-backed dynamic object (`{x: 1, y: 2}` heap struct) +
  bucket + ANY-tag value storage.
- API: `DynObj`, `dynobj_alloc`, `dynobj_get/set/has/delete`.
- Source: `runtime_str.c` dynobj* fns.
- Depends on: `torajs-pool`, `torajs-rc`, `torajs-anyvalue`,
  `torajs-str` (keys are Str).

**`torajs-collections`**
- Map / Set / WeakMap / WeakSet / WeakRef + iterators (MapIter,
  ArrIter).
- API: `Map`, `Set`, `WeakMap`, `WeakSet`, `WeakRef`, `MapIter`,
  `ArrIter`, per-type methods.
- Source: scattered across `runtime_str.c` (Map/Set use dynobj
  internally) + WeakRef registry.
- Depends on: `torajs-dynobj`, `torajs-rc`.

**`torajs-cycle`**
- Reference cycle detector (Bacon-Rajan-style trial deletion).
- API: `cycle_unbuffer()`, integration hooks for `__torajs_rc_dec` to
  buffer suspect roots.
- Source: `runtime_cycle.c`.
- Depends on: `torajs-rc`.

### Layer 4 — dispatch

**`torajs-microtask`**
- Microtask queue + drain machinery for Promise callbacks.
- API: `Queue::enqueue(fn: extern "C" fn(i64), arg: i64)`,
  `Queue::drain_until_idle()`.
- Source: `runtime_promise.c` microtask queue portion (T-15.c / T-15.e
  / T-16 await drain).
- Depends on: `torajs-pool` (callback node pool).
- Keywords: `["microtask", "event-loop", "torajs", "promise"]`

`torajs-regex` is in Layer 4 internally (regex VM is dispatch-ish) but
exposes only Layer-5-style surface so listed under Layer 5 below.

### Layer 5 — surface

**`torajs-promise`**
- Promise<T> state machine + `Promise.resolve/.reject/.all/.race/
  .any/.allSettled` + `.then/.catch/.finally` + await dispatch.
- API: `Promise<T>` heap type, all surface methods + alloc helpers.
- Source: `runtime_promise.c` surface portion (~1000 lines).
- Depends on: `torajs-pool` (Promise pool), `torajs-rc`,
  `torajs-microtask`, `torajs-throw`.

**`torajs-regex`**
- RegExp surface methods (`.exec`, `.test`, `match`, `matchAll`,
  `replace`, `replaceAll`, `split`). Internal sub-modules: `ir`,
  `compile`, `vm`.
- API: `RegExp` heap type + per-method fn.
- Source: `runtime_regex.c` 整片 (~3000 lines).
- Depends on: `torajs-rc`, `torajs-anyvalue`, `torajs-ucd`,
  `torajs-str`.
- Keywords: `["regex", "torajs", "ecmascript", "javascript"]`
- Categories: `["text-processing"]`

**`torajs-fetch`** *(optional / behind `fetch` feature)*
- HTTP fetch via libcurl (lib lookup at build.rs).
- API: `fetch_sync(url: &str) -> Response`.
- Source: `runtime_fetch.c`.
- Depends on: `torajs-rc`, `torajs-str`.
- Note: pulls libcurl as a sys-dep — keep behind feature flag so
  consumers without `--feature fetch` get a slimmer build.

### Glue crate

**`torajs-runtime`** (existing crate, **reshaped**)
- Becomes a **thin shim crate** that re-exports the 15 layer crates +
  glues their fn names to the `__torajs_*` symbols that `ssa_lower`
  emits IR-level calls to.
- API: re-exports + extern "C" wrappers.
- Source: small (~200 lines) hand-written glue.
- Stays in workspace but **NOT published to crates.io** (tora-
  specific; depends on `ssa_lower`'s symbol naming convention).

## Per-crate file template

Mirror mailrs. Each `crates/torajs-<name>/`:

```
crates/torajs-<name>/
├── Cargo.toml          (workspace metadata inherited + crate-specific)
├── src/
│   ├── lib.rs          (pub API)
│   └── ...             (internal mods)
├── benches/
│   └── <name>.rs       (criterion micro-bench, harness = false)
├── tests/
│   ├── perf_gate.rs    (budget regression catch)
│   └── ...             (unit / integration tests)
├── BUDGETS.md          (per-bench budget table + derivation)
├── CHANGELOG.md        (Keep-a-Changelog format)
├── LICENSE-APACHE      (Apache-2.0 text)
├── LICENSE-MIT         (MIT text)
└── README.md           (crates.io + docs.rs badges + short description)
```

## Per-crate Cargo.toml template

```toml
[package]
name = "torajs-<name>"
version = "0.1.0"  # 0.x until API stable
edition = "2024"
description = "<one-line subject; per crate>"
license = "Apache-2.0 OR MIT"
repository = "https://github.com/goliajp/torajs"
homepage = "https://github.com/goliajp/torajs"
documentation = "https://docs.rs/torajs-<name>"
authors = ["GOLIA K.K."]
keywords = ["torajs", "<...>"]  # max 5
categories = ["<...>"]
readme = "README.md"

[lints]
workspace = true

[dependencies]
# crate-specific

[dev-dependencies]
criterion = { workspace = true }
# crate-specific test deps

[[bench]]
name = "<name>"
harness = false
```

## Workspace updates required (Phase 0)

Apply once to root `Cargo.toml` before first sub-crate ships.

### `[workspace.package]`

Add centralized metadata for all sub-crates to inherit:

```toml
[workspace.package]
version = "0.1.0"
edition = "2024"
license = "Apache-2.0 OR MIT"
repository = "https://github.com/goliajp/torajs"
homepage = "https://github.com/goliajp/torajs"
authors = ["GOLIA K.K."]
```

### `[workspace.dependencies]`

Pin shared deps so workspace bumps are atomic.

```toml
[workspace.dependencies]
criterion = { version = "0.8", default-features = false, features = ["cargo_bench_support"] }
# add others as crates need (libc / serde / etc.)
```

### `[workspace.lints]`

Mirror mailrs's zero-warnings stance.

```toml
[workspace.lints.rust]
warnings = "deny"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
```

### `[profile.release-vanilla]` — honest baseline

Mirror mailrs's vanilla-Rust baseline profile so we can `cargo build
--profile release-vanilla` for any "is fat-LTO actually worth the
compile cost?" question.

```toml
[profile.release-vanilla]
inherits = "release"
lto = false
codegen-units = 16
opt-level = 3
```

## Acceptance gate (every sub-crate ship)

A new sub-crate **may not ship** until it passes ALL of:

1. `cargo build --workspace --release` — 0 error, 0 warn
2. `cargo fmt --check` — clean
3. `cargo test --workspace --release` — all green incl. the new
   crate's unit + perf_gate tests
4. `cargo run --release --bin torajs-conformance` — **666 / 0 / 1**
   (currently shipping number; must not regress)
5. **`cargo run -p bench-harness --release -- run --runs 5`** —
   geomean vs bun-aot ≥ **4.41× ±1%** AND **26/26 wins held** AND
   no single case regresses past noise band vs the latest shipped
   bench file
6. **New crate ships with**: Cargo.toml (full metadata), src/lib.rs,
   benches/<name>.rs (criterion), tests/perf_gate.rs, BUDGETS.md,
   CHANGELOG.md, README.md, LICENSE-APACHE, LICENSE-MIT

## Workspace-level docs

### `PERFORMANCE.md` (workspace root, mailrs-style)

Single source of truth for **honestly measured** perf claims. Every
number in a commit message / README / blog post must trace to a row
here. Replaces ad-hoc per-commit perf claims.

Initial content seeded from current state:

```markdown
# torajs perf — what's measured

## Workspace-level

| Path | Measurement | Run command |
|---|---|---|
| geomean speedup vs bun-aot | **4.41×** at HEAD 8f754ca (5-pass median,
  26/26 wins, sequential, no concurrent load) | `cargo run -p bench-harness --release -- run --runs 5` |
| geomean speedup vs bun-jsc | **4.52×** | same |
| geomean speedup vs node-v8 | **21.07×** | same |
| Conformance subset | **666 / 0 / 1** | `cargo run --release --bin torajs-conformance` |
| test262 in-scope pass rate | **12.20%** (3455 / 28314) | `cargo run --release -p torajs-test262 -- --json ...` |

## Per-crate (filled in as crates ship)

(empty until torajs-pool lands)
```

### Per-crate `BUDGETS.md`

Mirror mailrs:
- Path taxonomy explanation
- Budget table: path · budget · observed P95 · headroom · notes
- Methodology section
- "When to re-measure" trigger list

## Phased rollout

| Phase | Work | Duration estimate | Status |
|---|---|---|---|
| **P0** | Land this design doc + apply workspace.package / workspace.lints / workspace.dependencies / release-vanilla profile / workspace PERFORMANCE.md skeleton | 1 commit, <1 day | **DONE** `cf91150` |
| **P1 — pilot** | Build `torajs-pool` end-to-end: scaffold, port A6 implementation, criterion + perf_gate + BUDGETS.md + README. Plug Promise pool into it via runtime glue. **Full acceptance gate.** | 2–3 days | **DONE** `de39cc1` (standalone crate; runtime_promise.c pool integration is a P5 follow-up) |
| **P2 — Layer 1** | `torajs-rc`, `torajs-anyvalue`, `torajs-throw`, `torajs-ucd` (foundation) | 1–2 weeks | P2.1 (ucd) **DONE** `cba6a55`; P2.2 (rc) **DONE** `a446c1a` — Rust libtorajs_rc.a links into every `tr build` user binary; P2.3-a (anyvalue scaffold + AnyBox alloc/unbox/drop/payload_rc_inc) **DONE** `cc9f819`; P2.3-b (strict equality — payload_eq + any_any_strict_eq + any_strict_eq) **DONE** `b04d98c`; P2.3-c (any_to_str ToString coercion) **DONE** `a1b5a11`; P2.3-d.1 (any_to_number — ToNumber per ES §7.1.4 + Layer-1 link-wiring bug fix: torajs-core/build.rs now always reruns via non-existent sentinel so the staticlib copy step never goes stale, and conformance runner switched to `--workspace` so the staticlib half of every Layer-1+ sub-crate actually gets rebuilt — previously `-p torajs-cli` only refreshed rlib deps, leaving iter `.a` stale on every sub-crate edit) **DONE** this commit; P2.3-d.2 (any_compare) / .3 (any_arith) / .4 (any_add — string-concat fast path) NEXT — ~200 C lines remaining; P2.4 (throw) queued |
| **P3 — Layer 2** | `torajs-str`, `torajs-num`, `torajs-bigint` | 2–3 weeks |
| **P4 — Layer 3** | `torajs-arr`, `torajs-dynobj`, `torajs-collections`, `torajs-cycle` | 2 weeks |
| **P5 — Layer 4** | `torajs-microtask` | 3–5 days |
| **P6 — Layer 5** | `torajs-promise`, `torajs-regex`, `torajs-fetch` | 2 weeks |
| **P7 — glue cleanup** | Reduce `torajs-runtime` to thin shim; delete original `runtime_*.c` files | 2–3 days |

Total estimate ~2–3 months calendar, gated commit-by-commit on the
acceptance gate. Schedule subject to per-layer surprises (P3 / P4 are
the heaviest; `runtime_str.c` is the densest source).

## Risks + mitigations

| Risk | Mitigation |
|---|---|
| Conformance 666/0/1 regression mid-rewrite | Acceptance gate is non-negotiable; revert any commit that breaks. Pilot proves the gate works before scaling. |
| Bench geomean regression | Same gate; revert + investigate. |
| API design wrong, need break post-publish | All sub-crates start 0.1.x, semver allows breaking. v1.0 only after full rewrite + cross-layer integration validated. |
| Inline behavior changes (Rust pub fn vs C static inline) | All hot helpers get `#[inline]` or `#[inline(always)]` per case; perf_gate catches drift; fat-LTO erases boundary. |
| crates.io publish errors | Test on docs.rs preview before final publish; per-crate cargo publish dry-run first. |
| Schedule slip past 3 months | Acceptable — gate is more important than calendar. Stop / iterate plan if any layer takes 2× est. |

## Naming convention

- crate name: `torajs-<name>` (lower-kebab, hyphen-separated)
- Rust module name in src/lib.rs: `torajs_<name>` (underscore — Rust default)
- crates.io: `crates.io/crates/torajs-<name>`
- docs.rs: `docs.rs/torajs-<name>`
- internal symbol exported to ssa_lower IR: stays `__torajs_<fn_name>`
  (existing convention — ssa_lower emits these as `extern "C"` calls;
  Rust side uses `#[no_mangle] extern "C"` to expose)

## Open questions

1. Does `torajs-runtime` glue crate stay published or remain private?
   - Tentative: private. Tora-specific glue; depends on `ssa_lower`
     symbol naming. Re-export pattern for users to bypass.
2. `no_std` for foundation crates (pool / rc / anyvalue / ucd)?
   - Tentative: try `no_std + alloc`. Avoids dragging tokio /
     std::collections for downstream crate consumers that don't
     need them.
3. SIMD intrinsics use?
   - Tentative: not initial. Keep portable Rust; revisit when a bench
     case shows SIMD as the next leverage.

Update this file whenever a question gets answered, and reference the
commit that did it.
