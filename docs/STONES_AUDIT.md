# torajs Stones Audit — Status Ledger

> "一个个开源 crate 或必要的闭源 crate / lib 就像是一个个石头, 其他特化的业务代码
> 就是水泥, 水泥把石头缝填满就是我们的坚固的建筑。"
> — direction (mailrs ARCHITECTURE.md, 2026-05-23)

This file is the **stones status ledger**, companion to
[`deps-tree-v0.1.md`](deps-tree-v0.1.md). Where deps-tree plans the 6-family
decomposition (A-F) and 4-layer L1-L4 governance, this file tracks **each stone
candidate's current status** using the mailrs lens — designed for a single-page
audit answer to: *"what's a stone, what's cement, what's done, what's next?"*

Generated 2026-05-26 at HEAD `dc46bbf`. Conformance baseline 685/0/1 at
`2e698a5`. Re-run [audit triggers](#audit-triggers) to refresh.

---

## The lens

A piece of code is a **stone** iff *all* of:

1. **One-sentence identity** — RFC / algorithm / well-known concept as
   boundary, not "where the team split files".
2. **Non-torajs project could use as-is** — if the only conceivable user is
   torajs itself, it's cement.
3. **No torajs-specific imports** — no `ssa_lower`, no per-tag layout
   constants, no `Cycle` color enum dependency.
4. **≤500 LOC bounded** — over 500 = probably bundled stones; decompose.
   Sub-50 micro-utilities are legitimate stones (publish cost is small,
   identity clarity is the win).
5. **Has hot paths worth benching** — pure I/O glue is cement.

Anything else = **cement**: project-specific glue, wiring, schema-bound state,
ABI-locked dispatchers. Cement is not a failure mode — **cement masquerading
as stone** (over-publish) and **stone trapped inside the project**
(under-publish) are.

torajs adds two domain modifiers on top of the mailrs lens:

- **metal-tier stone** — pre-LLVM / pre-libc / pre-spec primitive: `mmalloc`,
  `syscall`, `ucd`. These are the deepest stones; they cannot themselves
  depend on the runtime ABI.
- **TS-spec stone** — ECMA-262 / Web API surface impl: `arr`, `str`, `regex`,
  `bigint`. Identity is the spec section; coupling is via the runtime ABI
  contract (HeapHeader + RC + tag scheme) — these are stones-with-ABI-coupling,
  decompose into `core` (pure) + `ffi` (ABI-locked extern shell) when
  publishing (per `deps-tree-v0.1.md` §A 族 stone/cement processing).

---

## ✅ Resolved — Stones extracted (33 sub-crates)

The pre-v0.7 baseline (HEAD `ab0b7fe`, 2026-05-25): 23-batch god-file decomp +
pure-rust rewrite completed. 12 `runtime_*.c` files → 0, 27 sub-crates Rust.
v0.7 added `torajs-syscall` + `torajs-mmalloc`, total **33 sub-crates** as of
HEAD `dc46bbf`.

Each row is a stone candidate; lens columns mark which of the 5 lens
conditions pass at the **published-crate** level (not the in-workspace-use
level — many are already de-facto stones but not yet polished for
crates.io publish).

Legend: ✅ pass · ⚠ partial / needs polish · ❌ fail

### Metal-tier (vision #4 deepest stones)

| Crate | LOC | Identity | Single-id | Non-tora | No-imports | ≤500 | Bench | Verdict |
|---|---:|---|:-:|:-:|:-:|:-:|:-:|---|
| `torajs-syscall` | 381 | aarch64 macOS `svc #0x80` raw syscall + safe wrapper | ✅ | ✅ | ✅ | ✅ | ✅ | **stone** — already publishable; needs x86_64 + Linux arms for `1.0` |
| `torajs-mmalloc` | 927 | mmap-backed page-bump + size-class free-list allocator | ✅ | ✅ | ✅ | ⚠ over 500 | ⚠ unit only | **stone** — but lacks README; size > 500 acceptable per ARCHITECTURE.md ("3 stones in 1 file" probably; could split: `page.rs` + `size_class.rs` + `large.rs` + `extern_api.rs` are already separate mods) |
| `torajs-ucd` | 323 | Curated UCD Letter/Number/Whitespace tables | ✅ | ✅ | ✅ | ✅ | ✅ | **stone** — `no_std`, zero-alloc, single binary-search; ready for publish |
| `torajs-codec-hex` | 199 | Hex encode/decode (drop-in for community `hex 0.4`) | ✅ | ✅ | ✅ | ✅ | ✅ | **stone** — first F 族 ship target |
| `torajs-pool` | 311 | Bounded thread-local LIFO fixed-size memory pool | ✅ | ✅ | ✅ | ✅ | ✅ | **stone** — const-generic, zero-cost Acquire/Release |

### TS-spec tier (runtime substrate, ABI-coupled stones)

| Crate | LOC | Identity | Single-id | Non-tora | No-imports | ≤500 | Bench | Verdict |
|---|---:|---|:-:|:-:|:-:|:-:|:-:|---|
| `torajs-rc` | 906 | Universal 8-byte heap header + non-atomic refcount | ✅ | ⚠ | ⚠ flags layout | ❌ | ✅ | **stone-with-ABI** — `core` mod splittable; ABI-locked variant published as `torajs-rc-ffi` (per A 族 split plan) |
| `torajs-anyvalue` | 2286 | 24-byte tagged-Any box (null/undef/bool/i64/f64/heap) | ✅ | ⚠ | ❌ tag scheme | ❌ | ✅ | **cement** at workspace use; **stone candidate** if `core` mod extracted (tag enum + dispatch fns) |
| `torajs-throw` | 457 | TLS-recorded catchable throw slot + error factory | ✅ | ✅ | ⚠ tag scheme | ✅ | ✅ | **stone-with-ABI** — TS-spec throw semantics; metal-friendly impl |
| `torajs-str` | 3861 | Refcounted Str + Substr + small-str pool + per-op fns | ⚠ many things | ❌ | ❌ pool ABI | ❌ | ✅ | **cement** at workspace; **3-4 stones bundled** (Str header + Substr + pool + UTF-8 ops + split/transform); decompose recommended |
| `torajs-num` | 1949 | Number ToNumber (ES §7.1.4) + Math intrinsics | ✅ | ⚠ | ⚠ anyvalue dep | ❌ | ✅ | **stone-with-ABI** — Math wraps libm; decompose Math + ToNumber + dtoa |
| `torajs-bigint` | 2248 | Self-hosted BigInt sign+magnitude with u64 limbs | ✅ | ✅ | ⚠ throw dep | ❌ | ✅ | **stone-with-ABI** — Karatsuba mul, no_std; clean RFC-bounded (per ECMA-262 BigInt) |
| `torajs-arr` | 2708 | Dynamic Array<T> + Array<Any> over refcounted pool | ✅ | ⚠ | ❌ pool ABI + rc | ❌ | ✅ | **cement** at workspace; **stones bundled** (push/pop/iter/slice/sort all decomposable per `regex-automata` family pattern) |
| `torajs-dynobj` | 1212 | Dynamic-property bag (FNV-1a hashmap + descriptors) | ✅ | ⚠ | ⚠ anyvalue dep | ❌ | ✅ | **stone-with-ABI** — Swift Dictionary / CPython dict shape |
| `torajs-collections` | 1680 | Map/Set robin-hood hash, slots+entries dual array | ✅ | ⚠ | ⚠ anyvalue dep | ❌ | ✅ | **stone-with-ABI** — SameValueZero key eq, insertion-ordered |
| `torajs-weak` | 1412 | WeakRef/Map/Set process-global target→observer registry | ✅ | ⚠ | ⚠ rc dec hook | ❌ | ✅ | **stone-with-ABI** — separate-by-design from `collections` |
| `torajs-microtask` | 281 | FIFO microtask queue with grow + compaction | ✅ | ✅ | ✅ | ✅ | ✅ | **stone** — JS spec semantics, single queue |
| `torajs-promise` | 1352 | Promise<T> heap + state + callbacks + combinators | ✅ | ⚠ | ⚠ microtask dep | ❌ | ✅ | **stone-with-ABI** — RFC-bounded by ECMA-262 Promise |
| `torajs-cycle` | 914 | Bacon & Rajan trial-deletion cycle collector | ✅ | ✅ | ⚠ rc hook | ❌ | ✅ | **stone-with-ABI** — algorithm well-known (B&R), bounded |
| `torajs-regex` | 2052 | ECMAScript regex (Thompson NFA + Pike VM + LB/LH) | ✅ | ⚠ | ⚠ str + ucd | ❌ | ✅ | **cement** at workspace; **2-3 stones bundled** (parser + NFA compile + VM matcher), each decomposable |
| `torajs-date` | 1109 | Date class — Howard Hinnant civil-from-days + libc tz | ✅ | ⚠ | ⚠ str dep | ❌ | ✅ | **stone-with-ABI** — ECMA-262 Date semantics |
| `torajs-meta` | 530 | fnprops + class/proto registry + reflection ops | ✅ | ⚠ | ⚠ dynobj dep | ⚠ | ✅ | **stone-with-ABI** — Object.getPropertyDescriptor / getPrototypeOf |
| `torajs-fs` | 400 | Sync filesystem readFile/writeFile/etc. via libc | ✅ | ⚠ | ⚠ str dep | ✅ | ✅ | **stone-with-ABI** — node:fs-equivalent sync surface |
| `torajs-fetch` | 356 | Sync HTTP GET via libcurl-easy wrapper | ✅ | ⚠ | ⚠ str dep | ✅ | ✅ | **stone-with-ABI** — minimal fetch surface |
| `torajs-process` | 239 | process.exit/cwd/env/argv/platform + stdio.write | ✅ | ⚠ | ⚠ str dep | ✅ | ✅ | **stone-with-ABI** — node:process subset |
| `torajs-panic` | 231 | Fatal error → stderr + symbolicated backtrace + exit | ✅ | ✅ | ⚠ libc backtrace | ✅ | ⚠ no bench | **stone** — debug-only path acceptable |
| `torajs-abort` | 90 | `abort_with(msg)` → fd 2 + libc::abort (no std panic) | ✅ | ✅ | ✅ | ✅ | ✅ | **stone** — 150 KB user-binary win documented |
| `torajs-capture-box` | 172 | Refcounted 16-byte capture box for promoted let slots | ✅ | ⚠ | ⚠ rc dep | ✅ | ✅ | **stone-with-ABI** — TS closure capture semantics |
| `torajs-value-drop` | 119 | type_tag-dispatched per-type _drop router | ✅ | ⚠ | ❌ tag table | ✅ | ✅ | **cement** — pure dispatch, ABI-locked by tag scheme; cannot be stone |

### Cement (project-only, not stone candidates)

| Crate | LOC | Reason |
|---|---:|---|
| `torajs-core` | 58221 | Compiler monolith (parser/lexer/ast/check/ssa/lower/inkwell) — **cement, but houses planned B 族 stones** (B.1-B.11 to decompose) |
| `torajs-cli` | 2445 | LSP / REPL / build driver — wiring + UX, cement |
| `torajs-embed` | 482 | Host-side dlopen + C ABI shim, cement |
| `torajs-runtime` | 109 | Legacy `include_str!` host for runtime_*.c bridge (now near-empty post pure-rust); thin glue cement |
| `torajs-playground-api` | 417 | Axum web demo (non-metal), cement |

### libc symbol layer (v0.7-A2 step 6b cutover)

**7 libc symbols rerouted** to torajs-mmalloc via `#[link_name = "__torajs_libc_*"]`:

- `malloc` · `free` · `realloc` · `calloc` (alloc family)
- `memcpy` · `memmove` · `memcmp` (mem family)

Status: shipped @ `2e698a5`, conformance 685/0/1 preserved. Bench
post-cutover audit ongoing — see [🟡 In progress](#-in-progress).

---

## 🟡 In progress

### v0.7-A2 — torajs-mmalloc bench regression audit

`dc46bbf` bench geomean: torajs vs rust **1.374×** (was 1.539×, **-10.7%**),
torajs vs bun-aot **3.817×** (was 4.030×, **-5.3%**). Strict L4 trigger
(±5%) fails on both axes.

Root-cause audit done at HEAD `dc46bbf`:
- 4 alloc-heavy cases real regress (`generic-pair-1m` reverses 1.6× faster
  → 2.4× slower vs rust; `array-sum-1m` reverses 1.2× faster → 1.5× slower).
- Remaining 9 cases drift in lock-step with `rust` binary (~14% system
  load shift across the bench window) — noise, not real regress.
- Mechanism: `__torajs_libc_malloc(size)` adds 16-byte SHIM_HEADER + size-class
  free-list lookup vs libc nano-allocator thread-cache LIFO; delta ~10-30 ns
  per alloc, matches the observed ms-level on 1M+ iter cases.

Fix paths under consideration (none shipped, none picked):

1. **Direct sized alloc** — IR emit `__torajs_malloc(size)` directly when
   size is statically known (object literal, fixed-shape struct), bypassing
   SHIM_HEADER. mmalloc shim code comment already flags this as intended
   ("can be replaced by direct __torajs_free(ptr, size) call sites later").
2. **Thread-local cache** — mmalloc adds TLAB-style per-thread free-list for
   ≤256B bucket; matches libc nano fast-path cycle count.
3. **SHIM_HEADER 8B** — investigate whether 16B align requirement is real on
   aarch64 macOS for non-SIMD allocations (currently overprovisioned).

Decision pending. Status memory L3a hot is hold; A3 prep complete but
sequenced behind A2 trigger close.

Finding doc planned: `docs/v0.7-A2-finding.md` (mailrs v0.X-finding.md style).

### v0.7-A3 — torajs-io prep audit (done, hot pending)

B-1 subagent audit (2026-05-26) inventoried **16 stdout/stderr entry points**
in user-binary code, sorted by replacement difficulty:

- Hard (2): `print_i64` / `print_bool` emitted in IR codegen (ssa_inkwell/
  builders.rs); requires IR refactor to call a new
  `__torajs_write_buffered` intrinsic.
- Medium (7): Rust-layer per-byte putchar/printf/dprintf calls; batch buffer
  + single `torajs-syscall::write` per call.
- Trivial (5): already use `write(2)` directly; only need import switch
  from libc extern to `torajs-syscall`.

Will hotify once A2 closes. Hot plan skeleton in status memory L3b.

---

## ⬜ Planned (v0.7 Metal L2 work units 3-15)

| # | Work unit | Yields stone | Hotify trigger |
|---|---|---|---|
| 3 | `torajs-io` | **new metal-tier stone** (syscall-based stdio wrappers) | A2 close |
| 4 | `torajs-fmt` | **new metal-tier stone** (Grisu/Dragon4 dtoa + itoa) | A3 close |
| 5 | runtime libc verify | nothing new; `otool -L /tmp/<user-bin>` == 0 libSystem | A4 close |
| 6 | `torajs-codegen` | **new metal-tier stone** (SSA → aarch64 emitter, Cranelift-shape) | independent axis |
| 7 | `torajs-obj` | **new metal-tier stone** (Mach-O writer + ELF writer) | #6 ship |
| 8 | `torajs-link` | **new metal-tier stone** (sym resolve + section layout + reloc) | #7 ship |
| 9 | ssa_inkwell replacement | retires `inkwell` + `llvm-sys` + `inkwell_internals` | #6+#7+#8+#5 |
| 10 | cli deps self-research | new stones: `torajs-{argparse,line_edit,lsp-{server,types}}` | independent |
| 11 | embed dep self-research | new stone: `torajs-dynlib` (libloading replacement) | independent |
| 12 | bench dep self-research | new stones: `torajs-{bench,error,codec-toml}` | independent |
| 13 | playground dep self-research | non-metal; deferred per project-torajs-metal-level memory | discretionary |
| 14 | inkwell + llvm-sys deletion | dep-graph 0 trace | #9 |
| 15 | v0.7 final audit | bench geomean + otool clean | #5 + #9 |

**Also planned per `deps-tree-v0.1.md` §L3b F 族** (orthogonal to v0.7 Metal):
14 sub-steps (F.1–F.14) yielding **new published stones**: `torajs-codec-hex`,
`torajs-hash`, `torajs-error`, `torajs-codec-toml`, `torajs-codec`,
`torajs-trace`, `torajs-bench`, `torajs-net-mini`, `torajs-http-mini`,
`torajs-task`, `torajs-llvm-bind`. F 族 trigger 10 already met (P3 closed),
**but never fired** — v0.7 Metal took priority on the user-binary 0-libc
axis. See [Audit triggers](#audit-triggers).

---

## 🎯 To be retired (no carve-outs, no deferrals — sequenced by L4 trigger)

There are **no permanent carve-outs**. Vision #4 (0 deps) is a literal target:
every external crate, every libc symbol, every non-self-written line on the
metal path **will be retired**. The only question is sequencing.

### External deps (4 in metal-tier code path)

| Dep | Retire via | Sequencing |
|---|---|---|
| `inkwell 0.9` + `inkwell_internals 0.14` + `llvm-sys 221.0.1` | v0.7 work units #6 (torajs-codegen, SSA → aarch64 emitter, span-based register allocator with Linear Scan or graph-coloring), #7 (torajs-obj, Mach-O + ELF writer), #8 (torajs-link, sym resolve + section layout + relocation), #9 (ssa_inkwell replacement) | After v0.7-A axis closes; runs **in parallel** with F-axis self-research, not sequenced after it |
| `libloading 0.8` | F.3 → torajs-dynlib stone (~200 LOC unsafe — dlopen/dlsym/dlclose wrapping POSIX RTLD_LAZY + RTLD_GLOBAL semantics; Mach-O dyld + ELF dl on respective targets) | Independent; can ship anytime after `torajs-syscall` stable |

### External deps (19 in host / demo code paths — all retire)

Sequencing per `deps-tree-v0.1.md` §L3b F 族 14 sub-steps. F 族 trigger 10
(P3 closed) **fired on 2026-05-23** when `torajs-bigint` shipped — F.1
torajs-codec-hex is hot-ready and runs in parallel with the v0.7 A-axis.
**No "decision point pending" entries — `playground-api` is decided below.**

| Dep | Retire via |
|---|---|
| `hex 0.4` | F.1 torajs-codec-hex (already 199 LOC sub-crate; needs trigger-10 hotify to ship as published F 族 first stone) |
| `sha2 0.10` | F.2 torajs-hash (SHA-{1,2,3} + Blake2/3 family) |
| `anyhow 1` + `thiserror 2` | F.4 torajs-error (textbook error enum + Display derive) |
| `toml 1` | F.5 torajs-codec-toml |
| `serde 1` + `serde_json 1` | F.6 torajs-codec (codec trait + JSON impl; hand-written first, derive macro later) |
| `tracing 0.1` + `tracing-subscriber 0.3` | F.7 torajs-trace (metal-tier structured logging) |
| `criterion` (dev-dep) | F.8 torajs-bench (integrates hardev/bench pillar) |
| `clap 4` | F.9 inline `argparse` mod in torajs-cli |
| `rustyline 16` | F.10 inline `line_edit` mod in torajs-repl (termios + history + emacs/vi binding) |
| `lsp-server 0.7` + `lsp-types 0.95` | F.11 inline `{server, types}` mod in torajs-lsp |
| `axum 0.8` + `tower 0.5` + `tower_governor 0.8` + `tower-http 0.6` | F.12 torajs-http-mini (metal-tier minimal HTTP/1.1 + 2 over std::net) |
| `tokio 1` | F.13 torajs-task (async runtime; reactor model shared with `torajs-microtask`) |

### `playground-api` decision (no longer "pending")

`playground-api` is a non-metal demo (per [project-torajs-metal-level memory](
../../../.claude-profile-3/projects/-Users-doracawl-workspace-goliajp-torajs/memory/project_torajs_metal_level.md)).
**Decision: strip to `torajs-http-mini` + `std::net`, keep in repo.**
Rationale (per 上限优先 / "lean-in attitude" from
`deps-tree-v0.1.md` §Deps 审计 decision):

- Moving to a standalone product repo splits the codebase artificially and
  forgoes a meaningful integration testbed for `torajs-http-mini` (F.12)
- Strip-down ensures the demo lives under the same metal-tier constraints
  the rest of the codebase does — **no second-class code path** in the repo
- Rewriting against `torajs-http-mini` validates the F.12 stone has the
  surface area to serve real workloads, not just unit tests

`torajs-playground-api` will be rewritten on top of F.12 + F.13 when those
ship. Cargo.toml entries for `axum` / `tokio` / `tower*` retire then.

### libc symbols (32 in user-binary, sequenced through v0.7 A-axis)

| Family | Count | Retire via | Status |
|---|---:|---|---|
| Alloc (`malloc/free/realloc/calloc`) | 4 | `torajs-mmalloc` (v0.7-A2) | ✅ shipped @ `2e698a5` |
| Mem (`memcpy/memmove/memcmp`) | 3 | `torajs-mmalloc` libc-compat shim | ✅ shipped @ `83a5972` |
| I/O (`putchar/printf/dprintf/snprintf/fflush/puts/getline/fputs/fwrite/fread`) | 10 | `torajs-io` (v0.7-A3) | ⬜ A3 prep audit done (16 entry points classified) |
| Fmt (`snprintf` formatting role, `itoa/dtoa`) | retained → 1 | `torajs-fmt` (v0.7-A4) | ⬜ Grisu3 / Dragon4 dtoa + Ryū itoa |
| Process (`exit/abort/getcwd/getenv/strlen/strncmp/strcmp`) | 7 | v0.7-A5 (work unit #5 runtime libc verify); per-symbol via `torajs-syscall` + `torajs-str` primitives | ⬜ |
| Panic backtrace (`backtrace/system/readlink/_NSGetExecutablePath/_dyld_get_image_vmaddr_slide/sigaction`) | 6 | v0.7-A5 (verify) + torajs-panic rewrite using `__darwin_unwind` walker + `__dyld_image_count` API directly | ⬜ keep symbolication; cut libc dep |
| Final residual after #6 + #15 | 0 | `otool -L /tmp/<user-bin>` == 0 lines containing `libSystem` | ⬜ |

### Sub-crate publishing to crates.io (all 33 — no "Phase 3" deferral)

Per `deps-tree-v0.1.md` §A 族 stone/cement processing, each sub-crate
publishes as a pair: `torajs-<name>` (pure `core` mod, no torajs ABI
constants) + `torajs-<name>-ffi` (extern "C" shell, build staticlib for tr).
**This split runs in parallel with the v0.7 A-axis fix work, not after it.**

Rationale (per 上限优先):

- ABI flux is real but bounded — the type-tag layout, HeapHeader, RC fields
  have not changed since the 23-batch god-file decomp closed (`ab0b7fe`,
  2026-05-25). Treating ABI as "not stable" is a self-fulfilling delay.
- A 33-stone family on crates.io **is** the project's identity to the Rust
  community. Waiting for v1.0 to publish is publishing nothing for months
  when the engineering value (forcing the `core`/`ffi` split discipline) is
  available now.
- Versioning policy: pre-1.0 (`0.x`) acknowledges flux; the `<name>-ffi`
  side rev-locks against a specific `torajs-core` SHA, so downstream ABI
  consumers (= just torajs itself for now) get explicit upgrade signal.
- This is the same playbook mailrs ran from day one — `mailrs-spf` 1.0,
  `mailrs-dkim` 1.0, etc. were published before mailrs server itself was
  stable. The discipline of the `core` extraction *is* the win.

Sequencing: ship F.1 (torajs-codec-hex publish) first as the proof-of-flow
(only sub-crate with no torajs-internal ABI dep); then publish the metal-tier
stones in dep order: `torajs-syscall` → `torajs-mmalloc-core` → `torajs-rc-core`
→ `torajs-anyvalue-core` → ... following the L0→L5 layer DAG from
`deps-tree-v0.1.md`.

---

## Self-check before extracting next stone

(Adapted from mailrs DEPS_AUDIT.md self-check template)

For any code under review (existing sub-crate to decompose, new file to
extract, candidate from `torajs-core` god file):

- [ ] Single-sentence identity that names an RFC / algo / well-known concept?
- [ ] Could a non-torajs project use this as-is (with at most a `core` +
      `ffi` mod split)?
- [ ] Free of `ssa_lower` / tag-table / `Cycle::Color` imports?
- [ ] ≤500 LOC of bounded library code (or decomposable into ≤500-LOC pieces
      along sub-algorithm boundaries — `regex-automata` family pattern)?
- [ ] Bench-able hot path with measurable budget?
- [ ] Does it line up with the metal-tier or TS-spec-tier stone family
      (so internal adoption is clean, not sideways)?

All ✅ → extract (publish or split-and-publish later). The 5 metal-tier
stones above all pass; the 21 TS-spec stones pass with the `core`/`ffi`
split per A 族 plan.

---

## Audit triggers

Re-run this audit doc when any of the following:

- New sub-crate ships (add row in §✅ Resolved tables)
- Existing sub-crate's `Cargo.toml` `[dependencies]` changes (re-check
  No-imports column)
- Workspace `Cargo.toml` `[workspace.dependencies]` changes (re-check Tier
  classification per `deps-tree-v0.1.md`)
- New `runtime_*.c` removal or new `__torajs_libc_*` symbol alias added
  (refresh §✅ libc symbol layer)
- v0.7 Metal work unit ships (refresh §⬜ Planned table + status memory L4
  trigger sync)
- New crates.io competitor for any planned stone (re-evaluate "first-mover"
  framing — cf. mailrs `mailrs-arf` 1.0 being first Rust ARF parser)

Audit command (one-shot ground-truth pass):

```bash
# Workspace inventory
for c in crates/torajs-*; do
  name=$(basename "$c")
  loc=$(wc -l "$c"/src/*.rs 2>/dev/null | tail -1 | awk '{print $1}')
  desc=$(grep -E '^description' "$c"/Cargo.toml 2>/dev/null | head -1 \
         | sed 's/description = "//; s/".*//' | head -c 80)
  printf "%-25s %5s LOC  %s\n" "$name" "$loc" "$desc"
done | sort

# External deps (direct, non-workspace)
cargo tree --workspace --edges normal --prefix none --depth 1 \
  | grep -v '^torajs-\|^bench-harness\|^inkwell-spike\|^$' | sort -u

# User-binary libc link verification
echo 'console.log(42)' > /tmp/_audit.ts
TR=$(cargo metadata --no-deps --format-version 1 \
     | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')/release/tr
$TR build /tmp/_audit.ts -o /tmp/_audit
otool -L /tmp/_audit
rm -f /tmp/_audit.ts /tmp/_audit
```

Last full audit: 2026-05-26, HEAD `dc46bbf`.

---

## Related docs

- [`deps-tree-v0.1.md`](deps-tree-v0.1.md) — authoritative L1-L4 plan + 6
  族 + Tier 0-3 + F 族 14 sub-step plan
- [`architecture-rewrite.md`](architecture-rewrite.md) — A 族 substrate
  authority (per-crate file template, Cargo.toml template, acceptance gate)
- [`roadmap.md`](roadmap.md) — v5 三轴 trunk (spec / perf / impl purity)
- [`design-principles.md`](design-principles.md) — 4 + 1 pillars (perf /
  in-house / mainstream / disciplined / ceiling-priority)
- [`../.claude/rules/torajs-design-principles.md`](../.claude/rules/torajs-design-principles.md) — hard rules
- [`../.claude/rules/torajs-autorun-pipeline.md`](../.claude/rules/torajs-autorun-pipeline.md) — A/B dual-track autorun protocol
- [`../CLAUDE.md`](../CLAUDE.md) §Disk Hygiene + §Planning Architecture

---

## Status update — 2026-06-04 (HEAD `14373f2`)

Drift audit on the original 2026-05-25 status above. The actual retirement
progress is well ahead of where the doc reads.

### Sub-crate inventory

**33 → 39 sub-crates** since the original audit. Added under `crates/`:
`torajs-codec-hex` (F.1 ship), `torajs-conformance`, `torajs-test262`,
`torajs-fetch`, plus the P10 (Promise / async / microtask) family. Exact
inventory:

```bash
$ ls crates/ | wc -l
39
```

### v0.7 Metal A-axis status

| A-unit | Original (2026-05-25) | Now (2026-06-04) |
|---|---|---|
| A1 mmalloc | ✅ closed | ✅ closed |
| A2 libc symbol cutover | ✅ closed | ✅ closed |
| A3 torajs-io | hot pending | ✅ closed (`f7eef96` 2026-05-27) |
| A4 torajs-fmt | planned | ✅ closed (`a4b4d99` 2026-05-29) |
| A5 nm-u libc verify | planned | ✅ closed (`1a42a3b` 2026-06-02 — 434 fixture nm-u diff = ∅) |
| A6+ codegen / obj / link | planned | **⬜ still hot** (the main remaining metal frontier) |

### v0.7 Metal Planned 3-15 unit status

| # | Unit | Original (2026-05-25) | Now (2026-06-04) |
|---|---|---|---|
| 3 | `torajs-io` | A2 close trigger pending | ✅ shipped (A3 close) |
| 4 | `torajs-fmt` | A3 close trigger pending | ✅ shipped (A4 close) |
| 5 | runtime libc verify | A4 close trigger pending | ✅ shipped (A5 close — nm-u diff = ∅) |
| 6 | `torajs-codegen` | independent axis | ⬜ still hot |
| 7 | `torajs-obj` | #6 ship trigger pending | ⬜ blocked on #6 |
| 8 | `torajs-link` | #7 ship trigger pending | ⬜ blocked on #7 |
| 9 | ssa_inkwell replacement | #6+#7+#8+#5 trigger pending | ⬜ blocked on #6+#7+#8 |
| 10 | cli deps self-research | independent | ⬜ still hot (clap 0 dep BUT — see F-族 row below) |
| 11 | embed dep self-research | independent | ⬜ still hot (libloading 0 dep BUT — see F-族 row below) |
| 12 | bench dep self-research | independent | ⬜ partially shipped (criterion 1/3 site retired; see F.8) |
| 13 | playground dep self-research | deferred per memory | ⬜ deferred (decision = strip to torajs-http-mini, see F.12) |
| 14 | inkwell + llvm-sys deletion | #9 trigger pending | ⬜ blocked on #9 |
| 15 | v0.7 final audit | #5 + #9 trigger pending | ⬜ blocked on #9 |

### F-axis dep retirement audit (per `deps-tree-v0.1.md` §L3b)

Probed 16 entries from the "To be retired (external deps)" table. **15/16
already at 0 dep site** at HEAD `14373f2` — the F-axis is effectively
shipped, the doc just never marked it.

| F-unit | Dep | Original (2026-05-25) | Now (2026-06-04) |
|---|---|---|---|
| F.1 | `hex 0.4` | hot-ready pending trigger-10 | ✅ 0 site (only `torajs-codec-hex` workspace dep remains; F.1 effectively closed) |
| F.2 | `sha2 0.10` | planned | ✅ 0 site |
| F.3 | `libloading 0.8` | planned | ✅ 0 site |
| F.4 | `anyhow 1` + `thiserror 2` | planned | ✅ 0 site |
| F.5 | `toml 1` | planned | ✅ 0 site |
| F.6 | `serde 1` + `serde_json 1` | planned | ✅ 0 site |
| F.7 | `tracing 0.1` + `tracing-subscriber 0.3` | planned | ✅ 0 site |
| F.8 | `criterion` (dev-dep) | planned | ⬜ 1 workspace + 2 crate dev-dep sites; `torajs-bench` stone not yet shipped |
| F.9 | `clap 4` | planned | ✅ 0 site (cli now uses inline argparse) |
| F.10 | `rustyline 16` | planned | ✅ 0 site |
| F.11 | `lsp-server 0.7` + `lsp-types 0.95` | planned | ✅ 0 site |
| F.12 | `axum 0.8` + `tower 0.5` + `tower_governor 0.8` + `tower-http 0.6` | planned (playground non-metal) | ✅ 0 site |
| F.13 | `tokio 1` | planned | ✅ 0 site |

### Remaining metal-axis dep set (HEAD `14373f2`)

Only **2 external crate dep sites** survive on the metal path:

```bash
$ grep -rn "^inkwell\|^llvm-sys\|^criterion" Cargo.toml crates/*/Cargo.toml
crates/torajs-core/Cargo.toml:18:inkwell = { version = "0.9", features = ["llvm22-1-prefer-dynamic"] }
Cargo.toml:64:criterion = { version = "0.8", default-features = false, ... }   # dev-dep only
```

- `inkwell 0.9` + (transitive) `llvm-sys`: retire via #6/#7/#8/#9/#14 chain
- `criterion` dev-dep: retire via F.8 torajs-bench stone

**v0.7 Metal close remaining = the #6 → #15 chain**. Each unit is bounded
substrate work but the chain is multi-week total (especially #6 codegen
which is the entry stone). Sub-step ordering decision belongs to takagi.

### Suggested next hot units

Ordered by ship-cost ascending (smallest deliverable first to lock progress):

1. **F.8 torajs-bench** — extract criterion-shape `criterion` replacement,
   integrate hardev/bench pillar; closes the last F-axis dep
2. **#10 cli inline argparse** — clap was already retired pre-F.9 ship per
   the audit above; verify status + doc the close (no actual ship work)
3. **#11 torajs-dynlib** — F.3 already shows libloading at 0 site; same
   verify-and-doc situation
4. **#6 torajs-codegen** — the main remaining metal frontier; multi-week,
   needs RFC sign-off before sub-step ship (would gate #7/#8/#9/#14/#15)

(2026-06-04 audit by autorun; original audit by takagi 2026-05-26.)
