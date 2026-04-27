# 0001 — Project direction

**Status**: open discussion
**Started**: 2026-04-26
**Participants**: takagi@golia.jp + Claude

## Context

torajs is a new repo with a public domain (torajs.com) and a hand-picked stack (PG18+/Valkey, Caddy on t01, Rust + TS, axum API, multi-binary). Web scaffold is up. Beyond that, the **actual product** is not decided.

Treating this as a closed-source research-and-learning project. Lots of experiments, lots of 废案, advance in small steps.

## What we already agreed

- Closed-source / internal — no community pressure
- Top-level layout: `web/`, `crates/`, `labs/`, `examples/`, `docs/`
- Rust workspace will be multi-binary, axum at the core
- DB: PG18+ (truth) + Valkey (cache)
- Deploy: Caddy on t01 under `/apps/torajs/`
- Working mode: new ideas → `labs/` first, no tests/CI/scaffolding pressure there

## Working hypothesis (tentative)

> torajs = an embeddable TS/JS runtime written in Rust, positioned as a Lua-replacement scripting layer for Rust hosts.

This came from two threads:

1. The original framing: "torajs is our engine; the engine is mainly Rust; TS is replacing what we used to do in Lua"
2. The "更先进的 bun?" detour, which we rejected as unwinnable head-on (Oven 30+ engineers, $35M+, perpetually catching up). The interesting niches Bun **doesn't** target are where there's room.

Bun-niche map:

| Niche | Verdict | Why |
| --- | --- | --- |
| General-purpose runtime, faster Bun | ✗ | Unwinnable, no leverage |
| **Embeddable JS in Rust hosts** | ✓ candidate | Real gap (deno_core / boa / rquickjs are libraries, not products); matches original "Lua replacement" framing; we presumably already have Rust hosts that need it |
| Agentic / sandboxed runtime for AI tools | ? | Trendy, real problem (Cloudflare Workers solves it closed-source). Plausible second crate later. |
| Edge multi-tenant isolate runtime | ✗ | Too deep, V8-isolate territory |

## Naming tension (parked)

"torajs" suggests JS-first product. If the **engine** (Rust) is the real product and TS is just one binding, the name undersells it (compare: swc / biome / turbopack — Rust tools branded under JS audience). Two resolutions:

- **A**: Lean into JS-first framing. Public face = "the TS-scriptable engine", Rust is implementation detail. Like Deno/Bun — users don't think of them as Rust/Zig projects. Keep `torajs` as the project + crate name.
- **B**: Project = `tora` (Rust-first identity), `torajs` = the npm package + TS binding name. `crates/tora-core`, `bindings/torajs`.

