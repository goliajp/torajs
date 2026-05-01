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
| `unary` | `-x`, `!b`, `~x` | 3 |
| `cmp` | `===`, `!==`, `<`, `>`, `<=`, `>=`, string equality | 4 |
| `bitop` | `&`, `\|`, `^`, `<<`, `>>` (one case per shape) | 4 |
| `logical` | `&&`, `\|\|` | 2 |
| `control` | `if/else`, `for`, `while`, `do-while`, `break`, `continue`, `try/catch/finally`, nested-try, throw-propagate, nested-if/for/while, finally-on-break, finally-on-return, throw-inside-loop, block-scope shadow | 19 |
| `switch` | basic + default, fall-through, string scrutinee | 3 |
| `func` | function declaration, recursion, mutual recursion, void return | 3 |
| `closure` | single capture, multi capture, closure-as-arg, pick-fn, passing-fn-back, chain-call `f(0)(5)`, stateful-counter, counter-pair | 8 |
| `object` | type-alias struct, field read/write, pass-to-fn, nested fields, mutation-via-fn, array of struct, struct with array field, deeply-nested, pass-to-multiple-fns | 9 |
| `class` | constructor + method, mutating method, single inheritance, `super(args)` passthrough, three-level extends, inherited method on subclass instance, string-typed fields, this-passed-around, multi-class-coexist | 9 |
| `string` | length, slice, includes, indexOf, charCodeAt, startsWith, endsWith, split+join, pass-to-multiple-fns | 9 |
| `array` | length, push+index, map, filter, reduce, forEach, method-chain, multi-element growth, nested array, pass-to-multiple-fns | 10 |
| `generic` | inferred `<T>`, generic struct `Pair<A, B>`, multi-typeparam factory | 3 |
| `math` | abs/min/max/floor/ceil/sqrt/pow, exp/log/PI/E | 2 |
| `throw` | string value, struct value with `catch (e: Err)` | 2 |
| `ternary` | `cond ? a : b` — boolean cond, matching branches, nested chain | 1 |
| `assign` | compound assignment `+= -= *= /= %=` | 1 |
| `inc-dec` | `x++` / `x--` / `++x` / `--x` | 1 |
| `typeof` | `typeof` returns "number" / "string" / "boolean" / "object" | 1 |
| `concat` | number→string auto-coerce on `+` | 1 |
| `let-const` | const happy path | 1 |
| `integration` | multi-feature combinators — fib precomputed, array-of-class, string-build, stats fns, quicksort, prime sieve, collatz step count, cmp+logical chain, string search, list stats, multi-throw-types, matrix multiply, csv-build | 14 |
| **total** | | **122** |

Filename ends with the original test262 stem (where one exists) to make grep'ing the lineage easy. Cases without a direct test262 lineage (closures, generics, classes — all TS / TS-subset additions outside ECMA-262 itself) carry a topical stem instead.

## Performance — torajs vs bun on the same source

The port files are valid TS, so bun runs them too. hyperfine n=10 warmup=3 on M4 Pro, measured 2026-04-30 on commit `7b8f5cb`:

