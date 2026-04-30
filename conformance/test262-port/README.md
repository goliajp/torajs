# test262 port

Hand-curated subset of [test262](https://github.com/tc39/test262) cases mechanically rewritten to torajs's TypeScript subset.

## Why a port, not a runner

test262 cases are deeply tied to JS object/class machinery — even the simplest `1 + 1 === 2` test does:

```js
//CHECK#1
if (1 + 1 !== 2) {
  throw new Test262Error('...');     // class + new
}
var x = 1;                            // var
new Object().prop = 1;                // Object()
```

torajs's subset doesn't have `class` / `new` / `var` / Object wrappers. So we can't run the harness file (`assert.js`, `sta.js`) at all, let alone the tests. **Pure test262 pass rate on torajs = 0%**, not because of bugs, but because the surface area we don't (yet) implement.

The port instead **mechanically rewrites** each case into torajs-compatible TS:

| original test262 | rewrite |
|---|---|
| `var x = 1;` | `let x: number = 1;` |
| `throw new Test262Error('msg')` | `throw "msg"` |
| `new Number(x)` | `x` (drop the wrapper — boxed numbers behave like primitives in `===` / `+`) |
| `new Object()` | a `type T = { ... }` + literal — case-by-case |
| `eval("...")` | the literal expression that `eval` would evaluate |
| `assert.sameValue(a, b)` | `if (a !== b) throw "..."` inline |

Each ported file links back to the original test262 path in a comment block; the port is the source of truth for what runs, the original is provenance.

## What the port proves

- **Three-way agreement** — bun, torajs JIT, torajs AOT all produce the same output on the ported source. Same as the rest of `conformance/cases/`.
- **Spec-anchored coverage** — each port's *intent* comes from a real ECMA-262 conformance test. A bug in our subset's spec compliance will show up here even if it never came up in our hand-written cases.
- **Honest claim** — we say "we implement TS-subset-of-ECMA-262 conformance for the listed test262 cases; here are X cases adapted to verify it." Not "we pass test262."

## What it doesn't prove

- It doesn't claim torajs runs raw test262. We'd need `class` / `new` / regex / Symbol / Map / Set / async / generator / spread / destructuring / template strings / prototype chain to get even close.
- It doesn't replace `conformance/cases/`. Those are torajs-shape examples; this is JS-spec-shape examples adapted.

## Naming

`<category>-<NNN>-<original-test262-stem>.ts` — categories so far: `add` / `sub` / `mul` / `div` / `cmp` / `bitop` / `string` / `array`. Filename ends with the original test262 stem to make grep'ing the lineage easy.

## Performance — torajs vs bun on the same source

The port files are valid TS, so bun runs them too. hyperfine n=10 warmup=3 on M4 Pro:

| case | torajs (AOT) | bun | speedup |
|---|--:|--:|--:|
| add-001              | 1.36 ms | 9.47 ms  | **7.0×** |
| sub-001              | 1.66 ms | 9.46 ms  | **5.7×** |
| mul-001              | 1.53 ms | 9.51 ms  | **6.2×** |
| cmp-001-strict-eq    | 1.42 ms | 9.15 ms  | **6.4×** |
| cmp-002-lt-gt        | 1.47 ms | 9.26 ms  | **6.3×** |
| bitop-001            | 1.47 ms | 9.22 ms  | **6.3×** |
| string-001-length    | 1.43 ms | 9.21 ms  | **6.4×** |
| string-002-slice     | 1.44 ms | 9.45 ms  | **6.6×** |
| string-003-includes  | 1.40 ms | 9.39 ms  | **6.7×** |
| string-004-indexOf   | 1.52 ms | 9.28 ms  | **6.1×** |
| array-001-length     | 1.42 ms | 9.10 ms  | **6.4×** |
| array-002-push-index | 1.54 ms | 10.65 ms | **6.9×** |
| array-003-map        | 1.43 ms | 9.41 ms  | **6.6×** |
| array-004-filter     | 1.45 ms | 10.00 ms | **6.9×** |
| array-005-reduce     | 1.47 ms | 9.47 ms  | **6.4×** |
| control-001-if-else  | 1.54 ms | 9.58 ms  | **6.2×** |
| control-002-for-loop | 1.47 ms | 9.82 ms  | **6.7×** |
| control-003-while    | 1.41 ms | 9.44 ms  | **6.7×** |
| control-004-try-catch| 1.48 ms | 9.13 ms  | **6.2×** |
| control-005-finally  | 1.64 ms | 9.48 ms  | **5.8×** |

**19/19 wins.** Mean torajs 1.5 ms vs bun 9.4 ms.

**Caveat — most of this gap is startup overhead.** torajs AOT is a ~35 KB static binary that exec()s and runs the case in ~1 ms. bun is a ~63 MB engine that bootstraps + warms up + JIT-compiles the TS in ~5-9 ms before user code even starts. The gap stays around 8 ms ± noise across cases regardless of work, because each port does very little actual computation.

On compute-heavy workloads (`bench/cases/` perf suite), torajs's edge over bun shrinks to 1.5-4× because the bun startup overhead amortizes over a longer execution. The "torajs is 6× faster than bun" headline is true for **this** suite (short programs); the wider perf claim is in `README.md`'s scoreboard.

Combined picture:
- **Conformance**: bun + torajs JIT + torajs AOT all produce identical output (bun is the oracle).
- **Short-program perf**: torajs 6× faster (startup-dominated).
- **Long-program perf** (`bench/cases/`): torajs 1.5-4× faster, depending on workload.
