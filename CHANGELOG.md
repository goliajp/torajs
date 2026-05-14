# Changelog

## Unreleased — develop trunk pivot to test262 100 % (2026-05-14)

The v3 wedge cycle (V3-XX, see `docs/roadmap-historical.md`) closed at
HEAD `52ba8ea` with curated `conformance/cases/` at **522 pass / 0 fail
/ 1 skip** (effectively saturated). Test262 200-sample at the same HEAD
shows in-scope pass rate at **3.96 %** (4/101) — single-method wedges
have hit the marginal-return wall: the curated suite is hand-typed, but
test262 is bare JS, and tora's strict typecheck rejects most cases at
the first `var x = "anything"`.

`develop` now pivots to a **single linear v4 trunk** in
`docs/roadmap.md` — 14 phases, each with a measurable test262 in-scope
pass-rate gate, total target ≥ 90 % at v1.0. The v1 / v2 / v3 plans
are preserved verbatim in `docs/roadmap-historical.md` as the audit
trail for tora's foundation.

The trunk introduces a **two-tier execution model**: typed-tier
(existing static-layout pipeline, 0 % perf regression invariant) +
untyped-tier (16-byte tagged-value slot for `Type::Any`, runtime
dispatch). Bare-JS source flows through untyped-tier; annotated source
stays in typed-tier. Both compile through the same SSA → LLVM pipeline
— no JIT, no interpreter.

P0 (Untyped-JS surface) is the entry point. All forward work runs in
strict phase-then-item order per the trunk's execution rules.

## v0.6.0 — 2026-05-08

Fifth release after `v0.5.0` (same day). Closes the **v0.6.0 sequence**
(T-20 / T-21 / T-22 / T-23) of the perf-gated 33-item plan from
`v0.3.0` (see [`docs/roadmap.md`](docs/roadmap.md) → "Roadmap v2").

Theme: **wasm target + HTTP fetch + sandboxed playground at
torajs.com**. tr now builds three flavors of artifact (native AOT,
`tr run` cache hit, wasm32-wasi) from the same TS source, ships
`fetch(url)` as a first-class API, and exposes the whole stack
through a Monaco editor + sandboxed-compile API at
torajs.com/playground.

### Headlines

- **`tr build --target wasm32-wasi -o foo.wasm`** — full wasm
  pipeline. Compiles + links via LLVM 22 + wasi-libc + wasm-ld.
  Produces a `.wasm` module that runs cleanly under wasmtime /
  wasmer / Node's wasi module with byte-identical output to the
  native build (arithmetic, arrays, strings, async/await, Promise
  chains all verified). Wasm artifact is ~37 KB for the
  multi-feature stress test (vs 63 MB bun-aot binary).
  Substrate: a 32-bit ABI bridge (`runtime_libc_bridge.c`) so
  tora's i64 size_t maps cleanly to wasi-libc's i32; main symbol
  renamed to `__main_argc_argv` to match wasi-libc's `__main_void`
  shim.
- **`fetch(url): Promise<Response>`** — sync MVP via libcurl. The
  Response heap struct (24 B: header + status i64 + body Str*)
  exposes `.text(): Promise<string>` and `.status: number`.
  Follow-redirects on; 30 s total / 10 s connect timeout; HTTPS
  via system trust store. Wasm-side fetch (browser fetch API)
  ships post-v0.6 alongside an emscripten-compatible runtime.
- **torajs.com/playground** — Monaco editor, curated examples,
  URL-encoded share-link (gzip + base64url so a /playground link
  carries the program inline), and a sandboxed Run that actually
  executes user TS:
  - Frontend: react-router 7 route mounts Monaco from CDN + new
    `views/playground.tsx`.
  - Backend (new crate `torajs-playground-api`): axum 0.8 + tokio.
    `POST /api/run` SHA-256-hashes the source → on-disk cache hit
    → otherwise `tr build --target wasm32-wasi` → `wasmtime -W
    fuel=2G timeout=5s max-memory=64MiB` with a 5 s tokio-side
    wall-clock as defense-in-depth. No filesystem mounts; per-IP
    rate limit (4-burst / 15 s refill via tower_governor's
    SmartIpKeyExtractor); 8 KB source cap.
