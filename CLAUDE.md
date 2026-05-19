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

## Planning Architecture (HARD RULE) — 4 层信息架构

任何时候在做 / 在想的所有"要做的事"，强制归到下面 4 层之一。**层错了 = 信息要么过期、要么把决策反推回 takagi、要么 hot 计划被冷计划的 noise 污染**。

| Layer | 是什么 | 写多细 | 多久换一次 | 我读它要做什么 |
|------|------|------|----------|----------------|
| **L1 — Roadmap** | "从现在到终态走哪条路" | 一段话讲清楚去哪 + 为什么 | 项目级（半年到几年） | 知道大方向。**不是**用来执行的。 |
| **L2 — Version 边界** | 当前版本（v1 / v2 / ...）含哪些工作单元 | 一行 scope + 工作单元列表 | 一个版本（数周到数月） | 判断"我在做的事是不是 v1 内"。**定下就不动**——超 scope 的事自动归 L3b 不做。 |
| **L3a — Hot 计划** | 现在到**下一个 checkpoint** 的执行步骤 | **细到 step + 每步的验收命令**；线性、无分叉、TDD-designed | 一个 checkpoint（一次工作 sprint） | 顶部 take 一项就开干。 |
| **L3b — Cold 计划** | v1 内剩下的、还没 hotify 的部分 | "做什么 / 验收标准 / 大致依赖"，**不写命令** | 一个版本周期 | 知道"今后还有什么"。读它**不是为了执行**，是为了 trigger 命中时知道该 hotify 谁。 |
| **L4 — Trigger** | 把 L3b 顶部一项 hotify 成新 L3a 的判定条件 | 一句状态判断式（"X 指标 ≥ Y" 或 "Z 完成" 之类） | 版本周期常驻 | 每次 ship 完一个 step，按这个查"该不该换 hot 计划" | 

### 我作为 agent 实际怎么用

**读 status memory 的第一动作**：扫一遍，每个段落都问"这是哪层？" 看到下面任一信号 = 当场停下重组：

- L3a 里出现"候选 A / B / C"或"做 X 还是 Y" → 这是 L3b 的写法漏到 L3a 了。**hotify 的时候就要决断**。
- L3a 步骤写到 grep / sed / 具体 commit 命令 → 那是执行 trace，不是计划。L3a 写"步骤名 + 验收命令"，不写实现命令。
- L3b 里写 step 级别细节 → 提前细化，会过时。删掉细节，留"做什么 / 验收 / 依赖"三件。
- L4 是空的或写"等做完再说" → 不是 trigger。trigger 必须是可机器判定的状态（`5k sample ≥ 3.5%`、`P0 acceptance fixture 全 pass` 之类）。

**ship 完一项后的固定流程**：

1. 把 L3a 顶部那项划掉（已 done）。
2. 检查 L4 trigger 是否命中。
3. **命中** → L3b 顶部一项 hotify：补 step、补每步验收命令、消掉所有"看情况"。完成后 L3a 接续。
4. **未命中** → 继续 L3a 下一项。
5. 中途发现新 follow-up → 写到 L3b 末尾，不 hotify。

**写 L3a 的硬性自检**：每一步都有可机器判定的 acceptance（`cargo test X` / `tr run fixture && diff bun_out` / `conformance 不回归` 之类）。如果想不到 acceptance，说明这步没设计完——回去 design，不要开始执行。

### 反模式（绝对不做）

- ❌ 把 L3b 的 "需要再决定" 项目直接搬到 L3a → 等于让 takagi 帮我决定执行细节
- ❌ L3a 写成自然语言段落 → 应该是 numbered steps + 每步 acceptance
- ❌ L4 trigger 写成 "感觉差不多了就升" → 必须是 state predicate
- ❌ 完成一个 checkpoint 不动 status memory，凭记忆接下一项 → 这是 status memory 存在的反面

### torajs 当前的具体映射

| Layer | 在哪 |
|------|------|
| L1 | `docs/roadmap.md`（v5 三轴 trunk · P0 → P15） |
| L2 | roadmap 内的 phase 节（每个 phase = 一个版本） |
| L3a | `memory/project_status_<date>.md` → "Next up" 节（numbered, with acceptance） |
| L3b | 同上 → "Watch list" / "Backlog" 节 |
| L4 | 同上 → 顶部"Trigger to next phase"节 |

每次 commit 后**先动 status memory**（按上面流程），再做下一步。memory 是计划的真源，commit log 只是历史。

## Disk Hygiene (HARD RULE) — 每个产生临时副产品的工具/runner 必须就近清理

**任何工具产生的临时文件/缓存/中间产物，必须在生成它的同一边界内被清理掉**。失控样例：bun build --compile 在 cwd 产 `.HEX-NNNN.bun-build` 缓存，bench-harness 跑了一个完整会话后留下 **11,724 个文件 / 688 GB**，硬盘塞满，takagi 手动清理。

### 规则

