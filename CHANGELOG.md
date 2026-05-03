# Changelog

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
