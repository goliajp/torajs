# torajs

Closed-source internal language project. Goal: a TypeScript runtime that runs the same TS programs `bun` does, with the same TS semantics, differentiated on **AOT to native binary + small artifact + fast startup**. Reference baseline is `bun` — anything bun runs, tr must eventually run; "not implemented" is always a roadmap phase, never an out-of-scope decision. Foreign idioms (Rust ownership / borrow checking / RAII, etc.) are not imported into the language surface unless takagi explicitly authorizes them. Two execution modes: **AOT to native binary** (`tr build`) and **JIT-style compile-and-run** (`tr run`), the Go shape. Implementation language is Rust. Public site: https://torajs.com

## Communication (HARD RULE)

**始终使用中文（简体中文）与 takagi 沟通。** 所有面向用户的回复、状态更新、解释、提问都必须是中文。仅以下场景使用英文：代码、变量名、git commit 消息、CLI 输出原文、PR / issue 正文（若仓库语言为英文）。混用其他语言（韩文、日文、法文等）严格禁止。这条规则强化 `.claude/rules/common/language.md`，违反视为重大错误。

## Working Mode (HARD RULE) — 顺序执行计划，不开候选清单

**项目永远只有一份"顺序执行计划"，由 status memory 维护，按顺序往下做。** Brief 给 takagi 时**严禁**列"下一步候选 1/2/3"或"按 leverage 推荐 A/B/C"这种 framing —— 那是把计划决策再丢回给 takagi，等于反复让他做同一件事。这条强化 `feedback_drive_dont_ask` 和 `feedback_no_fork_questions`：

- **Ship 完一个里程碑后，自动推下一项**。从 `project_status_*.md` 的执行计划列表 take 顺位第一项就开始做，不汇报"我要做 X 还是 Y"。
- **Brief 内容只有：已 ship 的 commit 总结 + 当前正在做的事 + 已观察到的事实**。不列"候选"，不问"要不要继续"。
- **计划调整由 takagi 主动提出**："改方向"、"插入 X"、"先做 Y" 才是 takagi 干预 plan 的入口。我自己的工作只是按 plan 推进 + 在 plan 末尾追加新发现的 follow-up 项（如 substrate 阻塞导致需要新 phase）。
- **顺序由依赖决定**：substrate 阻塞 / 必经路径 在前，平行 / 锦上添花在后。我维护这个顺序，不让 takagi 选。

违反这条 = 把决策成本反推给 takagi = 与 auto loop dev 的全部前提冲突。

## Anti-Hallucination (NON-NEGOTIABLE)

Follow `.claude/rules/common/anti-hallucination.md` — always. Five rules, zero exceptions: say "I don't know", use tools before memory, no chain-guessing, retract mid-sentence when wrong, cite the source. Tool output itself must never be fabricated: if a tool returns only `[rerun: bN]` or empty content, report that literally and rerun — never invent plausible-looking output.

## Design Principles (HARD RULES)

Every architectural decision, runtime / compiler addition, performance trade-off, and API design must satisfy all five pillars: **高性能 / 自研 / 正统 / 规范 / 上限优先**. See `docs/design-principles.md` for the full rubric and how to apply it. Any solution that fails one pillar — even as a temporary or MVP — is not acceptable.

**上限优先** is the tiebreaker: when multiple paths satisfy the first four pillars, always pick the one with the highest ceiling and most future runway. Ceiling is measured by standard metrics (run_ms, build_ms, artifact size, future extensibility). Never pick "easier short-term" or "70% solution" over "harder but uncapped" — short-term shortcuts become tomorrow's architectural debt.

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
