# torajs roadmap

> Canonical implementation plan. Living document — update as work progresses, decisions change, or steps reveal new sub-steps.
>
> Last revised: 2026-05-06 (full rewrite of the forward plan into 33 ordered items + three-layer perf-gate mechanism per `.claude/rfcs/20260506-roadmap-v2-perf-gated.md`. Status snapshot refreshed to HEAD `ba3c19d` — bench is now **21 cases** (was 19), test262 is **805/23941 (3.36%)** with 100% tr-accepted parity (was 651/23941), workspace has graduated to `crates/torajs-{runtime,core,cli}/`. v0.3 #4 DWARF + #5 LSP + #6 Graduation all shipped this session.)
>
> Provenance audit trail: `.claude/researches/0001-direction.md` through `0005-roadmap.md` (early discussion logs — note: pre-2026-04-30 they used a "TS syntax + Rust semantics" framing that was takagi-corrected on 2026-04-30; treat them as historical context, not as design source-of-truth). For the 2026-05-04 v0.x rewrite see git history at commit `fa69c31`.

---

## Foundation

### Goal

Build a TypeScript runtime that runs the same TS programs `bun`
runs, with **TS semantics** — same observable behavior as `bun` on
the same source. Anything bun runs, tr eventually runs; not-yet-
implemented features are roadmap phases, never out-of-scope decisions.
The differentiator is the runtime: AOT-compiled to a small native
binary via LLVM (one path serves both `tr build` and `tr run` — the
latter caches the binary at `~/.torajs/cache` for instant rerun),
with ARC under a universal heap header instead of GC.

When behavior is ambiguous, **bun is the oracle**. Write the equivalent in TS, run it in `bun`, and match.

### Hard requirements

1. **极致 perf** — beat bun/node on important benchmarks; hold them. Both `tr build` (AOT) and `tr run` (AOT-cache) win compute-heavy cases vs bun-jsc/bun-aot/node-v8 (e.g. popcount: 11 ms vs bun 57 ms; fib40: 226 ms vs bun 395 ms).
2. **Compile not too slow** — first `tr run foo.ts` after a source edit pays one full LLVM compile (~50–90 ms / small program); subsequent runs hit the cache and skip compile entirely. Production builds use `tr build` with O3 (~45 ms / case).
3. **Interpretable** — `tr run foo.ts` is the dev-loop entry point. AOT-with-cache replaced Cranelift JIT on 2026-05-01: cold compile is ~10× slower, but cache hits make iteration latency lower than JIT and runtime perf strictly better.
4. **No GC, internal ARC for shared heap** — no tracing GC. Single-owner heap values use compile-time ownership inference + deterministic drops. Multi-owner cases (Array<T>, throw/catch shared structs, closure captures crossing scopes) use a hidden ARC-style refcount on a universal heap header — the user never writes `.clone()` or sees `Rc<T>`. See `docs/design-principles.md` for the four-pillar rubric the refcount pivot satisfies.
5. **TS-shape semantics** — what works, works the same as bun. No Rust-flavored idioms in user code (no `.clone()`, no `Rc<T>`, no lifetime annotations).
6. **Full TS coverage as a roadmap target** — every TS feature bun supports has a roadmap phase. Programs that hit a not-yet-implemented feature get a clean compile error referencing the phase that will close the gap. The compile error is intermediate state, not a permanent boundary; users don't restructure to fit a "subset", they wait for the phase to land or open an issue if no phase exists.
7. **test262 conformance** (revised 2026-05-03) — test262 is the reference suite for ECMAScript runtime conformance. v1.0 hard target: ≥ **90 %** pass rate on the in-scope slice of test262 (estimated 5K–15K cases out of test262's ~50K total — the rest are negative tests / harness-dependent / `bun-skip`). The full-coverage ambition (`feedback_torajs_ambition`) supersedes the earlier "test262 not a goal" stance — every reject must point at a roadmap phase, not at a permanent design boundary.

### What's NOT in scope (corrections from earlier framing)

Earlier roadmap drafts (pre-2026-04-30) called out **"TS syntax + Rust-shaped semantics"** with explicit `Rc<T>` / affine moves / `.clone()` exposed to the user. takagi corrected this on 2026-04-30: **torajs uses TS semantics, not Rust idioms**. User-visible Rust idioms are out:

- `Rc<T>` / `Arc<T>` / `RefCell<T>` — never user-facing
- `.clone()` as a required call — compiler decides
- Lifetime annotations `'a` — none
- `&` / `&mut` reference operators — none
- `move` keyword — not needed
- Affine "use of moved value" errors on simple read sites — replaced by alias-aware ownership inference

The compiler still does ownership analysis under the hood (the no-GC requirement leaves no choice), but it's **invisible at the source level**. See `docs/language-status.md` for the current feature coverage and per-feature roadmap-phase mapping for everything not yet implemented.

### Resolved decisions

| Decision | Choice |
| --- | --- |
| Engine implementation language | Rust |
| Source language | TypeScript (TS surface, TS semantics; coverage expanding per the phase plan) |
| Embed existing JS engine? | No — write our own |
| Execution model | AOT in both modes — `tr build` writes to `-o`, `tr run` writes to `~/.torajs/cache/<hash>` then execs. Single SSA → LLVM pipeline. |
| Memory model | Compile-time ownership inference for single-owner; hidden ARC refcount on universal heap header for shared paths (no user-visible `Rc<T>`); no tracing GC |
| Compiler backend | LLVM via Inkwell — single backend for both `tr run` and `tr build`. Cranelift JIT was tried (P3.6) and removed 2026-05-01: weaker codegen lost compute-heavy benchmarks to V8/JSC. |
| TS conformance | Coverage expands phase-by-phase; goal is full TS as bun runs it (≥ bun) |
| Test262 conformance | **Hard target on the in-scope slice** (revised 2026-05-03; was "Not a goal"). v1.0 gate: ≥ 90 % pass on the in-scope slice (~5K–15K of test262's ~50K total). |
| First-class WASM target | Yes — torajs.com playground depends on it |

### Working mode

- Closed-source research project. Many experiments will be discarded.
- New ideas land in `labs/` first; graduate to `crates/` when stable.
- Drive forward — execute the committed milestones below without per-step asking. Stop only for genuine forks (design questions not in this doc, irreversible decisions, ambiguous-recovery failures).
- See `.claude/rules/common/` and `.claude/rules/{rust,typescript}/` for shared coding standards. `labs/` may relax them.

---

## Status snapshot (2026-05-06, HEAD `ba3c19d`)

`v0.1.0-beta` shipped externally on 2026-05-04: tr is publicly installable and a non-trivial slice of TS programs run on it byte-identically with bun. Since the beta tag, v0.2 substrate work, v0.3 #4 DWARF, #5 LSP (L-1..L-6), and #6 Graduation have all landed on develop.

### What works end-to-end

```
$ curl -fsSL https://install.torajs.com | bash    # darwin-arm64 / linux-x64
$ tr build foo.ts -o foo                          # AOT — LLVM 22 + Inkwell, ~33 KB
$ ./foo                                            # native, bench-leading
$ tr run foo.ts                                    # AOT + cache (~10 ms reruns)
$ tr lsp                                           # LSP server over stdio for VS Code
```

The 5 example projects under `examples/` (sha256, prime-sieve, fizz-buzz, wc-clone, json-pretty) compile and run on tr with byte-identical output to bun.

### Numbers (HEAD `ba3c19d`, 2026-05-06)

| metric | value | note |
|---|---|---|
| conformance suite | **326/326** pass + 1 skip | tr's own regression net |
| test262 in-scope pass | **805/23941 (3.36 %)** | full test262 run; ~50 K total cases, in-scope slice ≈ ~5K-15K |
| **tr-accepted parity** | **100.00 %** | every case tr accepts produces bun-equivalent output — zero silent wrong |
| bug bucket | **0** | accepted-but-diverges cases |
| compile-error bucket | **0** | accepted-but-LLVM-rejects cases |
| bench scoreboard | **21/21** `tr build` wins vs bun-aot | full sweep, M4 Pro hyperfine n=5 |
| bench geomean tr/rust | **0.656x** | tr 比 rust 平均快 ~34% |
| bench geomean tr/bun-aot | **0.245x** | tr 比 bun 平均快 ~4.1x |
| LSP hover P95 (1K-line file) | **0.49 ms** | budget < 50 ms; ~100x margin |

The two empty buckets (bug + compile-error) mean: when tr says "yes I accept this", the produced binary is correct. The remaining ~23K test262 cases are rejected with a clean message pointing at the missing substrate (regex matching, Date, Symbol, Function constructor, etc.) — no silent failures.

### Code size

```
crates/torajs-runtime/             C source for refcount / str / arr / json / regex / Date
crates/torajs-core/                compiler library — lex / parse / check / ssa / inkwell
crates/torajs-cli/                 `tr` binary — build / run / lsp / lsp-bench
docs/                              roadmap + language-status + getting-started + perf + stdlib
bench/                             19 cases × 7 runtimes + harness + scoreboard
examples/                          5 projects, byte-identical with bun
```

> **Graduation status**: ✅ engine graduated in v0.3 #6 — `labs/0001-walking-skeleton/` was promoted to `crates/torajs-{runtime,core,cli}/`. The runtime crate locks the C-side ABI behind a stable boundary so the compiler can evolve without breaking already-shipped binaries.

---

## Execution path — committed order, no per-step ask

The single committed plan from v0.1.0-beta to v1.0. Each milestone is a coherent slice of value the user can install and run; sub-steps roll up to it. The agent executes each step end-to-end (code + tests + commit) and then takes the next ordered item without checkpointing back, except on a real fork (genuine design question, irreversible decision, ambiguous-recovery failure).

### v0.1.0-beta — released 2026-05-04 (retrospective)

The `beta` tag means: tr is publicly installable, the surface is real, and every program tr accepts is bun-equivalent. Not yet stable enough for `crates/` graduation; not yet broad enough for "drop-in bun replacement"; broad enough to be useful for a meaningful slice of TS programs and to have its own test262 baseline.

#### Public release surface

- **Repo**: [github.com/goliajp/torajs](https://github.com/goliajp/torajs) (public, default branch `main`)
- **Site**: [torajs.com](https://torajs.com) — Caddy on t01 serving the landing page (Fraunces serif + JetBrains Mono + tiger orange `#ff6f1a` accent)
- **Install**: `curl -fsSL https://install.torajs.com | bash` — verified darwin-arm64 end-to-end; routes via Caddy 302 to the GitHub release tarball
- **Release**: [v0.1.0-beta](https://github.com/goliajp/torajs/releases/tag/v0.1.0-beta) — GH Actions builds darwin-arm64 + linux-x64 tarballs on tag push
- **Docs**: `docs/getting-started.md` / `docs/language-status.md` / `docs/perf.md` + `CHANGELOG.md`
- **Examples**: 5 byte-identical-with-bun TS programs under `examples/`

#### Language coverage shipped

- **Core surface**: `let` / `const` / uninit `let x;` / `if-else` / `while` / `do-while` / `for` / `for-of` / `break` / `continue` / `return` / block scope
- **Types**: `number` (i64 default, f64 promotion on Math intrinsics / decimal lits / div), `boolean`, `string`, `void`, `null`, `Nullable<T>`, object literal types, homogeneous arrays `T[]`, function types `(args) => R`
- **Generics**: `function f<T>(x: T): T` + `type Pair<A, B>` — monomorphized per call site; `any` / untyped fn params auto-promoted to fresh type-params
- **Closures**: implicit captures, mutable captures via boxed-cell, escape-aware env-drop machinery
- **Classes**: full feature set — fields, ctor, methods, single inheritance + `super(...)`, virtual dispatch (tag-switch today, vtable pending), static fields/methods, `private` / `protected`, abstract classes
- **Error model**: `throw` / `try` / `catch` / `finally` with non-number throws (`string` / struct via `catch (e: T)`); transitive may-throw analysis skips throw-check on non-throwing callees
- **Generators**: `function* g(): T` and `yield` / `yield *` via state-machine lowering
- **Modules**: `import` / `export` (named imports across files)
- **Operators**: `+ - * / % ** **=`, `& | ^ << >> >>>`, `< > <= >= === !==`, `&& || !`, optional chaining `?.`, nullish coalescing (basic), unary minus
- **Stdlib**: variadic `console.{log, error, warn}`, full `Math.*` (35+ methods + 8 constants), `Number.*` (parseInt / parseFloat / isNaN / isFinite / isInteger + constants), 21 `String` methods (slice / charCodeAt / startsWith / endsWith / includes / indexOf / split / repeat / padStart / padEnd / trim / trimStart / trimEnd / toLowerCase / toUpperCase / replace (string-only) / replaceAll (string-only) / concat / at / fromCharCode / charAt), `Array<T>.{push, pop, shift, unshift, length, map, filter, reduce, forEach, indexOf, includes, join, sort (basic comparator), reverse, slice}`, full `JSON.{stringify, parse}` (caller-driven typing — no `<T>` needed), `arguments.length` / `arguments[N]` (literal index)
- **Regex**: literal lex + AST + clean typecheck rejection (matching engine pending — see v0.2)

#### Substrate milestones

- **Universal heap header + ARC** — every non-Copy heap object shares one 16-byte header (`refcount`, `type_tag`); `__torajs_rc_inc` / `__torajs_rc_dec` are the single ARC entry points; Array / String / Closure / Substr all participate. Aliasing of `Array<non-Copy>` end-to-end correct.
- **Compile-time ownership inference** — single-owner paths drop deterministically at scope exit; multi-owner paths (Array<T>, throw/catch shared structs, closure captures crossing scopes) routed through hidden ARC. No user-visible `Rc<T>` / `.clone()`.
- **LLVM AOT backend with cache** — single SSA → Inkwell pipeline serves both `tr build` (write to user path) and `tr run` (write to `~/.torajs/cache/<hash>`, exec). Cranelift JIT prototype tried at P3.6 and removed 2026-05-01 — weaker codegen lost compute benchmarks.
- **String runtime**: small-Str 32-slot LIFO pool, view-aware split/concat (1 malloc per split), `Substr` as independent `Type::Substr` (Swift / .NET pattern) — view ops zero-alloc.
- **Width-aware monomorphization + bidirectional F64↔I64 call-site coercion** — Math.* in generic fns + numeric mixed call sites compile cleanly without manual annotations.
- **Source-rewrite layer** — `==` / `!=` rewritten to strict; small set of substrate normalizations applied pre-typecheck.
- **Panic hook** — every `unimplemented!` / `unreachable!` is reclassified into a typed "not yet supported" reject with the phase that will close it.

#### Bench position (M4 Pro, hyperfine n=5)

`tr build` wins **all 19 cases** vs bun on the current scoreboard (latest sweep includes csv-rebuild +18 % over bun). Vs `rust`: parity to small wins on most cases, large wins on throw-catch-100k (feature-asymmetric); `closure-counter` ~17 % behind rust is the remaining loss. `tr run` (cache hit) trails `tr build` by ~8 ms exec floor, still beats bun-jsc on every compute case.

See `README.md` and `bench/results/` for the full table.

---

### v0.2 — substrate completeness (mostly shipped on develop, not yet tagged)

**Goal**: every TS program bun runs either runs on tr or hits a clean rejection pointing at a known v0.3+ phase. The 100 % tr-accepted parity guarantee from v0.1 is preserved; what grows is the *accepted* set.

**Exit gate**:
- test262 in-scope pass ≥ **2500/23941** (~10 % of full-suite, ~25–50 % of in-scope slice depending on size estimate)
- `tr-accepted parity` stays at 100.00 % (zero regressions on the bun-equivalence guarantee)
- bench scoreboard: `tr build` wins all 19+ existing cases vs bun (no regression); 5+ new cases added covering regex / Date / Object stdlib

> **Status (2026-05-06, HEAD `ba3c19d`)**: 6 ✅ shipped + 1 partial (`JSON.parse` f64 path needs verify + fixture). Object stdlib / `arguments` / `String.raw` partials roll forward into v0.4. test262 currently 805/23941 — full v0.2 exit gate (≥ 2500) deferred to v0.4 as substrate-driven; the milestone closes with v0.3.0 tag.

#### Ordered execution

1. **Regex matching engine** *(highest leverage; multi-day phase)*
   - Regex literal lex + AST already shipped (commit `5434f12`)
   - Build: NFA construction → DFA conversion (Thompson + subset construction) in C runtime (`__torajs_regex_*`); textbook PL approach, no external dep
   - Surface: `RegExp` type + `s.match(re)` / `s.replace(re, repl)` / `s.replaceAll(re, repl)` / `re.test(s)` / `re.exec(s)` / `s.split(re)` / `s.matchAll(re)`
   - Flags: `g`, `i`, `m`, `s`, `u`, `y` (Unicode property escapes `\p{...}` deferred to v1.0 unless test262 forces earlier)
   - Exit gate: regex bench cases at bun parity; test262 regex-syntax cases that don't require Unicode property escapes pass

2. **Date class**
   - Constructor: full arity (no-arg = now, ms-since-epoch, ISO 8601 string, year/month/day/...); ISO 8601 parser hand-written
   - Methods: getFullYear / getMonth / getDate / getHours / getMinutes / getSeconds / getMilliseconds / getDay / getTime / valueOf / toISOString / toString / toLocaleString (basic) / setX-counterparts
   - Static: Date.now() / Date.parse() / Date.UTC()
   - Timezone: local + UTC; full Intl.DateTimeFormat deferred
   - Exit gate: ISO round-trip works; date-arithmetic bench cases run on tr at bun parity

3. **Object stdlib (real implementations, not stubs)**
   - `Object.{keys, values, entries, assign, freeze, isFrozen, getPrototypeOf, setPrototypeOf, getOwnPropertyDescriptor, defineProperty, defineProperties, fromEntries, hasOwn}`
   - Requires runtime introspection on the universal heap header → expose declared field list per `Type::Obj(StructId)`
   - Exit gate: programs that iterate over object properties work without source rewrite

4. **`arguments` full materialization**
   - Runtime heterogeneous `arguments` array (vs current literal-index rewriting)
   - `arguments.callee` (basic), `arguments.length`, `arguments[N]` (dynamic index), `[...arguments]` spread
   - Exit gate: variadic non-arrow function parity with bun

5. **`JSON.parse` number path**
   - Today: `JSON.parse(...)` only handles I64 default; `1.5` truncates
   - Add f64 path + auto-promote based on caller-side type annotation (extends the caller-driven typing already shipped for `JSON.parse`)
   - Exit gate: `JSON.parse("1.5")` returns `1.5`; round-trip `JSON.stringify(JSON.parse(s))` byte-identical with bun on a fixture set

6. **String method gaps test262 surfaces**
   - Likely: `s.normalize()`, `s.codePointAt`, `String.raw`, `s.replace` / `s.replaceAll` with regex (gates on item 1)
   - Exit gate: residual test262 string failures bucket drops to < 50 cases

7. **Array method gaps test262 surfaces**
   - Likely: `arr.flat`, `arr.flatMap`, `arr.findLast`, `arr.findLastIndex`, `Array.from`, `Array.of`
   - Exit gate: residual test262 array failures bucket drops to < 100 cases

---

### v0.3 — stdlib expansion + dev tooling base

**Goal**: tr can run a real-world TS program (an npm-equivalent slice — small CLI, file processor, log-line transformer) end-to-end, and the dev workflow has first-class IDE + debugger story.

**Exit gate**:
- 5 real-world TS programs (chosen from popular npm CLIs, rewritten in idiomatic TS) run on tr byte-identical with bun
- LSP server provides hover + goto-def + diagnostics in VS Code; round-trip latency < 50 ms on 1 K-line file
- DWARF debug info maps panic backtraces back to `.ts` source `file:line:col`
- Engine has graduated from `labs/0001-walking-skeleton/` to `crates/torajs-{core,cli,runtime}/`

#### Ordered execution

1. **`fs` module**
   - `readFileSync`, `writeFileSync`, `appendFileSync`, `existsSync`, `readdirSync`, `statSync`, `mkdirSync`, `unlinkSync`
   - Async equivalents (gates on v0.5 async/await; sync surface ships first)
   - Exit gate: file-processing CLIs work end-to-end on tr

2. **`Bun` namespace surface**
   - `Bun.file(path).text()` / `.arrayBuffer()` / `.json()`
   - `Bun.write(path, data)`
   - `Bun.argv`, `Bun.env` (Bun-style env access on top of `process.env`-equivalent)
   - Exit gate: bun-compatible file I/O programs run on tr

3. **`process` surface**
   - `process.argv`, `process.env`, `process.exit`, `process.cwd`, `process.platform`, `process.stdout.write`, `process.stderr.write`, `process.stdin.read` (sync)
   - Exit gate: node/bun CLI patterns work

4. **DWARF debug info on AOT**
   - Emit DWARF for ssa_lower output; map LLVM IR back to `.ts` source (line + col)
   - Panic backtrace shows source location
   - Exit gate: panic on a `.ts` program shows the right `file:line:col`

5. **LSP server skeleton**
   - Per-edit incremental typecheck against tr's own check.rs
   - Hover (type + JSDoc passthrough)
   - Goto-def (functions, types, classes, methods)
   - Diagnostics (errors + warnings from check.rs, mapped to LSP severity)
   - VS Code extension scaffold + publish to marketplace as "torajs (preview)"
   - Exit gate: VS Code extension provides hovers + jump-to-def; round-trip latency < 50 ms on a 1 K-line file

6. **Graduation: `labs/0001-walking-skeleton/` → `crates/`** ✅
   - `crates/torajs-runtime` — C source files (refcount, str/arr/json, regex, Date) exposed as `pub const SOURCES`; locks the runtime ABI behind a stable crate boundary
   - `crates/torajs-core` — compiler library (lex, parse, check, modules, ssa, ssa_lower, ssa_inkwell); depends on torajs-runtime
   - `crates/torajs-cli` — the `tr` binary (build / run / lsp / lsp-bench); depends on torajs-core
   - Exit gate met: cargo workspace builds clean, conformance 326/326 + 1 skip preserved, test262 805/0 parity preserved

7. **`tr fmt` + `tr lint`**
   - `tr fmt` (deterministic source reformatter — no config, prettier-shape opinionated)
   - `tr lint` (surfaces the warning set check.rs already emits, plus 5–10 hand-picked rules: unused-let, dead-code-after-return, unreachable-catch, etc.)
   - Exit gate: `tr fmt` formats valid TS; `tr lint` reports diagnostics on a curated bad-code suite

---

### v0.5 — playground + async/await

**Goal**: torajs.com/playground is live (people can try tr without installing), async/await + Promise + fetch work, async bench cases land on the scoreboard.

**Exit gate**:
- torajs.com/playground compiles + runs TS in-browser and supports share links
- A non-trivial async program (HTTP request + JSON parse + Promise.all over 10 fetches) runs end-to-end
- Bench scoreboard adds 5+ async cases; tr at bun parity or better
- Bench scoreboard auto-rendered from `bench/results/` on every commit

#### Ordered execution

1. **wasm32-wasi target for the engine**
   - Cargo target `wasm32-wasi`; Inkwell → wasm IR; engine runs inside a wasm worker
   - Exit gate: `tr run "console.log('hi')"` in a worker page

2. **`async` / `await`**
   - Parser/AST/typecheck for `async fn` and `await`
   - State-machine lowering: each `await` yields control to the executor (textbook PL approach — same shape as Rust async, Swift async, Kotlin coroutines)
   - Single-threaded executor (Tokio-shape, no thread pool — multi-threaded deferred to v1.x)
   - `Promise.all`, `Promise.race`, `Promise.allSettled`, `Promise.any`, `Promise.resolve`, `Promise.reject`
   - Async closures (gates on v0.1 closure machinery, already shipped)
   - Exit gate: 5 async bench cases at bun parity or better

3. **`fetch` (HTTP)**
   - Host (CLI): via `reqwest` — bundled in the runtime, not a runtime-time dep
   - wasm32: via browser `fetch`
   - Streaming response body (basic forms)
   - Exit gate: round-trip a real GET request; bench case `fetch-100-urls` at bun parity

4. **Playground UI**
   - Editor (Monaco) + run button + share-link (URL-encoded source)
   - Output panel + bench scoreboard (auto-rendered from `bench/results/`)
   - Hosted at `torajs.com/playground`
   - Exit gate: `playground.torajs.com` live; share link round-trips work

5. **Bench scoreboard auto-render**
   - Reads `bench/results/*.json` at build time; renders to a static page
   - Updates on every commit to `develop`
   - Exit gate: scoreboard live on torajs.com/bench

---

### v1.0 — polish + integration + 90 % test262

**Goal**: production-grade developer experience; the long-tail substrate (Symbol, BigInt, Function constructor, WeakRef family) closed; test262 in-scope slice ≥ 90 % pass.

**Exit gate**:
- test262 in-scope pass ≥ **90 %** (per `feedback_test262_v1_target.md`)
- `tr debug` step-debugger usable for non-trivial programs
- `libtora.a` + `tora_eval()` lets Rust hosts embed the runtime
- Multi-platform release: darwin-arm64 + linux-x86_64 + linux-aarch64 + windows-x86_64
- Bench scoreboard: `tr build` wins all cases on M-series + linux-x64 + linux-aarch64

#### Ordered execution

1. **`tr debug` step-debugger** (DWARF-driven, on the AOT-cache binary)
   - Step into / step over / step out
   - Breakpoint at `file:line`
   - Variable inspection (primitives + structs + arrays)
   - Exit gate: step through a non-trivial TS program; integrates with VS Code DAP

2. **`tr repl` interactive loop**
   - State preservation across lines; history; multi-line input
   - Exit gate: repl evaluates expressions live; can define functions / classes and call them across lines

3. **`libtora.a` + `tora_eval()` (embedding API)**
   - C ABI: `tora_eval(source, &result, &error)` + per-context state
   - Rust crate `torajs-embed` wraps it; idiomatic Rust API on top
   - Sandboxing knobs (no fs / no net / memory cap) — off-by-default = unsafe; embedders opt out explicitly
   - Exit gate: embed in a Rust app, run a TS script, read the result back

4. **Symbol + BigInt + Function constructor**
   - `Symbol` with metadata machinery (well-known symbols: `Symbol.iterator`, `Symbol.asyncIterator`, `Symbol.toPrimitive`)
   - `BigInt` via self-hosted arbitrary-precision integers (libgmp considered and rejected — violates pillar 2 "自研")
   - `Function` constructor via runtime eval — gates on a JIT-shaped pipeline; design open between (a) runtime invocation of the AOT pipeline + dlopen, (b) keeping an interpreter slice for `Function` only. Decision deferred to mid-v1.0
   - Exit gate: test262 cases requiring these features pass; the Symbol-and-friends bucket drops to < 100 cases

5. **WeakRef / WeakMap / WeakSet (cycle-collecting)**
   - ARC-aware cycle collector (textbook approach: trial deletion — Bacon & Rajan 2001)
   - Exit gate: cyclic data structures don't leak; bench case `cycle-1k` doesn't grow RSS

6. **vtable upgrade for virtual dispatch** (perf cleanup)
   - Today: tag-switch per virtual call site (`O(chain depth)`)
   - v1.0: vtable indirect call (`O(1)`) — vtable_ptr slot already reserved at offset 16 of the class layout
   - Exit gate: virtual-dispatch micro-bench shows the speedup; existing OO bench cases don't regress

7. **Multi-platform release pipeline**
   - darwin-arm64 (already shipping)
   - linux-x86_64, linux-aarch64
   - windows-x86_64
   - `install.torajs.com` script updated to detect platform + pull the right tarball
   - Exit gate: install + run on 4 platforms; CI matrix green

8. **test262 push to 90 %**
   - Close remaining substrate gaps as test262 surfaces them (running test262 every commit on `develop` since v0.1, so this is mostly catch-up work)
   - Likely large buckets: regex Unicode property escapes, Intl.*, advanced async edge cases, Tagged template edge cases
   - Exit gate: ≥ 90 % pass on in-scope slice; final number reported in CHANGELOG

`v1.0` tag at end.

**Time-to-v1.0 estimate (revised from v0.1.0-beta)**: 9–15 months, depending on stdlib scope creep at v0.3 and how much of the long-tail substrate (Symbol, BigInt, Function ctor) needs design rework.

---

## Roadmap v2: 33-item linear plan + three-layer perf gate (2026-05-06 rewrite)

The v0.x sections above describe the **milestone shape** (exit gates, scope). This section is the **execution checklist** — 33 items in strict dependency order, with three layers of perf gate that **block** advancing to the next item on regression. Source: [.claude/rfcs/20260506-roadmap-v2-perf-gated.md](../.claude/rfcs/20260506-roadmap-v2-perf-gated.md).

### Three-layer perf gate

| Layer | Frequency | Cost | Contents |
|---|---|---|---|
| **Mini** | Every commit | Seconds | `cargo build --workspace --release` clean + conformance 326+/0/1skip + `tr lsp-bench 1000` hover P95 ≤ 0.55 ms |
| **Mid** | Every 2-3 functional items | Minutes | Mini + full bench scoreboard sweep — every case `tr build` run_ms ≤ baseline × 1.10, 21+/21+ vs bun-aot, geomean tr/rust ≤ 0.66 |
| **Full** | Every version tag | ~30 min | Mid + full test262 — pass count ≥ baseline (805 minimum), bug bucket = 0, tr-accepted parity = 100.00 %, harness-error = 0 |

**Hard rule**: any gate fail → block next item; fix root cause → re-run gate → only on pass advance. No "I'll fix it later" — `feedback_no_tech_debt`.

### v0.3.0 tag — close partials + dev tooling completion (T-01..T-08)

| ID | Item | Notes / Exit gate |
|---|---|---|
| **T-01** | Documentation debt cleanup | This RFC + roadmap.md (this rewrite) + language-status.md table corrected to HEAD reality |
| T-02 | v0.2 #5 `JSON.parse` f64 path verify + fixture | substrate already in `runtime_str.c:2284` (`__torajs_json_parse_float`); needs caller-driven typing wired + `json-parse-float-001.ts` |
| T-03 | v0.3 #3 finish: `process.{stdout, stderr}.write` + `process.stdin.read` (sync) | runtime helpers + check.rs intrinsic decl + fixtures |
| T-04 | LSP L-2.b: `Checker.errors: Vec<String>` → `Vec<(Span, Severity, String)>` | ~80–100 push-site refactor; same change unlocks `tr lint`'s warning bucket |
| T-05 | v0.3 #7a `tr fmt` | deterministic prettier-shape; reuses `torajs_core::{lexer, parser, ast}` already exported |
| T-06 | v0.3 #7b `tr lint` (5 starting rules) | unused-let / dead-code-after-return / unreachable-catch / shadowed-let / unused-import; depends on T-04 |
| T-07 | Perf debt audit: rpn-eval ✓ (already at parity), fifo-queue-100k tracked → v0.4 | original target rpn-eval-100k 1.54x rust turned out to have been fixed during earlier perf checkpoint (current ratio 0.999, parity); status memory was stale. New only-loss surfaced: fifo-queue-100k 1.17x rust (5-run mean), root cause = tr Array `.shift()` is O(n) memmove vs Rust VecDeque O(1) ring-buffer. Theoretical floor ≈0.24 ms (12 MB memmove ÷ 50 GB/s); observed gap 0.31 ms — within ~30% of inherent algorithmic floor. Fix path is documented (Array deque header w/ `head_offset` field); high LOC + high blast-radius, scheduled into v0.4 substrate sprint. |
| **T-08** | `git tag v0.3.0` | CHANGELOG.md + GH release note + multi-platform tarball |

→ **Mid gate** after T-04, T-06, T-07 → **Full gate** before T-08

### v0.4.0 tag — Object/arguments/Symbol substrate (T-10..T-13.5)

> **Reorder note (2026-05-06)**: original RFC put T-09 (Object
> stdlib) first, but a pre-execution dependency audit found that 6 of
> the 9 Object methods (`entries / fromEntries / freeze / isFrozen /
> defineProperty / defineProperties / getOwnPropertyDescriptor`) all
> need T-10's heterogeneous-array / runtime-mutation-guard
> substrate to ship spec-compliant. Repositioned T-10 to v0.4.0
> opener; T-09 / T-11 then ride T-10's substrate.

| ID | Item | Notes / Exit gate |
|---|---|---|
| **T-10** | `Type::Any` boxing substrate | universal heap header gains type-tagged untyped slot; unlocks T-09 (Object methods that need heterogeneous arrays) / T-11 (`arguments`) / T-13 (Symbol metadata) / T-27 (Function ctor). Sub-tasks: T-10.a Type::Any in check.rs / T-10.b heap header type tag + boxing intrinsic / T-10.c `Array<Any>` runtime / T-10.d codegen sites. Mid gate after T-10.d (full bench scoreboard — must NOT regress geomean tr/rust ≤ 0.66 on the existing 21 cases). |
| T-09 | Object stdlib completion (depends on T-10) | `entries / freeze / isFrozen / getPrototypeOf / setPrototypeOf / defineProperty / defineProperties / getOwnPropertyDescriptor / fromEntries`. setPrototypeOf rejects with phase pointer (nominal class system; out of scope for v0.4). |
| T-11 | `arguments` full materialization | dynamic index, `arguments.callee`, runtime heterogeneous array; depends on T-10 |
| T-12 | `String.raw` + tagged template literals — DEFERRED to post-v0.4.0 | Pre-execution scope review (2026-05-06) found this needs its own substrate (parser support for `tag\`...\`` shape, AST carrying raw + cooked arrays, runtime heterogeneous-string-array emission, String.raw dispatch) — at least 200 LOC across parser/ast/check/ssa_lower. Disjoint from Symbol (T-13) and Array deque (T-13.5), both higher-leverage. T-12 ships as a clean parse-time reject pointing here so users hit a precise deferral message instead of a generic "expected )" parse error. Full T-12 work scheduled for post-v0.4.0 alongside other parser-level extensions. |
| **T-13** | Symbol substrate | per-Type metadata slot in universal heap header; well-known symbols `Symbol.iterator / asyncIterator / toPrimitive`; `Symbol(desc)` + `Symbol.for(key)` registry |
| **T-13.5** | Perf debt: Array deque substrate (`head_offset`) | inherited from T-07. Add `head_offset: u32` to Array universal header, change `arr_shift` from O(n) memmove to O(1) head++; opportunistic compact when `head + len == cap`. Touches every Array slot read/write; goal: drive fifo-queue-100k from 1.17x rust to ≤1.05x. May regress micro-bench startup if header grows; gate on bench scoreboard not regressing geomean tr/rust ≤ 0.66. |

→ **Mid gate** after T-10.d, T-09, T-11, T-13.5 → **Full gate** before v0.4.0 tag

### v0.5.0 tag — async/await + Promise (T-14..T-19)

| ID | Item | Notes / Exit gate |
|---|---|---|
| T-14 | `Type::Promise<T>` in check.rs | type-system support, no runtime semantics yet |
| T-15 | Single-thread executor in runtime crate | Tokio-shape, no thread pool (multi-thread post-v1.0) |
| T-16 | `async` / `await` state-machine lowering | reuse existing generator state-machine framework |
| T-17 | `Promise.all / race / allSettled / any / resolve / reject` | combinators on top of T-16 |
| T-18 | fs async API: `readFile / writeFile / readdir / stat / unlink / mkdir / append` | depends on T-15 |
| T-19 | `Bun.file(p).text() / .arrayBuffer() / .json()` | depends on T-15 + T-18 |

→ **Mid gate** after T-16, T-18, T-19 → **Full gate** before v0.5.0 tag (5+ new async bench cases required)

### v0.6.0 tag — playground (T-20..T-23)

| ID | Item | Notes / Exit gate |
|---|---|---|
| T-20 | wasm32-wasi target enabled | inkwell wasm config + runtime C compiles for wasm32-wasi |
| T-21 | `fetch` (HTTP) | CLI via reqwest (bundled into runtime crate); wasm via browser fetch |
| T-22 | Playground UI | Monaco editor + URL-encoded share-link + output panel; hosted at torajs.com/playground |
| T-23 | Bench scoreboard auto-render | static page from `bench/results/*.json`, updates on every commit; live at torajs.com/bench |

→ **Mid gate** after T-21, T-23 → **Full gate** before v0.6.0 tag

### v1.0.0 tag — polish + 90% test262 (T-24..T-33)

| ID | Item | Notes / Exit gate |
|---|---|---|
| T-24 | vtable upgrade for virtual dispatch | vtable_ptr slot already at offset 16; O(chain depth) → O(1) |
| T-25 | BigInt self-hosted substrate | hand-roll arbitrary-precision int (libgmp rejected per pillar 2 自研) |
| T-26 | WeakRef / WeakMap / WeakSet + ARC-aware cycle collector | Bacon & Rajan 2001 trial-deletion |
| T-27 | Function constructor / `eval` | runtime invocation of LLVM pipeline + dlopen (depends on T-10 Type::Any) |
| T-28 | Multi-platform release | linux-x86_64 / linux-aarch64 / windows-x86_64 + install.torajs.com platform detection |
| T-29 | `tr debug` step debugger | DWARF + DAP adapter (DWARF already shipped v0.3 #4) |
| T-30 | `tr repl` | cross-line state preservation + history + multi-line input |
| T-31 | `libtora.a` + `tora_eval()` embedding API | C ABI + `torajs-embed` Rust crate |
| T-32 | test262 push to 90% | catch-up on regex Unicode property escapes / Intl.* / async edge cases / tagged-template edges |
| **T-33** | `git tag v1.0.0` | CHANGELOG + multi-platform release |

→ **Mid gate** after T-24, T-26, T-28, T-30 → **Full gate** before v1.0.0 tag (test262 in-scope ≥ 90% is the milestone hard exit gate)

### Items deliberately **not** in the 33-item plan

- **LSP L-4.b** (scope-aware local goto-def) — polish, not substrate; deferred with semantic tokens / inlay hints / completion as a post-v1.0 LSP polish pass
- **LSP L-4.c** (cross-file goto-def) — same bucket
- **Decorators** — out of scope (per Out-of-scope features section below)
- **JSX** — out of scope
- **Mapped / conditional types** — out of scope
- **Multi-threaded executor** — post-v1.0 stretch

---

## Roadmap v3 — current execution checklist (2026-05-09, HEAD `983243e`)

v2 (2026-05-06 rewrite) projected 33 items through v1.0. As of HEAD
`983243e`, T-01..T-26 are shipped and the v0.7 substrate phase
(T-24 / T-25 / T-26 A·B·C) is complete. The checklist below merges the
remaining v2 items (T-27..T-33) with the post-T-26 follow-ups
surfaced during ship (BigInt math, cycle-collector class-field
substrate, perf debt) into a **single strict-order linear plan**.
status memory's "顺序执行计划" mirrors this list.

Working rule (unchanged from v2): one item ships at a time, mid-gate
every 2-3 items, full-gate before any tag. No "I'll fix it later" —
every ✅ has its commit hash + conformance gate recorded below.

### Done (v0.1.0-beta → BigInt div+mod)

| ID | Item | HEAD commit | Conformance gate |
|---|---|---|---|
| T-01..T-08 | v0.3.0 closeout (docs / JSON.parse-f64 / process.io / LSP errors / `tr fmt` / `tr lint` / perf debt audit) | `git tag v0.3.0` | mid + full passed |
| T-09..T-13.5 | v0.4.0 substrate (Type::Any boxing / Object stdlib / arguments / Symbol / Array deque) | `git tag v0.4.0` | mid + full passed |
| T-14..T-19 | v0.5.0 async/await + Promise + fs/promises + Bun.file | `git tag v0.5.0` | mid + full passed |
| T-20..T-23 | v0.6.0 wasm32-wasi + fetch + Playground + bench scoreboard | `git tag v0.6.0` (21d9783) | mid + full passed |
| T-24 | vtable upgrade for virtual dispatch | `0bd8aa1` | 370/0/1 |
| T-25 | BigInt self-hosted substrate | `30ce155` | 371/0/1 |
| T-26.A | WeakRef substrate (global registry) | `5420a1b` | 372/0/1 |
| T-26.B | WeakMap + WeakSet (shared registry) | `f3372fb` | 374/0/1 |
| T-26.C | Bacon-Rajan cycle collector substrate | `c76b3a3` | 374/0/1 |
| post-T-25 | BigInt division + modulo | `983243e` | 375/0/1 |

### Phase 1 — close BigInt arithmetic surface

- [ ] **V3-01** BigInt `**` exponent. Square-and-multiply; negative exponent → `RangeError` per spec. ~50 LOC.
- [ ] **V3-02** BigInt bitwise (`& | ^ ~ << >>`). Two's-complement simulation since BigInt has no fixed width. ~150 LOC.
- [ ] **V3-03** `BigInt(value)` ctor — string parse / number-with-Math.trunc / bigint-clone. ~80 LOC.
- [ ] **V3-04** Karatsuba multiplication (perf only; threshold ~32 limbs). Schoolbook stays default below threshold.

→ **Mid gate** after V3-04 (full bench scoreboard; BigInt cases get their own bucket).

### Phase 2 — class typechecker substrate

Currently `class Node { next: Node | null }` is rejected because Node
isn't yet in `c.aliases` when its own field types resolve. Same root
cause blocks `class C { f: C[] }`. These three items unblock both the
gc-001 fixture (Phase 3) and a long tail of common TS class-OO
patterns.

- [ ] **V3-05** Nominal class types refactor. Pre-register class names with placeholder before resolving fields, OR introduce `Type::ClassRef(String)` for nominal class identity (`Type::Struct` currently structural). Pick the minimum-blast-radius option; not both. ~200 LOC + careful migration.
- [ ] **V3-06** `class C { f: C[] }` accepted. Same root-cause family as V3-05; typically a free side-effect once V3-05 lands.
- [ ] **V3-07** `as` cast parser support. Needed for the `arr.push(self as any)` cycle pattern + general TS users.

### Phase 3 — close cycle collector (depends on V3-05..V3-07)

- [ ] **V3-08** `gc-001-basic.ts` conformance fixture — multi-class A↔B cycle, manual `gc()`, verify the cycle frees. Substrate is in place since `c76b3a3`; only the surface needed.
- [ ] **V3-09** Arr / Closure children visitor — cycle collector descends into Array slots + closure env captures (currently Obj-only).
- [ ] **V3-10** Cycle collector auto-trigger — buffer-size threshold + main-exit drain.

→ **Mid gate** after V3-10.

### Phase 4 — perf data refresh + outstanding debt

- [ ] **V3-11** Re-run 7-runtime bench scoreboard. Last full sweep at `14b894f` (commit `529143c`); 6 substrate commits since then need a fresh datum.
- [ ] **V3-12** rpn-eval-100k 1.29x rust — escape → alloca for stack-local 16-elem Array literal. `let stack: number[] = [0,0,...]` currently allocates per inner-loop iter (100k × malloc); promote to alloca when literal doesn't escape.

→ **Mid gate** after V3-12.

### Phase 5 — v1.0 tooling + features

- [ ] **V3-13** T-30 `tr repl`. Cross-line state preservation, multi-line input, history (rustyline). Highest-leverage dev-experience item.
- [ ] **V3-14** T-31 `libtora.a` + `tora_eval()` C-ABI embed + `torajs-embed` Rust crate. Unlocks third-party embed scenarios; also substrate for V3-16.
- [ ] **V3-15** T-29 `tr debug` step debugger. DAP adapter atop existing v0.3 #4 DWARF emission. VS Code extension as visible client.
- [ ] **V3-16** T-27 Function ctor / `eval`. Runtime invocation of LLVM pipeline + dlopen. Depends on V3-14 (eval ≈ embed + run).

→ **Mid gate** after V3-16.

### Phase 6 — multi-platform release infrastructure

- [ ] **V3-17** T-28 multi-platform release.
  - linux-x86_64, linux-aarch64 cross-compile from darwin-arm64
  - windows-x86_64 via mingw or native GitHub Actions runner
  - install.torajs.com platform-detection script
  - GitHub Actions matrix: build + bench + conformance per platform
  - Multi-arch tarball + signed release notes

→ **Mid gate** after V3-17.

### Phase 7 — test262 push to 90% (v1.0 hard exit gate)

- [ ] **V3-18** T-32 test262 push to ≥ 90% in-scope pass rate.
  - Likely large buckets: regex Unicode property escapes / Intl.* basic / async edge cases / tagged-template edge cases / BigInt edge cases (toLocaleString, toString radix).
  - Each bucket may surface its own substrate need; treat each sub-bucket as its own ship.

→ **Full gate** before V3-19.

### Phase 8 — ship v1.0

- [ ] **V3-19** T-33 `git tag v1.0.0`. CHANGELOG covering v0.7..v1.0 + multi-platform release announcement on torajs.com.

### Phase 9 — post-v1.0 polish (deferred)

- [ ] **V3-20** LSP L-4.b scope-aware local goto-def.
- [ ] **V3-21** LSP L-4.c cross-file goto-def.
- [ ] **V3-22** LSP semantic tokens / inlay hints / completion.
- [ ] **V3-23** Multi-threaded executor + Send/Sync (stretch).
- [ ] **V3-24** WebAssembly user-code target — distinct from v0.5's engine-as-wasm (stretch).
- [ ] **V3-25** `tora install` package manager (stretch; gates on a security/supply-chain story).

---

### Beyond v1.0 — stretch goals

Not committed; tracked here so they don't get lost.

- **Multi-threaded executor + `Send` / `Sync`** — parallel mandelbrot scales linearly; requires per-actor heap or shared-heap concurrent ARC
- **WebAssembly user-code target** — emit wasm artifacts from `.ts` source for non-browser deployment (different from v0.5's "engine-as-wasm")
- **torajs package manager** (`tora install`) — npm-shape; tarball + lockfile; resolves from registry. Gates on a security/supply-chain story
- **Decorators** — if user demand surfaces post-v1.0
- **Intl.*** — full ICU integration (basic Intl shipped at v0.3 / v1.0 as test262 forces)

---

## Principles

- **Every step is visible** — at the end of each step, there's a command you can run and see output. No "internal-only" steps.
- **Small grain** — each step is roughly 1-3 days of work, ~100-500 LOC. If a step grows past that, it splits.
- **Front-loaded detail** — milestones close to now are spelled out per-step; far milestones are headers + exit gates. We re-detail later milestones when we get there.
- **Each step is potentially throwaway** — research mode. If a step's outcome surprises us, we revisit before continuing.
- **bun is the oracle** — when behavior is ambiguous, write the TS equivalent, run in `bun`, match.
- **Engine lives under `crates/torajs-{runtime,core,cli}/`** as of v0.3 #6. Pre-graduation history is preserved in git; the v0.1.x bench + test262 numbers in this doc were measured under the prior `labs/0001-walking-skeleton/` layout.

---

## Backend pivot (2026-04-28) — historical

Through P3.1–P3.3 the AOT path was **wasm-via-C**: tr → wasm-encoder → wasm2c (wabt) → clang -O3 → native binary. This won the bench but had hard ceilings — `compile_ms` floor ~95 ms, no GC integration, no tail calls, no exceptions, external dep on wabt + Apple clang. Replaced (P3.4–P3.7) with a single LLVM-via-Inkwell backend:

```
frontend (lex → parse → check) → SSA IR (rich types, ownership-aware,
                                          partial-evaluated, pattern-matched)
                                              ↓
                                     Inkwell (LLVM 22)
                                              ↓
                                       AOT object + ld
                                              ↓
                                ┌──────────────────────────┐
                                ↓                          ↓
                          `tr build -o foo`     `tr run` (cache: ~/.torajs/cache)
                          (write to user path)  (write to cache slot, then exec)
```

Both modes are first-class and share one codegen path. `tr build` writes the binary to a user-given path (production / distribution). `tr run` writes it to the cache slot keyed by `hash(source + version + opt)`, then exec's — first run pays compile (~50 ms small / ~90 ms larger), reruns hit the cache and exec directly (~10 ms wrapper + native execution).

### run_ms ceiling = three layers, not one

```
run_ms 极限 = optimal_codegen × optimal_runtime × optimal_layout
            = LLVM            × no-GC ownership × Rust-style layout
```

Picking LLVM solved the codegen layer. The runtime layer (no GC, deterministic drop) is where bun/V8 lose 4–20× — their codegen is fine, their runtime + layout carry too much overhead. Specific commitments:

1. **IR carries rich type info** → emit specialized LLVM IR. Monomorphization, devirtualization at IR level, `noalias` from ownership analysis, `!range` from type narrowing.
2. **Compile-time ownership inference + hidden ARC** — alias-aware analysis under TS-shape semantics handles single-owner paths with deterministic drops at scope exit. Multi-owner paths (Array<T> aliasing, throw/catch shared structs, cross-scope closure captures) route through a hidden ARC-style refcount on the universal heap header. Inc on share, dec on drop, free at zero — Swift ARC / CPython pattern. The user never sees `Rc<T>` or writes `.clone()`. (Pre-pivot framing called for `Rc<T>` as user-visible escape valve; that was wrong — corrected 2026-04-30. Pre-2026-05-02 framing called for "no refcount" period; corrected when implementation revealed ownership-only didn't compose for Array<non-Copy> aliasing — refcount went hidden / runtime-internal instead.)
3. **Language-level PGO** — `@hot` / `@cold` attributes → LLVM `branch_weights` metadata.
4. **Pattern-detected intrinsics** — Brian Kernighan popcount → `@llvm.ctpop.i64`, ctz/clz/bswap, vectorizable nested loops → NEON.
5. **Stack/arena allocation first** — escape analysis to stack-allocate non-escaping locals; region inference for fn-scoped temporaries.
6. **Apple Silicon tuning** — Apple LLVM beats upstream LLVM by ~7% on M-series; Inkwell links against `/usr/lib/libLLVM.dylib` on darwin.
7. **Compile-time partial evaluation** — const folding, template literal concat, `[1,2,3].length → 3` happen in IR before LLVM.

---

## BENCH — cross-runtime perf benchmark (cross-cutting track)

A horizontal track running alongside every milestone, not numbered as a phase. Lives at `bench/` (top-level), implemented as a Rust harness crate that drives **bun, node, rust, go, python**, and torajs through a uniform per-case workload.

### Status (2026-05-04, HEAD `5434f12`)

`tr build` wins **all 19 cases** vs bun on the current scoreboard (latest sweep includes csv-rebuild +18 % over bun). `tr run` (cache hit) trails `tr build` by ~8 ms exec floor but still beats bun-jsc on every compute case. The seven runtimes compared: `rust`, `go`, `node-v8`, `bun-aot`, `bun-jsc`, `torajs` (`tr build` AOT), `torajs-run` (`tr run` cache hit).

See `bench/results/` for the full per-case table; `README.md` carries the rendered scoreboard.

### Adding a case

Drop a directory under `bench/cases/<name>/` with `main.<lang>` files, an `expected.txt`, and an optional `bench.toml` (runs / warmup / `torajs_opt` knob). The harness skips runners whose source file is missing — so a case can be torajs-only or torajs-+-rust if the workload doesn't translate to other langs.

**Rule (per `feedback_bench_tr_must_pass.md`)**: every committed bench case must have torajs producing `ok`. A case where torajs appears as `fail` (because the language doesn't support that workload yet) is treated as the milestone not having been achieved. The bench scoreboard and torajs's language capability grow in lockstep.

---

## Cross-cutting tracks

Work that runs **alongside** every milestone, not as one of them. Tracked here so it stays visible.

### Test infrastructure

- **Per-milestone acceptance criteria** — each row above carries its own exit gate. Cumulative test count drives a regression net.
- **Bench scoreboard as integration test** — every case is an end-to-end test; a regression there is a P0.
- **Integration test crate** at `crates/torajs-itest/` (post-graduation) runs full `tr build` + execute on every example under `examples/`. CI gate.
- **Property testing** — quickcheck-style for the type checker's alias-aware ownership analysis (random ASTs, must accept TS-valid programs and reject multi-rooted ones). Lands when alias bugs surface.
- **Fuzzing** — `cargo fuzz` targets for the lexer + parser. Lands during the `labs/` → `crates/` graduation.

### CI / release process

- **GitHub Actions on `develop`**: per-commit `cargo build` + `cargo test` + `cargo clippy --workspace --all-targets -- -D warnings` + `bun run check` for `web/`. Gates merge.
- **Release branches** per `git-flow`. `main` is production; `develop` is integration; milestones close on `develop` and roll up to `main` at version tags (`v0.1.0-beta` shipped 2026-05-04 from `develop` directly; subsequent stable tags follow git-flow `release/*` branches).
- **Tag-driven artifact publishing** on tag: build `tr` binary for darwin-aarch64 + linux-x86_64 + linux-aarch64 + windows-x86_64, package as a tarball, attach to GH release. Distributed via a future `tora-up` install script.

### Documentation

- **`docs/` is canonical** — this roadmap, `stdlib.md`, `language-status.md`, future `lang-reference.md`, `embedding.md`. Versioned with the code.
- **Public website** at `torajs.com` — landing (live since v0.1) + playground (v0.5) + docs + bench scoreboard (auto-generated from `bench/results/`).
- **No external blog/marketing** during research phase. Communications happen on takagi's discretion.

### Performance work as a continuous track

Perf work happens incrementally:
- v0.1 (shipped): codegen baseline established (LLVM AOT + cache); 19/19 bench wins vs bun.
- v0.2 / v0.3: avoid regressing existing bench cases as features land; new substrate (regex, Date, fs) gets its own bench cases.
- v0.3 (graduation to `crates/`): formal perf RFCs land — bit-packing for bool, SoA layouts for hot loops, vtable for virtual dispatch.
- v0.5 (DWARF / source maps): perf work becomes profile-guided as DWARF unlocks profiler workflows.
- v1.0+: monomorphization-driven inlining tweaks, ARC-aware cycle collector tuning.

### Security / threat model (for embedding + playground)

- **CLI binary** (shipped) — runs trusted user code; same threat model as Node/Bun.
- **Embedding API** (v1.0) — runs partially-trusted scripts. Sandboxing knobs mandatory; off-by-default = unsafe.
- **Playground** (v0.5) — runs untrusted code in an isolated wasm worker, hard memory + CPU caps. Fresh instance per Run.
- **No supply-chain story** until package manager exists (post-v1.0). Stdlib + user-relative imports only — no third-party packages can introduce vulnerabilities.

---

## Out-of-scope features

Things explicitly NOT in the v1.0 path. Some have been demoted from earlier drafts (under the wrong "Rust semantics" framing) or restored after a corrected understanding (`feedback_torajs_ambition`).

- **`==` / `!=`** — only `===` / `!==`. Source-rewrite layer normalizes loose equality to strict before typecheck.
- **`var` keyword** — only `let` / `const`.
- **Decorators** — not in v1.0 path. Use cases are better served by macros (far) or manual code; if user demand surfaces post-v1.0, revisit.
- **JSX** — out of scope. Use plain TS or a build-time preprocessor.
- **`Proxy` / `Reflect`** — dropped. No-GC runtime makes Proxy's interception model expensive; static typing covers the Reflect surface in 95 % of cases.
- **Conditional / mapped types** (`Pick<T, K>`, `Partial<T>`, `T extends U ? X : Y`) — TS-specific compiler tricks bound to its inference model. Probably never; the bun-equivalence guarantee is observable behavior, not type-system identity.
- **`Rc<T>` / `Arc<T>` / `RefCell<T>` user-visible types** — the runtime uses refcount-like techniques internally on the universal heap header, but these are NEVER user-facing.
- **WebAssembly user-code target** (different from "engine-as-wasm" in v0.5) — emit wasm artifacts from `.ts` source for non-browser deployment. Post-v1.0.
- **Multi-threaded executor + `Send` / `Sync`** — single-threaded async is enough for v1.0 (matches bun's main path). Multi-threaded deferred to post-v1.0.

### Restored to the path (corrections from earlier drafts)

- ~~`null` dropped by design~~ — `null` is supported (since 2026-05-04, `Type::Ptr` becomes Copy + null sentinel handling). `undefined` is a v1.0 work item — until it lands, use `Nullable<T>` (`T | null`).
- ~~Class syntax (initial) — possibly later~~ — class is **fully shipped in v0.1.0-beta** (instance + static fields/methods, single inheritance + super, virtual dispatch, abstract classes, `private`/`protected` modifiers).
- ~~`Symbol` / `WeakMap` / `WeakRef` dropped~~ — moved to v1.0 with a self-hosted ARC-aware cycle collector; bun supports them via V8 so tr must too.
- ~~`eval` / `Function` constructor dropped~~ — `Function` constructor is a v1.0 work item; the design is open between (a) runtime invocation of the AOT pipeline + dlopen and (b) keeping an interpreter slice for `Function` only. `eval` follows the same path.
- ~~Test262 conformance out of scope~~ — restored as a hard requirement on 2026-05-03. The in-scope slice of test262 (~5K-15K cases) is a v1.0 gate at ≥ 90 % pass; v0.1 baseline is 651/23941, v0.2 target is ≥ 2500/23941.
- ~~Cycle-collecting weak references~~ — v1.0 must support them; the implementation is an ARC-aware cycle collector (Bacon & Rajan trial-deletion approach), not a "users restructure to avoid" surface.

---

## Historical phase numbering

The roadmap has been through three numbering schemes:

- **Pre-2026-04-30**: phase-numbered sections (P0, P1, ..., P17) under the now-discarded "TS syntax + Rust semantics" framing. P0/P1/P2.4/P3/stdlib slice 1 descriptions were accurate (those shipped under TS-shape semantics regardless of framing); P2/P4-P17 baked in Rust-specific concepts (`Rc<T>`, affine moves, `Send`/`Sync` ownership types, `'a` lifetimes) that were corrected on 2026-04-30.
- **2026-04-30 to 2026-05-04**: milestone-numbered sections (M1, M2, ..., M9 + M-OO) targeting bun-shape feature parity with compile-time ownership inference. M1 / M2 / M3 / M4 / M-OO / M6.1 / M6.2 shipped under this scheme; M5 / M6.3 partial / M7 / M8 / M9 were planned but never fully executed under that numbering.
- **From 2026-05-04**: version-numbered milestones (v0.1.0-beta retro + v0.2 / v0.3 / v0.5 / v1.0 forward) — the **current committed plan above**. M-numbering was dropped because the v0.1.0-beta release coincided with several M-numbered milestones being either complete (M1-M4, M-OO, M6.1) or partially complete (M5, M6.2, M6.3) or not yet started (M7, M8, M9), and re-numbering around the public release boundary made the plan more legible than continuing to mix M-state.

For archival reference of what was discussed pre-pivot, see git history: commit `4892919` for the P3-onward industrial plan; commit `84241d9` for the M1-M9 pre-pivot version; commit `5434f12` for the M1-M9 + M-OO version (last revision before the v0.x rewrite).