User noted there might already be a project literally named `tora` in the workspace (mentioned in dotclaude README's git-tracking table); need to check before committing to (B).

**Decision**: parked. Research project, name doesn't block code. Revisit when something concrete is shipped.

## Open questions

These are the discussion items I want to ground out before writing code.

### Q1 — Is there a real Rust host needing scripting *right now*?

The whole "Lua replacement" framing only makes sense if there's a Rust app that *currently* needs (or will need) embedded scripting. Otherwise it's a solution looking for a problem.

- Is there a sibling project (in `goliajp/`, `qualcomm/insight`, dadaya, `mailrs`, etc.) that has a scripting need today or in the next 6 months?
- If yes — what does it want to script? (config / business rules / extensions / UI behavior / hot-reload of game logic / agent tool calls / ...)
- If no — do we accept this is **pure research with no internal customer**, and proceed anyway?

> **takagi (2026-04-26)**: 看进展，不做要求.

**Resolution**: pure research mode confirmed. No internal-customer constraint. We don't need to justify the project against a downstream consumer; the embeddable-in-Rust-host angle becomes one possible application, not the central thrust. This frees us to choose technically interesting paths over commercially defensible ones.

### Q2 — Which JS engine do we embed?

The interesting Rust-side gap isn't "implement a JS engine" — it's "wrap an engine well for Rust hosts". So the choice is among existing options:

| Engine | Speed | Footprint | TS support | Rust binding | Notes |
| --- | --- | --- | --- | --- | --- |
| **quickjs-ng** | medium | tiny (~600 KB) | needs swc/oxc transpile | `rquickjs` | proven, hackable, no JIT |
| **rusty_v8** | fastest | huge (~30 MB) | needs transpile | first-class | what Deno uses; complex C++ build |
| **boa** | slow | small | partial | native Rust | pure Rust, easy build, immature |
| **JSC (via napi-style)** | fast | medium | needs transpile | none mature | what Bun uses; binding work massive |

Probably: start with quickjs-ng + rquickjs for the embedded niche (small, hackable), keep v8 as escape hatch for "fast embedded host" scenarios.

> **takagi (2026-04-26)**: 有可能会要自己写.

**Resolution — major reframing**: writing the engine ourselves is on the table. This changes the project's gravity significantly:

- **Existing engines become references, not dependencies.** quickjs-ng / boa / rusty_v8 source becomes reading material; we may still embed one as a comparison baseline in `labs/`, but the primary arc is now "build our own".
- **WASM has to be first-class from day 1** — see Q4. Whatever we build must compile to wasm32 cleanly, since torajs.com depends on it.
- **No-JIT to start.** Writing a tree-walking or bytecode interpreter is a 6–12 month research arc by itself. Adding a JIT (cranelift / custom) is its own multi-year arc and goes in `labs/` only when we have a working interpreter.
- **Likely staging**: lexer → parser → AST tree-walker (subset of ES) → bytecode VM → optimizing passes → maybe JIT. Each stage is a series of `labs/` experiments before anything graduates to `crates/`.
- **Spec coverage is *not* a goal.** Test262 conformance is a tarpit; we pick the subset of JS/TS that's interesting for our purposes (almost certainly not the bits like `with`, `eval`, sloppy mode, full Intl).
- The "Lua-replacement for Rust hosts" angle may still be a downstream binding, but the engine itself is the project.

This shifts the working hypothesis (see top of doc) — needs a rewrite once we run the first experiment.

### Q3 — First lab experiment

What's the **smallest** thing whose result tells us "yes worth pursuing" or "no, kill it"?

Candidates (each ~1 day, lives in `labs/`):

- `labs/01-quickjs-tokio` — embed quickjs-ng in a tokio runtime, expose one async Rust fn (e.g. `fetch`) callable from JS, prove the bridge works without deadlocks.
- `labs/02-ts-direct` — pipe TS through swc/oxc → quickjs without writing tsc to disk. Time the cold-start vs Bun.
- `labs/03-script-as-config` — host a Rust "app" with hot-reloading TS scripts as its rule engine. Closer to the Lua-replacement product shape.

(03) is the most product-shaped; (01) is the smallest engineering risk; (02) tells us whether we can actually skip the heavyweight TS toolchain.

> **takagi (2026-04-26)**: lab 根据研究进展来定就好.

**Resolution**: defer lab choice. Given Q2 ("might write our own"), the original three candidates are mostly obsolete — they all assume embedding an existing engine, which is no longer the lead direction. The first lab will probably emerge naturally from "what's the smallest piece of an engine I can write end-to-end" — most likely a hand-rolled lexer + tree-walker for a tiny ES subset (e.g. number literals + binary ops), but I'm not picking it now. We choose when we're about to write code.

### Q4 — torajs.com — what does the site even show?

Current state: a "Hello from torajs.com" placeholder. Until we know what torajs is, the site has nothing to put there.

Options:
- Defer entirely — site stays a placeholder for months
- Turn the site into a **public lab journal** (markdown notes from `.claude/researches/` rendered as a blog) — works because the project IS a research log
- Make it a **playground** where visitors run scripts in the browser via wasm-built engine (only meaningful once Q3 produces something)

> **takagi (2026-04-26)**: torajs 先当成浏览器环境的 playground.

**Resolution**: torajs.com = in-browser playground. Implications:

- **WASM target is mandatory from day 1.** Anything in `crates/` that's part of the engine must compile to `wasm32-unknown-unknown` (or `wasm32-wasi`). We pick crates / patterns accordingly — no `tokio::net`, no `mio`, no native FFI in the engine core. Async is fine but only against wasm-compatible executors (e.g. `wasm-bindgen-futures`).
- **Site needs**, eventually:
  - Code editor (Monaco or CodeMirror 6) — CodeMirror is lighter, fits the "research playground" vibe
  - Output panel (stdout/stderr emulation, AST view, bytecode view, error display)
  - Engine loaded via dynamic `import()` of a wasm bundle
  - Probably tabs to switch between source / AST / tokens / bytecode for the educational angle
- **Until the engine produces output**, the playground can be a **shell with a stub** — editor on the left, "engine not yet implemented" on the right, plus the same lab-journal idea (markdown rendering of research notes) as a secondary tab. That gives torajs.com something real to show *now* while the engine catches up.
- We won't bundle the GDS demo content into the public site — `web/` currently uses GDS for theme/components; that's fine to keep, but the page content shifts toward "playground" framing.

## Working hypothesis (revised, 2026-04-26)

> torajs = a research-grade **TypeScript-native** engine written in Rust, with WASM as a first-class target so torajs.com can host it as an in-browser playground. TS is the source language the engine actually executes — there is no JS-output intermediate stage. Embeddable-in-Rust-hosts is a possible downstream application, not the central thrust.

Pure research, no customer, long arc, lots of throwaway expected.

## Constraints that fall out

- **WASM-clean engine core** — no `tokio` / `mio` / native FFI in the engine itself.
- **No spec-conformance treadmill** — Test262 is not a goal. We pick the JS/TS subset that's interesting.
- **TS-native, not TS-transpiled** (decided 2026-04-26 — see TS Decision below). Lexer / parser / IR / interpreter all see TypeScript. `tsc`-style "strip types, emit JS" is explicitly rejected.
- **No JIT until much later** — interpreter first (tree-walker → bytecode VM); JIT is its own multi-year arc.
- **Playground-first feedback loop** — every milestone in the engine should be visible in torajs.com (even if just "tokens for this input").

## TS Decision (2026-04-26)

> **takagi**: 我希望只支持 ts，不再是翻译成 js 再执行.

Engine source language is TypeScript only. No `.js` files at the input level (decision: revisit later if `.d.ts` import or interop ever needs it). No transpile step in the pipeline.

**Pipeline shape that this rules out**:
```
TS source → tsc/swc/oxc → JS AST → engine        ← REJECTED
```

**Pipeline shape this implies**:
```
TS source → torajs lexer → TS-aware AST → torajs IR → torajs interpreter
```

The lexer and parser must handle TS syntax natively: type annotations (`x: number`), generics (`<T>`, with the `<T>` vs JSX vs comparison ambiguity), `type` aliases, `interface` (even though our coding-style says no `interface` in TS *source code we write*, the engine has to parse it because users might), `as` / `satisfies` casts, `keyof`, `typeof` in type position, conditional types (`A extends B ? X : Y`), mapped types (`{ [K in keyof T]: ... }`), `import type`, declaration files (probably not in v1 scope), enums (probably reject as a language design call — they're a known TS wart).

This is significantly more parser surface than ECMAScript alone, but well-trodden — swc / oxc / tsc are all open-source references. We don't *use* them, but we read them.

**Open question — what do types do at runtime?** Three flavors, each implies a different engine:

| Flavor | What it means | Effort | Research interest |
| --- | --- | --- | --- |
| **A. Erased** | Parse types, throw away after parse, runtime is JS-shaped (`1 + "a" === "1a"`) | Low (just a more permissive parser) | Low — this is what `swc strip-types` already does. Boring. |
| **B. Statically checked then erased** | Run a type checker over the AST before execution; reject ill-typed programs at load time; runtime is JS-shaped | High (we're rebuilding `tsc` 's checker — the hardest part of TS) | Medium-high — owning the checker is real work but the runtime is unchanged. |
| **C. Live / type-directed runtime** | Types affect execution: `readonly` actually freezes, `as const` deep-freezes, branded types are nominally checked, `1 + "a"` is a runtime error not coercion, `null`/`undefined` distinction enforced, structural type tags carried at runtime | Very high (new language semantics, not just TS) | Highest — this is "more advanced than the mainstream". Effectively defines a TS *dialect* with stricter semantics. Closest to "更先进的 bun" idea applied to language design. |

(C) is the most genuinely interesting for a research project — it treats TS not as "JS plus optional type comments" but as its own typed language with semantic teeth. It also drifts further from `tsc` compatibility: code that runs in `tsc → node` may not run in torajs. For research that's fine; for "drop-in TS replacement" it's not. Given Q1 ("no customer"), drift is acceptable.

(B) is the moderate path — full `tsc` compatibility on the type-checking side, JS semantics at runtime. Reimplements the most expensive part of TS for least leverage.

(A) is the boring path; rejecting it implicitly when we say "more advanced than transpile-then-run".

**Recommendation, not decision**: lean toward (C) for research interest. Start with the *erased* path in `labs/` (cheapest way to bootstrap a parser + interpreter), then progressively let types influence runtime as the engine matures. That way (A) is a stage we pass through, not the destination.

> takagi: ___ (which flavor — A as a stepping stone, B, or C as the real target?)

## Open follow-ups

These didn't come up explicitly but matter soon:

- **Module system** — ESM only, or Common* too? ESM only is the modern and simpler choice.
- **GC strategy** — refcounting (quickjs-style, simple, leaks cycles) vs tracing GC (correct, harder)? quickjs-ng survives fine on refcount + cycle collector; that's a reasonable starting choice.
- **Enums / `namespace` / decorators** — TS has features that even the TS team regrets. Worth deciding which we reject at the language-design level.
- **`.d.ts` / declaration files** — probably out of scope for v1. The engine doesn't need them; they're a tooling artifact.
- **Repository name vs project name** — parking lot. Once the engine has a real shape, we revisit (`tora` for the engine, `torajs` for the JS-facing artifacts) is still on the table.

## Next step

When ready to write code: open `labs/0001-<slug>/` with a one-line README of the question being asked, write the smallest thing that answers it, and link the result back into this doc as a follow-up section.

This doc stays the canonical discussion log — append answers as they land, don't delete.