| case                                  | torajs (AOT) |    bun    | speedup |
| ------------------------------------- | -----------: | --------: | ------: |
| add-001-S11.6.1_A2.1_T1               |  1.35 ms |  10.49 ms | ** 7.8×** |
| add-002-S11.6.1_A2.1_T2               |  2.00 ms |   9.69 ms | ** 4.8×** |
| add-003-string-concat                 |  1.27 ms |  10.42 ms | ** 8.2×** |
| add-004-large-int                     |  1.53 ms |   9.65 ms | ** 6.3×** |
| array-001-length                      |  1.25 ms |   9.93 ms | ** 7.9×** |
| array-002-push-index                  |  1.16 ms |   9.91 ms | ** 8.5×** |
| array-003-map                         |  1.32 ms |   9.23 ms | ** 7.0×** |
| array-004-filter                      |  1.13 ms |   9.82 ms | ** 8.7×** |
| array-005-reduce                      |  0.91 ms |   9.40 ms | **10.3×** |
| array-006-forEach                     |  1.27 ms |   9.84 ms | ** 7.7×** |
| array-007-method-chain                |  1.87 ms |   9.27 ms | ** 5.0×** |
| array-008-multi-element               |  1.68 ms |   9.91 ms | ** 5.9×** |
| array-009-nested                      |  1.31 ms |   9.53 ms | ** 7.3×** |
| array-010-pass-to-multiple-fns        |  1.66 ms |   9.84 ms | ** 5.9×** |
| bitop-001-S11.10.1                    |  1.34 ms |  10.48 ms | ** 7.8×** |
| bitop-002-and                         |  1.08 ms |   9.57 ms | ** 8.9×** |
| bitop-003-or-xor                      |  1.12 ms |   9.88 ms | ** 8.8×** |
| bitop-004-shifts                      |  1.07 ms |   9.78 ms | ** 9.1×** |
| class-001-basic                       |  1.44 ms |  10.31 ms | ** 7.2×** |
| class-002-method-mutation             |  2.10 ms |   9.65 ms | ** 4.6×** |
| class-003-extends-basic               |  2.07 ms |  10.29 ms | ** 5.0×** |
| class-004-super-arg-passthrough       |  1.44 ms |   9.47 ms | ** 6.6×** |
| class-005-multilevel-extends          |  1.57 ms |   9.72 ms | ** 6.2×** |
| class-006-inherited-method-on-subclass |  1.32 ms |   9.25 ms | ** 7.0×** |
| class-007-string-fields               |  1.99 ms |  10.03 ms | ** 5.0×** |
| class-008-this-passed-around          |  1.15 ms |  10.00 ms | ** 8.7×** |
| closure-001-capture                   |  2.23 ms |  11.61 ms | ** 5.2×** |
| closure-002-multi-capture             |  1.40 ms |   9.37 ms | ** 6.7×** |
| closure-003-callback                  |  1.42 ms |   9.57 ms | ** 6.7×** |
| closure-004-pick-fn                   |  1.76 ms |   9.99 ms | ** 5.7×** |
| closure-005-passing-fn-back           |  1.78 ms |   9.71 ms | ** 5.5×** |
| closure-006-chain-call                |  1.32 ms |   9.36 ms | ** 7.1×** |
| closure-007-stateful-counter          |  1.88 ms |  10.48 ms | ** 5.6×** |
| cmp-001-strict-eq                     |  1.21 ms |   9.78 ms | ** 8.1×** |
| cmp-002-lt-gt                         |  1.58 ms |  10.29 ms | ** 6.5×** |
| cmp-003-le-ge                         |  1.43 ms |  10.35 ms | ** 7.2×** |
| cmp-004-string-eq                     |  1.57 ms |   9.80 ms | ** 6.2×** |
| control-001-if-else                   |  1.48 ms |   9.29 ms | ** 6.3×** |
| control-002-for-loop                  |  1.91 ms |   9.81 ms | ** 5.1×** |
| control-003-while                     |  1.47 ms |  10.27 ms | ** 7.0×** |
| control-004-try-catch                 |  1.31 ms |  10.50 ms | ** 8.0×** |
| control-005-finally                   |  1.29 ms |   9.60 ms | ** 7.4×** |
| control-006-nested-try                |  1.25 ms |   9.60 ms | ** 7.7×** |
| control-007-throw-propagate           |  1.41 ms |   9.67 ms | ** 6.9×** |
| control-008-break-continue            |  1.20 ms |   9.70 ms | ** 8.1×** |
| control-009-finally-runs-on-return    |  1.55 ms |   9.26 ms | ** 6.0×** |
| control-010-nested-if                 |  2.46 ms |   9.39 ms | ** 3.8×** |
| control-011-nested-for                |  1.31 ms |  10.10 ms | ** 7.7×** |
| control-012-throw-inside-loop         |  1.51 ms |   9.57 ms | ** 6.3×** |
| control-013-block-scope-shadow        |  1.85 ms |   9.76 ms | ** 5.3×** |
| control-014-finally-on-break          |  1.69 ms |   9.17 ms | ** 5.4×** |
| control-015-throw-inside-catch        |  1.41 ms |   8.79 ms | ** 6.2×** |
| control-016-fib-with-base-throw       |  1.61 ms |  10.01 ms | ** 6.2×** |
| control-017-nested-while              |  1.65 ms |   9.39 ms | ** 5.7×** |
| div-001-S11.5.2_A2.1_T1               |  1.48 ms |   9.59 ms | ** 6.5×** |
| div-002-negatives                     |  2.03 ms |  10.25 ms | ** 5.0×** |
| func-001-recursion                    |  1.30 ms |   9.88 ms | ** 7.6×** |
| func-002-mutual-recursion             |  1.26 ms |   9.36 ms | ** 7.4×** |
| func-003-void-return                  |  1.42 ms |   9.50 ms | ** 6.7×** |
| generic-001-id-fn                     |  1.20 ms |   9.72 ms | ** 8.1×** |
| generic-002-pair                      |  1.26 ms |   9.71 ms | ** 7.7×** |
| generic-003-multi-typeparam           |  1.96 ms |  10.20 ms | ** 5.2×** |
| integration-001-fib-precomputed       |  1.16 ms |   9.50 ms | ** 8.2×** |
| integration-002-array-of-class        |  1.40 ms |   9.73 ms | ** 7.0×** |
| integration-003-string-build          |  1.13 ms |   9.22 ms | ** 8.2×** |
| integration-004-stats-fns             |  1.66 ms |   9.29 ms | ** 5.6×** |
| integration-005-quicksort             |  1.19 ms |   9.58 ms | ** 8.1×** |
| integration-006-prime-sieve           |  1.38 ms |  10.11 ms | ** 7.3×** |
| integration-007-collatz               |  1.58 ms |   9.27 ms | ** 5.9×** |
| integration-008-cmp-chain             |  1.26 ms |   9.31 ms | ** 7.4×** |
| integration-009-string-search         |  1.59 ms |   9.81 ms | ** 6.2×** |
| integration-010-list-stats            |  1.82 ms |   9.51 ms | ** 5.2×** |
| integration-011-multi-throw-types     |  1.20 ms |   9.60 ms | ** 8.0×** |
| let-const-001                         |  1.02 ms |   9.50 ms | ** 9.3×** |
| logical-001-S11.11.1                  |  1.57 ms |   9.33 ms | ** 5.9×** |
| logical-002-S11.11.2                  |  1.19 ms |   9.68 ms | ** 8.1×** |
| math-001-builtins                     |  1.69 ms |   9.93 ms | ** 5.9×** |
| math-002-trig-and-log                 |  1.14 ms |   8.97 ms | ** 7.9×** |
| mod-001-S11.5.3_A2.1_T1               |  1.80 ms |   9.67 ms | ** 5.4×** |
| mod-002-negatives                     |  1.71 ms |  10.47 ms | ** 6.1×** |
| mul-001-S11.5.1_A2.1_T1               |  1.27 ms |  10.08 ms | ** 7.9×** |
| mul-002-zero-and-negatives            |  1.47 ms |   9.76 ms | ** 6.6×** |
| object-001-field-access               |  1.72 ms |   9.69 ms | ** 5.6×** |
| object-002-field-mutation             |  1.64 ms |   9.93 ms | ** 6.1×** |
| object-003-passed-to-fn               |  1.55 ms |  10.03 ms | ** 6.5×** |
| object-004-nested-fields              |  1.27 ms |   9.27 ms | ** 7.3×** |
| object-005-mutation-via-fn            |  1.17 ms |   9.52 ms | ** 8.1×** |
| object-006-array-of-struct            |  0.91 ms |   8.97 ms | ** 9.9×** |
| object-007-struct-with-array-field    |  1.03 ms |   9.83 ms | ** 9.5×** |
| object-008-deeply-nested              |  1.35 ms |   9.43 ms | ** 7.0×** |
| object-009-pass-to-multiple-fns       |  1.09 ms |   8.98 ms | ** 8.2×** |
| string-001-length                     |  1.14 ms |   9.44 ms | ** 8.3×** |
| string-002-slice                      |  1.41 ms |   9.63 ms | ** 6.8×** |
| string-003-includes                   |  1.11 ms |   9.22 ms | ** 8.3×** |
| string-004-indexOf                    |  1.09 ms |   9.35 ms | ** 8.6×** |
| string-005-charCodeAt                 |  1.00 ms |   9.52 ms | ** 9.5×** |
| string-006-startsWith                 |  1.48 ms |   9.35 ms | ** 6.3×** |
| string-007-endsWith                   |  0.75 ms |   9.77 ms | **13.0×** |
| string-008-split-join                 |  1.41 ms |   9.81 ms | ** 7.0×** |
| string-009-pass-to-multiple-fns       |  1.98 ms |  10.45 ms | ** 5.3×** |
| sub-001-S11.6.2_A2.1_T1               |  1.11 ms |   9.36 ms | ** 8.4×** |
| sub-002-negatives                     |  1.57 ms |   9.77 ms | ** 6.2×** |
| throw-001-string                      |  1.34 ms |   8.76 ms | ** 6.5×** |
| throw-002-struct                      |  1.28 ms |   9.31 ms | ** 7.3×** |
| unary-001-S11.4.7_A2.1                |  1.45 ms |   9.32 ms | ** 6.4×** |
| unary-002-S11.4.9_A2.1                |  1.20 ms |   9.50 ms | ** 7.9×** |

