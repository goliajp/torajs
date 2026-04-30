# torajs conformance suite

A growing set of small TypeScript programs whose output must match across **bun** (the oracle for TS spec behavior), **torajs JIT** (Cranelift), and **torajs AOT** (LLVM). Three-way agreement is the bar — any divergence is a bug in torajs (or, for known-divergent corners like mutable-by-reference closure capture, an explicit `.expected` opt-out).

## Why this exists, and what it isn't

There's no official "TypeScript runtime conformance" test suite — TS compiles to JS, and ECMAScript runtime conformance is **[test262](https://github.com/tc39/test262)**, which targets a full JS implementation. torajs is a TS subset (no class / prototype / regex / Symbol / Proxy / Map / Set / async / generator / …) so running test262 wholesale is neither realistic nor honest.

Instead, this suite is a **tractable, hand-curated, growing oracle**:
- Each case is a small TS program exercising one milestone-level feature.
- For most cases, **bun is the oracle** — the runner pipes the source through `bun run`, captures stdout, and asserts torajs produces identical output on **both** JIT and AOT.
- For the few cases where torajs's subset semantics intentionally diverge from JS (e.g. mutable-by-reference closure captures), an `.expected` file alongside the `.ts` overrides the bun result.
- Bench cases live in `bench/` and answer "is it fast"; conformance cases answer "is it correct."

## Layout

```
conformance/
├── cases/              ← .ts files (+ optional .expected)
│   ├── m1-01-hello.ts        # one feature per file
│   ├── m1-02-arith.ts
│   ├── ...
│   └── edge-04-string-empty.ts
├── runner/             ← Rust runner (cargo workspace member)
│   ├── Cargo.toml
│   └── main.rs
└── README.md           ← this file
```

Naming convention: `<milestone-prefix>-<NN>-<short-name>.ts`. Prefixes `m1` / `m2` / `m3` / `m4` / `m6` map to roadmap milestones; `edge-*` is for boundary conditions that don't belong to a single milestone.

## Running

```bash
cargo build --release --manifest-path conformance/runner/Cargo.toml
./target/release/torajs-conformance
```

The runner reports `<n> pass / <m> fail / <k> skip`, exits 0 iff all cases pass. A skip means bun couldn't run the source (e.g. when bun isn't installed); not a fail. Failures show the diverging output and which side disagreed (`jit ≠ oracle` or `aot ≠ oracle`).

## Current status (2026-05-01)

```
39 pass / 0 fail / 0 skip
```

All cases agree across bun, torajs JIT (Cranelift), and torajs AOT (LLVM).

Coverage so far:
- **M1** (subset core): hello / arith / bool ops / if-else / for-loop / break-continue / while / array.push+index / string concat / block scope / recursion / mutual recursion / bitwise ops including hex literals
- **M2** (closures): non-capturing arrow / single capture / multi capture / closure-as-arg / nested closures
- **M3** (generics): id<T> / multi-type-param fst/snd / struct types / generic structs (Pair<A,B>)
- **M4** (errors): throw+catch / 2-deep throw propagation / finally on normal+catch paths / return inside try-with-finally / throw "msg"+catch (e: string)
- **M6** (stdlib): String slice/includes/startsWith/endsWith/indexOf / split+join / Array.map/filter/reduce/forEach / method chains / Array<string>
- **edge**: deep recursion (Ackermann) / int math / empty arrays / empty strings

## Adding cases

For a new feature: write a small `.ts` that exercises only that feature, output something deterministic, run it through `bun run` once to verify the expected output, drop it under `cases/`, re-run the runner.

For a known-divergent case (torajs intentionally != bun): also drop a `.expected` file with the torajs-correct output. Document why in a comment at the top of the `.ts` (e.g. `// torajs uses by-value capture; bun follows TS spec ref-shape`).

## Future scope

When torajs grows toward a more complete subset:
- Pull selected test262 cases that fall within our supported syntax (object literal / array literal / string ops / bitops / control flow / closures).
- Add a CI gate so any divergence shows up before commit.
- Track conformance-suite size as a milestone metric (current: 39 cases; target post-M5 / M6 full: 100+).

The test262-port is plumbing, not a research goal — what we want from this suite is "torajs behaves like real TS for the parts it claims to support, with three independent backends agreeing."