- **torajs.com/bench** — auto-rendered bench scoreboard from
  `bench/results/*.json`. Per-cell tone scales with vs-torajs
  ratio (lighter = slower; darker = faster); hover for hyperfine
  stddev.

### Numbers (HEAD `69ee342`, 2026-05-08)

| metric | value |
|---|---|
| conformance suite | **370 pass / 0 fail / 1 skip** (was 370/0/1 at v0.5.0 — preserved through 8 substrate steps) |
| bug bucket | **0** (preserved through 50 commits) |
| compile-error bucket | **0** |
| wasm32-wasi target | **end-to-end working** (hello / arithmetic / arrays / strings / async fn / Promise chains all byte-identical to native) |
| bench scoreboard | **21 / 21** `tr build` ≥ ok vs bun-aot (preserved) |
| async bench | **5 / 5** `tr build` ≥ ok vs bun-aot |
| binary size (typical bench, native) | **36–38 KB** (vs bun-aot 63 MB) |
| binary size (wasm) | **15–37 KB** depending on substrate use |

### v0.6 substrate detail

- **T-20** wasm32-wasi target — Phase A toolchain wiring
  (CompileTarget enum + brew-detection of llvm@22 / wasi-libc /
  wasi-runtimes), Phase B 32-bit ABI bridge (libc_name resolver
  + `runtime_libc_bridge.c` `__torajs_libc_*` wrappers + main
  rename for the wasi-libc `__main_void` shim).
- **T-21** native fetch — `runtime_fetch.c` libcurl wrapper +
  TAG_RESPONSE = 9 in value_drop_heap dispatch + check.rs
  `(Type::Object("Response"), .text/.status)` arms +
  `parse_type` "Response" → Type::Ptr.
- **T-22** Playground — Phase A Monaco editor + URL share +
  examples, Phase B `torajs-playground-api` axum crate with
  sandboxed compile + run.
- **T-23** bench scoreboard auto-render at /bench.

### Scope notes

- **Wasm sandbox is bare** — no filesystem, no network. The wasm
  module's `fs/promises` and `fetch` calls fail cleanly inside
  the playground's wasmtime invocation. Run locally with
  `tr run` for the full surface.
- **Playground deploy** — Caddy + systemd unit on t01 ships as
  a follow-up doc commit; per-deploy config doesn't belong in
  the binary.

### Roadmap

v0.7+ = T-24..T-29: vtable upgrade, BigInt, WeakRef + cycle
collector, Function ctor + eval, multi-platform release expansion,
`tr debug` (DAP). takagi confirmed v0.6 close → perf focus next.

## v0.5.0 — 2026-05-08

Fourth release after `v0.4.0`. Closes the **v0.5.0 sequence**
(T-14 / T-15 / T-17 / T-18 / T-19; T-16 deferred — see "Scope notes"
below) of the perf-gated 33-item plan from `v0.3.0` (see
[`docs/roadmap.md`](docs/roadmap.md) → "Roadmap v2").

Theme: **async/await + Promise substrate**. tr ships its own Promise
implementation (32-byte heap layout + microtask queue + drain-on-await
+ auto-drain at main exit), the full ES2015/2018 Promise prototype
(`.then` / `.catch` / `.finally` / 2-arg `.then(onOk, onErr)`),
combinators (`Promise.all` / `.race` / `.any` / `.allSettled`),
thenable absorption, fs/promises (`readFile` / `writeFile` /
`appendFile` / `unlink` / `mkdir` / `exists` / `readdir`), Bun.file
(`text` / `exists` / `json` / `size`), and async-fn desugar.
**5 new async bench cases** added — tr crushes bun-aot on every one
(geomean tr/bun-aot 0.16x on async paths, 4–12x faster).

