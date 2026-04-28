# torajs

Research project (closed-source, internal). Working hypothesis: a statically-typed language in Rust with **TypeScript syntax** + **Rust-shaped semantics**, two execution modes — **AOT to native binary** (`tr build`) and **JIT-style compile-and-run** (`tr run`), the Go shape. Public site: https://torajs.com

## Anti-Hallucination (NON-NEGOTIABLE)

Follow `.claude/rules/common/anti-hallucination.md` — always. Five rules, zero exceptions: say "I don't know", use tools before memory, no chain-guessing, retract mid-sentence when wrong, cite the source. Tool output itself must never be fabricated: if a tool returns only `[rerun: bN]` or empty content, report that literally and rerun — never invent plausible-looking output.

## Tech Stack

- **Languages**: Rust (engine + API) + TypeScript (web + future scripting surface)
- **Web**: React 19 + react-router 7 + Vite 8 + Tailwind 4 + jotai + @tanstack/react-query, in `web/` (scaffolded from `devops/starters/web`)
- **Rust**: cargo workspace under `crates/`, multi-binary, axum as the API core. Concrete crate names TBD.
- **Database**: PostgreSQL 18+ (primary store) + Valkey (cache / ephemeral)
- **Public domain**: torajs.com

## Build & Test

```bash
# web (torajs.com)
cd web && bun install
bun run dev          # vite dev server
bun run check        # tsc --noEmit && eslint && prettier --check
bun run test         # vitest run
bun run build        # tsc --noEmit && prettier --write && vite build

# rust (workspace under crates/)
cargo build
cargo test
cargo clippy --workspace --all-targets -- -D warnings
```

## Project Structure

```
torajs/
├── web/             ← torajs.com website (React + GDS + Tailwind + Vite)
├── crates/          ← Rust workspace — engine, AOT compiler, CLI (`tr`), bindings
├── labs/            ← experiments, language-agnostic, throwaway-friendly
├── examples/        ← end-to-end demos and integration test fixtures
├── docs/            ← project documentation (incl. canonical roadmap)
├── .claude/         ← Claude Code rules (synced from devops/dotclaude, gitignored)
└── CLAUDE.md        ← this file
```

`labs/` is intentionally unconstrained — anything goes; promote to `crates/` or delete when done. `examples/` is for runnable demos, not unit tests (those live next to source).

## Plan

**`docs/roadmap.md`** is the canonical implementation plan. Phases P0 → P13 over 18-36 months. P0 = walking skeleton (`tr run hello.ts` prints `hello`); P2 end = ownership-correct interpreter (graduation point from `labs/` to `crates/`); P3 = AOT to wasm; P10 = playground on torajs.com.

The discussion logs that produced the roadmap live in `.claude/researches/0001-...` through `0005-...`, kept as audit trail. Read the roadmap for what to do; read researches for why.

## Core Principles

Closed-source research-and-learning project. Most code is exploratory and will be deleted; the rate of abandoned ideas is high and that's intentional. Working mode:

- New ideas → `labs/` first. Don't pre-design crate names, APIs, or directory layouts before there's something concrete.
- One question per experiment. Build the smallest thing that answers it; resist scaffolding ahead.
- No tests/CI/docs/refactor pressure on `labs/` code. Those come when something graduates to `crates/`.
- Be willing to delete more than is kept.
- Shared rules in `.claude/rules/` apply to production-bound code (`crates/`, `web/`); `labs/` may relax them.

## Conventions

- See `.claude/rules/common/` for shared coding standards (every project)
- See `.claude/rules/rust/` and `.claude/rules/typescript/` for language-specific rules
- Project-specific rules go in `.claude/rules/` at this project's own path — not in dotclaude
- `.claude/` is gitignored (per dotclaude policy for sibling projects); each developer runs `devops dotclaude sync torajs` to populate it locally

## Branches

- `main` — production
- `develop` — active development (current)
- Uses git-flow branching model (see `.claude/rules/common/git-workflow.md`)

## Deployment

Target: Caddy on **t01**, served from `/apps/torajs/...`.

```bash
# web — production build, then rsync dist/ to t01
cd web && bun run build
rsync -az --delete dist/ t01:/apps/torajs/web/
```

Layout under `/apps/torajs/` on t01 (TBD — finalize once Rust API exists):

```
/apps/torajs/
├── web/              ← static dist/ from web/
└── api/              ← Rust binary + assets (TBD)
```

Caddy site config lives in CaddyStore; redeploy via `devops caddy deploy t01` after edits.
