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

`<category>-<NNN>-<original-test262-stem>.ts` — categories so far:

| category | what it covers | count |
|---|---|--:|
| `add` / `sub` / `mul` / `div` / `mod` | binary arithmetic — multiple T-variants per op (operand patterns, negatives, large ints, string concat) | 12 |
| `unary` | `-x`, `!b` | 2 |
| `cmp` | `===`, `!==`, `<`, `>`, `<=`, `>=`, string equality | 4 |
| `bitop` | `&`, `\|`, `^`, `<<`, `>>` (one case per shape) | 4 |
| `logical` | `&&`, `\|\|` | 2 |
| `control` | `if/else`, `for`, `while`, `break`, `continue`, `try/catch/finally`, nested-try, throw-propagate, nested-if/for/while, finally-on-break, finally-on-return, throw-inside-loop, block-scope shadow | 17 |
| `func` | function declaration, recursion, mutual recursion, void return | 3 |
| `closure` | single capture, multi capture, closure-as-arg, pick-fn (return one of two callable args), passing-fn-back (combinator), chain-call `f(0)(5)`, stateful-counter (struct-wrapped state) | 7 |
| `object` | type-alias struct, field read/write, pass-to-fn, nested fields, mutation-via-fn, array of struct, struct with array field, deeply-nested, pass-to-multiple-fns | 9 |
| `class` | constructor + method, mutating method, single inheritance, `super(args)` passthrough, three-level extends, inherited method on subclass instance, string-typed fields, this-passed-around | 8 |
| `string` | length, slice, includes, indexOf, charCodeAt, startsWith, endsWith, split+join, pass-to-multiple-fns | 9 |
| `array` | length, push+index, map, filter, reduce, forEach, method-chain, multi-element growth, nested array, pass-to-multiple-fns | 10 |
| `generic` | inferred `<T>`, generic struct `Pair<A, B>`, multi-typeparam factory | 3 |
| `math` | abs/min/max/floor/ceil/sqrt/pow, exp/log/PI/E | 2 |
| `throw` | string value, struct value with `catch (e: Err)` | 2 |
| `let-const` | const happy path | 1 |
| `integration` | multi-feature combinators — fib precomputed, array-of-class, string-build, stats fns, quicksort, prime sieve, collatz step count, cmp+logical chain, string search, list stats, multi-throw-types | 11 |
| **total** | | **106** |

Filename ends with the original test262 stem (where one exists) to make grep'ing the lineage easy. Cases without a direct test262 lineage (closures, generics, classes — all TS / TS-subset additions outside ECMA-262 itself) carry a topical stem instead.

## Performance — torajs vs bun on the same source

The port files are valid TS, so bun runs them too. hyperfine n=10 warmup=3 on M4 Pro, measured 2026-04-30 on commit `25f0e9a`:

| case                                  | torajs (AOT) |    bun    | speedup |
| ------------------------------------- | -----------: | --------: | ------: |
| add-001-S11.6.1_A2.1_T1               |  1.81 ms |   9.04 ms | ** 5.0×** |
| array-001-length                      |  1.16 ms |   8.65 ms | ** 7.5×** |
| array-002-push-index                  |  1.43 ms |   8.78 ms | ** 6.1×** |
| array-003-map                         |  1.25 ms |   9.35 ms | ** 7.5×** |
| array-004-filter                      |  1.18 ms |   9.55 ms | ** 8.1×** |
| array-005-reduce                      |  1.22 ms |   9.49 ms | ** 7.8×** |
| array-006-forEach                     |  1.02 ms |   9.02 ms | ** 8.8×** |
| array-007-method-chain                |  1.30 ms |   9.42 ms | ** 7.2×** |
| array-008-multi-element               |  1.26 ms |   9.71 ms | ** 7.7×** |
| bitop-001-S11.10.1                    |  2.88 ms |   9.49 ms | ** 3.3×** |
| class-001-basic                       |  1.18 ms |   9.69 ms | ** 8.2×** |
| class-002-method-mutation             |  1.36 ms |   9.68 ms | ** 7.1×** |
| class-003-extends-basic               |  1.55 ms |   9.71 ms | ** 6.3×** |
| class-004-super-arg-passthrough       |  1.36 ms |   9.34 ms | ** 6.9×** |
| closure-001-capture                   |  1.37 ms |   9.69 ms | ** 7.1×** |
| closure-002-multi-capture             |  1.55 ms |   9.83 ms | ** 6.3×** |
| closure-003-callback                  |  1.41 ms |   9.44 ms | ** 6.7×** |
| cmp-001-strict-eq                     |  1.38 ms |   9.29 ms | ** 6.7×** |
| cmp-002-lt-gt                         |  1.20 ms |   9.76 ms | ** 8.1×** |
| control-001-if-else                   |  1.50 ms |   9.33 ms | ** 6.2×** |
| control-002-for-loop                  |  1.40 ms |   9.69 ms | ** 6.9×** |
| control-003-while                     |  2.30 ms |   9.45 ms | ** 4.1×** |
| control-004-try-catch                 |  1.52 ms |  10.72 ms | ** 7.1×** |
| control-005-finally                   |  0.64 ms |   8.47 ms | **13.2×** |
| control-006-nested-try                |  1.73 ms |   9.16 ms | ** 5.3×** |
| control-007-throw-propagate           |  1.86 ms |   9.28 ms | ** 5.0×** |
| control-008-break-continue            |  1.51 ms |   9.34 ms | ** 6.2×** |
| control-009-finally-runs-on-return    |  1.40 ms |   9.17 ms | ** 6.6×** |
| div-001-S11.5.2_A2.1_T1               |  1.88 ms |  10.35 ms | ** 5.5×** |
| func-001-recursion                    |  1.04 ms |   9.49 ms | ** 9.1×** |
| func-002-mutual-recursion             |  1.88 ms |   9.93 ms | ** 5.3×** |
| generic-001-id-fn                     |  1.60 ms |   9.89 ms | ** 6.2×** |
| generic-002-pair                      |  1.47 ms |   9.56 ms | ** 6.5×** |
| logical-001-S11.11.1                  |  1.60 ms |   9.64 ms | ** 6.0×** |
| logical-002-S11.11.2                  |  1.22 ms |  10.21 ms | ** 8.4×** |
| math-001-builtins                     |  1.67 ms |  10.02 ms | ** 6.0×** |
| mod-001-S11.5.3_A2.1_T1               |  1.85 ms |  10.09 ms | ** 5.5×** |
| mul-001-S11.5.1_A2.1_T1               |  1.45 ms |   9.60 ms | ** 6.6×** |
| object-001-field-access               |  1.23 ms |  10.06 ms | ** 8.2×** |
| object-002-field-mutation             |  1.61 ms |   9.76 ms | ** 6.1×** |
| object-003-passed-to-fn               |  1.53 ms |   9.83 ms | ** 6.4×** |
| object-004-nested-fields              |  1.83 ms |  10.01 ms | ** 5.5×** |
| string-001-length                     |  1.36 ms |   9.79 ms | ** 7.2×** |
| string-002-slice                      |  1.57 ms |   9.86 ms | ** 6.3×** |
| string-003-includes                   |  1.63 ms |  10.37 ms | ** 6.4×** |
| string-004-indexOf                    |  1.34 ms |   9.94 ms | ** 7.4×** |
| string-005-charCodeAt                 |  1.88 ms |   9.99 ms | ** 5.3×** |
| string-006-startsWith                 |  1.89 ms |  10.23 ms | ** 5.4×** |
| string-007-endsWith                   |  1.69 ms |   9.40 ms | ** 5.6×** |
| string-008-split-join                 |  1.22 ms |   9.04 ms | ** 7.4×** |
| sub-001-S11.6.2_A2.1_T1               |  1.65 ms |   9.25 ms | ** 5.6×** |
| throw-001-string                      |  1.22 ms |   9.45 ms | ** 7.7×** |
| throw-002-struct                      |  1.63 ms |   9.61 ms | ** 5.9×** |
| unary-001-S11.4.7_A2.1                |  1.76 ms |   9.42 ms | ** 5.4×** |
| unary-002-S11.4.9_A2.1                |  1.26 ms |   9.23 ms | ** 7.3×** |

**55/55 wins.** Mean torajs 1.50 ms vs bun 9.59 ms — geometric-mean speedup **6.4×**, range 3.3× (`bitop-001`, the noisiest case) to 13.2× (`control-005-finally`, where bun's `try/catch` setup amortizes badly over a one-shot test).

**Caveat — most of this gap is startup overhead.** torajs AOT is a ~36 KB static binary that exec()s and runs the case in ~1 ms. bun is a ~63 MB engine that bootstraps + warms up + JIT-compiles the TS in ~5-9 ms before user code even starts. The gap stays around 8 ms ± noise across cases regardless of work, because each port does very little actual computation.

On compute-heavy workloads (`bench/cases/` perf suite), torajs's edge over bun shrinks to 1.5-4× because the bun startup overhead amortizes over a longer execution. The "torajs is 6× faster than bun" headline is true for **this** suite (short programs); the wider perf claim is in `README.md`'s scoreboard.

Combined picture:
- **Conformance**: bun + torajs JIT (`tr run`) + torajs AOT (`tr build`) all produce identical output (bun is the oracle).
- **Short-program perf** (this suite, 55 ports): torajs 6.4× faster (startup-dominated).
- **Long-program perf** (`bench/cases/`, 15 cases): torajs ties or beats rust on 13/15, sweeps bun/node 15/15.