### Headlines

- **Promise<T> as a first-class type** — `Type::Promise<T>` in
  check.rs flows the inner T through the type system; the SSA layer
  type-erases to a unit `Type::Promise` (heap ptr) but preserves T
  via a per-Expr type side-channel (T-15.g.6) for await-result
  recovery (`IntToPtr` for heap T, `TruncI64ToBool` for Bool).
- **Microtask queue + drain-on-await** — single-thread executor with
  grow-by-doubling backing; `await` drains pending microtasks before
  reading the resolved value so `.then` chains fire in spec FIFO
  order. Auto-drain on main exit so async-unaware programs that
  schedule top-level microtasks still settle correctly.
- **Capture-box ARC for closures** — escape-captured Copy lets are
  now refcounted heap boxes (8-byte rc header + 8-byte value); each
  Closure construction inc's, each env_drop dec's, free at zero.
  Fixes the multi-closure shared-capture double-free.
- **Promise.then / .catch / .finally** — both raw-fn and capturing-
  closure cb shapes; heterogeneous T → U inferred from cb's actual
  signature (Number / String / Boolean); 2-arg `.then(onOk, onErr)`
  desugars to `.then(onOk).catch(onErr)`.
- **Promise combinators (Promise.all / .race / .any / .allSettled)**
  on already-fulfilled inputs (sync fast-path); pending inputs
  yield a rejected outer Promise per the v0.5 MVP — full callback
  fan-in lands with v0.6's real-IO substrate.
- **fs/promises module + Bun.file substrate** — readFile / writeFile
  / appendFile / unlink / mkdir / exists / readdir; Bun.file(p) with
  `.text()` / `.exists()` / `.json()` / `.size`. The async surfaces
  ride on top of the existing sync helpers + Promise.resolve wrap;
  real-suspension I/O lands with v0.6.
- **Capturing arrow expr-body return-type inference** —
  `(v: number) => v + cap` no longer requires `: T` annotation; the
  desugar pass pre-collects all top-level let-decl + FnDecl-param
  annotations and seeds the return-type sniff.
- **`function main()` rename** — user-declared `main` no longer
  collides with the synthesized OS-entry symbol; renamed internally
  to `__user_main` with all references rewritten.
- **5 new async bench cases** (`promise-then-100k`, `promise-await-
  100k`, `promise-all-1k`, `promise-chain-1k`, `async-fn-call-100k`)
  — every one is `ok` on torajs AOT, geomean tr/bun-aot 0.16x.

### Numbers (HEAD `21d9783`, 2026-05-08)

| metric | value |
|---|---|
| conformance suite | **370 pass / 0 fail / 1 skip** (was 343/0/1 at v0.4.0) |
| bug bucket | **0** (preserved through 27 substrate steps) |
| compile-error bucket | **0** |
| bench scoreboard | **21 / 21** `tr build` ≥ ok vs bun-aot (preserved) |
| async bench | **5 / 5** `tr build` ≥ ok vs bun-aot |
| bench geomean tr/rust | **0.669** (was 0.671 — flat within noise) |
| bench geomean tr/bun-aot | **0.231** (tr ~4.3x faster than bun-aot) |
| **async geomean tr/bun-aot** | **0.16x** (tr ~6x faster than bun-aot) |
| binary size (typical bench) | **37–38 KB** (vs bun-aot 63 MB) |

### Async bench detail

| case | tr (ms) | bun-aot | bun-jsc | node-v8 | tr vs bun-aot |
|---|---|---|---|---|---|
| promise-then-100k | 5.94 | 21.69 | — | — | **3.7x** |
| promise-await-100k | 8.48 | 17.93 | — | — | **2.1x** |
| promise-all-1k | 1.56 | 12.93 | — | — | **8.3x** |
| promise-chain-1k | 1.47 | 17.75 | — | — | **12.1x** |
| async-fn-call-100k | 2.81 | 16.81 | — | — | **6.0x** |

