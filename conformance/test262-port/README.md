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
