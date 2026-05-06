# Changelog

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