### Substrate steps (T-NN.x sub-items)

T-14 Promise<T> → T-15 (a–h + g.5 closure-cb-ARC) substrate →
T-17 combinators → T-18 (a–c) fs/promises → T-19 (a–p) Bun.file
+ thenable absorption + .catch/.finally + closure variants +
T → U + capturing-arrow inference + user-main rename.

### Scope notes

- **T-16 (state-machine async/await) DEFERRED** — the v0.5 MVP uses
  sync-resolve + microtask drain at every `await`. Spec-correct
  for every Promise that's settled by the time control reaches its
  await (which is every Promise constructible in v0.5 — chained
  `.then` middle-state Promises are drained by the same await).
  Real PENDING sources only appear with v0.6's real-IO substrate
  (`fetch`, `setTimeout`, real async fs); T-16 ships in lockstep
  with the first such API to avoid premature substrate.
- **`Promise.all` / `.race` / `.any` PENDING fan-in** — same
  reasoning; sync fast-path covers every case constructible
  pre-v0.6.
- **Heap T → U for `.then` / `.catch`** — Number / String / Boolean
  ship now; Array / Struct / Date / RegExp lands with the full
  generic-method substrate (T-15.g.4 TypeVar substitution at
  method call sites).

### Roadmap

v0.6 = playground + real I/O (`fetch` via reqwest; wasm32-wasi
target; Playground UI on `torajs.com`; auto-rendered bench
scoreboard from `bench/results/*.json`).

## v0.4.0 — 2026-05-06

Third release after `v0.3.0` (same day). Closes the **v0.4.0 sequence**
(T-09 / T-10 / T-11 / T-12-deferred / T-13 / T-13.5) of the perf-gated
33-item plan introduced at v0.3.0 (see [`docs/roadmap.md`](docs/roadmap.md)
→ "Roadmap v2").

Theme: **Type::Any boxing substrate + heterogeneous collections**, so the
spec-faithful versions of `arguments`, `Object.entries / fromEntries`,
`Symbol`, and the v0.5+ async substrate can build on top. **Array deque**
substrate (`head_offset`) closes the only `tr build` ≥ rust loss
remaining at v0.3.0 (`fifo-queue-100k` 1.225x → 1.024x), without
regressing on element-walk-heavy benches.

### Headlines

- **`Type::Any` boxing substrate** — universal heap header gains a
  type-tagged untyped slot (24-byte ANY box; 5 ANY_* tags for
  NULL / BOOL / I64 / F64 / HEAP). `Array<Any>` uses 16-byte tagged-slot
  stride. `let xs: any[] = [42, 'hi', true, 3.14, -0.0]` round-trips
  byte-identical with bun (incl. IEEE 754 sign-bit preservation).
