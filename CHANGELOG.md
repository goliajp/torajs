# Changelog

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