1. **新增 runner / 新增 build-step 必须自带 cleanup**。同一文件 / 同一函数生成产物 + 同一函数清理。不允许"先 ship 跑 1000 次再回头补 cleanup"——压满硬盘是不可逆的代价。
2. **既有 runner 发现产生新副产品 → 立刻补 cleanup commit**，不留到下次。
3. **`.gitignore` 不是 cleanup**——它只防止入库，不防止占盘。两条都要做。
4. **临时文件命名要 grep-able**——`.bun-build` / `__torajs_test_*` / `bench-cache-*` 之类的可枚举模式，让 cleanup glob 可写。
5. **Runner 退出前的 finally 模式**——任何 panic / 中断后 cleanup 也要跑。Rust 用 `Drop` impl + scopeguard 之类。

### 常见副产品来源（按 5-pillar "规范" 检查表）

- `bun build --compile` → 写 `.<HEX>-<NUM>.bun-build` 到 cwd。**bench-harness 已有 cleanup**（commit X，bench/harness/src/bench.rs）。
- `cargo build` → `target/` 目录（已 gitignored，但占盘也很大；by `cargo clean -p` 按需清）。
- 临时 .ts / 输出 binary → tora 的 conformance/test262 runner 用 `$TMPDIR/torajs-*-<pid>-<n>.ts` 自带清理；新增 runner 沿用此模式。
- llvm 中间 IR / .bc / .o → 现走 `mktemp` 再 `remove_file` 模式（runtime_str 的 cc 调用），保持。

### 触发清理 audit 的信号

- 任何"目录里东西好像越来越多" → 立刻 `du -sh` + `find . -name 'pattern*' | wc -l` audit
- 任何 takagi 报告硬盘问题 → 视为 P0，立刻清理 + 加规则 + commit

### 我（agent）每个会话末尾必须跑的清理

每次结束工作前（commit 前/loop 停 前/notify 前），跑下面 audit 一遍：

```bash
# 1. 没用的 target 子目录（debug / cross-arch 当我只用 release）
du -sh target/debug target/aarch64-apple-darwin 2>/dev/null
# 若存在且 > 1 GB → rm -rf
# 工作流默认只用 cargo build --release；debug/incremental cache 是浪费

# 2. bun-build 缓存（bench-harness 已自动清理，但兜底 audit）
find . -maxdepth 3 -name '*.bun-build' 2>/dev/null | wc -l
# 若 > 0 → find -delete

# 3. /tmp 下的 t262 / 5k-sample dump（每次重新生成，旧的没价值）
ls -la /tmp/full-dump*.txt /tmp/conf-full.log /tmp/t262-*.log 2>/dev/null
# 每次新会话开始不需要旧 dump；> 5 个时清掉前几个

# 4. /tmp 临时 fixture .ts（我用过的 /tmp/aa*.ts / /tmp/p*.ts / 等等）
# CRITICAL: macOS /tmp 是 symlink 到 /private/tmp，find -delete 必须用真实路径，
# 否则跨 symlink 删除被拒绝且不报错。
find /private/tmp -maxdepth 1 -name '*.ts' -delete 2>/dev/null
# 这些 fixture 每次 session 重新写，旧的没价值（4 KB 一个，跑几百次 session
# 累积 5+ MB；不大但是 1000+ 文件污染）
```

不照办的代价：takagi 硬盘塞满，loop 中断，**信任损耗远大于任何 ROI 收益**。这是 5-pillar "规范" 的硬执行表现。

### 自我执行触发器（agent 必须主动）

- 每次 `cargo build --release` 后顺手 `du -sh target/`，超 5 GB 立刻 audit
- 每次大规模跑 t262 / conformance 后顺手清 `/tmp/full-dump*` `/tmp/t262-*.log`
- 每次 ship 完一个 commit + 准备 stop loop / push notification 前跑上面 audit checklist

不要把"我没看到问题"当成"没问题"——硬盘填满是渐进的，发现就晚了。

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
├── .dev/            ← R&D-environment optimization theme (perf + auto-cleanup, quality-preserving; persistent & evolving)
├── .claude/         ← Claude Code rules (synced from devops/dotclaude, gitignored)
└── CLAUDE.md        ← this file
```

`labs/` is intentionally unconstrained — anything goes; promote to `crates/` or delete when done. `examples/` is for runnable demos, not unit tests (those live next to source).

**`.dev/` (read `.dev/README.md` before any dev-environment / build-speed / disk-cleanup work)** — the standing R&D-environment optimization theme. First hard rule: no optimization may reduce verification coverage or correctness. Contains `ENVIRONMENT.md` (build/cache ground truth + corrected gotchas), `OPTIMIZATION.md` (quality-neutral perf backlog, prioritized), `clean.sh` (safe dry-run-default stale-file cleaner). It is committed and evolving — extend it, don't bypass it, when doing dev-env work.

## Plan

**`docs/roadmap.md`** is the canonical implementation plan — v5 三轴 trunk (rewritten 2026-05-17). Phases P0 → P13 to v1.0 + P14 / P15 post-v1.0. Each phase has a substrate-checklist acceptance gate (concrete spec sections + ssa-lower paths + runtime helpers landed) — NOT a test262 pass-rate %. Pass rate is regression-detection diagnostic only.

Current state: P0 / P1 / P2 / P3 closed; P4 (Class hierarchies + prototype chain) in progress (Phase A1 shipped at `a65e51f`).

Historical trunks (v1 P0-P13 foundation, v2 perf-gated, v3 wedge cycle, v4 test262-100%) are preserved in `docs/roadmap-historical.md`. Read the active roadmap for what to do; read history for why.

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