**106/106 wins.** Mean torajs 1.44 ms vs bun 9.70 ms — arithmetic-mean speedup **6.7×**, range 3.8× (`control-010-nested-if`, the noisiest case in this run) to 13.0× (`string-007-endsWith`).

**Caveat — most of this gap is startup overhead.** torajs AOT is a ~36 KB static binary that exec()s and runs the case in ~1 ms. bun is a ~63 MB engine that bootstraps + warms up + JIT-compiles the TS in ~5-9 ms before user code even starts. The gap stays around 8 ms ± noise across cases regardless of work, because each port does very little actual computation.

On compute-heavy workloads (`bench/cases/` perf suite), torajs's edge over bun shrinks to 1.5-4× because the bun startup overhead amortizes over a longer execution. The "torajs is ~6× faster than bun" headline is true for **this** suite (short programs); the wider perf claim is in `README.md`'s scoreboard.

Combined picture:
- **Conformance**: bun + torajs JIT (`tr run`) + torajs AOT (`tr build`) all produce identical output (bun is the oracle).
- **Short-program perf** (this suite, 106 ports): torajs 6.7× faster (startup-dominated).
- **Long-program perf** (`bench/cases/`, 15 cases): torajs ties or beats rust on 14/15, sweeps bun/node 15/15.