- **`Object` stdlib completion** — `entries / fromEntries / freeze /
  isFrozen / getPrototypeOf / setPrototypeOf-reject / defineProperty /
  defineProperties / getOwnPropertyDescriptor`. Mutation guard at every
  `obj.field = X` site emits a `TypeError`-shaped panic on frozen
  objects (matches bun's strict-mode throw).
- **`arguments` full materialization** — dynamic index, runtime
  heterogeneous array via the T-10 substrate.
- **`String.raw` + tagged templates DEFERRED** — clean parse-time reject
  pointing at post-v0.4.0 (parser substrate of its own).
- **`Symbol` substrate** — 16-byte Symbol value type with `Symbol(desc)`,
  `Symbol.for(key)` global registry + `Symbol.keyFor(s)`, well-known
  singletons `Symbol.iterator / asyncIterator / toPrimitive`. `symbol`
  is a primitive type alias (`let s: symbol = Symbol()` /
  `let xs: symbol[] = [...]`).
- **Array deque substrate (`head_offset`)** — array universal header
  packs cap (u32 @16) + head (u32 @20) into the same 8 bytes;
  `arr.shift()` / `arr.unshift()` are now O(1) (head++ / head--);
  `arr.push()` compacts (memmove + reset head=0) when phys_used hits
  cap before falling back to realloc. All user-visible element-walk
  paths (Index / pop / fill / indexOf / includes / find* / map / filter
  / reduce / flatMap / sort / at / JSON.stringify) route through the
  head-aware byte-offset helper.
- **Three-layer perf gate held throughout** — every commit in this
  milestone carries the gate result; mid gates green at every
  `T-NN.<sub-step>` close.

### Numbers (HEAD `0bbff91`, 2026-05-06)

| metric | value |
|---|---|
| conformance suite | **343 pass / 0 fail / 1 skip** (was 329/0/1) |
| test262 in-scope pass | **827 / 23941 (3.45 %)** (was 805 / 23941) |
| **tr-accepted parity** | **100.00 %** (827/827 — preserved through 6 v0.4 bug clears) |
| bug bucket | **0** (was 0 at v0.3.0; +6 transient bugs all cleared in T-13.5.fix / T-13.5.fix2) |
| compile-error bucket | **0** |
| bench scoreboard | **21 / 21** `tr build` ≥ ok vs bun-aot (preserved) |
| bench geomean tr/rust | **0.671** — tr ~33 % faster than rust (was 0.722; **+7 % net** vs v0.3.0) |
| bench geomean tr/bun-aot | **≤ 0.244** — tr ~4.1× faster than bun-aot (preserved) |

**Major bench movements (vs v0.3.0):**
- `fifo-queue-100k` 1.225 → 1.024 (the v0.3.0 substrate-debt entry — closed)
- `rpn-eval-100k` 2.748 → 0.909 (tr 9 % faster than rust)
- `csv-trim-100k` 1.472 → 0.822 (tr 18 % faster than rust)
- `csv-rebuild-100k` 0.591 → 0.365 (tr 2.7× faster than rust)
- `split-only-100k` 0.685 → 0.423 (tr 2.4× faster than rust)
- New perf debt: `generic-id-1m` 0.943 → 1.129 (tr 19 % slower; LLVM can't
  LICM-hoist the head-offset load past opaque fn calls; tracked, fix path
  documented for v0.5+: function-purity attributes / escape-analysis short-
  circuit on never-shifted arrays)

### Stdlib surface added (since v0.3.0)

- **Object stdlib** completion as listed above (T-09).
- **`arguments`** dynamic index + runtime heterogeneous array (T-11).
- **`Symbol`** value type + registry + well-known singletons (T-13).

### Bug-clears in this milestone

Six transient bugs surfaced post-T-13.5 in the v0.4.0 full gate; all
cleared in two follow-up commits before the tag.

- **`Object.freeze(false / "abc" / 42)` SIGSEGV / SIGBUS** — runtime
  helper deref'd primitive bit patterns as heap headers. ssa_lower
  now compile-time short-circuits when arg type is I64 / F64 / Bool
  per ES2015 spec (freeze returns arg, isFrozen returns true). Covers
  test262 `15.2.3.9-1-3` / `15.2.3.9-1-4` / `15.2.3.12-1-3`.
- **`Object.freeze("static literal")` SIGBUS** — heap-shaped value with
  `STATIC_LITERAL` flag set lives in `.rodata`; writing the FROZEN bit
  faulted. C-side helper now skips the bit set (and reports
  `isFrozen=true`) on static literals.
- **`let xs: symbol[]` rejected as unknown type** — check.rs's primitive
  resolver had no `symbol` entry. Adds it alongside `string` / `number` /
  `boolean` / `any` mapping to `Type::Symbol`.
- **`Object.is(arr[i], arr[j])` in a loop crashed after iter 1** —
  ssa_lower's Object.is arm always called `emit_drop_value` on both
  args; lower_expr on Index / Member / Ident returns a *borrow* (slot
  still owns the ref), so the drop rc_dec'd the owner's ref and freed
  a still-live element. Borrow guard (mirroring console.log's existing
  pattern) skips the drop on borrow exprs. Covers test262
  `staging/sm/Symbol/equality.js`.

Each bug shipped a regression fixture under `conformance/cases/`
(`object-002-freeze-primitive`, `symbol-004-array-equality`).

### Known not-yet-supported (now scheduled into v0.5 / v0.6 / v1.0)

- **v0.5**: `async` / `await` / `Promise.{all,race,allSettled,any}`,
  fs async, `Bun.file(p).text()`, `process.stdin.read`. Plus the
  `String.raw` + tagged template literal substrate deferred from v0.4
  (T-12).
- **v0.5 perf debt**: `generic-id-1m` 1.19x rust regression — function-
  purity attributes / escape-analysis short-circuit on never-shifted
  arrays so LLVM LICM can hoist the head-offset load past opaque
  fn calls. Tracked in `project_perf_debt_2026_05_06.md`.
- **v0.6**: `wasm32-wasi` engine target, `fetch`, Playground UI on
  torajs.com.
- **v1.0**: vtable for virtual dispatch (currently tag-switch),
  `BigInt` self-hosted, `WeakRef` / cycle collector, `Function`
  constructor, `tr debug` / `tr repl` / `libtora.a` embedding,
  multi-platform release, test262 push to ≥ 90 % in-scope pass.

### Install

```sh
curl -fsSL https://install.torajs.com | bash
# or, pinned to GitHub raw:
curl -fsSL https://raw.githubusercontent.com/goliajp/torajs/main/install.sh | bash
```

Or build from source: `cargo build --workspace --release` then
`target/release/tr --version`.

### Verifying parity with `bun`

```sh
diff <(bun run yourfile.ts) <(tr run yourfile.ts)
```

Empty diff means tr's output matches bun byte-for-byte.

---

## v0.3.0 — 2026-05-06

Second release after the v0.1.0-beta tag. Closes the **v0.3.0
sequence** (T-01..T-08) of the perf-gated 33-item linear plan
introduced this milestone (see [`docs/roadmap.md`](docs/roadmap.md)
→ "Roadmap v2").

Theme: **dev tooling base** (`tr lsp` / `tr fmt` / `tr lint` /
DWARF source maps), **engine graduation** out of `labs/0001-walking-
skeleton/` into a multi-crate workspace, and a sweep of v0.2 and
v0.3 stdlib partials that had accumulated on `develop` since beta.

### Headlines

- **`tr lsp`** speaks Language Server Protocol over stdio. VS Code
  extension scaffold under [`web/torajs-vscode/`](web/torajs-vscode/);
  packages to a 272 KB / 196-file `.vsix`. Hover, goto-def
  (top-level functions / classes / types / let / const), and
  per-edit diagnostics — round-trip latency P95 **0.49 ms** on a
  1 K-line synthetic fixture (~100x under the 50 ms budget). The
  diagnostic stream now carries `Diagnostic { span, severity,
  message }` (`Severity::Warning` is what `tr lint` and the LSP
  squiggle path consume).
- **`tr fmt`** deterministic source reformatter. 2-space indent,
  single quotes, no trailing semicolons, no config knobs. Refuses
  to silently strip comments (comment-aware reformat lands in v0.4).
- **`tr lint`** with five starter rules: `unused-let`,
  `dead-code-after-return`, `unreachable-catch`, `shadowed-let`,
  `unused-import`. `--deny` exits non-zero on any warning (CI-gate
  shape, matches `cargo clippy -- -D warnings`).
- **DWARF debug info** on every `tr build` artifact — panic
  backtraces resolve to `.ts:line:col` via libc backtrace + macOS
  `atos` symbolication.
- **Engine graduated** from `labs/0001-walking-skeleton/` to
  `crates/torajs-{runtime,core,cli}/`. The runtime crate locks the
  C-side ABI behind a stable boundary so the compiler can evolve
  without breaking already-shipped binaries. `cargo build
  --workspace --release` is clean; bench / conformance / test262
  numbers preserved end-to-end through the move.
- **Three-layer perf gate** mechanism shipped (per-commit mini
  gate / per-2-3-items mid gate / per-tag full gate). Every commit
  in this milestone carries the gate result in the message.

### Numbers (HEAD `016da18`, 2026-05-06)

| metric | value |
|---|---|
| conformance suite | **329 pass / 0 fail / 1 skip** (was 301/301) |
| test262 in-scope pass | **805 / 23941 (3.36 %)** (was 606 / 23941) |
| **tr-accepted parity** | **100.00 %** (was 99.67 % — every accept now bun-equivalent) |
| bug bucket | **0** |
| compile-error bucket | **0** |
| bench scoreboard | **21 / 21** `tr build` wins vs bun-aot (was 19/19) |
| bench geomean tr/rust | **0.654** — tr ~35 % faster than rust on average |
| bench geomean tr/bun-aot | **0.244** — tr ~4.1× faster than bun-aot |
| LSP hover P95 (1 K-line) | **0.49 ms** (budget 50 ms) |

### Stdlib surface added (since beta)

- **Regex (v0.2 #1)** — literal `/pattern/flags` + `new RegExp(...)`;
  flags `g i m s y` (full `u` Unicode property escapes deferred to
  v1.0); methods `re.test / re.exec / s.match / s.matchAll /
  s.replace / s.replaceAll / s.split`. NFA → DFA in
  [`runtime_regex.c`](crates/torajs-runtime/src/runtime_regex.c)
  (1760 LOC, self-hosted; pillar 2 自研).
- **Date (v0.2 #2)** — full constructor arity + ISO 8601 round-trip;
  `getFullYear / getMonth / getDate / getHours / getMinutes /
  getSeconds / getMilliseconds / getDay / getTime / valueOf /
  toISOString / toString` (local + UTC variants); static
  `Date.now() / Date.parse() / Date.UTC()`.
- **Object stdlib (v0.2 #3, partial)** — `Object.is` /
  `Object.hasOwn` / `Object.getOwnPropertyNames` /
  `Object.values` / `Object.assign` (single-source MVP).
- **`JSON.parse` f64 path (v0.2 #5)** — `let v: number =
  JSON.parse('1.5')` no longer truncates; bun-parity preserved on
  integer-valued JSON. Substrate
  `__torajs_json_parse_float` already shipped; T-02 wires the
  caller-driven typing path.
- **fs sync (v0.3 #1)** — `readFileSync`, `writeFileSync`,
  `appendFileSync`, `unlinkSync`, `mkdirSync`, `existsSync` (async
  forms gate on v0.5 async/await).
- **Bun namespace (v0.3 #2)** — `Bun.write(path, data)`,
  `Bun.argv`. `Bun.file(path).text() / .arrayBuffer() / .json()`
  deferred to v0.5 (gates on Promise).
- **process surface (v0.3 #3)** — `process.argv` / `env` / `env.NAME`
  / `platform` / `cwd` / `exit` (T-3 finishes with
  `process.stdout.write` / `process.stderr.write`). `stdin.read`
  deferred to v0.5 (bun's API is async).
- **String / Array method gaps (v0.2 #6/#7)** — `s.normalize` /
  `s.codePointAt` / `s.matchAll` / `Array.flat` / `Array.flatMap` /
  `Array.findLast` / `Array.findLastIndex` / `Array.from` /
  `Array.of`.

### Known not-yet-supported (now scheduled into v0.4 / v0.5 / v1.0)

Each item lives in the v2 plan's 33-item table (`docs/roadmap.md` →
"Roadmap v2"):

- **v0.4**: complete Object stdlib (entries / freeze / fromEntries /
  defineProperty / ...), Type::Any boxing, `arguments` full
  materialization, `String.raw`, Symbol substrate, **Array deque
  substrate** (`head_offset`) to fix the only remaining vs-rust loss
  (`fifo-queue-100k` 1.17x; tracked, fix-path documented).
- **v0.5**: `async` / `await` / `Promise.{all,race,allSettled,any}`,
  fs async, `Bun.file(p).text()`, `process.stdin.read`.
- **v0.6**: `wasm32-wasi` engine target, `fetch`, Playground UI on
  torajs.com.
- **v1.0**: vtable for virtual dispatch (currently tag-switch),
  `BigInt` self-hosted, `WeakRef` / cycle collector, `Function`
  constructor, `tr debug` / `tr repl` / `libtora.a` embedding,
  multi-platform release, test262 push to ≥ 90 % in-scope pass.

### Install

```sh
curl -fsSL https://install.torajs.com | bash
# or, pinned to GitHub raw:
curl -fsSL https://raw.githubusercontent.com/goliajp/torajs/main/install.sh | bash
```

Or build from source: `cargo build --workspace --release` then
`target/release/tr --version`.

### Verifying parity with `bun`

```sh
diff <(bun run yourfile.ts) <(tr run yourfile.ts)
```

Empty diff means tr's output matches bun byte-for-byte.

---

## v0.1.0-beta — 2026-05-04

First external release. **Beta**: the core compile pipeline runs
real-world TS programs end-to-end, but the language coverage and
stdlib surface are still expanding (see
[`docs/language-status.md`](docs/language-status.md) for the full
table of currently-working features and roadmap items).

### Headlines

- **`tr build` produces native binaries** that run faster than
  `bun --compile` on every committed bench case (see
  [`docs/perf.md`](docs/perf.md))
- **`tr run`** is the dev-loop entry — compile + cache + execute
  in one shot, mirroring `bun run`'s ergonomics
- **5 self-contained examples** under [`examples/`](examples/) —
  SHA-256, prime sieve, FizzBuzz, wc-clone, JSON serializer demo;
  each output is byte-identical to `bun run`
- **Test262 conformance**: 606 / 23941 in-scope cases (2.53 %),
  99.67 % of cases tr accepts produce the bun-identical output

### Language coverage (highlights)

- Classes (instance + static, inheritance, `abstract`, visibility
  modifiers), generics, closures
- Generators + `yield` / `yield *`
- `try` / `catch` / `finally` / `throw`
- Function expressions in any expression position
- `let x;` (uninitialized) — first-assignment splice
- `any` and untyped fn params auto-promote to fresh type-params,
  monomorphized at call sites
- Cross-file named imports + multi-file `tr run` cache
- Full string / array / Math / JSON stdlib surface (see status doc)
- `>>>` UInt32 coercion idiom (SHA-256 / hash-heavy code)

### Known not-yet-supported (each tracked under a roadmap phase)

- `Object.{getPrototypeOf, getOwnPropertyDescriptor, defineProperty,
  freeze, ...}`
- Regex (`/.../`, `new RegExp`)
- `Symbol`, `Proxy`, `WeakMap`, `WeakSet`, `WeakRef`
- `BigInt`
- `async` / `await` / `Promise`
- ESM default + namespace + side-effect imports
- `Date`
- `fs` / `Bun.file`
- Top-level `xs.push(...)` for mutable refcount globals (workaround:
  wrap in `main()`)
- `JSON.stringify(value, replacer, indent)` indent-aware emission
- Full `arguments` object (only `arguments.length` /
  `arguments[N]`-literal-index rewrites today)

If `bun` runs your code and `tr` rejects it, that's a roadmap-phase
gap to fix. File an issue.

### Install

```sh
curl -fsSL https://install.torajs.com | bash
# or, pinned to GitHub raw:
curl -fsSL https://raw.githubusercontent.com/goliajp/torajs/main/install.sh | bash
```

Or build from source: `cargo build --release -p tr`.

### Verifying parity with `bun`

```sh
diff <(bun run yourfile.ts) <(tr run yourfile.ts)
```

Empty diff means tr's output matches bun byte-for-byte.
