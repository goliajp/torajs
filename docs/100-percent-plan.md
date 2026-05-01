# 100% test262 plan

Goal: cover the test262 surface as much as the TS-subset compilation
model can. The subset boundary is mutable — when a test262 case needs
a feature that's "out of subset", we extend the subset (with notes
in this doc + `docs/ts-subset.md`).

The constraint that doesn't move: **runtime perf of `tr build` /
`tr run` must not regress on the bench scoreboard**. Every new feature
routes through SSA + LLVM + cc, no interpreter fallback.

This is an internal escape-hatch document, like rust's `unsafe`
boundary. We document _why_ each escape is taken so future readers
can audit.

---

## Status (as of commit 0291cc3)

234/234 ports. 49 commits since the original 184-port baseline.

---

## Phase B — Variadic & Aggregate (HIGH priority)

| feature | shape | est | port-impact |
|---|---|---|---|
| spread in fn-call args | `f(...arr)` | M | high — exits `f.apply` |
| rest parameters | `function f(...args)` | M | high — many test262 use |
| object spread | `{...a, b: 1}` | M | medium |
| Map (string-key minimal) | class-shape API | M-L | high |
| Set (string / number) | class-shape API | M-L | high |
| WeakMap/WeakSet | aliased to Map/Set | S | low |

## Phase C — Big Types

| feature | shape | est | port-impact |
|---|---|---|---|
| BigInt | i128 / arbitrary | L | high (arithmetic tests) |
| Symbol | unique String wrapper | M | medium |
| Date | i64 epoch ms + format | L | medium |

## Phase D — Iterators & Async

| feature | shape | est | port-impact |
|---|---|---|---|
| Generators / yield | state-machine | XL | high |
| async / await | Promise + statemachine | XL | very high |
| Promise (basic) | callback chain | L | very high |
| Iterator protocol | next() returning {value, done} | M | medium |

## Phase E — Pattern & Element

| feature | shape | est | port-impact |
|---|---|---|---|
| regex literal `/p/` | NFA / DFA | XL | very high |
| String.match / search / matchAll | regex-driven | M | medium |
| Modules (import/export) | per-file linking | L | medium |

## Phase F — OO

| feature | shape | est | port-impact |
|---|---|---|---|
| M-OO.3 vtable + method override | per-class vtable | XL | high |
| `instanceof` operator | tag-bit + walk | M | medium |
| `class extends ... implements` | interface check | M | low |

## Phase G — Misc

| feature | shape | est | port-impact |
|---|---|---|---|
| Object.entries / fromEntries | tuple support | M | medium |
| Object.assign (shallow merge) | static-known fields | M | low |
| Array.from(string) | per-char unfold | M | medium |
| Number.toString(radix) | snprintf %s + custom | S | low |
| Math.copySign / fmod | libm | S | low |
| try/catch/finally + throw + return — fix lower bug | known v0 issue | M | medium |

---

## Execution rule

1. One feature per commit; suite runs green before commit.
2. Bench refresh every 10 commits.
3. New cap-doubling / cap-shrinking heap behavior must include a
   budget assertion in the perf-gate test (TBD — gate hasn't been
   set up yet at this scale; for now visual inspection).
4. Document every "subset extension" decision here, with one-line
   why-now justification, just like `unsafe` blocks document why
   safety isn't statically verifiable.
