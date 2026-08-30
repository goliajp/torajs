# torajs roadmap

> **v5 — 多轴并行 trunk.**（立时三轴；轴 D 2026-06-08、轴 E
> 2026-08-30 追加，现为五轴 —— 见 Foundation。）Rewritten 2026-05-17 (HEAD `a65e51f`, curated
> conformance 590/0/1, 5k diagnostic 152/15/2975). Supersedes v4
> (test262-100% trunk, 2026-05-14). The v4 trunk treated test262
> in-scope pass rate as the per-phase acceptance metric; v5 replaces
> that with **substrate-checklist acceptance** (concrete spec sections
> worked, runtime + ssa-lower paths landed). Pass rate stays as a
> diagnostic / regression detector — never a milestone.
>
> Prior trunks (v1 P0-P13 foundation, v2 33-item perf-gated, v3 V3-XX
> wedge cycle, v4 test262-100%) are preserved verbatim in
> `docs/roadmap-historical.md`. Read them for *why* tora's foundation
> looks the way it does, not for *what to do next*.
>
> **HARD RULE for execution.** This file is the only forward-looking
> plan. Phases run strictly in order. Within a phase, items run
> strictly in order. No "candidate A vs B" branching at execution
> time — the order is the decision. Stop and discuss only on (a)
> genuine forks not in this doc, (b) irreversible decisions, (c)
> ambiguous-recovery failures.
>
> Living document — append observations / sub-items as they surface,
> but never reorder shipped items.

---

## Foundation

### Goal — five axes

torajs 是 AOT 编译型 TypeScript runtime，差异化是 native binary + 小
artifact + fast startup。Long-arc 终态由五轴定义，**五轴并行推进，不接受
为某一轴妥协另一轴**。每 phase 同时推，任一轴失败 = phase 不收口。

（标题此前写着 "three axes" 而正文已列四轴 —— 轴 D 2026-06-08 追加时
没改标题。2026-08-30 轴 E 追加时一并订正。）

**轴 A — spec completeness（正统）**

终态：test262 全量 100% pass over in-scope（不是 5k sample，不是 90%
gate）。每 phase 的 acceptance 用 **spec-section checklist**（具体 spec
章节的硬事实，"§7.1.3 ToNumber via valueOf works on Struct" 这种粒度）
验收。pass rate 数字是 diagnostic / regression detector，不作 milestone。

**轴 B — performance ceiling（高性能 + 上限优先）**

终态：在 bench-tr 套件 cross-runtime 对比上 SOTA — 每个 case 严格优于
bun-aot / bun-jsc / nodejs / go / rust 各自对位。perf push 跟 spec 推进
**并行**（不是 v1.0 完了再优化）：每 phase 内部既推 spec 又拉 bench。
**bench-tr 0 regression 是每 commit 的硬阈值**；每 N phase 做一次
perf-focused push 拉新 case 进 SOTA 范围。

**SOTA 的口径（2026-08-21 收紧，见 `P-PERF`）**：总墙钟领先**不算**
SOTA，因为它会被我们的固定启动成本红利掩盖。判据是
**work-only = `run_ms − startup_ms`**（`startup` case 就是纯固定成本）
的对位领先，**外加** `startup` 与 `artifact_bytes` 两条独立指标各自
对位领先。2026-08-21 实测：总口径领先 39/44，**work-only 口径 17/41
是输的**——差别全在这里。

**轴 C — implementation purity（自研）**

终态：runtime + 编译器内核全自研。不嵌入 V8 / JSC / QuickJS。允许的
外部依赖：

- build-time 工具：**（2026-08-21 更新：inkwell / LLVM 已随 `eded11f`
  退役，现在是自研 aarch64 后端 `torajs-codegen` 10,150 行 + 自研
  e-graph `torajs-egraph` 22,923 行 + 线性扫描寄存器分配 + 自研 Mach-O
  链接器 `torajs-link`。本条白名单目前无在用项。）** 若将来重新引入，
  last-stable 且 pin 到具体 minor
- runtime-side 系统接口：libc 唯一
- Rust host crates（serde / tokio / 等）仅用在 host 编译期，不进
  runtime binary
- **每 phase ship 时不允许引入新非高品质依赖**。"我能找到一个 crate 做
  这事" 不是引入它的理由 — 必须 audit (a) crate 质量 (b) 是否能自研
  替代 (c) last-stable 锁版本。

**轴 D — multi-thread-ready substrate**（2026-06-08 追加；ceiling-first
multi-thread vision，详见 `.claude/vision.md` 三-1 节 + `rules/torajs-
design-principles.md` 第六条 HARD RULE）

终态：v1.0 ship 后立 P14+ multi-thread implementation trunk，落地
biased ARC（owner-thread fast path 0 atomic 增量 + 跨线程 share
transition + atomic 慢路径，CPython 3.13 PEP 703 + Lee PACT 2018 路径），
single thread cost 对齐 Rust native thread（几十 KB stack + 极小 TLS），
无 GC（永远 RC + Bacon-Rajan cycle collector 并发版，不引入 mark-sweep /
stop-the-world）。**ceiling 目标比 bun "<2MB / thread + 1 GC" 高得多**。

**v1.0 期间（现在 ~ P13）该轴的接受形态**：substrate 按 multi-thread-
ready shape 长，**真切换不做**。每 phase 接受时除其余四轴外加审：

- refcount inc/dec emit 必走 `emit_rc_inc(op)` / `emit_rc_dec(op)` helper，
  禁止直接 emit `InstKind::Call(intrinsics.rc_inc, ...)` 等原始 site
- 新加 global mutable state 默认 `thread_local!` 或 `Atomic*`，不
  `lazy_static<Mutex<T>>` / `static mut` 等 single-mutator 形态
- heap header layout 不固化 single-mutator 假设（owner_thread_id 字段位预留）
- helper / FFI 签名不假设 single-mutator

**该轴 0 增量 perf 代价**（helper 抽象今天等价 raw call；real biased ARC
切换在 v1.0 后），但**早期 framing 不落 = 后期 50-100 处 site-by-site
retrofit**。这是 ceiling-first early framing 实践 — vision 现在落，实施推迟。

**轴 E — platform reach（全平台，2026-08-30 takagi 立）**

takagi：「全平台也必须拉入 v1」。

终态：v1.0 在**五个 ISA × 格式 × ABI 组合**上产出可运行的 native
binary，不是单一 `aarch64-apple-darwin`。差异化叙事本身要求这一条 ——
「AOT 出 native 小产物」如果只在一种机器上成立，那它不是一个 runtime
的属性，是一次移植的属性。

**当前面（2026-08-30 实测）**：三个维度各自只走了一格。

| 维度 | 已有 | 缺 |
|---|---|---|
| ISA | AArch64 | x86-64 |
| 对象 / 可执行格式 | Mach-O | ELF64、PE-COFF |
| 系统调用与 ABI | XNU BSD | Linux、Windows |

**绑定面实测**（行数为 `wc -l`，命中为 grep）：

- `torajs-codegen` **10,995 行** — `enc/*` **1,257 行**是纯 AArch64 指令
  编码（ISA 专有，x86-64 变长编码需全新写）；`compile/*` **4,930 行**
  是指令选择（ISA 专有）；`reg` / `regalloc` / `linear_scan*` /
  `liveness` / `spill_weight` / `frame` **4,096 行**是寄存器分配与帧布局
  —— **算法可复用**，需把寄存器集与调用约定参数化。
- `torajs-obj` **2,464 行** — `macho/*` **1,285 行**格式专有；
  `object.rs` 1,013 行是中间层。
- `torajs-link` **25,536 行 / 78 文件** — **56 个文件命中 Mach-O 专有
  符号**（`LC_*` / `MH_*` / `__TEXT` / `dyld` / chained fixups）。
  **这是最重的一面**，也是唯一一处需要先做格式抽象才能动的地方。
- `torajs-syscall` **1,066 行** — 好消息：`arch_aarch64_macos.rs` 已经把
  XNU 的 carry-flag 错误约定**归一化成 Linux 风格**（`raw < 0 → -errno`），
  所以 `safe.rs` 那层是**格式无关的**。换平台只换 trampoline 与号表。

**顺序 — 每步只动一个维度**，这样任何回归都能归因到那一个变量：

1. **E1 `aarch64-unknown-linux-gnu`** — ISA 不变，只换格式（ELF64）与
   syscall（`svc #0`，号走 `x8`）。这一步真正做的是**把格式抽象做对**，
   后面每一步都吃它的红利。
2. **E2 `x86_64-unknown-linux-gnu`** — 只加 ISA（x86-64 变长编码 +
   SysV AMD64 调用约定）。格式已由 E1 抽象。
3. **E3 `x86_64-apple-darwin`** — 格式与 syscall 都已有，复用 E2 的 ISA。
   这一步的成本应该接近零；**它不接近零就说明 E1/E2 的抽象没做对**，
   是一条免费的架构自检。
4. **E4 `x86_64-pc-windows-msvc`** — PE-COFF + Microsoft x64 ABI +
   **没有稳定 syscall 面**（必须走 ntdll / kernel32 的 import table）。
   三者全新，最重的一步。
5. **E5 `aarch64-pc-windows-msvc`** — ISA 已有、ABI 与 E4 共用。

**v1.0 gate 取 E1–E4**（五组合中的四个，覆盖 macOS / Linux 双 ISA +
Windows x64）。E5 post-v1.0。按第五设计原则，这里没有「先做一个看看」
的中间态可选 —— 「全平台」就是全平台，取哪一步不是可议价的 scope。

**与其它轴的关系**：轴 E 与轴 B 有真冲突风险 —— 为跨平台做的抽象
（trait object / 间接分派）可能吃掉 codegen 的热路径收益。**冲突时按
下方优先级：轴 B 让位于轴 A，但不让位于轴 E** —— 平台抽象必须是
**编译期单态化**的（泛型 + `cfg`），不是运行期分派。这条是硬约束，
不是偏好：一个为了可移植而变慢的后端，违反第一设计原则。

---

五轴的硬冲突时：质量优先（轴 A 正确性）> 性能优先（轴 B）> 平台覆盖
（轴 E）> 自研优先（轴 C）> multi-thread-ready 形态（轴 D framing 优先级
最低，因 framing 本身 0 代价；真冲突极罕见，通常同向）。**轴 E 排在轴 B
之后是刻意的**：跨平台不得以热路径变慢为代价换取，见上。

### Hard requirements (kept from v1)

1. **极致 perf** — beat bun/node on important benchmarks; hold them.
2. **Compile not too slow** — first `tr run foo.ts` pays one full
   compile through the in-house pipeline (lower → e-graph → codegen →
   link); subsequent runs hit the cache. （2026-08-21：原文写的
   "one full LLVM compile (~50–90 ms)" 已陈旧，LLVM/inkwell 随
   `eded11f` 退役；当前数字以 bench 的 `compile_ms` 列为准。）
3. **Interpretable** — `tr run foo.ts` is the dev-loop entry point.
4. **No GC, internal ARC** for shared-heap values via a universal heap
   header. Single-owner uses compile-time ownership inference. **强化
   (2026-06-08)**：永远不引入 GC（mark-sweep / generational / concurrent /
   stop-the-world / GC pause），对标 Rust `Arc<T>` + `Weak<T>` + 自动 cycle
   collection。RC 多线程下走 biased ARC（详见轴 D）。
5. **TS-shape semantics** — what works, works the same as bun. No
   Rust-flavoured idioms in user code.
6. **Full TS coverage as a roadmap target** — every TS feature bun
   supports has a roadmap phase. Compile errors point at the phase.
   **（2026-08-21 重述）**"anything bun runs" 是**方向，不是一份可穷尽
   的清单**：bun 1.4 一次新增 8 个内建（`Bun.Image` / `WebView` /
   `markdown` / `cron` / `Terminal` / `JSON5` / `XML` / `Archive`）并
   把 Node 兼容推到 26.3.0（+1,517 条新过测试）。**可收敛的分母只有
   test262 in-scope**（P-SURF 的 gate predicate）；bun 专有 API 面按
   需求驱动进 Backlog，不进 v1.0 gate。
7. **test262 in-scope 100%** as the v1.0 stretch target — gated by
   substrate completeness, not by pass-rate %.
8. **比 bun 上限高得多的真多线程能力**（2026-06-08 追加）— 1 shared
   heap + no GC + 任意 object cross-thread + thread cost 对齐 Rust native
   thread + biased ARC 保 single-thread 0 regress。v1.0 期间走轴 D
   framing，真实施 P14+ trunk。

### Two-tier execution model (introduced 2026-05-14, kept)

The single biggest insight from the wedge cycle (v3, V3-XX → 522/521
curated, 4 % test262 in-scope): **test262 is 100 % JS source with no
type annotations, and tora's strict typecheck rejects most of it at
the first `var x = "anything"`.** Continuing to ship per-method wedges
plateaus against curated; it cannot move test262.

The v4 trunk fixed this with a two-tier model; v5 keeps it.

| Tier | When it applies | Layout / lower | Perf cost |
|---|---|---|---:|
| **typed-tier** | Source has explicit annotations (`x: number`) or the inference is concrete | Static layout, monomorphic ops, existing tora pipeline | 0 % regression |
| **untyped-tier** | Source is bare JS (`var x = expr`) where inference can only conclude `any` | 16-byte tagged-value slot `{tag: u8, payload: u64}`, runtime dispatch on tag | tagged-dispatch overhead, still AOT — no JIT, no interpreter |

**Performance-first invariant**: typed-tier code MUST NOT regress when
the untyped-tier lands. Every existing bench case stays in typed-tier
(the inference returns concrete types from `: T` annotations and from
unification). Adding untyped-tier is purely additive.

**Architecture-clean invariant**: Type::Any is a first-class type at
every layer (parser AST, check, ssa-lower, codegen). It is not a patch
on `__nullable` or a special case in BinOp — every op gets an Any-aware
arm. This makes the untyped-tier a clean parallel rail, not an escape
hatch.

---

## Status snapshot (2026-05-18, HEAD `aac1934` — P6 phase closed)

> ⚠️ **HISTORICAL — every number below is from 2026-05-18 and is wrong
> now.** Kept as a one-shot mark, per the Checkpoints section that
> follows. For current ground truth: conformance gate and sweep numbers
> live in `.claude/plan-state.md`, the test262 census lives in the
> **P-SURF** section of this file, and bench lives in
> `bench/results/*.json`. For scale of the drift: curated conformance
> 618 → **2070**, test262 (5k sample then, full 53174 corpus now)
> 344 pass → **13762**.

### Curated conformance (`conformance/cases/`)

**618 pass / 0 fail / 1 skip** committed. +11 from P6 phase (map-001
..005 / map-for-of-001 / set-001..004 / array-iter-001). The 1
committed skip is `perf-005-dwarf-panic-fs` (bun-side crash, not
tora's bug).

### test262 5k diagnostic

**344 pass / 16 bug / 3279 incompatible** at last measured baseline
(`00c4d12`). Re-measurement post-P6 deferred (Map/Set unlock expected
to surface +N test262 cases — most Map / Set / iterator-protocol
fixtures previously hit `typecheck reject` due to substrate gap).
Pass rate is regression-detection only — not a phase trigger or
milestone.

### Bench position

Typed-tier 0 regression invariant holds across P6 substrate (binary
artifact_bytes essentially unchanged through Map/Set/MapIter/ArrIter
additions — these add new code paths, don't modify Array / Closure /
Str / Number hot paths). Multi-run median bench verification on
idle-system window pending; single-run measurements during P6 ship
were noise-dominated (mac thermal ±20-40% with concurrent godot /
rustc / node tsc background load).

Last committed bench baseline: `bench/results/2026-05-18-mini-2004980
.json` (torajs vs bun-aot geomean **4.02×** at HEAD `00c4d12`).

### Code size

Three crates (`crates/torajs-{runtime,core,cli}/`), single SSA → LLVM
pipeline, no JIT, no interpreter.

---

## Checkpoints (project-state snapshots)

> **Why**: the `## Status snapshot` block above is a one-shot historical
> mark from 2026-05-18; it does not stay current. Long-running trunks
> (P10 Promise / P11 Unicode / multi-chunk substrate trunks like W-J /
> fn-name registry) need periodic ground-truth snapshots so trend
> direction is visible at a glance — not buried in 200+ commit history
> + 1500-line plan-state.
>
> Checkpoints are NOT inline in this file. They live in
> [`docs/checkpoints/`](checkpoints/) as one self-contained MD per
> checkpoint, plus a `README.md` trend table that stacks every
> checkpoint's key metrics into columns for mechanical diff.

### Trigger conditions

A checkpoint is produced when ANY of these fires (mechanical commit
count is **not** a trigger — substrate shape matters, not volume):

1. **roadmap phase transition** — a `## Pn` section moves CURRENT → DONE
   (e.g. P9 → P10, P10 → P11). Capture before opening the next phase.
2. **substrate trunk close** — a multi-chunk trunk in
   `.claude/plan-state.md` L3a finishes all chunks (W-J A0→D close,
   Phase 2 fn-name close, RegExp UCD trunk close, etc.).
3. **roadmap framing change** — takagi reframes a phase scope / order /
   acceptance criterion (a v5 → v6 rewrite, axis insertion, etc.).
4. **takagi explicit ask** — "做个 checkpoint" / "全面汇报" / `/checkpoint`.

### Fixed 4-section template

Each checkpoint MUST have these 4 sections in order, with the same
data shape — so cross-checkpoint diffs are mechanical:

1. **整体进度 / 当前位置** — HEAD short hash, branch, roadmap phase,
   active L3a hot trunk (chunk N of M), this-period ship summary +
   last 5 commits.
2. **Metal 化 / 全自研** — workspace crate count, metal-core ext dep
   audit (target 0), grandfathered scope (cli + cloud-api only), C
   runtime status (`find crates -name '*.c'` count).
3. **test262** — in-scope passRate (passTotal / 53174), top blocking
   bucket + size, next-trunk plan reference, in-house conformance
   gate baseline `N / F / S`.
4. **Benchmark** — bench JSON path + git sha + host + timestamp,
   representative 10-15 case TR vs BUN-AOT run_ms table, artifact
   size (tr vs bun-aot vs rust), known regression follow-up.

Every number must be sourced (file:line, command output, or jq query)
per `.claude/rules/common/anti-hallucination.md` — no recall, no
estimate.

### Most recent

- 📄 [`2026-06-14-w-j-a3a.md`](checkpoints/2026-06-14-w-j-a3a.md) — W-J
  substrate trunk Phase A3a close (4/9 chunk). First checkpoint after
  the protocol was established; baseline for future diffs.

See [`docs/checkpoints/README.md`](checkpoints/README.md) for the full
trend table and the "how to add" instructions.

---

## Trunk

The trunk is **P0 → P13 → P-SURF (v1.0 gate) + P14 / P15 / P16
post-v1.0**, executed in strict order. Phase order is fixed by substrate
dependency — earlier phases unlock later phases' work.

**P0–P13 are all closed** (2026-07-26; P5.4 was the last box and had
been working since P6). The live phase is **P-SURF**, which sits between
P13 and the v1.0 gate and is derived from a test262 cluster census
rather than from design intent — see its section for why the trunk
needed a phase that measurement, not planning, produced.

**并行的 cross-cutting track（不在 trunk 顺序里，但同样是 live）**：
**`P-PERF`**（2026-08-21 立，轴 B 的当前执行面；见文件末尾该节）。
轴 A 的 P-SURF 与轴 B 的 P-PERF 并行推进，按 `plan-state.md` 的 L3a
顺序取顶项。

**Per-phase acceptance has three parts (all required):**

1. **Substrate checklist** — concrete spec sections / ssa-lower paths /
   runtime helpers landed. Phase-specific, listed below.
2. **Bench gate** — bench-tr cross-runtime suite shows 0 regression
   vs phase-start baseline，**三项口径同时看：work-only（`run_ms −
   startup_ms`）/ `startup` / `artifact_bytes`**（2026-08-21 起，见
   `P-PERF`）。Untyped-tier additions don't gate; they are correctness
   work.
3. **自研 audit** — no new external dependencies introduced beyond the
   foundation set (libc；LLVM / inkwell / cranelift 已随 `eded11f`
   退役，当前无在用项)。Any addition requires explicit justification +
   last-stable pinning.

Phase budgets are rough (1 item ≈ 1–3 days, 1 phase ≈ weeks). Planning
estimates only; the **substrate checklist is the contract**.

---

### P0 — Untyped-JS surface (DONE substantial)

**Goal**: tr accepts arbitrary unannotated `.js` source through
typecheck. Type::Any is a first-class participant in every operation.

**Substrate checklist** (closed):

- [x] **P0.1** Type::Any tagged-value SSA representation (16-byte
      `{tag: u8 + 7 pad, payload: u64}`)
- [x] **P0.2** Implicit `any` for unannotated bindings
- [x] **P0.3** Any-aware BinOp / UnaryOp / Compare (`+` / `-` / `*` /
      `/` / `%` / `**` / `===` / `==` / `<` / etc.)
- [x] **P0.4** Member / Index access on Any (placeholder Any →
      typed-shape bridge; full property-bag in P3)
- [x] **P0.5** Call on Any (Closure / FnSig tag dispatch)
- [x] **P0.6** typeof on Any (spec §13.5.3)
- [x] **P0.7** ToBoolean / ToNumber / ToString on Any (spec §7.1.2 /
      §7.1.4 / §7.1.17)
- [x] **P0.8** test262 runner --bucket-by-tier flag + per-incompat
      reason tracking

**Bench**: typed-tier 0 regression ✅
**自研**: 无新依赖 ✅

---

### P1 — undefined as a real value (DONE)

**Goal**: `undefined` is a distinct value from `null` end-to-end.

**Substrate checklist** (closed):

- [x] **P1.1** Type::Undefined first-class in ssa.rs / check.rs
- [x] **P1.2** Tag value for Undefined slot
- [x] **P1.3** Default parameter missing → undefined
- [x] **P1.4** Array.find / .at / .indexOf OOB → undefined
- [x] **P1.5** typeof undefined → "undefined"
- [x] **P1.7** `Nullable<T>` = `T | null | undefined`
- [x] **P1.8** `undefined === null` → false

**Bench / 自研**: ✅ / ✅

---

### P2 — var/function hoisting (DONE substantial)

**Goal**: ES §14.1.3 hoisting rules. TDZ for `let` / `const` stays.

**Substrate checklist** (closed):

- [x] **P2.1** Two-pass scope analysis + var/function hoist
- [x] **P2.4** for-var leak (`for (var i ...) { ... }; use(i)`)
- [x] Module-top block fn lift (P3.4-followup-A)
- [x] Bare FnDecl-as-stmt (P3.4-followup-A2)

**Bench / 自研**: ✅ / ✅

---

### P3 — Property-bag objects (DONE close, `d9b13c7`)

**Goal**: objects support runtime add / delete / computed keys + full
property descriptor semantics. Static shape inference picks dict-shape
vs struct-shape so existing typed code stays on static layout.

**Substrate checklist** (closed):

- [x] **P3.1** SwissTable dict-shape runtime
- [x] **P3.2** Dynobj inference (struct vs dict)
- [x] **P3.3** defineProperty runtime + spec-shaped descriptor
- [x] **P3.4** Nested fn hoist + module-top block-fn lift
- [x] **P3.5** OptChain Any-tier (`a?.b?.c` 多层链)
- [x] **P3.struct-method-dispatch** — inline `obj.method()` for FnSig +
      Closure field (`1a308f7`)
- [x] **P3.closure-in-struct-field** — narrow Closure ABI via
      `__cls(...)->R` struct-field tagging (`749c1d4`)
- [x] **P3.attribute-flag-tracking** — bucket.tag 高位 packed flag bits
      + `__torajs_dynobj_define` 实施 spec §10.1.6.3 (`dcf069f`)
- [x] **P3.getOwnPropertyDescriptor** — `__torajs_get_property_descriptor`
      一步构造 spec-shaped descriptor Any-box (`d9b13c7`)
- [x] Object.keys / values / entries
- [x] Computed property keys `{ [k]: v }`
- [x] Symbol keys
- [x] Object.freeze (universal heap header flag bit)

**Bench / 自研**: ✅ / ✅

**P3 后续残项**（升 L3a 时按 substrate-correct 标准 ship）：

- T-42 ToNumber via valueOf (§7.1.3) — prerequisite to many spec paths
- P3.5 OptChain 链式 typed-dispatch
- P3.4 nested fn 真实 closure capture
- T-31-followup closure / FnSig 间接调 callee real_argc
- T-45-b `in` operator on Struct / Closure / FnSig / String

---

### P4 — Class hierarchies + prototype chain (DONE)

**Goal**: tora 的 nominal class system 升级到 spec §10.1
OrdinaryObject + §10.4 ExoticObject 的 `[[Prototype]]` / `[[Get]]` /
`[[Set]]` 内部 method 模型。class extends + super() + builtin extends
全部走 prototype chain。

**Substrate checklist** (closed at `fc0e125`):

- [x] **P4.1 Phase A1** First-class class objects (`a65e51f`) —
      `synthesize_class_globals` desugar pass; `const x = MyClass`
      resolves to dynobj-backed Any
- [x] **P4.0** Nested Any-dynobj field identity fix (`94e5773`) —
      Type::Any arm above is_refcounted in 3 match sites
- [x] **P4.2** Phase B+C prototype chain (`e9b6779`) — `__proto_<C>`
      singletons + class-tag side table + Object.getPrototypeOf real
      readback
- [x] **P4.3 extends-chain** (`15e2e9b`) — Object.getPrototypeOf
      borrow semantics fix for Ident args + chain walk
- [x] **P4.4 function-prototype** (`5ec3810`) — `Function.prototype.
      bind / call / apply` via desugar
- [x] **P4.5 new-meta** (`1debabe`) — `new.target` meta-property full
      spec
- [x] **P4.6 extends-builtins** (`fc0e125`) — synth Error ClassDecl +
      class-prefix typeof; **closes P4 phase**
- [x] **P4.7 catch-destructure** (`9b960a8`) — `try {} catch ({code,
      msg}) {}` 真 binding + tagged throw substrate

**Acceptance**: 7 substrate items all 完成 ✅ + conformance 0 fail ✅ +
bench-tr 0 regression ✅ + 无新非高品质外部依赖 ✅。Phase closure
commit `fc0e125` (2026-05-17 → 18 between).

**Design decisions taken in P4**:

- **K.3 globals 不扩 Type::Any** — prototype singleton 选 **runtime
  side table** (class-name 字符串 keyed)，bypass K.3 entirely。Long-
  term most robust + decoupled from K.3 design constraints.
- **Prototype helpers via desugar**: `Function.prototype.bind / call /
  apply` 走 parser desugar + runtime closure wrapping，避免在 Closure
  ABI 上加 reflective overhead.
- **Builtin extends via synth ClassDecl**: `class MyError extends
  Error` 通过在 AST 阶段 synth Error ClassDecl 实施，避免 runtime
  builtin-type erasure.

---

### P5 — Iterator protocol (DONE)

**Goal**: `Symbol.iterator` is a real resolvable property; for-of
dispatches via it; spread-in-call works for arbitrary iterables.

**Substrate checklist** (7 / 7 complete — P5.4 closed late, see below):

- [x] **P5.3 Phase A** First-class `Stmt::ForOf` substrate (`9e38c87`)
      — parse-time desugar 升级成 AST node + Array<T> / Array<Any>
      subset 走 existing `Expr::Index` lowering
- [x] **P5.1** `IteratorResult<T>` structural alias (`56036f7`) —
      `{ value: T, done: boolean }` via `__inlobj`; `Iterator<T>` /
      `IterableIterator<T>` opaque-Any
- [x] **P5.2** Symbol.iterator well-known computed-key (`1aa889b`) —
      class `[Symbol.iterator]()` parses with synth name
      `__sym_Symbol_iterator__`
- [x] **P5.3 Phase B** for-of via `[Symbol.iterator]()` dispatch
      (`1a4fa09`) — Stmt::ForOf dispatches through iterator protocol
      for user-class iterables; Array / Str / Set fast path preserved
- [x] **P5.4** `arr.entries()` / `.keys()` / `.values()` Array
      Iterator objects — was deferred to P6 同期 (blocker: the
      generic-over-T iter class substrate P6 Map/Set surfaces). P6
      shipped that substrate and this came with it, but the box was
      never ticked. Verified 2026-07-26 @ `9215301c`: all three,
      including the `for (const [i, v] of a.entries())` destructuring
      form, are byte-equal with bun. **This was the last open box in
      P0–P13** — see the v1.0 release gate section for why that no
      longer means what it used to
- [x] **P5.5** Spread in fn calls — literal-array spread fold
      (`26310bd`); `f(...[a,b,c])` parser desugars to `f(a,b,c)`;
      dynamic-spread via rest-param already worked; fixed-arity-
      dynamic-spread defer (runtime arity check)
- [x] **P5.6** Spread in array literal (`bdfe417` → `b3afb55`) —
      `__torajs_arr_extend_any` tagged-slot extender, Array<Any>
      spread substrate-complete

**Acceptance**: ✅ all 5 P5.1-P5.6 substrate items closed (P5.4
explicitly deferred to P6 同期 by design) + conformance 0 fail +
bench-tr 0 regression + 无新外部依赖。**5k pass rate movement
during P5 push: 145 (4.62 %) → 344 (9.45 %)** — diagnostic only.

---

### P6 — Map / Set / WeakMap / WeakSet (DONE)

**Goal**: real hash containers, all spec methods, spec-mandated
insertion-order iteration.

**Substrate checklist** (closed at `aac1934`):

- [x] **P6.1** `Map<K, V>` open-addressing robin-hood hash table
      (`7480912` + `f0a33be` TAG audit fix + `86776a6` undef tag fix).
      SameValueZero key equality; tagged-Any keys + values; 8 runtime
      helpers.
- [x] **P6.2** `Set<T>` SSA-level distinction over Map storage
      (`d598ac5`). `add` writes ANY_UNDEF for the value side; method
      dispatch forwards to Map helpers.
- [x] **P6.3** WeakMap / WeakSet audit (`f0a33be`). T-26.B substrate
      verified consistent with P6.1 Map heap-header layout +
      value_drop_heap dispatch (TAG_MAP = 15 collision fix surfaces
      here).
- [x] **P6.4** Spec methods — full surface across 4 sub-commits:
      - P6.4a forEach + V8 OrderedHashMap insertion-order substrate
        refactor (`2004980`). Split-table layout (slots[] robin-hood
        + entries[] packed insertion-order) — spec §23.1.4 / §24.2.4
        ordering preserved.
      - P6.4b MapIter substrate + Map/Set.keys/.values (`c62fe69`).
        `Type::MapIter` first-class refcounted handle; `iter.next()`
        returns `IteratorResult<any>` struct via SSA-side obj_alloc.
      - P6.4c-C1 Map/Set.entries (`73cb278`). ITER_ENTRIES +
        ITER_SET_ENTRIES kinds; per-step `[k, v]` / `[v, v]`
        Array<Any> alloc with refcount=0 pre-dec trick balancing
        any_box's rc_inc.
      - P6.4c-C2 for-of @@iterator dispatch for Map/Set/MapIter
        (`80939ba`). `for (let [k, v] of m)` destructuring works via
        `lower_for_of_map_like` binding var as `Type::Arr<Any>` for
        Map source; Set/MapIter bind as Type::Any.
      - P6.4c-C3 Array<Any> iter methods (`aac1934`). `Type::ArrIter`
        parallel to MapIter; P5.4 (Array iterator methods) unblocked
        for Array<Any> source. Typed Array<T> for non-Any T uses 8B-
        per-slot layout requiring elem-tag substrate — separate
        follow-up.
- [x] **P6.5** Iterator interop with P5 — for-of @@iterator dispatch
      (P6.4c-C2 / C3) integrates with P5.3 Phase B substrate; P5.4
      Array iter methods unblocked. Phase close audit verified: 11
      conformance fixtures (map-001..005 + map-for-of-001 + set-
      001..004 + array-iter-001) all bun-parity GREEN; conformance
      gate 618 pass / 0 fail / 1 skip.

**Acceptance**: ✅ 5/5 substrate items closed + conformance 0 fail +
no new external dependencies. Bench multi-run median verification
deferred to system-idle window (mac thermal / load noise ±20-40%
makes single-run gate unreliable; binary unchanged through P6
substrate path so theoretically 0 regression on hot Array / Closure
/ Str hot paths).

---

### P7 — Error type hierarchy + throw any (DONE)

**Goal**: real Error subtypes (TypeError, RangeError, SyntaxError, …);
`throw` accepts any value; try/catch/finally state machine
spec-conformant.

**Substrate checklist** (strict order):

- [x] **P7.1** Error class + subclass hierarchy in stdlib — SHIPPED
      `f2f5aa0` (inject TypeError/RangeError/SyntaxError/ReferenceError)
- [x] **P7.2** `throw <any value>` — SHIPPED `42d072d`/`d57bbfc`/`e3d5c7d`
      (throw undefined→ANY_UNDEF + Any-return coercion + untyped catch→Any)
- [x] **P7.3** Stack trace captured at throw site — SHIPPED `68b24dc`
      (minimal-correct §20.5.3.4 toString header; frame capture → L3b)
- [x] **P7.4** Native errors throw real RangeError / TypeError —
      SHIPPED a-1 `76252b6` / a-2 `c2dc3af` / a-b+#15 `76ace15` /
      frozen `683bd95` (conformance 629/0/1)
- [x] **P7.5** try / catch / finally state-machine matches spec
      ordering — SHIPPED `1e84f1b` (suspend pending throw across
      finally body per ECMA §14.13.3; closes the O5 spurious-
      propagation bug where finally's first may-throw call saw
      throw_active=1 from the pending and propagated before the
      callee could complete) — **P7 substrate 5/5 DONE**, trigger
      P7→P8 MET, conformance 629/0/1 preserved

---

### P8 — Class spec full (private + static blocks + accessor + super) (DONE)

**Goal**: complete the class feature set started in P4 — private
fields, static blocks, accessor properties, super-in-arrow.

**Substrate checklist** (strict order):

- [x] **P8.1** `#priv` private fields (parser + lower with name
      mangling) — SHIPPED A1 `2747225` (lexer PrivateIdent) /
      A2 `cb806d3` (parser field-decl accept + mangling) /
      A3 `915afa9` (parser dot-access raw `#`) /
      A4+A5 `e966db8` (parse-time current_class mangling + round-trip
      fixture `class-priv-001-this-field.ts` → 630/0/1 conformance).
      Hard-private (exact-class only — no Protected); cross-class
      and subclass access rejected at typecheck. Static `#x` and
      non-`this` typed-receiver `c.#x` (where c: C) outside the
      class body defer to P8.x followups.
- [x] **P8.2** Class getters / setters (accessor descriptors) —
      SHIPPED A1+A2 `acd6202` (parser detects get/set contextual
      keywords + AST `accessor_kind` on ClassMethod) / A3
      `5db54bf` (desugar renames accessors to
      `__cm_<C>__<name>_get/_set` + side-channel maps + filter
      from `__dispatch_<M>` synth) / A4+A5 `550a6fd` (check.rs
      Member read + Assign-Member write resolution via the maps
      + ssa_lower Call emission + round-trip fixture
      `class-accessor-001-get-set.ts` → 631/0/1 conformance).
      `c.value` reads the getter; `c.value = v` writes the
      setter; both single-Call, no runtime dispatch.
- [x] **P8.3** Static blocks `static { ... }` — SHIPPED A1
      `551a34a` (AST shape migration: ClassDecl `static_fields` →
      `static_init: Vec<StaticInit>` with `Field | Block` variants,
      mechanical refactor across ast.rs / parser.rs / formatter.rs /
      linter.rs) / A2 `641f0b3` (parser adds `static + LBrace`
      lookahead branch parsing block stmts into
      `StaticInit::Block(stmts)`; desugar still loud-panics on Block
      pending A3) / A3 `322be4f` (desugar walks Vec<StaticInit> in
      source order: Field → existing `__sf_<C>__<name>` LetDecl;
      Block → `__sb_<C>__<idx>` named-fn appended + top-level
      `Stmt::Expr(Call(...))` pushed into static_field_inits at the
      entry's index — preserves ES spec §15.7.10 interleaving) /
      A5 `class-static-001-blocks.ts` (4-case fixture: single
      block / interleave field+block / multi-block cross-ref /
      block-only). Known follow-up (NOT phase-blocker, parallel
      to existing static-method limitation): `this` inside a
      static block body currently fails typecheck the same way
      `this` inside a static method body does — both lift to
      top-level fns with no `__this` param. Existing class fixtures
      use `ClassName.member` form (per
      `class-static-inheritance-001.ts`); A5 follows that
      convention.
- [x] **P8.4** Arrow return-type inference for Call shape
      [SHIPPED `416c606` (A1) + `<A2-pending>` (A2)]. Originally framed
      as a narrow super-in-nested-arrow fix; probing during the ship
      cycle surfaced the actual root cause as a broader inference gap:
      `infer_expr_ann_with` bailed on every `Expr::Call`, so any
      bare-arrow body of shape `() => fn()` (with or without super)
      had its lifted closure FnDecl's return_type default to Void and
      typecheck rejected the surrounding code as a mismatch.
      A1 plumbs a fn_sigs table (built at desugar_implicit_generics
      entry from non-`__closure_*`, non-generic top-level FnDecls with
      an explicit return ann; includes desugar_classes-synthesized
      `__cm_<C>__<m>` whose return ann comes from the user-declared
      method) through the static return-ann sniff chain
      (infer_return_ann / infer_return_ann_seeded /
      collect_return_anns(_stmt) / collect_let_binding_anns(_stmt) /
      infer_expr_ann_with). infer_expr_ann_with gains a `Expr::Call`
      arm that resolves bare-Ident callees through fn_sigs. Three
      filters keep propagation sound: skip `__closure_*` (their own
      return ann is being inferred this pass), skip generic fns
      (TypeVar return is per-call-site mono), require bare-Ident callee
      (Member/Index need typechecker collaboration). Super in nested
      arrows rides this fix for free: desugar_classes Pass 1.5 / 1.6
      collectors already recurse into ArrowFn body and rewrite super
      ExprIds in place. A2 ships fixtures `class-super-arrow-001-
      nested.ts` (P8.4 named surface — super.method() / super.method(a,b)
      / super(args) in nested arrows + let-aliased) and
      `arrow-infer-callret-001.ts` (broader surface — number / string /
      boolean returning fns / block-body / param-forwarded). Known
      follow-ups parked in L3b: (i) let-bound closure call sites
      (`const inner = () => super.x(); const outer = () => inner()` —
      outer's Call(Ident("inner")) needs binds-level closure return-
      type resolution, separate substrate item); (ii) IIFE shape
      `(() => fn())()` where ssa_lower rejects `callee = Closure {...}`
      (independent of inference scope, lift_arrow_fns emits the closure
      as immediate callee — substrate gap in ssa_lower call-site
      dispatch).
- [x] **P8.5** Class expressions as values — SHIPPED A1+A2
      `769a224` (parser-level substrate) + `<A2>` (fixture-lock +
      roadmap). ES §15.7.4 ClassExpression covered: anonymous form
      (`const F = class { ... }`), named-inner-discarded form (`const
      F = class Inner { ... }`), extends form (`const F = class
      extends A { ... }`), and parenthesized-new-callee form (`new
      (class { ... })()`). Strategy (a) parser-synth ClassDecl +
      value-ref Ident — parse_primary's new Token::Class branch buffers
      the class as `__ClassExpr_<id>` in a `synth_classes` Vec (flushed
      before each stmt push in parse_program to preserve parent-
      before-child + synth-before-use ordering), emits Ident at the use
      site; the existing class-as-value substrate (`__class_<C>` +
      synthesize_class_globals's Ident rewrite) lifts it uniformly.
      parse_new gained Token::Class + Token::LParen arms for the
      `new class { ... }()` and `new (...)()` forms. A narrow alias map
      (`class_value_aliases: HashMap`) registers `const F = class {
      ... }` bindings so `new F()` rewrites to the static factory
      `__new___ClassExpr_<id>` at parse time — avoids a downstream
      dynamic-ctor-dispatch substrate. A2 fixtures: `class-expr-001-
      anonymous.ts` (single-method / ctor+field / two distinct
      classes / alias chain / cross-method call), `class-expr-002-
      named-extends.ts` (inner-name discarded, extends with method
      override, alias-of-extends), `class-expr-003-immediate-new.ts`
      (bare IIFE, IIFE-with-ctor-args, no-paren `new class`, extends
      with own ctor, instance-as-value-through-fn). Conformance
      634 → 637 (+3). Substrate-untouched downstream (desugar_classes
      / synthesize_class_globals / check / ssa_lower zero changes).

      L3b follow-ups (parked, not P8.5 scope):
      - Inner self-binding (`class Inner { ... }` body referencing
        Inner currently fails — Inner is discarded by force_synth)
      - Anonymous `.name === ""` per spec (currently
        `"__ClassExpr_<id>"`)
      - Full dynamic-ctor-dispatch substrate for `let F = class {};
        F = ...; new F()` / `function makeF() {...}; const F =
        makeF(); new F()` / arbitrary callee expressions through New
      - Alias scope-stack for fn-body shadowing (inner const-decl
        currently overwrites outer alias of the same name)
      - 3+ classes sharing the same method name → dispatch corruption
        (pre-existing, surfaced by P8.5-A2 fixture draft; reproducible
        with literal top-level form too — `class A { tag() {...} }
        class B { tag() {...} } class C { tag() {...} }` emits
        `a c c` instead of `a b c`)
      - Subclass without own constructor inheriting parent's
        constructor signature (pre-existing — subclass default-ctor
        synthesis ignores parent's arity; reproducible with literal
        top-level subclass form)

      With P8.5 shipped, P8 is fully closed (P8.1/2/3/4/5 all done) —
      P8 → P9 phase trigger met.

---

### P9 — Regex full (DONE)

**Goal**: spec-complete RegExp incl. lookahead / lookbehind, named
groups, Unicode flag, sticky flag.

**Substrate checklist** (strict order):

- [x] **P9.1** Lookbehind / lookahead — SHIPPED A1+A2 `0404b08`
      (runtime_regex.c lookbehind substrate) + `<A2>` (fixture-lock +
      roadmap). Lookahead `(?=X)` / `(?!X)` was already in place from
      Phase 1c.4 (sub-Program + sub_probe at current pos); P9.1 lands
      lookbehind `(?<=X)` / `(?<!X)` to complete the zero-width
      assertion set. Approach (B) variable-width: vm_match_at gains an
      `end_target` param (-1 = leftmost-first as before; ≥0 = only
      MATCH at pos==end_target commits + outer loop short-circuits once
      pos > end_target), and a new `sub_probe_ending_at` scans candidate
      start positions j ∈ [0..pos] invoking vm_match_at on the forward
      sub-Program with end_target = pos. The forward-compile sub stays
      shared with lookahead — no second compile mode. A2 fixtures:
      `regex-008-lookbehind.ts` (positive/negative × at-start /
      mid-pattern / combined-with-lookahead), `regex-009-lookbehind-
      variable.ts` (alternation in body / quantifier in body / char-
      class with quantifier / negative-with-quantifier), `regex-010-
      lookbehind-replace.ts` (replace / replaceAll / match / .test()
      with anchors). Conformance 637 → 6XX (+3). No AST / parser /
      lookahead behavioral change.

      Why Approach B vs (A) reverse-compile sub: minimal AST surface
      (no second compile mode), narrow-surface fix per 设计原则 #4
      (规范). Worst-case O(pos · sub_len) acceptable for v0.1 — body
      lengths in practice are short. Upgrade path to (A) replaces only
      sub_probe_ending_at; AST / op / parser stay put — a future
      perf-axis phase can swap implementations transparently.

      P9.1 closing advances L3a to P9.2 (named capture groups + back-
      references). P9 phase has 5 substeps; closing all unlocks P9→P10
      trigger.
- [x] **P9.2** Named capture groups + back-references — SHIPPED A1+A2+A3
      `8a5aa61` (A1: parser + matcher substrate) + `4b12de4` (A2: .groups
      accessor) + `<A3>` (fixtures + this roadmap). A1 lands the regex
      engine substrate: parser accepts `(?<name>X)` (records name in a
      new Parser.names table aligned with capture_idx); `\k<name>` emits
      NK_BACKREF resolved post-parse via the name table; `\1..\9` emits
      NK_BACKREF{idx} validated against final n_captures. New OP_BACKREF
      opcode + per-thread `br_offset` state machine in the outer match
      loop consumes the captured slice byte-at-a-time across steps
      (continuation re-scheduling bypasses the visited table so a fresh
      backref entry isn't blocked by an in-flight continuation at the
      same pc — they carry different state). i-flag aware via the
      existing char_eq path. A2 attaches `.groups` to match-result
      arrays: RegExp now persists capture_names past parse, and a new
      attach_groups helper builds a dynobj of name → captured Str, set
      on the array via the existing arrprops side-table. `m.groups`
      reads already lower through arrprops_get (the typechecker routes
      Array.<unknown> to Type::Any), so no compile-side changes were
      needed. Non-participating named groups → ANY_UNDEF entries per
      spec §22.2.5.7. A3 fixtures: `regex-011-named-capture.ts` (parser
      acceptance + positional / `.exec` access), `regex-012-backref.ts`
      (positional `\1..\9` single/multi-char + non-participating +
      i-flag + alternation), `regex-013-named-backref.ts` (`\k<name>`
      single/multi-char + forward refs + mixed positional/named refs +
      i-flag), `regex-014-groups-dict.ts` (`.groups.NAME` access on
      `match` / `exec` + non-participating undefined + named+positional
      coexistence). Conformance 640 → 6XX (+4).

      Narrow-surface design choice (per [[feedback-narrow-abi-surface]]):
      parser owns the name table; matcher stays positional. Alternative —
      push name resolution into the matcher — would surface-broaden Op +
      Thread for no runtime benefit. The Thompson NFA + multi-byte backref
      tension (Russ Cox style normally precludes backref) is resolved by
      the per-thread `br_offset` state machine — the only invasive change
      was replacing Thread's unused `pad` field with `br_offset` (same
      sizeof).

      L3b follow-ups recorded:
      - ECMA Annex B OctalEscape / IdentityEscape for `\N` when N >
        n_captures (currently rejected at parse; bun returns false-
        match on regex-execution rather than rejecting the literal).
      - OP_CLASS i-flag awareness (pre-existing; `[a-z]/i.test("A")` →
        false on tora vs true on bun). Independent of P9.2 but surfaced
        while writing fixtures.
      - Typechecker RegExpMatchArray type — `.match()` / `.exec()`
        currently return `Array<String>`, so `.groups` access requires
        `as any` cast in source. Surface ergonomics improvement, not a
        correctness gap.

      P9.2 closing advances L3a to P9.3 (Unicode flag).
- [x] **P9.3** Unicode flag (`u`) — character class handling — SHIPPED
      A1+A2+A2.1+A3 `3fd8cfe` (A1: `\u{}` / `\uHHHH` escape + `.`
      astral) + `97dcf93` (A2: `\p{Letter|Number|ASCII}` + OP_CLASS
      code-point) + `6244622` (A2.1: search start-pos skips UTF-8
      continuation bytes) + `<A3>` (fixtures + this roadmap).

      A1 adds u-flag mechanics: parse_escape recognises `\u{HHHH..}`
      (extended form, u flag only) and `\uHHHH` (4-digit form,
      always — also fixes a pre-existing parser bug where `\uHHHH`
      was treated as literal `u<digits>` even without u flag). Both
      forms encode to UTF-8 bytes and emit as NK_CONCAT of NK_CHARs,
      so the byte-step Thompson NFA matches the encoded sequence
      naturally without a new opcode. `.` (OP_ANYCHAR) under u flag
      advances by `utf8_len_for(s[pos])` (1–4 bytes); the destination
      thread is patched with a new `Thread.u_skip` defer counter so
      the outer step queue waits adv-1 steps (consuming continuation
      bytes implicitly) before dispatching the next op. Bypass-
      visited defer keeps the queued thread alive across step swaps
      without colliding with fresh entrants at the same pc — same
      pattern as P9.2-A1's OP_BACKREF `br_offset`.

      A2 adds Unicode property classes: `\p{L|Letter}`, `\p{N|Number}`,
      `\p{ASCII}` parsed in parse_escape (outside class) and
      parse_class (inside `[...]` — OR-unions into the existing
      class). `\P{X}` outside class = class-level negate. ASCII
      portion lives in the regular bitmap; cp ≥ 128 portion is
      covered by curated UCD subset tables (Greek, Cyrillic, Hebrew,
      Arabic, Devanagari, Thai, Hiragana, Katakana, CJK, Hangul,
      common decimal-digit scripts). A new CharClass.u_props bitfield
      + cc_test_cp helper dispatches: cp < 256 → bitmap, cp ≥ 128 →
      uprop_range_contains binary search. OP_CLASS under u flag
      decodes one code point at s[pos] via utf8_decode_cp, tests via
      cc_test_cp, and reuses the A1 u_skip patch for multi-byte
      advance.

      A2.1 is a follow-up to A2: the byte-iterating search start
      loops (vm_search_from + vm_search_from_with_ws) skip UTF-8
      continuation bytes (`(s[st] & 0xC0) == 0x80`) under u flag,
      so `/[^\p{L}]/u.test("漢")` doesn't accidentally accept the
      mid-sequence continuation byte 0xBC as a stand-alone non-
      Letter code point. Code-point-aligned start positions only.

      A3 fixtures: `regex-015-unicode-flag.ts` (extended `\u{}` BMP
      + astral, `\uHHHH` with/without u flag, `.` astral + anchored,
      `.match(/./u)` for emoji / BMP / ASCII, `.+` over multiple
      astrals, mixed ASCII+astral, literal multi-byte in pattern,
      `\u{}` leading-zero variants), `regex-016-unicode-properties.ts`
      (\p{L|Letter} / \p{N|Number} / \p{ASCII} positive, \P
      negation, .match with \p{L}+, /\p{L}+/gu global, `[\p{L}\p{N}]/u`
      union, `[^\p{L}]/u` class-level negate, mixed bitmap+property
      `[a-z\p{N}]/u`, replace with property, anchored property,
      alias resolution). Conformance 644 → 6XX (+2 with the new
      fixtures).

      Narrow-surface design choice (per [[feedback-narrow-abi-
      surface]]): no new opcode, no Inst layout change, no Node
      field addition. Code-point semantics realised entirely via the
      Thread.u_skip defer queue + CharClass.u_props bitfield +
      static UCD tables — same narrow-surface playbook as P9.1
      (sub_probe_ending_at) and P9.2 (br_offset). The Thompson NFA
      "1 byte per outer step" invariant is preserved; u-flag work
      happens in scheduled-defer pattern instead of changing the
      outer loop's step granularity.

      L3b follow-ups recorded:
      - Full UCD property tables — v0.1 ships hand-curated subsets
        (Greek/Cyrillic/Hebrew/Arabic/CJK/Hangul/Hiragana/Katakana
        + common-script decimals). Real \p{L} has hundreds of
        ranges; auto-import from UCD data files is L3b. Dominant
        test262 cases pass with the curated subset.
      - `\P{X}` inside class (complement semantics inside `[...]`)
        — current v0.1 errors out; correct semantics requires either
        per-property complement tables or a "negative-bitfield" mode
        on CharClass.
      - `v` flag (ES2024 set notation `[[\p{X}--[a-z]]]`) — separate
        substep beyond v0.1.
      - Lone surrogate handling — `"\uD800".match(/\uD800/)` differs
        from bun's UTF-16 view (tora's WTF-8 byte representation
        doesn't perfectly round-trip ill-formed inputs). Edge of
        spec, low test262 impact.
      - Property=Value form (`\p{Script=Latin}`) — parser accepts
        only Name-only form; Name=Value is L3b.

      P9.3 closing advances L3a to P9.4 (Sticky flag).
- [x] **P9.4** Sticky flag (`y`) — lastIndex semantics — SHIPPED
      A1+A1.1+A2 `9fe2ebb` (A1: RegExp.last_index field + accessors +
      sticky/global lastIndex semantics in __torajs_regex_exec and
      __torajs_str_match_regex non-global path) + `4f59eb8` (A1.1:
      sticky-aware replace/replaceAll/split/matchAll) + `<A2>`
      (fixtures + this roadmap).

      A1 introduces `int64_t RegExp.last_index` (calloc init 0) +
      runtime accessors __torajs_regex_get_last_index /
      __torajs_regex_set_last_index + a new `vm_match_anchor` helper
      for single-position anchored match (used by sticky paths to
      anchor at lastIndex with miss-on-continuation-byte under u
      flag). Surface routing: ssa_lower adds read-side branch (call
      get_last_index returning I64) and write-side branch (coerce_to_i64
      + call set_last_index) for the `re.lastIndex` member; check.rs
      adds `(Type::RegExp, "lastIndex") => Type::Number` for reads
      and a permissive write-arm before the struct-only check.

      Semantics: sticky (`y`) anchors at lastIndex with single
      attempt; global (`g`) starts search from lastIndex; plain
      ignores lastIndex and never writes it. Y takes precedence
      when both flags set. On miss with tracking, reset lastIndex
      to 0 per spec §22.2.5.2.2; on hit, write match end.

      A1.1 surfaced during fixture verification — the other regex
      iterators (replace / replaceAll / split / matchAll) kept their
      own loops over vm_search_from_with_ws and silently disagreed
      with bun under y flag (e.g. `"aXab".replace(/a/gy, "Y")` gave
      "YXYb" because the loop walked past the sticky failure at
      index 1 to the next 'a'). Same narrow-surface fix in all four
      functions: branch on sticky → vm_match_anchor at `pos` → break
      loop on miss. Pattern mirrors P9.3-A2.1 (substrate fix exposed
      at fixture-write time, ship as independent gated commit).

      A2 fixture `regex-017-sticky.ts` (15 cases): sticky anchor +
      r/w/reset, sticky walk via repeated exec, sticky miss mid-string,
      lastIndex > length / negative clamp, g-only advance, plain
      flag ignore, g+y interaction (both anchor and advance),
      sticky replace + replaceAll (the cases that surfaced A1.1),
      s.match with sticky hit + miss, lastIndex from indexOf,
      multi-char pattern anchored advance. Each block byte-equal
      vs bun. A1 + A1.1 gates each 646/0/1 (0 regression vs
      post-P9.3 baseline). A2 ships fixture-only per autorun
      pipeline fixture-only exception (no substrate change).

      Narrow-surface design (per [[feedback-narrow-abi-surface]]):
      no Inst layout change, no new IR opcode, no Node field
      addition. RegExp struct grows by one int64 (last_index); two
      new runtime accessors + one vm-internal helper
      (vm_match_anchor); two new compile-time intrinsics; one
      read-side + one write-side dispatch arm in both check.rs and
      ssa_lower.rs.

      L3b follow-ups recorded:
      - `RegExp.prototype.test()` should also honor sticky/global
        lastIndex per spec — currently calls vm_search_from(0,
        flags) ignoring lastIndex (line 2083). Trivial fix mirror
        of exec(); deferred to surface the trade with a clear
        commit (test() semantics affect many test262 entries).
      - `vm_match_anchor` internally allocates workspace; could be
        promoted to a `_with_ws` variant for tight-loop reuse. Not
        a measurable hot-path concern at current case sizes.
      - regex_exec's miss returns `[]` not `null` (Phase 1c.4
        Nullable<Array>) and hit-result lacks `index` / `input` /
        `groups` props as separate attachments (Phase 1c.4 array-
        prop). Unrelated to P9.4 but the new fixture uses
        `m !== null && m[0] === ...` shape to side-step them.
      - sticky split / matchAll behaviour on g+y is now correct
        for the iteration but doesn't yet match bun's `g`-required
        TypeError for `matchAll(non-g-regex)` (Phase 1c.4 work).

      P9.4 closing advances L3a to P9.5 (replace callback fn).
- [x] **P9.5** `String.prototype.replace(regex, fn)` callback form —
      SHIPPED A1 `851f26d` + A2 `b0389f0` + A1.1 `a554f8d` + A1.1-A2
      `503c928` + A1.2 `ee92139` + A1.2-A2 `<A2>` (fixture + roadmap
      close). A1 lands the **first cross-boundary closure invoke from
      C runtime** in tora — runtime helpers load fn_addr from env+8
      (same ABI as `promise_then_closure`) and invoke
      `(env, m, g1, ..., gN [, off, input]) -> ret_str` per match.
      A1.1 extends from `(m)` to `(m, g1, ..., gN)`; A1.2 adds the
      trailing `(offset, input)` args per ES spec §22.1.3.18.

      `ssa_lower:19613` regex-receiver branch dispatches by repl SSA
      type: `Type::Str` → existing `regex_replace` / `_all` (expand_repl
      path); `Type::Closure` → fn-variant intrinsics. Closure user-sig
      shape detected at lower time:
       - `[Str; N+1] -> Str`            → A1.1 (has_off_input=0)
       - `[Str; N+1, I64, Str] -> Str`  → A1.2 (has_off_input=1)
      with N matching the regex's static capture count; mismatches +
      N>9 panic with a clear compile-time message — never silent-wrong
      from C ABI cast mismatch. `check.rs:3732` widened the 2nd arg from
      `Type::String` to `Type::Any` so both Str and Closure pass
      typecheck. Sticky / global handling mirrors the Str-repl
      siblings (P9.4-A1.1 semantics preserved through fn path).

      **Runtime layout** (`runtime_regex.c`): 20 cb typedefs total —
      `replace_cb_N_t` (N=0..9, A1.1 shape) + `replace_cb_N_off_t`
      (N=0..9, A1.2 shape with trailing offset+input). The
      `invoke_replace_cb(n_caps, has_off_input, env, fn_ptr, m,
      caps, off, input)` static helper picks the right cast via a
      branch on `has_off_input` and a 10-arm switch on `n_caps`.
      `build_capture_strs(n_caps, saves, s, out_caps)` constructs N
      Strs from `saves[2*(i+1)..]` per match. Outer helpers pass
      match-start `st` as offset and the receiver `str_ptr` as input
      (borrowed — cb must not retain past invocation).
      Non-participating capture groups (saves slot == -1) emit empty
      Str rather than `undefined` — A1 narrow scope; Nullable<Str>
      cb params + true undefined semantics are A1.1.1 follow-up.

      **ssa-lower side** (`ssa_lower.rs`): new top-level fn
      `count_capture_groups(pattern) -> usize` with 9 unit tests
      (plain / nested / non-capturing / named / char-class / escaped).
      Intrinsic sigs widened from `[Str, RegExp, Ptr]` to
      `[Str, RegExp, Ptr, I64, I64]` to thread n_caps + has_off_input
      through. Closure user-sig shape detected per dispatch site
      (A1.1 vs A1.2). For ident-bound regex (where capture count
      can't be statically derived) n_caps defaults to 0; N≥1 cb with
      ident regex panics with a clear message.

      Fixtures:
      - `regex-018-replace-callback.ts` (15 cases, A1 N=0 baseline)
      - `regex-019-replace-callback-captures.ts` (14 cases, A1.1 N=1..3
        including the canonical bun idiom `(\w+) (\w+)` swap)
      - `regex-020-replace-callback-offset-input.ts` (10 cases, A1.2
        full arity — offset + input, mixed with N=0..3 captures)
      All three fixtures byte-equal vs bun.

      Conformance 646 → 650 across the A1/A1.1/A1.2 chain (gates
      `/tmp/torajs-conformance-p95a*.log`, 0 regression at each step).
      The +4 comes from regex-017 (P9.4-A2), regex-018 (P9.5-A1),
      regex-019 (P9.5-A1.1), regex-020 (P9.5-A1.2).

      A1/A1.1/A1.2 scope is intentionally narrow (per
      [[feedback-narrow-abi-surface]]): this is the first C-runtime
      closure invoke surface in tora. A1 shipped the ABI pattern with
      strict `(m) => ret`; A1.1 generalized to N captures via static
      parse; A1.2 added the trailing offset+input args. Each was a
      clean increment on the proven A1 substrate.

      Constraint: callback param types must be explicitly annotated
      (e.g. `(m: string, g1: string, off: number, input: string) =>
      string`). Tora's `build_fn_type` already requires this for arrow
      fns (consistent with `arr.map` / `filter` / `forEach`), so the
      friction matches existing patterns.

      L3b follow-ups (independent of P9, deferrable to future phases):
      - **A1.1.1 non-participating groups as undefined** — current
        A1.1 emits empty Str for `(a)|(b)`-style alternation where
        one group doesn't fire. Spec says `undefined`. Requires
        Nullable<Str> cb param support — independent typecheck
        work.
      - **String-receiver fn callback** — `"foo".replace("o", fn)`
        (non-regex string pattern) also accepts fn callback per spec;
        currently rejected at typeck via the Str-pattern arm.
        Independent of P9 substrate.

      P9.5 closing completes the full P9 phase (5/5). P9 → P10
      trigger met (substrate checklist 5/5 ✓; conformance 650/0/1
      holds). L3a advances to P10.1 (Microtask queue with drain at
      every yield point).

---

### P10 — Promise + async-await + Generator (DONE)

**Goal**: real microtask queue, ordering guarantees, async iterators,
generator full state machine. v5 merges v4's P9 (Promise) + P14
(Generator) into one phase — both share state-machine substrate.

**Substrate checklist** (strict order):

- [x] **P10.1** Microtask queue with drain at every yield point —
      SHIPPED A1 `b252492` + A1-A2 `a0f699f` + A1.1 `6d134e3` +
      A1.2 `2d3a317` + A2 `<closing>`. Wires WHATWG HTML
      §queueMicrotask global to the existing T-15.c microtask
      queue + T-15.e main-exit drain. Substrate (queue +
      run-until-idle drain + main-exit auto-call + await drain)
      was already complete since v0.5; this phase adds the
      language-layer entry point and the closure-capture
      visibility fix that lets the cb body schedule further
      microtasks.

      **A1 `b252492`** — `queueMicrotask(cb)` for closure-typed
      cb. Runtime: new `__torajs_queue_microtask_closure(env)` +
      `queue_micro_closure_dispatch_(arg)` mirroring
      `finally_closure`'s env+8 fn_addr ABI (cb is `void (env*)`;
      rc-inc env at attach, drop via `__torajs_value_drop_heap`
      after invoke). SSA: new `microtask_enqueue_closure`
      intrinsic + bare-name lowering arm in `ssa_lower:15842`.
      check.rs: new bare-name type-check arm at `~5586`
      enforcing `Type::Function([], Void)`. Fixture
      `micro-001-queueMicrotask-basic.ts` byte-equal vs bun.

      **A1-A2 `a0f699f`** — docs-only roadmap progress note.

      **A1.1 `6d134e3`** — simple-fn (named fn decl) cb path. A1
      always emitted the closure intrinsic regardless of cb
      type, so passing a named-fn ident (`Type::FnSig`, raw fn
      ptr) → runtime read garbage at env+8 → SIGBUS. Fix mirrors
      `promise_then_{simple,closure}` dispatch (`ssa_lower:17152`):
      branch on cb's static type at the call site. Type::Closure
      → `_closure` (existing); Type::FnSig → new
      `__torajs_queue_microtask_simple(fn_ptr)` which casts back
      to `void ()` and invokes (no rc; fn pointers live in
      .text). Fixture `micro-002-queueMicrotask-named-fn.ts`.

      **A1.2 `2d3a317`** — visibility of `queueMicrotask` inside
      closure bodies. Surfaced by nested-microtask probe: cb body
      that calls `queueMicrotask(...)` failed with "closure
      `__closure_N` references unknown identifier `queueMicrotask`"
      because the closure-capture analyzer
      (`check.rs:7032`) treated the bare ident as a captured
      local. Pre-existing globals (parseInt / isNaN / ...) were
      exempt via `ast.rs::is_global_name`; the list missed
      queueMicrotask. Added it to both that list and
      `check.rs::is_known_builtin_global` per the in-code
      sync-comment. Fixture
      `micro-003-queueMicrotask-nested.ts` exercises 3-level
      nested chain (mt-1 → mt-2 → mt-3 inside same drain cycle
      via `__torajs_microtask_run_until_idle`'s
      `while (mt_head_ < mt_len_)` loop).

      **Drain-coverage audit @ P10.1 close**: spec "every yield
      point" reduces to main-exit + await + nested-cb scheduling.
      tora covers all three: T-15.e main-exit drain
      (`ssa_lower:6163`); T-16 await drain (`ssa_lower:23659`);
      nested drain via `run_until_idle`'s growing-tail loop.
      Audit fixtures (`/tmp/p10.1-a1.2-audit-{sequential,nested,
      deep-nested}.ts`) all byte-equal vs bun.

      **Conformance** monotonic non-decreasing across A1/A1.1/A1.2
      gates: 651/0/1 (A1, `/tmp/torajs-conformance-p10.1-a1.log`)
      → 652/0/1 (A1.1,
      `/tmp/torajs-conformance-p10.1-a1.1.log`)
      → 653/0/1 (A1.2,
      `/tmp/torajs-conformance-p10.1-a1.2.log`). +3 = micro-001
      + micro-002 + micro-003 picked up.

      **Follow-up (L3b, not P10.x scope)**:
      - **A1.3 `Window.queueMicrotask`** namespaced form — defer
        until/unless namespace globals matter (tora is
        Node-runtime style; the bare-name binding suffices).
      - **const-lambda binding crash** — pre-existing SIGBUS on
        `const cb = () => {...}; queueMicrotask(cb)` (also
        reproduces on `.finally(cb)`). Affects multiple
        closure-cb sites; needs ident-resolution audit (Closure
        value vs Closure-box rc handling on var read). Out of
        P10.1 scope.

      P10.1 closing advances L3a to **P10.2** (Promise.all /
      .race / .allSettled / .any per spec). P10 phase has 7
      substeps; closing all unlocks P10 → P11 trigger.
- [x] **P10.2** Promise.all / .race / .allSettled / .any per spec
      (currently allSettled is single-T MVP).

      **DONE** (resumed-session 2026-05-21 + 2026-05-22):

      - **A1** `5be6b5c` — `Promise.resolve()` / `Promise.reject()`
        0-arg form per ES spec §27.2.4.7 / §27.2.4.5. 0-arg ≡
        passing `undefined`. Inner T = `Type::Undefined`.
        - check.rs:5167 — `args.is_empty()` early-returns
          `Promise<Undefined>`; `args.len() > 1` errors as
          "expects 0 or 1 arg".
        - ssa_lower:17575 — new early branch for `args.is_empty()`
          synthesizes `Operand::ConstI64(0)` (undefined sentinel,
          shares i64-0 ABI with null) and dispatches the non-heap
          `promise_alloc_fulfilled` / `_rejected` allocator. No
          runtime / IR-helper changes.
        - Fixture `conformance/cases/async-018-promise-resolve-0arg.ts`
          covers `Promise.resolve().finally(...)` chain; byte-equal
          vs bun. Gate **654/0/1** (baseline 653 + async-018, 0
          regression).
        - reject() 0-arg runtime-smoked (exit 0, no segfault);
          fixture deferred to A1.1 once `.catch` accepts inner
          T=Undefined.

      - **A1.1** `6c93b90` — `.then` / `.catch` accept inner
        T=Undefined on `Promise<Undefined>`. Builds on A1.
        - check.rs:~5001 — new Call-time arm specializing
          `(Type::Promise(Type::Undefined), "then" | "catch")`
          with cb sig `() => U` (0-arg form, ergonomic over
          spec `(v: undefined) => U`). cb return U: primitive
          (Number/String/Boolean) → Promise<U>; Void/Undefined
          → Promise<Undefined>; other → typecheck error.
        - ssa_lower zero changes — SSA Type::Promise is unit
          (no inner T), existing cb_ty Closure/FnSig dispatch
          at line ~17220 routes correctly to promise_then_*
          helpers without inner-T inspection.
        - runtime zero changes — `then_simple_dispatch_` casts
          cb to `int64_t (*)(int64_t)`; SystemV puts unused
          arg in rdi (cb ignores). Standard ABI tolerance.
        - Fixture `conformance/cases/async-019-promise-resolve-then-catch.ts`
          chains `.then` + `.catch` on resolve()/reject() 0-arg
          with sync interleave; byte-equal vs bun
          (`sync\nr1\nr2`). Also closes A1's reject() 0-arg
          runtime-smoke gap (now fixture-tested).
        - Gate **655/0/1** (baseline 654 + async-019, 0 regression).

      P10.2-A1 family closed (A1 substrate + A1-A2 docs + A1.1
      substrate). Two gates monotonic 654 → 655 / 0 / 1.

      - **A2** `ef3c895` — ssa_lower static_ctor whitelist for
        Promise statics. Smoke probe during this rotation revealed
        `Promise.race(ps).then(cb)` failed at lower time with
        "not yet supported: ssa-lower: unsupported member call
        shape: then". Root cause: `src_is_builtin_promise`
        whitelist at `ssa_lower:~17098` recognized only resolve /
        reject; the four T-17.a/b/c statics (all / race / any /
        allSettled) returned `Type::Promise` from check.rs but
        weren't picked up by the lowering whitelist, so chained
        calls fell through to the (non-existent) user-class
        fallback.
        - ssa_lower:~17098 — extend `static_ctor` match's name
          set to all six Promise namespace statics via
          `matches!(src_m.as_str(), "resolve" | "reject" | "all"
          | "race" | "any" | "allSettled")`. Pattern stays
          identical (obj==Ident("Promise")).
        - Zero runtime / IR-helper changes. Zero check.rs
          changes (each static already returns Type::Promise).
        - Fixture `conformance/cases/async-020-promise-race-any-then.ts`
          chains `.then` on `Promise.race(ps)` and
          `Promise.any(ps)` (both yield Promise<Undefined> for a
          Promise<Undefined> input array; A1.1's then/catch arm
          takes over from there). Byte-equal vs bun
          (`sync\nrace-done\nany-done`).
        - Gate **656/0/1** (baseline 655 + async-020, 0
          regression).

      Three gates monotonic 654 → 655 → 656 / 0 / 1 through A2.

      - **A3** `5dd1a91` — `Promise.allSettled` accepted T
        extends from Number-only to {Number, String, Boolean}
        primitive set (parity with Promise.all current T support).
        - check.rs:5333 — match arm widens to
          `Type::Number | Type::String | Type::Boolean`.
        - check.rs:5338-5341 — result struct value field type
          tracks inner T monomorphically (was hardcoded
          Type::Number). ssa_lower picks up field type from
          Type::Struct, emits str_drop for String / no-op for
          Number/Boolean.
        - runtime_promise.c:570 — `__torajs_promise_allsettled_sync`
          rc_inc's when `pp->value_is_heap` (the settled struct
          co-owns the heap value alongside the source Promise).
          Non-heap path unchanged.
        - Fixture `conformance/cases/async-021-allsettled-string-bool.ts`
          covers String (heap, 3 promises), Boolean (non-heap),
          Number (regression guard). `await Promise.allSettled(...)`
          per async-017 pattern. Byte-equal vs bun.
        - Gate **657/0/1** (baseline 656 + async-021, 0 regression).

      - **A4** `15caa67` — `.then` / `.catch` accept inner
        T=Array<U> on Promise<Array<U>>. Unblocks
        `Promise.all(ps).then(cb)` (previously rejected at
        typecheck because generic .then arm limited inner T to
        primitives).
        - check.rs — new specialized arm matching
          `(Type::Promise(Type::Array(_)), "then" | "catch")`
          placed between A1.1's Promise<Undefined> arm and the
          fetch arm. cb sig validated as `(arr: Array<U>) => V`
          1-arg structural form.
        - cb return V: primitive (Number / String / Boolean) →
          Promise<V>; Void / Undefined → Promise<Undefined>;
          Array<W> deferred (would need helper-side
          value_is_heap=true propagation, separate sub-A).
        - Zero ssa_lower / runtime changes — SSA Type::Promise
          is unit; existing cb_ty Closure/FnSig dispatch routes
          correctly; SystemV `int64_t (*)(int64_t)` passes Array
          ptr in rdi.
        - Fixture `conformance/cases/async-022-promise-all-then.ts`
          covers number/string/boolean array → primitive return
          + number array → void return. Byte-equal vs bun.
        - Gate **658/0/1** (baseline 657 + async-022, 0 regression).

      Five gates monotonic non-decreasing across A1/A1.1/A2/A3/A4:
      654 → 655 → 656 → 657 → 658 / 0 / 1.

      **Deferred to a later phase / sub-A**:
      - **A_n** Heterogeneous T-tuples for Promise.all /
        .allSettled per spec — depends on PromiseId interning
        substrate (T-15.g.6+). Not blocking P10.2 closure;
        moves to L3b until that substrate lands.
      - Array<W> cb return on `Promise<Array<U>>.then(cb)` —
        would need helper-side value_is_heap=true propagation
        for next Promise's heap value. Not blocking common
        patterns (Promise.all().then() with .length / element
        reads is the dominant use case).

      **Naming-drift note (rotation boundary)**: e5a1944 (A1-DONE
      docs) initially queued "A2" as "extend allSettled T". A
      smoke probe right after A1.1 ship exposed the ssa_lower
      whitelist gap, which was narrower + more foundational, so
      that rotation shipped it as "A2" instead, and the
      allSettled T extension became A3. Recorded so future
      audits trace the sub-step naming progression cleanly.

- [x] **P10.3** Async iterator + for-await-of narrow MVP (depends on P5)
  - **A1** `for-await-of` narrow MVP on `Array<Promise<T>>` (`3348c9b`).
  - **A2** `async fn` accepts idiomatic `Promise<T>` return annotation
    (`0062f0e`).
  - **A3a** class async method substrate — `[static] async name(...)
    {}` with `this.field` access, `Promise<T>` return, caller `await`
    / `.then` consumption (`f6f55bf`, 2026-06-03; full sequence
    `3d98ba4` → `e8b3b86` → `ddfb33a` → `f6f55bf` = 2 god-fn decomp
    prereqs + parser substrate + ast substrate). Fixture
    `async-023-class-async-method.ts`.
  - **A3b** object literal async method real substrate — `{ async
    name() {} }` Ident-key form (`bb995fb`, 2026-06-03). Synth-fn
    route: parser mints `__obj_async_method_<id>` `Stmt::FnDecl`,
    registers it in `ast.async_fns`, property value =
    `Expr::Ident(synth_name)`. Computed-key `async [Symbol.X]()`
    stays on stub-drop (gated on P3/P7 Symbol.X dispatch).
  - **Deferred to P10.7 / later**: user-defined async iterator
    (`async function* gen()` / `[Symbol.asyncIterator]()` implementing
    user class) — deeper feature outside P10.3 narrow MVP.
- [x] **P10.4** await on non-Promise — value-flavor `await` per ES
  spec wraps non-Promise via `Promise.resolve`.
  - Primitive (Number / String / Boolean) (`d6dab10`).
  - Rejected Promise throws the rejection reason — was silent `return
    0` (`1b48a77`).
  - Identity extends to non-Promise heap T (Array, BigInt)
    (`081b25f`).
- [x] **P10.5** unhandledrejection handler hook
  - **A1** `fac7bbe` — async fn body throw → `Promise.reject` per spec
    §27.7.3.6 (synthetic try/catch wrap; catch returns
    `Promise.reject(__async_err)`); `desugar_async` extracted to
    `ast/desugar_async.rs` sidekick by prereq `684a270`. Fixture
    `async-024-async-throw-non-propagating.ts`.
  - **A2** `6059899` — `Promise.reject` accepts `Type::Any` with
    contextual unify against enclosing `Promise<T>`; `desugar_async`
    `catch_type` lifted to `"any"` (spec-correct, drops A1's narrow
    MVP). `check/promise_static.rs` sidekick extracted by prereq
    `a9d1d5f`. Fixture `async-028-async-throw-typed-mismatch.ts`.
  - **A3** default unhandled-rejection reporter — split into A3-a +
    A3-b after RFC `.claude/rfcs/20260603-p10-5-a3-unhandled-rejection/`
    (initial attempt `b484990` with process-global counter + sync
    syscall_exit reverted via `2ed5aa1` after 2-fixture regression).
    - **A3-a** `b6f80c1` — `Promise.has_handler u8` (`_pad[6]` →
      `has_handler u8 + _pad[5]`; ABI 32 B unchanged). `attach_then` /
      `get_value` entries set `has_handler = 1`; 0 substrate behavior
      change (detector not yet wired).
    - **A3-b** `9d2ae5d` (+ followup `34b4e88`) — HostPromiseRejectionTracker
      microtask per spec §27.2.1.9 (`promise::unhandled`
      sidekick); `UNHANDLED_REJECTION_OCCURRED` flag + new
      `__torajs_main_exit_code() -> i32` intrinsic; `synthesize_main`
      tail emits `call + ret i32` via new sidekick
      `ssa_lower_main_exit.rs` (net `ssa_lower.rs` -6 LOC, known-debt
      reduction). Followup moves HPRT off the microtask queue onto a
      dedicated `UNHANDLED_LIST: Mutex<Vec<i64>>` swept once at main
      exit — fixes `await`'s mid-sync `microtask_run_until_idle`
      popping HPRT-check before `get_value` had a chance to mark the
      promise observed. Fixture
      `async-029-unhandled-rejection-default.ts` byte-equal stdout +
      stderr `error: <reason>` + exit 1.
  - **A4** `79f9d6d` — `process.on('unhandledRejection', cb)`
    listener (Node/Bun extension on `process`). New sidekicks
    `check/process_on.rs` (82 LOC, typecheck `'unhandledRejection'`
    literal + `Type::Function` cb) and `ssa_lower_process_on.rs`
    (101 LOC, dispatch FnSig vs Closure to two register intrinsics).
    `promise::unhandled` UNHANDLED_CB slot (AtomicPtr + AtomicU8 kind
    discriminator; rc_inc on closure env; prior listener env released
    on overwrite). Sweep calls user cb with reason lifted into
    NaN-box AnyValue wire form; reporter suppressed + flag not set →
    exit 0. Fixture `async-030-process-on-unhandled.ts`.
- [x] **P10.6** Generator full state machine — `yield*` delegation +
      `Generator.prototype.return` / `.throw`
  - **yield\*** `00165e5` — generator delegation (pre-existing,
    landed before P10.6 sub-step naming).
  - **A1** `cccc5a1` — `Generator.prototype.return(value)` per
    ES §27.5.1.7.
  - **A2** `2a11667` — `Generator.prototype.throw(err)` per
    ES §27.5.1.4.
  - **A3** `d58a96c` — multi-generator same-method dispatch fix
    (`__gen_nominal_<name>: number` field unique per class →
    sibling-class static dispatch routing correct) + `may_throw`
    post-call `emit_throw_check` guard for `.throw` cross-fn
    propagation. Fixture `gen-003-multi.ts`.
- [x] **P10.7** Default-Any Generator/Async fn (T-33 substrate)
  - **generator-side** `157c828` — Default-Any generator; followup
    fix `6d161a6` (`As`-widen to Any only on primitive sources).
  - **async-side** `c44aa39` — Default-Any async fn +
    `Promise<Any>.then/.catch` (also lifts a prior cold L3b: dynobj
    method-call now type-erases through `Any` cleanly).
  - Fixture `async-031-default-any.ts` byte-equal vs bun.

**P10 phase close** (dashboard snapshot `2c3dd77`, 2026-06-03):
A1–A7 all sub-steps shipped. Trigger → unblock P11.

---

### P11 — String Unicode

**Goal**: UTF-16 internal representation, codepoint iteration, full
Unicode case folding.

**Substrate checklist** (strict order):

- [x] **P11.1** Convert byte-Str runtime to hybrid Latin-1 / UTF-16
      (SpiderMonkey-style instance flag bit) — RFC
      `20260603-p11-1-str-unicode-internal/` + S0/S1/S2.1/S2.3/S2.4/
      S2.5(R1+R2+R3)/S5/S6 ship chain (`d6af28e` / `1d8d5b3` /
      `41049d9` / `6e242a2` / `3ab696f` / `9983398` / `804e69c` /
      `751812c` / `c7225fe` / `eaf47ae` / `aba4c78`). conformance gate
      697/0/4.
- [x] **P11.2** `String.length` = UTF-16 code unit count
      (`bc5eb37` fixture pack + `e5d2930` P11.2-A1 Phase 1 honesty
      cleanup: surface assign type-mismatch). Substrate already
      delivered by P11.1-S1 + S5; this step is the audit + multibyte
      fixture pack proving spec-parity.
- [x] **P11.3** `charCodeAt` vs `codePointAt` distinction
      (`10c695d` codePointAt surrogate-pair combine). P11.3-A2 BMP-only
      branch elimination deferred — needs Number type-inference work
      tracked under P12 phase D.
- [x] **P11.4** for-of on string yields codepoints with surrogate
      combining (`fb78e94` for-of on Str yields code-point Substr).
- [x] **P11.5** Full Unicode default case folding per UAX #21 / ES
      Default Case Conversion (`d8354fc` A1+A2 default fold +
      `17770c1` A3 Final_Sigma context-dependent + `bfa6944` A4
      Case_Ignorable skip). Locale tailoring (Turkish / Lithuanian)
      out of scope — separate Intl-flavored surface.
- [x] **P11.6** `String.normalize` NFC / NFD / NFKC / NFKD via UCD
      16.0.0 embedded tables (no libicu / no external crate). RFC
      `.claude/rfcs/20260603-p11-6-string-normalize/`. Ship chain
      `fec7cb4` S1 generator + DECOMP / CCC / COMPOSE / QC bitmaps +
      Hangul algorithmic helpers / `e3c12d0` S2 decompose +
      canonical_order / `35c7001` S3 compose + normalize driver /
      `3262e32` S4 SSA dispatch + FFI seam + RangeError on invalid
      form / `ae7a799` S5 5-fixture pack (NFC/NFD/NFKC/NFKD +
      form-arg). conformance gate 708/0/4 (703 baseline + 5).

**P11 phase close** (`ae7a799`, 2026-06-03): all six sub-steps
shipped (P11.1 hybrid encoding -> P11.6 normalize). L4 trigger
"6 sub-step ship + Unicode fixture pack bun-parity" met -> next
phase = P12 Number IEEE 754.

---

### P12 — Number IEEE 754 conformance

**Goal**: Number.toString / parseFloat / arithmetic match spec
exactly, incl. the long-tail rounding cases.

**Substrate checklist** (strict order):

- [x] **P12.1** `Number::toString` self-ported Ryū (Adams 2018) replaces
      core::fmt::Display Grisu3 delegate (`f4cb3b2` S1 tables + generator /
      `e3ad4db` S2 d2d kernel + intrinsics + 46/46 unit test /
      `655808b` S3 wire into dtoa + drop std::fmt detour). 0 std::fmt
      detour, 0 external dep. Full bun-byte-equal across 24 JS-spec edge
      cases (NaN / Infinity / -0 / Min/Max/MIN_VALUE/MAX_VALUE / 1e-6 /
      1e-7 / 1e20 / 1e21 boundaries / 1/3 / Math.PI / 0.1+0.2 / etc.).
      gate 708/0/4 preserved; perf flat (json-stringify-100k +0.4% noise).
- [x] **P12.2** parseFloat / parseInt edge cases (`be8ca95` fixture lock-in).
      Audit at P12.2 start showed tora already byte-equals bun across 23
      advanced cases (radix sentinel 0 + bounds [2,36], JS-spec exponent
      shape, longest-prefix match, denorm rounding, whitespace + hex
      rejection). Existing parse.rs (parse_int + parse_float) is feature-
      complete; ship is fixture-only to lock parity against future
      refactor regressions.
- [x] **P12.3** IEEE rounding modes for `toFixed` / `toPrecision`
      (`3a1a71a` substrate fix + `6e18f51` fixture lock-in). Two spec-
      conformance bugs fixed in `crates/torajs-num/src/format.rs`:
      (1) toFixed didn't honor ES §22.1.3.32 "|n| >= 10^21 → ToString(n)"
      so `(1e21).toFixed(2)` emitted `"999999999999999868928.00"` instead
      of `"1e+21"`; (2) toPrecision ran a C-subset `strip_trailing_zeros_
      in_frac` post-process that destroyed JS spec §22.1.3.36 precision-
      indicating zeros — `(1.5).toPrecision(3)` → `"1.50"` (was `"1.5"`),
      `(0).toPrecision(3)` → `"0.00"` (was `"0"`),
      `(1e-7).toPrecision(2)` → `"1.0e-7"` (was `"1e-7"`),
      `(100).toPrecision(2)` → `"1.0e+2"` (was `"1e+2"`). 18-case fixture
      added; conformance gate 709/0/4 (was 708, +1 P12.2 +1 P12.3).
- [x] **P12.4** BigInt full operator coverage incl. `**`, mixed-shift,
      spec-conformant overflow
      - Audit at P12.4 start (HEAD `a0e61db`) showed `**` exp / mixed-shift /
        `RangeError` on /0 / `Object("BigInt")` instance methods already
        bun-byte-equal across 27 edge cases.
      - **P12.4-A** toString(radix) per ES §6.1.6.2.13 (`78efe91` substrate
        + `05c2baa` fixture) — refactor tostring.rs to support all radix in
        [2, 36] via `to_string_radix(a, radix)` + new
        `__torajs_bigint_to_string_radix` extern; SSA dispatch arm branches
        on arg count; check.rs sig becomes `(radix?: Any) → String`.
      - **P12.4-B/C** asIntN/asUintN per ES §21.2.2.1/§21.2.2.2 (`3b73546`
        substrate + `d4ee825` fixture) — new `asintn.rs` runtime fns + SSA
        static dispatch + typecheck arm. Fast path covers bits ∈ [0, 64]
        via u64 mask + two's-complement reduction.
      - **L3b deferred** (not blocking close): bits > 64 path for
        asIntN/asUintN (currently throws RangeError; spec wants wider
        masking — no real-world bench/app needs it); mixed-type ops
        (`1n + 1`) static compile-time TypeError vs spec runtime TypeError
        (architectural choice — typechecker strictness vs catchability).

---

### P13 — Module system → v1.0 gate

**Goal**: ESM static analysis, dynamic `import()`, top-level await.

**Substrate checklist** (strict order):

- [x] **P13.1** Static import / export resolution at compile time
      (audit at P13 start showed K.2 cross-file resolver already
      shipped — named import/export including alias path bun-byte-
      equal; expanded in 4 sub-step ship chain below)
- [x] **P13.2** Dynamic `import()` returning Promise (P13-S5 —
      string-literal source only; parser synthesizes
      `import * as __dyn_ns_<n> from "<source>"` + wraps the
      expression as `{ value: __dyn_ns_<n> }` so the canonical
      `await import(...)` pattern reads through the wrapper's
      `value` field per tora's `await` desugar)
- [x] **P13.3** Module-level top-level await (audit at P13 start showed
      `async fn + await` at script top-level already bun-byte-equal —
      no substrate gap; tora's main-as-async-fn wrap handles it)
- [x] **P13.4** Module namespace object (`import * as X`) — P13-S2;
      every value-export injects under its original name and a
      synthetic `let X = { name1: name1, name2: name2, ... }` ObjectLit
      lands after, dispatching member access through struct-field-method
      lookup

**P13 ship chain** (all gates 0 fail, 723 pass / 4 skip at close):
- P13 audit: 9-feature probe surfaced K.1+K.2 + TLA already worked; 4
  K.2 subset boundaries and 1 parser gap remained
- `5fdda8e`-class P13-S1 default import + `export default <expr>`
  (modules.rs check_k2_form lifts default reject; WorkItem carries the
  importer's default alias; lib walk converts the default_expr into a
  synthetic LetDecl)
- P13-S3 side-effect `import "./y"` (modules.rs walks the full lib
  top-stmt list under the `side_effect_only` flag)
- P13-S2 namespace `import * as M from "./y"` (synth ObjectLit lands
  after the named-decl injection; dispatch via struct field type)
- P13-S4 re-export `export { a } from "./b"` (ast.rs ExportDecl gains
  `source`, parser/formatter wired; modules.rs's bare-named-export arm
  splits on source: Some → nested BFS load with transitive alias chain.
  Resolver's visited-set replaced with per-path injected-name tracking
  so the re-export's nested load can revisit a lib for a different
  name)
- P13-S5 dynamic `import()` (parser parse_primary emits the synth
  ImportDecl + the wrapper struct; downstream typecheck + ssa-lower
  reuse the P13-S2 namespace path)

**P13 phase close ≠ v1.0**. Per the axes definition above, v1.0
requires every axis closed (**as written at P13 close there were three;
轴 D was added 2026-06-08 and 轴 E 2026-08-30 — the live gate is the
five-axis one under "v1.0 release gate" below, not this snapshot**):
- 轴 A (spec) — P13 close ✓ (this phase)
- 轴 B (perf, bench-tr 0 regression) — held across the chain ✓
  (3-run compare @ `79865f4` shows 32/32 byte-identical artifacts)
- 轴 C (implementation purity / metal) — **still hot** under the v0.7
  Metal series (A1-A5 closed; A6+ + STONES_AUDIT planned units 3-15
  open, dominated by self-research codegen + obj-writer + linker to
  retire inkwell + llvm-sys)

**L3b deferred** (not blocking phase close): dynamic-import's non-await
shape (`.then()` chain) — would require Promise<struct-with-fn-fields>
typecheck/lowering polish; non-literal source for dynamic import (`tr
build` is AOT so the only viable extension is a build-time URL
whitelist).

### P-SURF — core spec-surface closure (the countable face of v1.0)

**Why this phase exists.** P0–P13 is 84 boxes and, as of 2026-07-26, all
84 are ticked (P5.4 was the last, and it had been silently working since
P6). Yet tr rejects 26476 core test262 cases at the checker. The old
gate — "P0–P13 closed" — is therefore satisfied and simultaneously
meaningless: the checklist enumerated the substrate we set out to build,
not the surface a TS runtime has to present. P-SURF is that surface,
and unlike the trunk above it is **derived from measurement rather than
from design intent**.

**Where the numbers come from.** Full sweep @ `a33f6cb50` (53174
cases, `hardev/test262-latest.json`), then the `incompatible` bucket
dumped per case (`--incompat-ndjson`) and clustered by
`hardev/autorun/cluster_incompat.py`. **The script is the authority** —
every count below is its output for that sweep, and the next sweep
re-derives them mechanically. Treat every number in this section as a
snapshot stamped `@ a33f6cb50`, never as a constant (the two shas this
paragraph used to carry were four rotations stale, which is exactly
what "never a constant" is warning about).

**Latest @ `a33f6cb50`** (2026-08-29, rotation 528 — the property face
528 found nobody reading). Gate predicate: **151** clusters of ≥ 4
holding **1165** cases, register 2 · 251, residue 577 · 736 (34.2%),
core **2152**. Against rotation 527: clusters 156 → **151 (−5)**,
cases 1187 → **1165 (−22)**, core 2184 → **2152 (−32)** — a second
consecutive move, from the same cause as the first: two of this
rotation's five commits remove compile errors. `(new Map() as any).zz
= 1` and `+xs` on a `number[]` were programs bun runs that tr refused
to build, so the cases carrying them sat in the `incompatible` bucket
the P-SURF denominator counts.

Sweep passTotal 35163 → **35213 (+50)**, pass 30032 → **30081 (+49)**,
passNoOracle 984 → **985 (+1)**, passNegative 4147 → **4147 (=)**,
bug 12474 → **12456 (−18)**, incompatible 5537 → **5505 (−32)**,
trAccepted 47637 → **47669 (+32)**; conservation exact
(+32 = +50 + −18). Verdict diff 74 changed: 26 `incompatible:type
error` → `pass`, 21 `bug:exit 1` → `pass`, 2 `bug:exit 3` → `pass`,
1 no-oracle forward, 24 sideways between failing buckets, and
**0 backward**. `built-ins/TypedArray/invoked.js` (registered 524)
is still failing and still correctly so.

Unattributed head by directory: `language/expressions` 630,
`language/statements` 327, `staging/sm` 295, `built-ins/Array` 172,
`language/module-code` 127. Coverage curve: top-100 **52.7%**,
top-200 **70.2%**, top-400 **83.9%**.

**Prior @ `87b7b0d8d`** (2026-08-29, rotation 527 — things with
internal state need a property face too). Gate predicate: **156**
clusters of ≥ 4 holding **1187** cases, register 2 · 251, residue
587 · 746 (34.2%), core **2184**. Against rotation 526: clusters
157 → **156 (−1)**, cases 1196 → **1187 (−9)**, core 2259 → **2184
(−75)** — **the first movement in four rotations**, and the reason is
that two of this rotation's six commits are compile-error removals:
`(new Map() as any).zz` and `"get" in new Map()` were programs bun
runs that tr rejected, so the cases carrying them sat in the
`incompatible` bucket the P-SURF denominator counts.

Sweep passTotal 35032 → **35163 (+131)**, pass 29964 → **30032
(+68)**, passNoOracle 921 → **984 (+63)**, passNegative 4147 →
**4147 (=)**, bug 12530 → **12474 (−56)**, incompatible 5612 →
**5537 (−75)**, trAccepted 47562 → **47637 (+75)**; conservation
exact (+75 = +131 + −56). Verdict diff 139 lines: 63
`incompatible:no-oracle:not yet supported` → `pass-no-oracle` (the
`not-a-constructor` family, which the member-read compile error used
to stop before its first assert — spot-checked against bun, the
substance matches), 60 `bug:exit 1` → `pass`, 9 more forward, 6
sideways between failing buckets, **1 backward**.

**That one backward verdict is de-watering, and is recorded as such.**
`built-ins/Boolean/prototype/valueOf/S15.6.4.3_A2_T3.js` asserts that
`Boolean.prototype.valueOf` transferred onto a Date throws. It used to
pass because the ASSIGNMENT threw — a Date had no property face at all
— which is not what the case is testing. The assignment now succeeds
(correctly), the call runs, and tr answers the Date's time value where
bun brand-check-throws. The case fails honestly and names the gap it
was standing on: transferred builtin prototype methods do not
brand-check their receiver.

Unattributed head by directory: `language/expressions` 660,
`language/statements` 327, `staging/sm` 296, `built-ins/Array` 172,
`language/module-code` 127. Coverage curve: top-100 **51.9%**,
top-200 **69.7%**, top-400 **83.4%**.

**Latest @ `c62c820a2`** (2026-08-29, rotation 526 — the write side
climbed only the links that were written down). Gate predicate:
**157** clusters of ≥ 4 holding **1196** cases, register 2 · 251,
residue 653 · 812 (35.9%), core **2259** — every one unchanged from
rotations 524 and 525. Expected for the third round running: all five
commits landed on the conformance face (gate 3431 → 3435), and the
P-SURF denominator counts the `incompatible` bucket, which has not
moved in three rotations.

Sweep passTotal 35007 → **35032 (+25)**, pass 29939 → **29964 (+25)**,
passNoOracle 921 → **921 (=)**, passNegative 4147 → **4147 (=)**,
bug 12555 → **12530 (−25)**, incompatible 5612 → **5612 (=)**,
trAccepted 47562 → **47562 (=)**; conservation exact (0 = +25 + −25).
Verdict diff 25 lines, every one `bug:exit 1` → `pass`, zero
regressions: five are §20.2.3.6's own directory, the rest are §10.2.4
`restricted-properties` and the `13.2-*-s` family — both faces of one
missing chain consult on the write side.

**The previous rotation's去水分 regression is still there and still
right**: `built-ins/TypedArray/invoked.js` fails because tr has no
`%TypedArray%` intrinsic, which the same sweep names again.

**Ingest correction.** The verdicts file carried into rotation 524 was
one sweep older than the json beside it (pass 29890 / bug 12603 —
rotation 523's), so a verdict-level diff spanned two rotations while
the json-level Δ spanned one. Both artifacts now come from this run.
Against that stale baseline the move is 49 `bug` → `pass`, 1
`incompatible` → `pass`, 1 `pass` → `bug`.

**The one backward verdict is de-watering, and is recorded as such.**
`built-ins/TypedArray/invoked.js` asserts that calling `%TypedArray%`
throws. `testTypedArray.js` obtains it as
`Object.getPrototypeOf(Int8Array)`, and tr has no `%TypedArray%`
intrinsic — it answers `%Function.prototype%`. The case used to pass
because that object was not callable; §20.2.3 says it is, so the case
now fails honestly and names the gap it was always standing on. Three
other backward verdicts appeared in the mid-rotation sweep and were
fixed rather than reported (`46ccd7dd8`).

Two corrections to the previous entry, both from reading the stored
sweep json rather than the handoff prose: its pass / passTotal / bug
were each off by one (29890 / 34958 / 12603 are the recorded values),
and its "unattributed head by directory" listed the **tail** of the
layer-3 section. The real head, this sweep: `language/expressions`
666, `language/statements` 327, `staging/sm` 300, `built-ins/Array`
172, `language/module-code` 127. Coverage curve: top-100 **50.4%**,
top-200 **67.7%**, top-400 **81.0%**.

**Latest @ `4129d778b`** (2026-08-29, rotation 523 — the answers that
were given before the walk began). Gate predicate: **157** clusters of
≥ 4 holding **1197** cases, register 2 · 251, residue 653 · 812
(35.9%), core **2260**. Against the previous sweep (`0fffd3edb`,
rotation 522): clusters 157 → **157 (=)**, cases 1196 → **1197 (+1)**,
core 2259 → **2260 (+1)**. Sweep passTotal 34957 → **34958 (+1)**,
pass 29889 → **29890 (+1)**, passNoOracle 921 → **921 (=)**,
passNegative 4147 → **4147 (=)**, bug 12605 → **12603 (−2)**,
incompatible 5612 → **5613 (+1)**, trAccepted 47562 → **47561 (−1)**;
conservation exact (−1 = +1 + −2). Verdict diff **3 lines**: two
forward (`Array/prototype/map/15.4.4.19-8-b-11` is the map-hole knife,
`Object/prototype/S15.2.4_A1_T2` is the tombstone pair) and **one
pass → `incompatible:tr-timeout`**, `RegExp/S15.10.2_A1_T1`. That one
is **not this rotation's**: it is a regex-compilation stress case that
takes ~22 s solo against the runner's 30 s timeout, so under 10-way
concurrency it sits on the boundary, and an A/B rebuild of rotation
522's sources times it at **22.29 / 22.16 / 22.19 s** against this
rotation's **22.10 / 21.94 / 21.94 s** — the new code is marginally
faster. Registered as a flaky sweep cell, not a regression. Coverage
curve: top-100 **50.4%**, top-200 **67.7%**, top-400 **81.0%**.
Unattributed head by directory: `built-ins/RegExp` 38,
`built-ins/Promise` 25, `built-ins/BigInt` 21, `built-ins/Uint8Array`
20, `language/identifiers` 16.

**Latest @ `0fffd3edb`** (2026-08-29, rotation 522 — a lookup that starts
after the place it should start, in seven spellings). Gate predicate:
**157** clusters of ≥ 4 holding **1196** cases, register 2 · 251,
residue 653 · 812 (35.9%), core **2259**. Against the previous sweep
(`6e075bab9`, rotation 521, which this section skipped): clusters
158 → **157 (−1)**, cases 1201 → **1196 (−5)**, core 2261 → **2259
(−2)**. Sweep passTotal 34951 → **34957 (+6)**, pass 29876 → **29889
(+13)**, passNoOracle 928 → **921 (−7)**, bug 12609 → **12605 (−4)**,
incompatible 5614 → **5612 (−2)**, trAccepted 47560 → **47562 (+2)**;
conservation exact (+2 = +6 + −4). Verdict diff **16 lines, 13 forward
and zero pass regressions**; the other 3 are bucket reclassification
(`bug:no-oracle` → `bug`, bun acquired an oracle this run). The forward
set is the rotation's own two halves: `Symbol.unscopables` × 3,
`match-indices` / `named-groups` × 3 and `staging/sm/Array/unscopables`
are the `.call`-rewrite knife, `Array/prototype/{filter,forEach}/
15.4.4.*-b-11` are the array-hole knife, and both
`class/elements/static-field-declaration` moved off
`incompatible:type error`. Coverage curve: top-100 **50.4%**, top-200
**67.7%**, top-400 **81.0%**. Unattributed head by directory:
`built-ins/RegExp` 38, `built-ins/Promise` 25, `built-ins/BigInt` 21,
`built-ins/Uint8Array` 20, `language/identifiers` 16.

**Latest @ `f87f2ab3d`** (2026-08-28, rotation 519 — the Get that was
skipped, and the doc that explained why skipping it was safe). Gate
predicate: **158** clusters of ≥ 4 holding **1201** cases, register
2 · 251, residue 653 · 810 (35.8%), core **2262** — every one of those
flat, which is what a rotation that moves the `bug` bucket rather than
the `incompatible` bucket looks like: 21 cases went from "runs but
answers wrong" to "answers right", and not one went from "will not run"
to "runs", and only the latter moves a cluster. Sweep passTotal 34929 →
**34950 (+21)**, pass 29854 → **29875 (+21)**, passNoOracle **928 (=)**,
bug 12630 → **12609 (−21)**, incompatible **5615 (=)**, trAccepted
**47559 (=)**; conservation exact (0 = +21 + −21). Verdict diff **21
lines, every one `bug` → `pass`**: 17 are `@@toPrimitive` (each
operator's `S11.x_A2.3_T1`, addition's two symbol-to-primitive cases,
trimStart/trimEnd's four meth-priority cases, and
`String/prototype/concat/S15.5.4.6_A4_T2`), 2 are `Iterator/concat`
(`arguments-checked-in-order` and `get-iterator-method-throws` — the
eager-GetMethod semantics exactly), 1 is
`Array/fromAsync/asyncitems-iterator-throws`. The `@@match` family
knife moved **zero** verdicts: the corpus has no accessor-shaped
`@@match`/`@@replace`/`@@search`/`@@split` case, so its evidence is
probes and fixtures, recorded as such. Zero pass regressions, zero
other verdict movement. Coverage curve unchanged: top-100 50.3%,
top-400 81.0%. Unattributed head by directory: `built-ins/Date` 60,
`built-ins/Object` 59, `language/import` 42, `built-ins/Function` 40,
`built-ins/Iterator` 38, `built-ins/RegExp` 36.

**Latest @ `85a64a7ba`** (2026-08-28, rotation 515 — an argument the
program never passed pins no type parameter, and a call the checker
could not infer says so itself). Gate predicate: **158** clusters of
≥ 4 holding **1201** cases, register 2 · 251, residue 653 · 810
(35.8%), core **2262** — the cluster count is flat across rotations
503-515 while the cases inside them keep coming down. Sweep passTotal
34862 → **34863 (+1)**, pass 29787 → **29788 (+1)**, bug 12702 →
**12696 (−6)**, incompatible 5610 → **5615 (+5)**, trAccepted 47564 →
**47559 (−5)**; conservation exact (−5 = +1 + −6). Verdict diff **17
lines**: `bug:exit 3 → incompatible:type error` **6** and
`incompatible:not yet supported → incompatible:type error` **5** are
the same change — a generic call the checker could not infer used to
reach the lowerer and panic on the callee's NAME, and now answers with
the checker's own verdict; `incompatible:not yet supported → pass`
**1**; the remaining 5 are crash fault-kind churn (138 ↔ 139) with the
crash count flat at **32**. Zero pass regressions, zero new crashes.
The rotation also located the largest crash family — 11 of those 32
are one file name across 11 syntactic positions, one root, and their
OUTPUT is already correct — but the fix for it was reverted (the
discriminator it used was unsound; see
`.claude/tasks/2026-08-28/async-gen-forawait-uaf-r515.md`), so the
count does not move here.

**Latest @ `4bd64984a`** (2026-08-27, rotation 511 — the slot's parameter
list has to carry its own defaults and its own tail, because the call
site will not carry them for it. Rotation 510 left one shape refused on
purpose: a class method's default is supplied by the CALL SITE, and
widening a slot is exactly what makes its owners disagree, so a
defaulted row would have answered NaN. The handoff blamed a pass that
never looks inside `Stmt::Multi`; a five-line diagnostic printed
`__cm_B__f` on its first line — the method IS at top level, and the real
cause was one layer over, a parser rule that gives a method-position
defaulted parameter an annotation inferred from its literal, which put
it outside the only lane that moves a default into a body. So it was
never the whole-corpus size change that estimate implied: the default
now moves into the row as an `if (y === undefined) y = 5` guard, and
because the widening question is asked FIRST, a hierarchy whose rows
already agree keeps its narrow parameters and its call-site defaults —
three artifact probes byte-identical to the rotation before. The
parameter keeps a default, now the `undefined` literal, and that is not
bookkeeping: it is what evicts the method NAME from the by-name pad
table when an unrelated class declares the same name with a real default
of its own; clearing it outright let a neighbour's `q = 3` be pasted into
this row's call sites, which the fifteenth probe caught and the first
fourteen did not. A rest tail joins the same way when the rows' fixed
arities agree — `class A { f(x) }` overridden by `f(x, ...r)` did not
compile at all before. When they DISAGREE the rows unpack the same
argument list two different ways, and the ABI check downstream cannot
see it: a defaulted scalar and a rest array are both one word, so
`f(x, y = 5)` beside `f(x, ...r)` filled the same three registers,
passed every gate, and died at exit 139 with its output truncated
mid-run. That now refuses out loud, while the parameter lists are still
legible. And one lane over, a spread was being counted toward a callee's
required prefix: `g(...xs)` on `function g(x, ...r)` answered `unknown
identifier g` about a function plainly in scope, because the gate that
decides whether a static expander will take the site said "covered" and
stood aside for an expander that then also declined.) Gate predicate:
**158** clusters of ≥ 4 holding **1209** cases, register 2 · 251,
residue 652 · 809 (35.7%), core **2269** — clusters identical to
rotations 503-510. Sweep passTotal **34841 (=)**, pass **29766 (=)**,
bug **12717 (=)**, incompatible **5616 (=)**, trAccepted 47558 (=);
the verdict diff is **zero lines**. That is honest rather than
disappointing: the `call/spread` family's seven non-passing cases are
all object-spread (`{...obj}`) shapes, and the named-function-with-rest
site this rotation fixed is not among them — test262 reaches that shape
only through callee forms tr already handled. Zero pass regressions,
zero new timeouts or crashes; build determinism 44/44 (N=12), Guard
Malloc 3030/3032 clean with 0 true hits.

**Prior @ `e0b4688bf`** (2026-08-27, rotation 510 — what a call through
a vtable slot actually hands the body. Rotation 509 refused
`class A { f(x: number) }` overridden by `f(x: number, y: number)`
outright, and withdrew the fix for it because the fix answered wrongly
instead: two rows, two ABIs, one slot. The withdrawal was right, and
the reason turns out to be two defects underneath, both on the slot's
call sites and both older than the attempt. Every other call lane pads
an omitted argument with `undefined` before it emits; the slot lane and
the sibling-class lane sent exactly as many operands as the source
wrote, so the parameter the call did not fill was read out of whatever
the caller had left in that register — `x.f(1)` on `f(x, y)` printing
`object null` for one row and a garbage any-tag for the next, and no
output at all when two were missing. And every other lane converts its
arguments against the callee's signature first; the slot lane did not,
so an i64 reaching an unannotated (`any`) parameter arrived as raw bits
and `class A { f(x) { return x } }` called through the slot with `2`
answered `null` — the sibling lane's own S2.42, one lane over, five
rotations late. With both fixed the parameter join lands: as wide as
the widest row, `any` where the rows spell different types, and byte
for byte identical for every program whose rows already agreed. One
shape stayed refused on purpose — a class method's default is supplied
by the call site, and widening a slot is what makes its owners disagree,
so a defaulted row would have answered NaN where the language owes it 6;
the honest answer is the loud refusal until a class method's default
lives in its body the way a plain function's already does (510-02).
Rotation 509's one pass regression is also closed: a type parameter
given `X` and then a `Y` that extends it was two answers to tr and one
to TS, and the join now consults the class parent map.) Gate predicate:
**158** clusters of ≥ 4 holding **1209** cases, register 2 · 251,
residue 652 · 809 (35.7%), core **2269** — clusters identical to
rotations 503-509. Sweep passTotal **34841 (+1)**, pass **29766 (+1)**,
bug **12717 (=)**, incompatible **5616 (−1)**, trAccepted 47558 (+1);
conservation holds (+1 = +1 + 0) and the verdict diff is **one line**:
`super-reference-resolution.js` incompatible → **pass**. Zero pass
regressions, zero new timeouts or crashes.

**Prior @ `90030c182`** (2026-08-27, rotation 509 — a slot-agreement
rotation, closing the three things rotation 508's new vtable assertion
found once someone was finally checking. A slot's return type is now
the join of its rows: a base that answers a value and an override that
does not are not in conflict about the language — falling out of a body
is `return undefined` — so `any` is the spelling that holds both, and
the common shape that had become a compile-time refusal runs again
(`superPropProtoChanges.js` incompatible → pass). A prototype level
lists only the methods its class declares: the class-methods table
merges parent chains on purpose, because it IS the dispatch resolution,
and reifying it wholesale made every inherited method an own property
of every subclass prototype — a phantom name for `hasOwnProperty` and
`getOwnPropertyNames`, and a shadow that outlived re-linking the chain.
And the `__dispatch_<M>` stub is a row of its slot like any other: it
took its return annotation from the base owner's declaration, read
before the unannotated bodies were given theirs, so for an ordinary
program it disagreed with every row it forwards to — `ww` against the
row's `wf`, the same read-the-wrong-register shape rotation 507
measured, on the one face the assertion could not see. Making it adopt
what the rows say took a two-class program's artifact from **35,121 to
18,609 bytes**. One attempt is registered rather than shipped: making
the any-lane method call follow a re-linked prototype needs the chain
arm to become a complete resolution first — forcing its gate flag true
answers 3316 / 50 fail, almost all generators.) Gate predicate:
**158** clusters of ≥ 4 holding **1209** cases, register 2 · 251,
residue 653 · 810 (35.7%), core **2270** — clusters identical to
rotations 503-508. Sweep passTotal **34840 (=)**, pass **29765 (=)**,
bug **12717 (+1)**, incompatible **5617 (−1)**, trAccepted 47557 (+1);
conservation holds (+1 = 0 + 1) and the verdict diff is 3 lines. Net
pass movement zero, honestly one gain and one loss: `super`'s
`superPropProtoChanges.js` becomes pass, and
`super-reference-resolution.js` moves pass → incompatible because the
stub's now-concrete return type meets a type-parameter inference that
demands equality where TS takes a common supertype (logged 509-03; the
runtime answer is still correct).

**Prior @ `fc6c22c4d`** (2026-08-27, rotation 508 — a name-resolution
rotation. `super.x` resolves `[[HomeObject]].[[Prototype]]`, which is a
runtime value, and the class desugar spelled it statically as the
parent: all five forms — name call, computed call, name read, computed
read, getter — answered from where the chain used to go once a program
re-linked a prototype. The static spelling is what makes `super.m()` a
direct call, so it is kept behind one whole-program judgment (a program
that never spells `setPrototypeOf` or `__proto__` cannot invalidate
it). A class whose builtin heritage was stripped was spelling its base
`Object.prototype`, so `super["has"](x)` on a `Set` subclass answered
"not a function"; and only `Iterator` was wired for the §15.7.14
prototype link, so `Object.getPrototypeOf(MySet.prototype) ===
Set.prototype` answered false for every other builtin. On the emitter
side, the vtable now proves one-slot-one-ABI before it is emitted —
per hierarchy root, machine shape, measured silent on the whole case
corpus and loud on the rotation-507 pair when its union-find is
disabled — and an empty forwarding block is threaded away instead of
emitted as a `b` nothing jumps to.) Gate predicate: **158** clusters of
≥ 4 holding **1209** cases, register 2 · 251, residue 653 · 811
(35.7%), core **2271** — clusters identical to rotations 503-507. Sweep
passTotal **34840 (+1)**, pass **29765 (+1)**, bug **12716 (−2)**,
incompatible **5618 (+1)**, trAccepted 47556 (−1); verdict diff 3 lines,
zero pass regressions. Two of those three are cases the new vtable
assertion now refuses loudly rather than running with the wrong ABI —
an override that returns nothing sharing a slot with a base that
returns a value, previously silent, logged as 508-06.

**Prior @ `80de8cea4`** (2026-08-27, rotation 506 — an artifact-mass
and link-soundness rotation: vtable slots became relative offsets
(`fn - table`, the LLVM / Swift relative-vtables shape) so the table
needs no dyld fixup and leaves `__DATA_CONST` — an override program
51,657 → 35,121; two release-only silent-wrongs the conformance gate
is blind to were fixed (a link-judged stub read one name of an atom
that MergeFunctions had given two, stubbing the closure props drop
under every static-`this` class program; float_demote's post-op growth
guard on a block-ending add was never installed, so a sum past 2^53
stayed exact where bun's rounds) and the release-artifact sample that
caught them became `hardev/autorun/release_batch.sh`; a counted
accumulator's exit bridge now folds and prints through the i64
printer — `total += i; console.log(total)` 34,937 → 18,425, the
empty-program floor.) Gate predicate: **158** clusters of ≥ 4 holding
**1210** cases, register 2 · 251, residue 652 · 809 (35.6%), core
**2270** — identical to rotations 503-505. Sweep passTotal **34840
(0)**, bug **12717 (0)**, incompatible **5617 (0)**, trAccepted 47557
(0); verdict diff empty.

**Prior @ `3da78bcf7`** (2026-08-26, rotation 500 — an artifact-mass
rotation on the S2-5 刀 4 line, Phase A + three knives. Phase A
(`.claude/rfcs/20260824-s2-5-selective-registration/blade4-phase-a.md`)
overturned the handoff's framing: the dispatcher's 15 runtime-internal
in-edges are the cluster's internal topology, and the whole any world
of an ordinary program hangs off three speculative typed→any edges —
the closure `__boxed_` adapter's ToNumber, the typed join kernel's
exotic-index branch, and the species guard's props probe — any two of
which price at zero and all three at −85% live text. Knife 1 put the
join / species slow paths behind link seams judged by the text
liveness of the arr crate's two `#[inline(never)]` writer entries
(`Guard::Symbols`): a join-only program 348,649 → **84,537**. Knife 2
judged closure escape by SSA operand type and was reverted the same
rotation (gate 3325/14/4 — closures are `Ptr`-typed at the crossing
sites; redo direction recorded). Knife 3 gave scalar-kind arrays a
drop kernel with no element walk and no cycle hook, its props /
subclass legs behind seams: an array-only program 84,537 → **51,569**;
the reach pass now records which user symbols live atoms import so an
unreferenced stub is never emitted, and the landing pad's messages
were shortened out of `__TEXT` — the empty program stays on the r499
floor, **34,961**.) Gate predicate: **159** clusters of ≥ 4 holding
**1225** cases, register 2 · 251, residue 652 · 810 (35.4%), core
**2286** — identical to rotations 498-499. Sweep passTotal **34818
(0)**, bug **12723 (0)**, incompatible **5633 (0)**, trAccepted 47541
(0); verdict diff empty — every shipped knife touched runtime text
and artifact layout only.

**Prior @ `d6735a63e`** (2026-08-26, rotation 499 — an artifact-mass
rotation on the S2 line, link-judged conditional shapes: the synthesized
`main`'s three end-of-program drains become `ElidableCall` sites and the
rc-hit-zero weak-observer hook a `GuardedStub`, each decided by a
fix-point over member-text liveness with the shape assumed; the forced
table globals derive from what the stripped archives still reference
so an empty `__DATA_CONST` no longer ships. Empty program 84,449 →
**34,961 (−59%)**: `__TEXT` 3 pages → 1 (text 43,110 → 13,653),
`__DATA` 180,224 → 16,384, `__DATA_CONST` gone. One gate-caught
regression — the load-command sizing tested five tables for
`__DATA_CONST`, the layout seven, invisible while every table was
forced on — fixed in-rotation by a single shared predicate.) Gate
predicate: **159** clusters of ≥ 4 holding **1225** cases, register
2 · 251, residue 652 · 810 (35.4%), core **2286** — all identical to
rotation 498. Sweep passTotal **34818 (0)**, bug **12723 (0)**,
incompatible **5633 (0)**, trAccepted 47541 (0); verdict diff empty —
every knife touched dead runtime text and artifact layout only.

**Prior @ `2e25039c4`** (2026-08-26, rotation 498 — an artifact-mass
rotation on the S2 line: the empty-program census showed LC_SYMTAB +
strtab at 63% of the 273,665-byte release artifact, so `tr build` now
strips the runtime members' symbols by default (ld64 `-x` class;
`--no-strip` keeps them, `tr run` keeps them), `__text` packs right
after the load commands, the `_main` wrapper's argv-init call is
emitted only when a `__torajs_process_*` reloc can observe it, and
user fns get an ld64-style dead-strip whose address-taken roots are
collected from the materialized LinkConfig tables (one gate-caught
regression — the class-method table's boxed adapters — fixed
in-rotation). Empty program 273,665 → **84,481 (−69%)**; roadmap S3's
"hello-world in the hundred-KB range" is met). Gate predicate:
**159** clusters of ≥ 4 holding **1225** cases, register 2 · 251,
residue 652 · 810 (35.4%), core **2286** (was 2287). Sweep passTotal
34798 → **34818 (+20)**, bug **12723 (−19)**, incompatible **5633
(−1)**, trAccepted +1, conservation exact (+1 = +20 − 19), zero pass
regressions — the 20 forward moves are rotation 497's 18
`_main_user` cases returning (empty-statement programs now
synthesize main), one RegExp timeout → pass, and
param-duplicated-non-strict back to pass-no-oracle.

**Prior @ `80e2f265a`** (2026-08-24, rotation 487 — the generic
array-like receiver sweep: HOF thisArg over a single-write `var arr`
receiver (the knife-4 arraylit census admits mutable-never-rewritten
bindings; struct_data_field_set gains the I64←integer-F64 arm),
ToObject end to end for string/bool/number primitives and wrapper
cells (the callback's O answers `instanceof`, wrapper own-miss reads
walk wrapper-prototype → %Object.prototype%), arraylike_len reads
the struct +24 expando on a layout miss (Error receivers), and
function values join the generic receiver set (checker + lowering +
Closure scan arm). One gate-caught regression (bool/number mint
shadowing the prototype lane) reverted in-rotation. Gate predicate:
**159** clusters of ≥ 4 holding **1225** cases, register 2 · 251,
residue 652 · 810 (35.4%), core **2286** (was 2310). Sweep passTotal
34699 → **34795 (+96)**, bug **12746 (−72)**, incompatible **5633
(−24)**, trAccepted +24, conservation exact (+24 = +96 − 72), zero
pass regressions — the 96 forward moves are the Array HOF applied-to
family (13 each every/filter/forEach/map/some, 12+11 reduce family,
3+3 indexOf/lastIndexOf) plus two census-widening rides (fromAsync,
gOPD).

**Prior @ `41a66d4ee`** (2026-08-24, rotation 484 — a perf rotation:
takagi set the v1 target at 3-4x vs bun, the S7 abstraction-gap track
opened with a Phase-A decomposition, and four codegen knives landed
(immediate-form shifts / scaled addressing / select-form blade 3
retirement / STR-XZR zero stores); bench median tr/bun-aot 0.618 →
**0.593**. The spec surface moved only by rotation 483's own
harness-shake fix (`d19e896b9`, committed after that rotation's sweep):
9 staging/sm cases — 3 to pass, 4 into scope as bugs, 2 rewordings —
zero pass regressions). Gate predicate: **161** clusters of ≥ 4 holding
**1248** cases, register 2 · 251, residue 653 · 811 (35.1%), core
**2310** (was 2313). Sweep passTotal 34696 → **34699 (+3)**, bug
**12818 (+4)**, incompatible **5657 (−7)**, trAccepted +7,
conservation exact (+7 = +3 + 4).

**Prior @ `db6e2a063`** (2026-08-23, rotation 483 — the buffer
family joins the exotic-subclass table (one shared mint + one shared
super kernel with a transplant onto the minted cell; a fixed
three-slot forward is the exact derived default ctor); the
resizableArrayBufferUtils.js harness port (the `new Function` probe
statically declassed, 223 resizable cases enter scope); the Array
read/iterator/mutator families made intentionally generic over
TypedArray receivers (OOB view answers length 0 / per-index
undefined where the typed twins ValidateTypedArray-throw); an
eight-site index-assign UAF fix (the assignment-expression stake
taken BEFORE the store — a consuming receiver released the
transferred pair inside the kernel and the old inc-after-call ran on
a freed cell); and `get ArrayBuffer.prototype.resizable`'s reified
accessor face. Gate predicate: **161** clusters of ≥ 4 holding
**1253** cases, register 2 · 251, residue 651 · 809 (35.0%), core
**2313** (was 2399 — first drop below 2400; the resizable harness
keys left the ledger). Sweep passTotal 34493 → **34696 (+203)**, bug
**12814 (−20)**, incompatible **5664 (−183)**, trAccepted +183,
conservation exact (+183 = +203 − 20); TypedArray/prototype 81 +
Array/prototype 73 are the head, then class 23 / ArrayBuffer 7.
**2 sweep regressions** (staging/sm cases self-declaring `var ctors`
vs the port's top-level `const ctors` — redeclaration): fixed the
same rotation (`d19e896b9`, the shake yields to a case's own
top-level declaration), targeted re-runs green.

**Prior @ `0835c8931`** (2026-08-23, rotation 482 — DataView whole:
the cell + ctor + accessors + the 22 get*/set* methods on two
endianness-parameterized kernels; the TypedArray species CONSTRUCT
channel for filter / map / slice / subarray (products really swap,
§23.2.4.2 validation real, filter's read after the callback loop per
its own step order); and the buffer family answering getPrototypeOf
(per-kind slots, ArrayBuffer 19, DataView 32 — no %TypedArray%
intermediate yet, iterator-proto shape)). Gate predicate: **162**
clusters of ≥ 4 holding **1339** cases (both flat — the harvest sits
in the bug bucket), register 2 · 251, residue 650 · 809 (33.7%), core
**2399** (flat). Sweep passTotal 33987 → **34493 (+506)**, bug 13335 →
**12834 (−501)**, incompatible −5, trAccepted +5, conservation exact
(+5 = +506 − 501); **DataView's 377 prototype cases are the head**,
then 50 TypedArray prototype (species-construct family), 41 ctors, 6
ArrayBuffer. Zero pass regressions in the closing sweep (the four
filter callbackfn-called-before cases regressed mid-rotation and were
fixed the same rotation — filter is the one family method whose
species read follows the callback loop), zero new crashes or timeouts.

**Previous @ `bce8fd9c0`** (2026-08-23, rotation 481 — own properties on
a view: the buffer family's lazy expando bag across all nine
own-property surfaces, both halves of the @@species read, the
canonical-numeric-key element face, and the harness shake gated on
declared includes). Gate predicate: **162** clusters of ≥ 4 holding
**1339** cases (both flat), register 2 · 251, residue 650 · 809
(33.7%), core **2399** (was 2400). Sweep passTotal 33835 → **33987
(+152)**, bug 13485 → **13335 (−150)**, incompatible −2, trAccepted +2,
conservation exact (+2 = +152 − 150); **218 verdict moves: 170
bug→pass** (the speciesctor family across filter / map / slice /
subarray, the DefineOwnProperty / Set internals now running real
§10.4.5.3/.5 semantics, and HasProperty / GetOwnProperty over the new
own face), **18 attributed pass regressions — every one the IsHTMLDDA
family** (`$262.IsHTMLDDA` host face): their old passes were lexical
water — the harness shake used to mis-inject the detachArrayBuffer
port's `$262` object, and the case happened to assert true against its
missing field. The include-gate fix withdrew it; the family is a host
surface tr does not carry (L3b). Zero new crashes or timeouts.

**Previous @ `469743816`** (2026-08-23, rotation 480 — the typed-array
substrate's iteration slab plus `transfer`, and three ownership fixes
the crashes led to). Gate predicate: **162** clusters of ≥ 4 holding
**1339** cases (was 163 · 1348), register 2 · 251, residue 651 · 810
(33.8%), core **2400** (was 2408). Sweep passTotal 33399 → **33835
(+436)**, bug −129, incompatible 6161 → **5854 (−307)**, trAccepted
+307, conservation exact (+307 = +436 − 129); **654 verdict moves,
5 attributed pass regressions (4 = @@species debt now that filter/map
exist; 1 = harness-shake lexical over-include on the helper's own
self-test), zero new crashes or timeouts**. The single key that moved
most of it: `detachArrayBuffer.js` ported over §25.1.6.7 `transfer`
(no host hook — both engines have the spec method), which released
326 cases split roughly half `→ pass`, half `→ bug`.

**Previous @ `7977d5e80`** (2026-08-22, rotation 474 — a name a program
writes to needs somewhere to put it). The gate predicate moves for the
first time in three rotations: **163** clusters of ≥ 4 holding **1351**
cases (was 165 · 1379), register 2 · 259, residue 657 · 824 (33.9%),
core **2434** (was 2475). Sweep passTotal 32352 → **32376 (+24)**
(pass +22, passNoOracle +2, passNegative flat), incompatible 7984 →
**7943 (−41)**, trAccepted 45190 → **45231 (+41)**, conservation
exact; **50 verdict moves, zero pass regressions, zero new crashes or
timeouts**. Half the movement is the `incompatible` bucket emptying,
which is what this section counts.

Three declarations turned out to be lying about what their binding
was. A `function` declaration creates a plain mutable binding
(§14.1.23), so `f = 123` is an ordinary PutValue — tr typed the name
from the declaration and refused, and a write of another *function*
died a layer lower because a declaration is a direct-call symbol with
no slot at all. An array a program `delete`s indices out of was never
a `number[]`: an unboxed slot has no value that means absent, so the
declaration widens to `any[]`. And §23.1.3 has seven callback methods
ask HasProperty before each visit, which tr never did —
`[1, <hole>, 3].some(v => v === undefined)` answered true where bun
says false, and `filter(() => true)` answered three elements where bun
says two.

The last of those is also this rotation's perf lesson: gating each
visit cost **+34%** on a dense 3M-element probe, and hoisting the test
to the loop preheader only bought 29% — the branch is not the price,
splitting a one-block loop body is. It is now emitted only for an
`any`-element source, where a hole can exist at all, and the typed
probe compiles **byte-identical** to the pre-gate build.

Two pre-existing silent-wrong bugs surfaced while probing the above and
are closed: a `for` loop filling an `any[]` by `push` wrote the
PRE-LOOP length back at the exit and erased everything it had appended
(the pre-reserve fast path claimed a binding the push site never
serves), and both `defineProperty` gap-fills created own `undefined`s
where §10.4.2.1 creates nothing — on the shared `Array.prototype` that
made index 0 an own property of every array in the program.

**@ `efb6541b3`** (2026-08-23, rotation 476 — a SuperProperty is
not a read followed by a call). Gate predicate **164** clusters of
≥ 4 holding **1346** cases, register 2 · 257, residue 651 · 812
(33.6%), core **2415**; coverage top-10 14.4% / top-25 25.1% /
top-100 52.2%. Sweep passTotal 32375 → **32387 (+12)**, pass 27322 →
**27334 (+12)**, bug 12855 → **12863 (+8)**, incompatible 7944 →
**7924 (−20)**, trAccepted +20, conservation exact; **22 verdict
moves, zero pass regressions**. §13.3.6 invokes a Super Reference
with the CURRENT `this`, and every spelling that was not a
statically-declared name had been reading off the base instead —
loudly for the computed call, silently for the read. One kernel
(`__torajs_anyv_super_prop_{get,set,call}`) over the receiver-aware
[[Get]] / [[Set]] the Reflect lane already had now serves the class
and object-literal sides alike. Two of the twelve are unrelated: the
timeouts rotation 475 put on watch came back on their own.

**@ `6fcf5becc`** (2026-08-22, rotation 473 — one encoding
question asked in five places, four of which had stopped answering
it). Nothing here moved P-SURF: the gate predicate is **165**
clusters of ≥ 4 holding **1379** cases, register 2 · 274, residue
656 · 822 (33.2%), core **2475** — every number identical to
rotation 465's, because every fix this rotation landed in the `bug`
bucket rather than the `incompatible` one. Sweep passTotal 32346 →
**32352 (+6)**, bug 12844 → **12838 (−6)**, trAccepted flat,
conservation exact; **10 verdict moves, zero pass regressions**, and
four of the ten are crashes downgrading to ordinary failures
(exit 139 → 1). The common root is P11.1-S2: a Str payload became
Latin-1 or UTF-16 LE with `length` counting code units, and five
places kept reading it as a UTF-8 byte tape with `length` as a byte
count. The Any-lane `+` fast path packed Latin-1 bytes into a
UTF-8 ShortStr (crash, and the one the two `parseInt` / `parseFloat`
cases were sitting on); the JSON builder copied Latin-1 payload into
a buffer another producer had already made UTF-8; both JSON parse
drivers read half of a UTF-16 source and answered SyntaxError on the
first character. The `incompatible` face is untouched by all of it,
which is the point of counting the two buckets apart.

**@ `92ea7337`** (2026-08-21, rotation 465 — the loud reject was
standing in front of a different, already-wrong thing). The rotation
opened on the cluster the previous handoff named, `fnexpr this in
unclaimed receiver position` (25 cases across 6 directories), and the
same one-probe method turned it over: reduce the cluster's own starting
case and what is left is `var x = function () { … this … }; x = 1;` —
a WRITE to the binding, which the zero-alias parity read as an unsafe
use. But asking what a write costs led somewhere else entirely. On
HEAD, with no `this` anywhere in the program, `var x = function () {
return 1 }; x = function (a) { return a }; x(7)` answered **NaN**: the
bare indirect call is shaped by the slot's first face, so the second
function read an argument register the caller never filled. The
mismatch census that exists to prevent exactly that (424-04) had three
independent coverage holes — it walked `let` but not `var`, resolved a
store rhs as a bare Ident but not as a function LITERAL (which
`lift_arrow_fns` has already turned into a closure naming a lifted
FnDecl, and which the forwarder pass's own signature snapshot
deliberately drops), and compared stores against the slot's ANNOTATION,
so a slot that never spelled one had nothing to compare. All three
produced silent garbage; all three are closed. Only then does the
receiver half become decidable: an assign TARGET is an lvalue, the
cleanest member of the never-calls family since it has no reader at
all, and such a binding promotes with every call on a receiver-aware
lane rather than the static seed. That shipped first for `any` slots
(the restriction was measured, not assumed — the NaN above is what
admitting a typed slot would have bought), then for typed slots once
the last blocker was named: two stored functions can share a user face
and still differ in whether they carry the receiver slot, and the
census that routes odd slots to the boxed dual entry runs BEFORE this
pass decides who is promoted, so it structurally cannot see that
difference. The promoter knows what the census could not and its
consumer reads the set far later, so it registers the binding itself.
Sweep passTotal **32341** (0), every bucket unchanged; **2 verdict
moves, zero pass regressions** — both are the cluster's named starting
case (`ctorExpr-fn-ref-before-args-eval*.js`) advancing from
`incompatible:not yet supported` to `incompatible:type error`, which
names the next layer precisely: assigning a non-function to a fn-typed
slot is a type error in tr where JS simply rebinds. Gate predicate:
**165** clusters of ≥ 4 (0) holding **1379** cases (−2), register
2 · 274, residue 656 · 822 (33.2%), core **2475** (0). Comparators
re-baselined against **bun 1.4.0** (two full rounds at this HEAD,
agreeing within 1% on every self-normalized reading): against rust —
the one comparator whose toolchain and sources did not move — tr is
flat (×0.9940 / ×1.0025) while bun gained ~13% (×0.8734 / ×0.8721), so
`tr/bun-aot` moved ×1.30 because the opponent advanced. tr still leads
39 of 44 (median 0.635, geometric mean 0.620); **four cases lose in
both rounds and all four led before** — regex-dfa-dotall 1.132,
json-stringify 1.106, regex-dfa-iflag 1.096, split-only 1.033.

**@ `c86cb0d0`** (2026-08-21, rotation 464 — four static
shortcuts standing in front of a runtime that already answered
correctly). The rotation's method was one probe: route the same code
through an `any`-typed alias and see whether the runtime gets it right.
Three of the four times it did, which says the kernel is fine and a
compile-time shortcut is guessing — a different repair entirely.
`charCodeAt` out of range answered 0 on the typed tier and NaN on the
Any tier, a divergence recorded in the kernel doc as a known deviation
rather than fixed; §22.1.3.2 step 5 wants NaN, which is not an integer,
so both kernels move to an f64 ABI and their in-range halves split out
for the callers that carry their own out-of-range answer. The half that
mattered was `num_width`: a `let c: number` slot is i64 unless
something widens it, and a too-narrow slot here does not run slow, it
fails to build — `rpn-eval-100k` stopped compiling on the kernel change
alone, which is how the bench found it. A missing member on an
ECMAScript global (`Math.nope`, `Reflect.enumerate`, `Array.myproperty
= 1`) was a compile stop rather than the ordinary extensible object's
undefined, across 22 cases in 7 directories; the checker's terminal-miss
posture, the `Math` / `Number` constant lowering's panic, and `typeof`'s
guess of "function" were three separate shortcuts on the same path, the
last already wrong before the rotation (`typeof console.nope`). Writes
are gated harder than reads and admit only unmodeled names, because
admitting `Math.max = fn` while the static call sites keep running the
builtin is the silent-wrong faucet rotation 448 named; tr's own host
handles (`Bun` / `process` / `fs`) stay a loud reject on purpose, so
our modeling holes keep showing up in this bucket. A builtin reached
through a binding (`const f = String.fromCharCode; f("65")`) was
shape-checked against a signature that describes what the spec coerces
to, not what it demands. And `String.raw` existed only as a call: one
table row gave it `.name`, `.length`, its descriptors, and
not-a-constructor, all of which resolve off the carried id.
Sweep passTotal **32341** (+17), pass 27290 (+16), passNoOracle 904
(+1), bug 12849 (+9), incompatible **7984** (−26), trAccepted 45190
(+26 — conservation exact). 28 verdict moves, **zero pass regressions**:
15 `incompatible:type error → pass`, 1 → `pass-no-oracle`, 1
`bug → pass`, and 10 `incompatible → bug` that now compile and fail
further in. Gate predicate: **165** clusters of ≥ 4 (−2) holding
**1381** cases (−25), register 2 · 274, residue 655 · 820 (33.1%),
core **2475** (−25). Bench A/B interleaved same-HEAD on the two cases
that read `charCodeAt`: no detectable regression (same-round
tr/bun-aot ratio, rpn 0.6590 both sides, csv 0.6330 → 0.6345 with every
fixed reading inside the clean spread). *Rotation 463 shipped without
adding its own entry here; its numbers are in that rotation's handoff.*

**Previous @ `65cc2e6c`** (2026-08-21, rotation 462 — three coercion
steps that had been implemented as shape gates, plus the discovery that
the gate's own oracle had gone stale). `Symbol(desc)` admitted only
String / Undefined, so `Symbol(1)` — how test262 mints a throwaway
symbol — was a compile error reaching 45 cases across 9 directories;
§20.4.1.1 step 3 is `? ToString(description)` and the gate is gone
entirely, the lowering taking the implicit ToString face so a Symbol
description still throws the catchable TypeError the spec asks for.
`Reflect.set` accepted a fourth argument and ignored it, getting three
of four observable answers wrong; `Reflect.get` did not compile at all
on an `any` target, which is the shape the function exists for. Both
now ride receiver-aware [[Get]] / [[Set]] kernels — the only place in
the language where the object whose property table decides and the
object that receives can differ — and the split cost almost nothing
because the dynobj chain walk already carried a receiver parameter for
the ordinary inherited-accessor case. Two member reads that mint their
answer (`sym.description`, `f.name`) were leaking a cell per read, found
by re-measuring memory on a lane that had just been given new inputs.
Sweep passTotal **32274** (+43), pass 27224 (+43), bug 12843 (+8),
incompatible **8057** (−51), trAccepted 45117 (+51 — conservation
exact). Gate predicate: **174** clusters of ≥ 4 (−1) holding **1458**
cases (−21), register 2 · 276, residue 650 · 813 (31.9%), core **2547**
(−27). One pass regression, `class/dstr/meth-obj-ptrn-rest-getter`
pass → exit 139, proved by an inert-perturbation experiment to be a
LATENT layout-sensitive null-deref rather than a defect of this
rotation: the r461 baseline plus a behaviourally empty two-function
module crashes at the same 10/40 rate. Registered, not reverted.

**Previous @ `7756c8c0`** (2026-08-21, rotation 459 — Promise combinators
over an arbitrary constructor `this`, RFC
20260820-combinator-any-constructor, plus the implicit-generic value
positions that blocked it. §27.2.4.{1,3,5,6} name |this| as the species
constructor C and ask only IsConstructor: step 1 is
NewPromiseCapability(C) and everything after it reaches C through Call
and Invoke alone. tr's receiver arm admitted only the builtin Promise
and classes reaching it, and its heir shortcut could not serve an
arbitrary C even with the gate open — it hands the element walk to the
typed fan-in kernels, which mint a builtin promise and only then
resolve it into the capability, so a user resolve function is handed
that promise where the algorithm hands it the combined value, and a
bare thenable element never has its own `then` invoked with the
capability's functions. `combinator_spec` writes the four algorithms
out instead. Three orderings are load-bearing and sit where the spec
puts them: the undefined placeholder is appended before the element is
resolved (4.c), the counter is incremented between
Call(promiseResolve, …) and Invoke(…, "then", …) (4.m), and the walk's
own hold comes off only once the iterator is drained (4.b, which is
what resolves an empty iterable). [[AlreadyCalled]] lives in the shared
record keyed by index, not on the cell — allSettled's pair shares ONE
record. The element cells declare no traceable captures (N cells behind
one record would let a trial deletion subtract N times for one edge),
a boundary noted in the module doc. Prerequisite: an implicit-generic
decl has no lowered original, so `typeof f` answered `undefined`
silently and `f.prop = v` died loud; both value positions now ride the
canonical forwarder cell, with `name` / `length` kept on their static
arms because the shim has lost the parameter defaults. The test262
harness also gained `Test262Error.thrower`, which real sta.js has and
ours did not — 36 cases were handing `undefined` to code as a callback.
Sweep vs 458: passTotal 32132 → **32199 (+67)**, all of it in the
oracle-matched bucket (passNoOracle +0), bug −2, incompatible −65,
trAccepted +65, conservation ✓. 73 forward moves: allSettled 24 / all
18 / any 16 / race 8. Six cases left `pass`, and the evidence says
water rather than regression: our `assert.throws` drops the class
argument, and `tr ssa` shows the TypeError for a class declared inside
a function body is raised from `main()` (its static accessor define is
hoisted to module init) as a bare string — the old pass was that throw
being picked up by whichever check came first inside the thunk. Both
that hoist and a `parseInt(str, obj)` SIGSEGV the harness change
exposed are logged. Gate 3192 → **3196/0/4**. Guard Malloc caught a
real use-after-free in this rotation's own new fixture — an element
cell freed without retiring from the cycle buffer — which the gate is
structurally blind to. Unattributed ≥4 clusters 182 → **177**, cases
1574 → **1509**, core 2680 → **2615**.)

**@ `8c84e873`** (2026-08-20, rotation 458 — constructor
return-override, RFC 20260820-ctor-return-override, five blades. A
ctor returning an object now makes `new C(o)` answer THAT object and
carries the class's own elements onto it (§10.2.2 step 13 / §7.3.28),
and a DERIVED ctor answering a non-object throws (step 13.c). Blade 1
names the narrow set that needs the answering ABI — seed on classes
whose ctor carries a value return, spread down to descendants, then up
to ancestors; the two spreads do not compose into a fixed point and
their order is load-bearing (widening first pulls in every cousin).
Blade 3 gives a member ctor `__this_in` copied into an ordinary local,
appends `return __this`, and lets one step-13 pick at the factory map
`return e` / bare `return` / fall-through — no second body walk. The
mint stays TYPED (an `any` let would send the literal down the dynobj
lane and cost the class tag and vtable); only the parameter and the
factory's answer widen. Three ownership facts, each read off the SSA
and each hiding the next: `x = <call>` retains, `let x = <call>` takes
over, `f(g())` releases nothing — so the pick is borrow-shaped,
consumed by assignment at both sites, and the parent's answer lands in
a `__sup` local first. Blade 5 needed `ast_throw_info` told that the
pick raises, or the ctor / factory / `new` all pruned their checks and
the caller read a stranded zero back as an unknown any tag. Sweep vs
457: passTotal 32117 → **32132 (+15)**, bug +4, incompatible −19,
trAccepted +19, conservation ✓. **Zero pass regressions** — all five
bug arrivals came from `incompatible:type error`, i.e. cases tr used
to refuse outright now compile and show their own assertion. 11
`derived-class-return-override-*` cases plus
`privatefieldset-evaluation-order-3` turn pass. Gate 3191 →
**3192/0/4**. Unattributed ≥4 clusters 183 → **182**, cases 1584 →
**1574**, core 2699 → **2680**.)

**@ `a11daeb5`** (2026-08-20, rotation 457 — the
"unsupported member call shape: call" cluster (31 cases) + the
cpn-class-*-yield generator side quest, RFC
20260820-member-call-route, five knives. Knife 1: an inline
class-instance method's `.call/.apply/.bind` rides the any-dispatch
lane both ends (checker `is_class_method_read` Any admit ahead of
the static replay; lowering `class_method_value` joins the
non-Any-receiver admit set — the S2.34 method-value read is a
runtime any cell, so the detached-binding kernel already knew how).
Knife 2: private WRITES gate on the brand before installing
(`__torajs_any_member_set_priv`, statically selected by the
`__priv_` prefix; undeclared brand throws instead of minting an
expando — the read twin existed since r297, and the directed run
showed all 10 residual bugs were setter/write shapes). Knife 3:
`invoke_with_this` routes primitive AND nullish thisValues through
the `__cmany_` twin — the spec TypeError belongs to the member
read/write site, not the call boundary. Knives 4+5 (recon agent
report in `.claude/tasks/2026-08-20/gen-class-computed-key-recon.md`):
the generator lift now walks class computed keys in their side
table, pins this-reading keys against the hoist, annotates
generator-local class instances `any`, and bridges the class
binding itself into a state-machine field (`this.C = C` post-
rewrite) so cross-arm reads survive. Directed: access-on-inner
32/32; the 31-case cluster → 25 pass, residue = eval-private ×4 /
class-expr cpn ×4 / staging ×1 + independents, all logged. Sweep
vs 456: passTotal 32066 → **32117 (+51)**, bug −16, incompatible
−35, trAccepted +35, conservation ✓. **One pass regression**,
attributed: `privatefieldset-evaluation-order-3` — its old pass
was inflation (the base set kernel installed an expando on the
foreign receiver and read it back, two wrongs cancelling); knife 2
removed the silent half, exposing the real gap = constructor
return-override (§10.2.2 — `return o` from a base ctor must brand
the returned object), logged in the recon file. New crash/timeout
0. Gate 3185 → **3191/0/4** across five commits (+5 fixtures).
Unattributed ≥4 clusters 184 → **183**, cases 1616 → **1584**,
core 2734 → **2699**.)

**@ `84a557dc`** (2026-08-20, rotation 456 — RFC
20260820-dstr-deferred-close knife D: rest-form target-first
sequencing. The bounded walk parks a three-state resume value
(derived-iterator cell / boxed-int resume index for the builtin
indexed lane / undefined = drained-to-done — mutually exclusive,
and the pending close no-ops on a non-cell), a prefix-0 stop runs
GetIterator before the pattern can suspend, and the rest slot
expands as recovered-yield → lref hoist → park take + clear →
`target = __torajs_dstr_drain_rest(taken, raw)` — the statement
order itself carries §7.4.6: an lref throw leaves the park intact
(close fires), a mid-drain next() throw finds the slot cleared
(thrw-close-skip). The checker pins a deferred-rest group to the
iterator lane regardless of static source type. One red gate
mid-chain (ArrayLit source): the generator lift's already-lifted-
alias sniff outranks the `__dstra_src_` any fallback, so both temps
now pin `any` explicitly. Sweep vs 455: passTotal 32054 → **32066
(+12)**, bug **−12**, incompatible ±0, conservation ✓, **zero pass
regressions** (24 diff rows = exactly the rest basic/err/null ×
plain/trlg × assignment/for-of target set). Gate 3183 → **3185/0/4**
across five commits (+2 fixtures). Remaining: per-element lazy
sequencing (the 2 elem-family rtrn-close-err stragglers — spec
evaluates an AssignmentElement's lref before its IteratorStep),
declaration-form deferred close, nested-pattern inner iterators.)

**@ `862ad5fb`** (2026-08-20, rotation 455 — deferred
IteratorClose for suspendable destructuring patterns, RFC
20260820-dstr-deferred-close. Knife 1: generator-lifted destructure
group temps step the iterator protocol (the lift's field-store had
bypassed both ends of the RFC-20260714 iter lane — every index read
answered undefined, a silent-wrong for any non-Array iterable inside
a generator body); rest patterns stay gated (eager drain would hang
where the spec suspends, probe-proven). Knife 2 (gate-caught):
§13.15.2 init-position identity — `var r = ([x] = vals)` reads the
RHS reference back through a chain-style hoist, not the pattern's
group temp. Knife 3: §7.4.6 step 9 — a close whose return() answers
a non-Object throws TypeError, both close tiers. Knife 4 (the core):
a pattern containing a yield defers its IteratorClose — the desugar
wraps the element statements in the engine-canonical try/catch/
finally, the walk PARKS the still-open iterator (`__dstra_it_<id>`,
local slot or lifted field) instead of closing, and the finally's
`__torajs_dstr_close_pending` settles it; gen.return() routes
through the D3b finally region, which is what runs the close at the
spec's moment. Knife 5: the objlit boxed-only argv census stops
refusing fields whose next/return/throw spelling only appears off
generator-instance bindings (every value source a registered
generator factory call). Sweep vs 454: passTotal 32029 → **32054
(+25)**, bug **−25**, incompatible ±0, conservation 0 = +25 − 25 ✓,
**zero pass regressions** (50 diff rows all forward: rtrn-close +
nrml-close-null across assignment/for-of dstr, 4 for-await siblings,
iterator-close-non-object). Gate 3179 → **3183/0/4** across five
substrate commits (+4 fixtures; one red gate mid-chain caught and
fixed same-day). Remaining in the RFC: rest-form target-first
sequencing, declaration-form deferred close, nested-pattern inner
iterators.)

**@ `b317112e`** (2026-08-20, rotation 454 — the test262
dstr-yield family's expression form, computed-name yields, and the
return-escaped `arguments` face. Knife 1: the generator-expression
hoist sees through `var iter, x;` multi-name declarations
(Stmt::Multi) — top-level names were never captures, so the whole
"reassigned capture" refusal for the t262 template shape dissolves;
the real capture-box boundary (a written LOCAL capture) still
refuses. Knife 2 (a gate-caught correction of knife 1's second
half): the dstr-assignment src temp's `any` belongs at the
field-annotation sniff's FALLBACK, not as a parser-side pin — a
sniffable ArrayLit source keeps its typed lane (the pin had broken
the conditional-default guard), and r453's nested-point pin retires
into the same fallback. Knife 3: a free-ident generator field init
(the `var vals = value;` t262 aliasing idiom) falls back to `any`
instead of number. Knife 4: the lifted-local → `this.<name>` rewrite
walks the objlit_computed_keys side table, so `{ [yield]: v }` /
computed accessor pairs work. Knife 5 (survey-driven): fn-exprs
escaping through RETURN join the arguments argv face (fifth arm on
admit_boxed_only), and argv-face members publish the checker's
rest-tail spelling (`__fn(__rest(any[]))->R`) instead of leaking
`__argvptr()` into inferred sigs — the r452 a1/a3 repros and
Promise `invoke-resolve-get-once-multiple-calls` ×2 go green.
Sweep vs 453: passTotal 32006 → **32029 (+23)**, bug **+12**,
incompatible **−35**, conservation +35 = +23 + 12 ✓, **zero pass
regressions** (37 diff rows all forward; 12 rtrn-close cases
compile now and fail honestly on the missing IteratorClose
semantics). Gate 3176 → **3179/0/4** across five substrate commits
(+3 fixtures; one red gate mid-chain caught and fixed same-day).
The gate predicate: **184 unattributed clusters / 1616 cases /
register 2 · 278 (SR-1 190, SR-2 92) / residue 670 · 840 / core
2734** — both headline numbers down together (−4 clusters, −32
cases).)

**Prior @ `5fc545b9`** (2026-08-20, rotation 453 — the
`not callable: type Object("Function")` cluster's constant-text half
and the whole yield-in-destructuring parse face. Knife 1: annexB
§B.1.3 HTML-like comments under a new script-goal lexer entry
(`tokenize_script`; `<!--` anywhere, `-->` on a token-free line via
fresh_line tracking) — the eval / dynamic-Function text channel
rides it, tr's module-shaped main source keeps refusing. Knife 2:
dynamic-function text that fails to parse throws SyntaxError at the
call (§20.2.1.1 steps 11/17, the posture the eval channel already
took for §19.2.1.1 step 12) instead of keeping the call site and
failing the whole compile; a strict-prologue labelled
FunctionDeclaration is the §B.3.2 creation-time early error. Knife 3
(after a pure move of the yield-hoist events family): a yield in a
destructuring-assignment DEFAULT recovers its YieldInto from the
hoist buffer and re-emits under a statement-level undefined-guard
(§13.15.5.3 step 4's conditional evaluation) — array-elem / object
field / computed key lanes, plus for-of and async-gen faces for
free. Knife 4: a yield in the TARGET (`[x[yield]] = v`) re-emits at
its slot (§13.15.5.3 step 1); nested patterns let their leaves
recover. The mid-rotation sweep caught knife 4's regression pair
(nested src temp lifted to a number-fallback field — "can't index
into Number") and the fix annotates generator nested-pattern sources
`any`. Sweep vs 452: passTotal 31980 → **32006 (+26)**, bug **+9**,
incompatible **−35**, conservation +35 = +26 + 9 ✓, **zero pass
regressions** (61 diff rows all forward, 0 new crash/timeout). Gate
3172 → **3176/0/4** across five substrate commits (+4 fixtures).
The gate predicate: **188 unattributed clusters / 1648 cases /
register 2 · 282 (SR-1 194, SR-2 92) / residue 670 · 839 / core
2769** — cluster count flat, cases −35.)

**Prior @ `76673ad9`** (2026-08-20, rotation 452 — GetPromiseResolve
lands whole: heir, builtin, and the checker face. Knife 1 (handoff
451's instruction #1): the inherited combinators
(`Promise.all.call(CP, xs)`) reorder to spec —
NewPromiseCapability(C) first, then GetPromiseResolve(C)
(Get(C, "resolve") through the any-lane read, IsCallable gate,
abrupt settles the capability per IfAbruptRejectPromise), then the
new `__torajs_promise_combinator_dyn_c` kernel entry runs
`Call(promiseResolve, C, «v»)` per element on dyn_iter's new heir
channel — every-iteration-of-custom went 5/5. Knife 2: the
checker's `Array<non-{Promise,Any}>` combinator reject predates
rotation 449's dyn routing — the arm now answers `Promise<Any>`
(step 6.i resolve-wraps any element), unlocking the bare
`Promise.all([1,2,3])` spelling and the every-iteration-of-promise
family. Knife 3: the builtin lane runs ONE REAL Get before
iteration (`__torajs_promise_ctor_get_static`: accessor getters
invoke with this = the ctor cell, get-once counted, abrupt rejects
before GetIterator), both dyn and sync detours ride it, and the
per-element re-reading `call_patched` retired with no caller.
Knife 4: the await-dictionary keyed pair joins the ns-static value
surface (recv-first rows, `.length`/`.name`/proto/ctx-non-ctor).
Knife 5: bare `p.then()` / `p.catch()` — §27.2.5.4's absent
handlers — ride a new passthrough kernel (mint pending + repr
stamp + adopt, has-handler marked); the then checker's 500-line
watch tripped on the wiring and `try_then_undefined` moved to its
own sibling (r310's recorded cut). Sweep vs 451: passTotal 31925 →
**31980 (+55)**, bug **−5**, incompatible **−50**, conservation
+50 = +55 − 5 ✓, **zero pass regressions** (verdict diff 69 rows
all forward). New exposure: 14 combinator thenable-observation
cases (reject-deferred / resolve-poisoned-then families)
incompatible → bug — knife 2 compiles them now and they surface
the recorded PromiseResolveThenableJob gap. Gate 3167 →
**3172/0/4** across five substrate commits + one fixture-collision
fix (+5 fixtures net). The gate predicate: **188 unattributed
clusters / 1683 cases / register 2 · 282 (SR-1 194, SR-2 92) /
residue 671 · 839 / core 2804** — clusters and cases down
together.)

**Prior @ `74c35f75`** (2026-08-19, rotation 449 — handoff 448's
instruction #1 plus all three alternates landed in five knives, and
a probe-chase rooted out a real SIGSEGV. Knife 1: the dyn combinator
lane (`combinator_iter::dyn_iter`) probes the patch once at entry
(the spec's single GetPromiseResolve fetch) and runs
`Call(promiseResolve, C, «v»)` per element INSIDE the iteration —
an abrupt (or non-promise answer) closes the iterator per §27.2.4.1
step 8.a; the never-activated exit of a patched run delegates
straight to the `*_sync_any` sibling so the sync entry cannot probe
again. The 448 timeout ×4 family is gone. Knife 2 (alternate ①): a
fn-expression stored into `Promise.resolve`/`.reject` whose body
touches `arguments` joins the boxed-face store profile
(`boxed_face_store_target` admits the two consulted names,
As-peeled, shadow-gated) — both read-back channels are the boxed
dual entry with real argc/argv, so the argv face is exactly right.
Knife 3 (probe): `Promise.all([1,2] as any)` — a typed value-kind
array reaching the sync kernels through the as-cast road — read raw
scalar slots as promise pointers (silent-forever-pending unpatched;
rc_inc(0x1) SIGSEGV patched, crash-stack proven). The aggregate
lowering now routes plain-element arrays through the dyn entries
(boxing stamps the elem kind). Knife 4 (alternate ③):
`RegExp.prototype.compile` joins the any method lane — mid 177
threads the four append-only tables and the receiver arm hands the
existing in-place kernel its two AnyValues. Knife 5 (alternate ②):
the detached `Promise.resolve`/`.reject` cells ride the
`this_aware_id` recv-first channel — `r.apply(Promise, [v])`,
`r.call(Promise, v)` and the member spelling run the real settle
(new `__torajs_promise_reject_any` mirror kernel); the bare call
keeps the step-1 TypeError, and the ctor-static dynamic-key invoke
now passes the member base as thisValue per §13.3.6. Sweep vs 448:
passTotal 31897 → **31913 (+16)**, bug **−12**, incompatible
**−4**, conservation +4 = +16 − 12 ✓. Forward: annexB compile
family ×13 → pass, invoke-resolve ×3 → pass, invoke-resolve-error-
close ×4 tr-timeout → 1 pass + 3 bug:no-oracle:exit 1,
sm/RegExp/flags-param-handling → pass. Two compile-family cases
moved pass → bug:exit 1 (`pattern-string-invalid-u`,
`this-subclass-instance`) — **de-watering, not regression**: the
harness's `assert.throws` rewrite drops the constructor arg, so the
old "pass" was a wrong-typed TypeError from the pre-compile
not-callable path; the real call now exposes the kernel's lax
u-mode parse (`{` under `u` must be a SyntaxError) — an upstream
tr-regex strictness gap newly reachable, recorded. Gate 3148 →
**3155/0/4** across five substrate commits. The gate predicate:
**192 unattributed clusters / 1716 cases / register 2 · 282 /
residue 681 · 856 / core 2854**.)

**Prior @ `502ce156`** (2026-08-19, rotation 448 — the Promise
static-slot monkey-patch family landed in five knives, handoff 447's
instruction #1 off the survey's knife plan. Ground truth correction
first: a bare `Promise` in value position answers
`builtin_ctor_cell(10)` — a Closure-shape immortal cell whose
expando dict (CLOSURE_PROPS_OFF) is the patch's single store — not
the injected-class registry the survey had guessed. Knife 1: every
static `Promise.resolve` / `Promise.reject` call site now emits a
patch probe first (§13.3.6.1 callee GetValue order; two loads and a
compare when unpatched) and branches to the any-method dispatcher
against the boxed ctor cell when patched, with an Any→Promise
return contract (a non-promise answer is a LOUD TypeError — the
typed slot cannot hold it). Knife 2: the checker accepts
`Promise.resolve = fn` / `.reject = fn` (only the two consulted
names — a write nobody reads back would be a silent-wrong faucet).
Knife 3 (the fix the probe forced): the async desugar spells its
internal settles as `Promise.resolve(e)`, which must NOT see a user
patch (§27.7.5.2 works the capability directly) — a synthesized-call
side table + a monomorph clone carry routes them straight to the
typed lane. Knife 4: all four combinator kernels gate on the probe
and detour to a collect-then-delegate lane that runs
`Call(promiseResolve, C, «v»)` per element (this = the ctor cell,
abrupt = IfAbruptRejectPromise). Knife 5: `Promise.resolve =
function(){this}` joins the store-position fnexpr-this face, so
`this === Promise` holds on both read-back channels. Sweep vs 447:
passTotal 31891 → **31897 (+6)**, bug **+8**, incompatible **−14**,
conservation +14 = +6 + 8 ✓, **zero pass regressions**; forward:
`resolve-non-callable` ×4 → pass, keyed `resolve-not-callable` ×2 →
pass-no-oracle, invoke-resolve ×3 → bug:exit 3 (now blocked on
`arguments`-in-fnexpr + detached-builtin `.apply` thisArg, both
probed loud), every-iteration-of-custom ×5 → bug:exit 1. New
exposure: `invoke-resolve-error-close` ×4 incompatible →
**tr-timeout** — an infinite iterator + poisoned resolve expects a
first-element close, but the dyn lane collects before the sync
entry's consult; the fix (per-element consult inside
`combinator_iter::dyn_iter`'s loop) is next rotation's instruction.
Gate 3142 → **3148/0/4** across five substrate commits. The gate
predicate: **192 unattributed clusters / 1720 cases / register 2 ·
282 / residue 681 · 856 / core 2858**.)

**Prior @ `71df1ed8`** (2026-08-19, rotation 447 — the replace
family closed out in five knives. Handoff 446's instruction #1
candidate ①: the runtime REPLACE arm's regex-cell branch still
ToString'd a functional replaceValue (source-text splicing, the
pre-existing silent-wrong residue) — rooted out by generalizing the
regex replace walk over two invoke strategies and adding
`__torajs_str_replace_regex_fn_boxed`, which reads **n_caps off the
RegExp cell's own `n_captures`** and drives the closure's boxed
entry (spec-full «matched, p1..pn, position, string» argv, real
undefined for non-participating groups, RECV_FIRST shift). That
unblocked the face widening: inline and variable-routed
`new String(...)` receivers (a syntactically-certain strwrapper
census over both post-var-hoist decl spellings) and the
`[Symbol.replace]` protocol spelling all joined the replace cb-slot
face. Fifth knife: annexB §B.2.4.1 `RegExp.prototype.compile` —
checker arm + lowering + an in-place kernel that swaps the freshly
compiled body under the receiver's header (throws propagate before
the receiver changes, pinning lastIndex; rejected parses record the
same catchable SyntaxError as `new RegExp`). Sweep vs 446: passTotal
31880 → **31891 (+11)**, bug **−3**, incompatible **−8**,
conservation +8 = +11 − 3 ✓, **zero pass regressions, zero new
timeouts/crashes**; the Symbol.replace bug family (fn-coerce ×2 /
fn-err / poisoned-stdlib, all exit-1) flipped to pass as a side
effect of the boxed kernel. Gate 3137 → **3142/0/4** across five
substrate commits (+5 fixtures). fnexpr-this unclaimed 37 → **33**;
one forward re-shape: Symbol.replace fn-invoke-this-no-strict moved
incompatible → bug (sloppy-this residue now runs). Gate predicate
**189 unattributed clusters / 1733 cases / register 2 · 282 (SR-1
194, SR-2 92) / residue 684 · 857 / core 2872**. Next attackable:
the Promise monkey-patch family (6-7, needs builtin static-slot
override semantics — survey first), class-field-init arrow `this`
(sm ×2), tagged-template call-expression-context ×2, any-receiver
`.compile` (mid wiring, loud today).)

**Prior @ `a50b07bf`** (2026-08-19, rotation 446 — the fnexpr-this
unclaimed scatter, three claim knives plus two cross-cutting root
causes. Handoff 445's instruction #1 (45 unclaimed-receiver cases,
scattered dirs) opened with shape claims: computed-key object-literal
fn-expr VALUES join the method receiver channel (both computed value
paths — folded literal key and runtime `__computed_N__` sentinel —
were missing the RFC-20260725 marking the name-keyed path had), and
the fn-expr RETURN face widened from the flattened class-member synth
names to every top-level FnDecl (fail-safe on the reject line).
Probing the dominant `var x = function () { …this… }` spelling then
exposed root cause one: the injected Error-family builtins flatten to
`__cm_/__sm_` FnDecls whose params carried bare user-spellable names
(`message` / `options` / `x` / `errors`), and every program-wide
by-name census saw a same-named user binding as shadowed — renamed to
`__bi_<name>` (param names are unobservable), making every census
immune to injected code at the root. Root cause two: the widened
return face made §22.1.3.19 step 5's IsCallable a runtime question —
the static replace lanes' checker-type dispatch would have turned the
rotation-375 loud reject into source-text splicing — so both the
static lane (`__torajs_str_replace_any_repl`) and the runtime
any-dispatch REPLACE arm (`__torajs_str_any_replace_fn`, closure-cell
probe over owned_src glue) now branch on the cell tag; the
variable-routed replacer joined the cb-slot face on top. Sweep vs
445: passTotal 31873 → **31880 (+7)**, bug ±0, incompatible **−7**,
conservation +7 = +7 + 0 ✓, **zero pass regressions, zero new
timeouts**. Gate 3131 → **3137/0/4** across six substrate commits
(+6 fixtures). fnexpr-this unclaimed 45 → **37**. Gate predicate
**190 unattributed clusters / 1743 cases / register 2 · 283 (SR-1
195, SR-2 92) / residue 683 · 854 / core 2880**. Next attackable:
the unclaimed residue's Promise monkey-patch family (7-8, needs
builtin static-slot override semantics), regex-pattern + closure
replaceValue n_caps threading (unblocks the replace face's any-recv
widening), class-field-init arrow `this` (sm ×2).)

**Prior @ `18fd74ef`** (2026-08-19, rotation 445 — the not-callable
cluster taken apart by callee semantics, six knives. Handoff 444's
instruction #1 (not-callable `Object("X")`, 65 grep-total) split
four ways: A (13, closed-set builtins — no-[[Call]] namespaces and
new-only constructors — now arena-rewrite to a throw-IIFE with
§13.3.6.2 argument sequencing, goal-triage family fifth member), D
(4, `Date()` without new returns the current-time string, args
evaluated then discarded with no ToPrimitive), B (~12, value-typed
callees — `true()` / `f.length()` / `/re/()` — admitted by the
checker via the single-source `Type::is_uncallable_value` predicate
and routed by a lowering wedge through the any-call kernel, whose
non-closure arm already raises the catchable TypeError), C (34,
`Function(...)` dynamic compile — deep water, unchanged). The D
recon exposed a real bug: `Date.parse` could not read its own
`toString` output (§21.4.3.2 round-trip) — fixed with a
toString/toUTCString fallback parser feeding both `Date.parse` and
`new Date(string)`. Then two member-side siblings: nullish
receivers (`undefined.toString()` / `null.constructor`) ride the
any-member/any-method lanes to the runtime TypeError (terminal-miss
nullish posture), and object-rest bindings (`{a, ...rest}`;
`rest.a`) ride the runtime [[Get]] probe past their static anchor
(new `ast.obj_rest_names` side-table). Sweep vs 444: passTotal
31836 → **31873 (+37)**, bug +25 (honest incompat→bug motion — the
rest-val-obj family now runs and fails on later asserts),
incompatible **−62**, trAccepted +62 (conservation +62 = +37 + 25
✓), **zero pass regressions**. Gate 3125 → **3131/0/4** across six
substrate commits (+6 fixtures). Gate predicate **190 unattributed
clusters / 1751 cases / register 2 · 283 (SR-1 195, SR-2 92) /
residue 682 · 853 / core 2887**. Next attackable: fnexpr-this
unclaimed (45, scattered dirs), `.compile` annexB RegExp (4),
String.raw meta-properties (3); the not-callable residue is the
`Function(...)` dynamic-compile deep water.)

**Prior @ `322e0287`** (2026-08-19, rotation 444 — the catch
destructure port and the goal-triage family growth. Handoff 443's
instruction #1 (catch destructure default, measured 73 cases across
six signatures) fell to one structural move: the catch parameter's
ad-hoc flat destructure walk was replaced by the shared PatShape
machine (`read_pattern_shape` + `emit_pattern_binds`), so defaults /
nesting / rest / elision / alias-to-pattern all work in a catch head
exactly as in `let PAT = src` (net −31 lines). The recon first
surfaced a deeper substrate bug: a `throw` is a boxing boundary, and
a thrown typed array crossed into the catch `any` unmarked —
`catch (e) { e[0] }` read `undefined` from every slot while
`e.length` answered fine (one-line fix: the refcounted throw arm now
emits `arr_mark_kind`). Then the goal-triage family grew two members:
sloppy implicit globals (§9.1.1.4.6 — a never-declared assignment
target synthesizes a hoisted `var`, with a positional strict-body
tree walk, delete-name / with-program / contextual-keyword
exclusions, and the `slot_type_supported` `__`-sentinel whitelist so
sputnik's `__x` user names promote to data globals), and the
non-writable builtin-namespace properties (§21.1.2 / §20.2.2 /
§21.3.1 — `Number.NaN = 1` folds sloppy, throws strict). A for-in
head over a statically non-struct source (`for (k in undefined)`)
now rides the kernel keys arm instead of the struct-arm panic. The
mid-rotation sweep caught 16 regressions from the first
implicit-globals cut (strict-prologue bodies, deleted names, `with`,
and the `let a, b;` Multi-arm blind spot in
collect_local_binding_names — a whole-family fix); the final sweep
is clean. Sweep vs 443: passTotal 31654 → **31836 (+182)**, bug −87,
incompatible **−95**, trAccepted +95 (conservation +95 = +182 − 87
✓), **zero pass regressions** in the final case-level diff. Gate
3119 → **3125/0/4** across six substrate commits (+6 fixtures; a
seventh commit is a behavior-equal fn-cap refactor). Gate predicate
**194 unattributed clusters / 1793 cases / register 2 · 288 (SR-1
200, SR-2 92) / residue 689 · 865 / core 2946**. Next largest
attackable: not-callable Object("X") (50, dirs=17), fnexpr-this
unclaimed (42, dirs=13); #7 dstr-assignment-default-yield (32) was
reconned RFC-grade — conditional-position yield needs the
state-machine CPS face, not a parser fix.)

**Prior @ `330a056e`** (2026-08-19, rotation 443 — the strict-shell
harness port and the eval-text early-error knives. Handoff 442's
instruction #1 (`sm/non262-strict-shell.js`, 35 cases) turned out to
be unportable as a harness function — the stock helpers eval a
RUNTIME string, which tr's literal-only eval desugar cannot meet — so
the runner grew a call-site EXPANSION (strict_shell.rs): every
`testLenientAndStrict('code', L, S)` becomes the literal eval /
Function IIFE pair the desugar resolves, strict predicate first, the
code literal re-embedded raw; a computed code string keeps the
attributable reject (3 of 35). The recon surfaced three substrate
bugs the port then rode on: a strict-code early error inside
eval/Function literal TEXT — `delete <bare name>`, then
assign/update targeting `eval`/`arguments` — bubbled up as a
whole-program compile refusal where §19.2.1.1 step 12 wants a
catchable runtime SyntaxError (parse_eval_source grew a DeleteSites
gate, then a Result refusal split NoParse/StrictEarlyError, then a
Mode::StrictOwned arena walk so a nested 'use strict' prologue arms
its body inside a sloppy Function text); and writes to the
non-writable globals (NaN/Infinity/undefined) got their §6.2.5.6
runtime semantics per goal (strict = throw-IIFE at the site, `.cts` =
fold-to-rhs). The MID-ROTATION sweep then caught what six green
gates could not: the eval inline's early sloppy fold broke
`eval("with(o){delete p}")` (§14.11 property reference — the goal
triage's own ordering bug re-created inside inlined text), and the
`void <literal>` fold let `void 1 = 1` reach the readonly-globals
rewrite (parse-phase SyntaxError became runtime) — both repaired
in-rotation (Parser::void_folds + `.cts` defers every delete site to
the triage). Sweep vs 442: passTotal 31641 → **31654 (+13)**, bug
+16, incompatible **−29**, trAccepted +29 (conservation +29 = +13 +
16 ✓), **zero pass regressions** in the final case-level diff.
Forward 13: readonly-globals 6, eval-assignment 4, strict-shell 3.
Gate 3114 → **3119/0/4** across five substrate commits (+5
fixtures). Gate predicate **202 unattributed clusters / 1872 cases /
register 2 · 303 (SR-1 215, SR-2 92) / residue 690 · 866 / core
3041**. Next largest attackable: fnexpr-this unclaimed (43,
dirs=13), field-assignment-on-Object("X") (37, Promise 30 needing
patched-resolve, RFC-grade), catch destructure default (32, dirs=2,
parser lane).)

**Prior @ `f7bab07f`** (2026-08-19, rotation 440 — the computed-key
destructuring + objlit-super + decl-init dstr-assign knives. Handoff
439's instruction #1 opened top cluster #4: `{ [expr]: binding }`
(§14.3.3 ComputedPropertyName) landed across all three destructuring
lanes — the declaration PatShape machine grew a FieldKey enum, the
param walker moved to destr_obj_param.rs and grew a computed-field
method, and the assignment lane un-folds the objlit cover's
`__computed_N__` sentinel through objlit_computed_keys (that lane
used to parse and then silently assign nothing — an existing
silent-wrong now closed). One shared recipe: the key hoists into a
`__ck_N` temp in field order, the load is an any-key index read;
computed-key-alongside-rest rejects loud in every lane (the omit-set
protocol carries static names only). Then §10.2.4 [[HomeObject]] super
reads in object-literal methods: a new pass right after
desugar_classes claims the declaration-init shape (home binding
pre-declares mutable-undefined and assigns back — the closure capture
is a box), each `__superbase__` read marker rewrites in place to
`Object.getPrototypeOf(__home_N)` re-minted per site; supercalls,
writes and nested homes stay loud (the `__superbase__` cluster fell
49 → 25). Then cluster #5's real gap: `var y = { p: x } = src`
(§13.15.2 — a destructuring assignment as a declaration INIT) expands
through the shared desugar with the binding reading back the hoisted
RHS temp. The sweep then caught what five green gates could not: the
init expansion let keyword SHORTHANDS (`var x = { default } = src`)
through to runtime — 37 syntax-error-ident-ref cases flipped
pass-negative → negative-phase-mismatch — fixed in-rotation with a
§12.7.2 ReservedWord table in emit_dstr_assign_slot (contextual
keywords stay valid). Sweep vs 439: passTotal 31343 → **31430
(+87)**, bug +27, incompatible **−114**, trAccepted +114
(conservation +114 = +87 + 27 ✓), **zero pass regressions** in the
final case-level diff (the 37 mid-rotation regressions were caught
and repaired before close). Forward 87: class 24, super 9,
for-await-of 8, object 7, for-of 5, async-generator 4, import-defer
4, the rest spread across the dstr template families. Gate 3099 →
**3104/0/4** across five substrate commits (+5 fixtures). Gate
predicate **214 unattributed clusters / 2192 cases / register 2 ·
309 (SR-1 221, SR-2 92) / residue 683 · 858 / core 3359**. Next
largest attackable: deepEqual.js harness port (45, dirs=13) and the
class computed-property-name accessor family (40, `index assignment
target must be an array, got ClassRef`).)

**Prior @ `177a2709`** (2026-08-19, rotation 439 — the NewDynamic
spread + FnSig-box + String.raw knives. Handoff 438's instruction #1
took three cuts: a `__torajs_anyv_construct_spread` kernel (with_argv
over the fixed-argc construct path) with the call cascade's
`build_args_arr` reused verbatim on the lowering side and the checker
walking a spread argument's SOURCE; then the arguments object grew
both faces for the new shapes — the inline NewDynamic callee joins
the static-argv face (the IIFE knife's twin), and a spread-carrying
site poisons the static face (args.len() cannot count a spread — the
pre-fix census silently mis-slotted every value after the spread, an
existing silent-wrong now closed) while its anon closure joins the
argv face instead, because the spread lowering already routes every
such call through the boxed dual entry. The FnSig-box residue fell to
one axis extension: a Call / NewDynamic CALLEE is the r290 indirect
shape one level up (`f()(fn)`, the tagged
argument-list-evaluation idiom), and an inline Closure callee's
un-annotated params ride the Any lanes — both now wrap top-fn
arguments (explicit fn-type annotations keep raw-FnSig dispatch).
String.raw's direct-call kernel read `raw.length` at the torajs-arr
cell offset, so a plain-object `raw` dereferenced garbage as a length
and crashed (the t262 return-empty-string family's SIGSEGV, 7 cases)
— it now runs §22.1.2.4 steps 3-5 exactly (LengthOfArrayLike through
the owned length kernel + ToLength clamp, shape-blind indexed
element reads). A TRIG-2 gap knife closed the length getter's
struct arm (an inline object literal's anon struct fell to the tail
undefined, so an inline `{length: n, …}` array-like counted as 0
while the bound spelling answered — the indexed half already had its
chunk-744 struct arm). Sweep vs 438: passTotal 31306 → **31343
(+37)**, bug +29, incompatible **−66**, trAccepted +66 (conservation
+66 = +37 + 29 ✓), **zero pass regressions** (case-level diff).
Forward 37: new/spread 14, String/raw 12, call 4, tagged-template 2,
sm/class 1, Array HOF 4 (the struct-length arm). Gate 3093 →
**3099/0/4** across six substrate commits (+6 fixtures). Gate predicate **216 unattributed clusters / 2266 cases /
register 2 · 354 (SR-1 266, SR-2 92) / residue 681 · 853 / core
3473**. Rotation 438's sweep @ `f49aedd9` (31306 / 217 / 2289 /
core 3496) was banked to hardev but never stamped here — this entry
carries both deltas.)

**Prior @ `301a20f3`** (2026-08-18, rotation 437 — the C1 with-family
knives. Three root causes stacked under the 21 S12.10 with cases fell
in sequence: var_hoist's keep-the-fn-type escape hatch is
top-level-only, so a nested-block `var f = function(){…this…}` escaped
the promote census (a by-name hoisted-assignment census now admits
it); the with helpers' params were spelled `w`/`k` and, spliced into
the user arena, collided with same-named user bindings (renamed
`__twith_*` — injected code may not occupy user namespace); and all
seven receiver censuses recursed FnDecl/Block-only, so a `with`
inside a `try` hid its `__with_<n>` binding from the any-census
(shared nested-list spine now). Then the instruction-#2 cluster
turned out to be one shape — an object literal used as a `with`
scope object is dynamic by contract — and a declaration widen to
`: any` (the detached-objlit-method move) opened the whole S11.13.2
compound-assignment family plus the prefix/postfix twins. Two thin
knives beside it: the test262 harness's assert-family `msg`
parameters accept `any` (real test262 only ToStrings the message on
failure), and Str + RegExp concatenates through §22.2.6.14 toString
(checker and lowering landed together — the checker alone printed
the heap pointer as a number). The mid-close sweep caught ONE pass
regression (the `[Symbol.unscopables]` deleted-binding strict read),
A/B attributed it to the widen, and the fix — computed-key literals
keep their (g)-leg lane — re-verified to zero. Sweep vs 436:
passTotal 31056 → **31180 (+124)**, bug +69, incompatible **−193**,
trAccepted +193 (conservation +193 = +124 + 69 ✓). Gate 3083 →
**3088/0/4** across five substrate commits + one harness commit
(+5 fixtures). Gate predicate **222 unattributed clusters / 2438
cases / register 2 · 355 (SR-1 267, −107: a batch of sloppy-surface
cases genuinely fixed out of the attribution into pass) / residue
685 · 863 / core 3656**.)

**Prior @ `54cbac3f`** (2026-08-18, rotation 436 — takagi asked for a
confirmation bench of the perf fix, then the `__this` C1 honest-reject
gate plus three knives landed. The full bench at HEAD holds **44/44
cells faster than bun** (median 0.513×, zero drift from the post-fix
run); gcd1m 36.4ms vs bun 42.1 — the "residual 3ms vs the 7/31
baseline" measured down to ~0.4ms, inside the layout-noise band, so
that L3b item closes. C1: a closure mint still capturing `__this`
after the LAST receiver rule rejects in the pipeline with `not yet
supported: fnexpr this in unclaimed receiver position` instead of the
checker's internal-name-leaking unknown-ident spelling — ~60 cases
re-bucketed onto one self-describing signature (dirs=13). The recon's
C3 shape turned out deeper than a census gap: the construction-site
snapshot typed a fn-expr's `__this` capture from the MINT scope, so a
nested `function` in a class method sniffed `return this` as the
class, and storing the plain-call undefined through the class-typed
slot was a store/drop SIGSEGV — the snapshot now skips `__this`
exactly when the mint is a registered function expression (arrows
keep the entry, §8.3.4). Two dynamic-function knives: duplicate
parameter names rename before assembly (the parse ADMITTED them and
both slots resolved to the last binding, so `arguments[0]` answered
the second argument — the rename makes the positional snapshot exact,
and §10.4.4 makes the duplicate case unmapped anyway), and non-string
literal arguments fold through ToString (`new Function(undefined)`).
Sweep vs 435: passTotal 31046 → **31056 (+10)**, bug −13,
incompatible +3, conservation exact (−3 = +10 − 13), **zero pass
regressions**; forward = 8 bug→pass + 2 incompat→pass, all Function
ctor family; 3 Function cases now run far enough to expose the next
gap (`delete` on a fn object's typed layout) and 2 staging cases take
the C1 reject at compile time instead of the same not-yet-supported
at runtime. Gate 3080 → **3083/0/4** across four substrate commits
(+3 fixtures). Gate predicate **224 unattributed clusters / 2514
cases / register 2 · 462 / residue 686 · 864 / core 3840**.)

**Prior @ `15c22e20`** (2026-08-18, rotation 435 — takagi called for a
full bench re-measure and a perf-first rotation, and the re-measure
found the one broken cell: gcd1m at 1.38× slower than bun, the only
AOT cell violating the no-cell-slower invariant. A nine-probe
release-build bisect (same-day hyperfine judge, good 36.0ms / bad
56.9ms, every reading consistent) landed on 55f0464a (rotation 272):
the FoldArity catch-all's mutation scan counts ANY param write as a
hit without ever asking whether the body spells `arguments`, and
ArgcMode::Unmapped materializes unconditionally — every plain fn that
reassigns a param paid an allocated, never-read
`__torajs_arguments: any[]` prologue per call. The arm now gates on an
actual arguments spelling; gcd1m 57 → 37.6ms, collatz −5.2%, and the
post-fix full run has **44/44 cells faster than bun** (median 0.514×).
The first gate (Length ∪ NonLengthTouch) left one pass behind —
`delete arguments.length` is dark in both scans — so a new
ScanFor::AnyTouch answers "any spelling, any position" and took
S10.6_A5_T3 back the same rotation. The `__this` recon's C4b and C5
landed alongside: Array.fromAsync hands its thisArg to every mapfn
call (an `__torajs_any_call_with_this` dispatch twin, a third borrowed
kernel operand, the lowering boxes args[2], the cb-slot face admits
fromAsync), and an inline fn-expr promotes as a `.bind` receiver (one
parent per expression node = alias-free by construction; the lifted
mint rides the kernel lane via FLAG_CLOSURE_RECV_FIRST). Sweep vs 434:
passTotal 31043 → **31046 (+3)**, bug +1, incompatible −4,
conservation exact (+4 = +3 + 1), **zero pass regressions** in the
final sweep; forward = fromAsync thisarg ×2 + flatMap
bound-function-argument, and thisarg-primitive-sloppy now runs for
real (stdout-mismatch — the sloppy ToObject wrap, recorded). Gate
3077 → **3080/0/4** across four substrate commits (+3 fixtures).
Gate predicate **224 unattributed clusters / 2510 cases / register
2 · 462 / residue 684 · 865 / core 3837**.)

**Prior @ `a9537e62`** (2026-08-18, rotation 434 — two recon agents
split the `__this` cluster into five structural shapes and the
Struct-write-through cluster into four, then the four biggest honest
cuts landed. Destructuring pattern field loads take the §13.15.5.4
lenient mark on EVERY slot, not just the defaulted ones (~31 cases —
the mark's insert sat below the no-default early return). A thrown
fn name wraps into its canonical `__forward_` cell (the throw
lowering packed a raw code address under the heap tag — typeof
"object", identity false, SIGBUS on the scope-end drop, a silent
EXIT=138 the probe chain caught while chasing `__this`). A
`__proto__: v` PropertyName literal rides the dynobj lane (anylane
(h) leg + checker twin; the struct lane recorded it as an own data
field with no chain at all), and a null-proto dynobj's named-member
miss stops at null instead of falling into the builtin reify tail.
%Iterator.prototype% grows its helper method surface: a tag-15
ownership row, the dynobj-chain re-dispatch shunts the child
receiver straight into `try_helper_chain`, validation failures run
§7.4.9 IteratorClose (original abrupt wins, ToNumber poison
preserved, close-time throw discarded), and the `.call` reflection
family keeps the strict no-ToObject receiver rule — the mid-rotation
sweep caught 14 regressions from the wrapper-seeded redispatch and
the family-15 shunt took them all back. Sweep vs 433: passTotal
30881 → **31043 (+162)**, bug −102, incompatible −60, conservation
exact (+60 = +162 − 102), **zero pass regressions** in the final
sweep; 118 bug→pass + 41 incompat→pass forward, 22 newly-running
exposures (9 exit-1 + 6 async-failure + 3 exit-3 + 1
stdout-mismatch + 3 rebucketed not-yet-supported). Gate 3070 →
**3077/0/4** across six sequential substrate commits (+6 fixtures).
Gate predicate **224 unattributed clusters / 2513 cases / register
2 · 463 / residue 684 · 865 / core 3841**.)

**Prior @ `aa82c5bc`** (2026-08-18, rotation 433 — the builtin
constructor cells grow real behavior on every first-class face. The
groupBy pair and Promise.withResolvers join the ns-static value
family (length / name / detached calls — groupBy has no |this| step
so the detached call runs the real kernels; withResolvers' detached
call raises the step-1 TypeError; the reflection face reads the
checker sig for .length, so withResolvers' sig takes an empty param
list). The generator lift learns arrow bodies through a flat
expr-arena scan (the stmt spine never sees ExprId-hung bodies — the
GeneratorPrototype not-a-constructor trio was this). Function.call
rides the dynamic-function inline channel (thisArg peeled when
side-effect-free, §20.2.1.1 never reads it). And the interned ctor
cells get a callable face (Number = ToNumber, String = display
coercion with the empty-string zero-arg arm, Boolean, Object =
ToObject, Array = length form, Date = current-time string; families
without [[Call]] raise the catchable TypeError) plus a construct
face (wrapper mints / containers / Date ms — the fifteen
is-a-constructor probes flip). Sweep vs 432: passTotal 30843 →
**30881 (+38)**, bug −9, incompatible −29, conservation exact (+29 =
+38 − 9), all 38 forward — **zero pass regressions**. Gate 3070/0/4
(+5 fixtures). Gate predicate **227 unattributed clusters / 2570
cases / register 2 · 465 / residue 687 · 866 / core 3901**.)

**Prior @ `e626a596`** (2026-08-18, rotation 432 — the with desugar's
two probe-confirmed silent-wrong gaps close (`new Base()` names its
constructor as a string the free-name walk never saw — the object arm
now constructs through NewDynamic; a logical compound's cloned left
operand took a second HasBinding guard — the shell now rewrites whole
under one), and the no-member rescan's top faces land: generator field
annotations learn Object statics and unannotated value-returning
functions (the forbidden-ext b2 family's 45 cases stored descriptors
through a number slot), the Object proto family keeps its runtime
nullish gate under `.call` with isPrototypeOf's primitive-V-first
ordering, and Math.hypot joins the ns-static value family. Sweep vs
431: passTotal 30784 → **30843 (+59)**, bug +2, incompatible −61,
conservation exact (+61 = +59 + 2), moved 62 — **zero pass
regressions**. Gate 3065/0/4 (+5 fixtures). Gate predicate **227
unattributed clusters / 2597 cases / register 2 · 465 / residue 688 ·
868 / core 3930**.)

**Prior @ `74c3db43`** (2026-08-18, rotation 431 — the resolver's
dead-copy sweep and the no-member cluster's largest face. The module
channel that stranded orphan expressions closes at its single exit:
after the final splice, everything unreachable from the spliced
statement list — floored at the entry's own arena, with the five
value-side-ExprId side tables as extra mark roots — is tombstoned, so
no whole-arena lift can materialize a discarded lib copy's fn/class
literals again (the 2B stub demotes to defence-in-depth; the
instn-iee-bndng family now dies on real TDZ semantics instead of
orphan fallout). The brand-checked prototype families (Date / Map /
Set / Promise, plus Function's bind/toString and literal-wrong-brand
call/apply) skip the `X.prototype.m.call(recv)` direct-method rewrite
and ride the reified proto cell's brand gate — the spec's runtime
TypeError instead of a compile-time member reject — 111 cases across
five directories. Date's toJSON generic leg consults the receiver's
own toISOString (§21.4.4.37 Invoke), and Map/Set receivers join the
method-value reify families (17 not-a-constructor cases). Sweep vs
430: passTotal 30655 → **30784 (+129)**, bug −14, incompatible −115,
conservation exact, moved 142 all forward/neutral — **zero pass
regressions**. Gate 3060/0/4. Gate predicate **227 unattributed
clusters / 2613 cases / register 2 · 510 / residue 688 · 868 / core
3991**.)

**Prior @ `e9fc6a80`** (2026-08-18, rotation 430 — static-resolution
loud rejects and the orphan-closure shelf. The entry's static requests
gain a ledger: named imports plus named `export {x} from` clauses —
previously ignored entirely — are judged after the BFS drains, and an
ambiguous (§16.2.1.6.3) or never-landing (§16.2.1.6.2) binding rejects
compilation with a module-resolution SyntaxError the t262 runner now
credits as pass-negative (that marker only — every other reject stays
incompatible). Pass 2B shelves construction-less orphan closures as
unreachable stubs: a module re-parsed through its own import chain
strands statement-dead fn literals that the whole-arena lift still
lifts, and the 65-case capture-types stop clears to ZERO (19 straight
to pass, the rest run into their next real layer). Entry-side writes
to import bindings turn into runtime TypeErrors (§16.2.1.5 immutable
view; lib-side self-writes keep the live-binding path). The runner
stages the case under its corpus filename plus every sibling `.js` a
fixture chain references, so self-import families reach their real
semantics. Sweep vs 429: passTotal 30617 → **30655 (+38)**, bug +48,
incompatible −86, conservation exact, forward 39 / regression 1
(instn-iee-iee-cycle, a de-watering: its old pass rode the
export-from-ignored accident). Gate 3057/0/4. Gate predicate **235
unattributed clusters / 2700 cases / register 2 · 510 / residue 708 ·
896 / core 4106**.)

**Prior @ `dc7435f8`** (2026-08-17, rotation 429 — the declare-
arguments arrow lane and the dyn-import instantiation rejects. The
§19.2.1.3 legal half lands: a direct eval in a true arrow's default-
parameter position that var-declares `arguments` gets real
parameter-scope semantics (defaults lower into the body, a synthesized
per-arrow var carries the binding, lexical references α-rename;
a parameter itself named `arguments` throws at call time) — the
arrow-fn family goes 12/12, and the throws-harness thunk splits into
a typed literal entry and an any-arity identifier entry, closing the
func-decl four. Dyn-import candidates whose indirect exports never
resolve stop failing the build: a dangling `__reex_` binding
(circular `export {x} from` chains, missing sources) or an AMBIGUOUS
star landing (two `export * from` clauses landing one name from
different modules — the Landings ledger records decl landings by
path, so transitive diamonds never false-positive) poisons the
dispatcher entry into a §16.2.1.5 SyntaxError promise reject — the
instn-iee-err families go green (31 forward). The class value-
reference rewrite turned shadow-aware (a param/let/catch binding
spelling a class name owns its references — the flat arena scan
silently handed them to the class object). Sweep vs 428: passTotal
30570 → **30617 (+47)**, bug −22, incompatible −25, conservation
exact, **zero pass regressions** (the first in-rotation sweep caught
two self-inflicted ones — an eval pre-parse polluting side-tables
and the any-lane thunk tripping the capture-types stop — both fixed
before close). Gate 3053/0/4. Gate predicate **234 unattributed
clusters / 2754 cases / register 2 · 516 / residue 732 · 922 / core
4192**.)

**Prior @ `37fc80ab`** (2026-08-17, rotations 427-428 — the hidden-
dependency census and the full ClassDecl rename. Rotation 427 (knife
C/D0/D1/D2): a named import's injection closure — the free variables
of every injected decl, recursed to a fixpoint over the lib's own
top-level names — injects hidden (census-mangled, never importer-
visible) with per-path memory separate from the requested-mangle
memory; class deps first landed bare-only, same-path dyn candidates
stopped colliding with their own static bindings, class fan-out
aliases bind by reference, and a bare export's face rename moved
into the census so sibling references follow. Sweep +36 passTotal,
all forward (the dynamic-import/usage family). Rotation 428 (knife
D): the census renames ClassDecl outright — `__priv_` brands
(declaration, member refs, the `#x in o` key), `__ccmk_` computed-
key hoists, `__cm_gen_` forwarders, `__cm_/__sm_` super-call bakes,
type-ann strings, and the name-keyed parser tables migrate as one
move (row ownership: arena-offset ExprIds, a parse-window snapshot
diff with overwrite restore, then structure-gated COPY); `.name`
keeps the source spelling via the NamedEvaluation display table; a
same-spelled type param declines the mangle. Dyn candidates with a
colliding class stopped DROPping, and a single-aliased class import
renames in the census (the walk's shallow rename retired). Sweep vs
427: passTotal 30570 flat, zero pass regressions, one forward
deepening (`instn-uniq-env-rec`) — this knife's value lands in the
conformance face (+5 fixtures, gate 3048/0/4) and in the silent-
wrong family it closes, not in t262 counts. Gate predicate **235
unattributed clusters / 2771 cases / register 2 · 522 / residue
735 · 924 / core 4217**.)

**Prior @ `27c7b62d`** (2026-08-17, rotation 426 — the dynamic-import
deconflict chain. Knife B narrowed the candidate DROP to unmanglable
names, and the probes then overturned the recon: the
assignment-expression family's real blockers were three GENERAL
ns-lane gaps, static and dynamic alike. Four knives: the namespace
object carries `default` (§16.2.1.10, synthetic `__nsdefault_`
binding riding the one-binding machinery; a bare `export * from`
pour must NOT mint it, §16.2.3 — caught by the gate as a red and
fixed with a WorkItem star-feed flag); a source-ful named re-export
face claims a ns field bound to a synthetic `__reex_` binding
(§16.2.3); the deconflict census mangles a bare-exported decl on its
FACE surface (aliased named requests follow the path's mangle
memory; the ssa-side Any promote gate admits `__reex_` / `__m<k>_`
minted bindings). Sweep vs rotation 425: passTotal 30502 → **30534
(+32)**, bug −72, incompatible +40 — conservation exact; the
dynamic-import assignment-expression double-FIXTURE family (14
cases) converted whole. One pass regression listed honestly: a
circular-instantiation case whose old pass was the DROP coinciding
with the expected rejection. Gate predicate **235 unattributed
clusters / 2807 cases / register 2 · 522 / residue 737 · 925 /
core 4254**.)

**Prior @ `c40b5319`** (2026-08-17, rotation 422 — the async-generator
half of the `got Star` cluster, which a re-survey first overturned:
async generators themselves had worked in all three hosts since
rotation 413's blade 4 — the 49 remaining cases all die on
EXPRESSION-position `yield*`. The parser now hoists `v = yield* src`
into a mutable `__yx_` temp plus the statement form's yield-bearing
ForOf, and the F1 manual protocol's done arm writes the inner
iterator's final `.value` into the temp (§27.5.3.2 — the yield*
expression's value). Alongside: reserved words can name object-literal
generator methods (`{ *yield() {} }`, §12.7.6); a namespace member the
module never exported answers undefined instead of the struct typo
reject (§10.4.6.8); %GeneratorPrototype% carries its real
@@toStringTag own entry ("Generator" / "AsyncGenerator", §27.5.1.5);
and an uninitialized annotation-less `let` in a generator body lifts
as `any` — the old number pin could not even hold the initial
undefined, and turned out to be load-bearing for two lucky
pass-negatives and 24 masked eval-substrate crashes (both documented,
L3b 422-02). Sweep vs rotation 421: passTotal 30278 → **30296
(+18)**, bug +89, incompatible −107, trAccepted +107 — conservation
exact; passNegative −2 (de-watering, A/B-attributed), 24 new exit-138
in one signature family (unlocked pre-existing crashes, registered).
The `got Star` cluster is **gone entirely** (87 → 49 → 0 across two
rotations). Gate predicate **236 unattributed clusters / 2891 cases /
register 2 · 573 / residue 763 · 979 / core 4443**.

**Prior @ `e787fd8d`** (2026-08-16, rotation 421 — the module
graph's missing half. `export * from "m"` / `export * as ns from "m"`
parse and resolve (a star forwards the importer's request minus the
hub's own exports; namespace objects accumulate per ALIAS and
materialize after the BFS drains, in reverse discovery order);
module bodies evaluate in dependency post-order per §16.2.1.5
(requester → requested recorded as the walk pushes, statements
spliced back once the queue drains — cycles stop at the member on
the stack); `with { type: "json" }` import attributes parse as part
of the declaration; `default` works on both sides of an export
specifier (exposed name answers the importer's default binding,
source name rides the default lane); a second request shape against
one module no longer re-declares its bindings (every lane records
injected decl names in the per-path ledger); and three §16.2
early errors landed — declaration terminator (`export * from "m"
null;`), duplicate ExportedNames, and lone-surrogate string export
names (checked on the RAW spelling — the lexer folds lone
surrogates to U+FFFD). Sweep vs rotation 420: passTotal 30267 →
**30278 (+11)**, bug +11, incompatible −22, trAccepted +22 —
conservation exact; **zero pass regressions, zero new crashes**
(three surrogate regressions appeared mid-rotation from accepting
string export names and were repaid the same rotation). The
`expected expression, got Star` cluster (87 cases) lost its entire
module-code half; the 49 that remain are all async generators.
Gate predicate **239 unattributed clusters / 2960 cases / register
2 · 611 / residue 762 · 979 / core 4550**.

**Prior @ `e8fd4d8c`** (2026-08-16, rotation 414 — the any-lane
generic-accessor ABI knife landed (a GENERIC class's accessor rides
its `__cmany_` twin through a flags-aware finder + recv-first
dispatch — the 404-01 method-face pattern extended to accessors),
RegExp subclasses took the §22.2.4.1 two-argument `super(pattern,
flags)` form (the exotic rest ctor family's last loud arm), tuple
type annotations widen to their element array, and arrow bodies
inherit the enclosing function's `arguments` (§9.4.4 — the Babel
alias shape, with fn-expr scopes and `arguments`-named formal
parameters correctly excluded after one gate red and one sweep
regression each). Sweep vs rotation 413: passTotal 30186 →
**30196 (+10)**, bug −12, incompatible +2, trAccepted −2 —
conservation exact; one case-level regression
(`arrow-body-private-direct-eval-err-contains-arguments`
expressions form, a lucky pass turned loud incompatible — the eval
inline path now surfaces the class-field-init `arguments` early
error as unsupported instead of accidentally passing). Gate
predicate **238 unattributed clusters / 2987 cases / register 2 ·
615 / residue 771 · 1002 / core 4604**.

**Prior @ `b037a042`** (2026-08-15, rotation 410 — the value-shaped
heritage lane finished its admission story. An `extends` naming a
VALUE binding (`{ let B = Real; class K extends B }`) extracts to the
`__ccp<N>` lane like any heritage expression (the class-name census
decides which idents stay static); a this-writing named fn carried
into an `: any` binding keeps its receiver channel (the
`__fwdrecv_` site copy — `.call`, bare call, `new`, and value-parent
`super` all thread `this`; rotation 366's global-stamp lesson stays
honored), which closed the fn-value-parent residue; `extends null`
defines with a null-prototype chain and throws at `new`
(`es5_null_parents` side-table); and a non-constructor heritage value
throws at class-DEFINITION time (`__torajs_anyv_heritage_check`,
§15.7.14 step 5 — the `{prototype:{}}` silent-accept became a loud
TypeError). A probe also un-wedged named-member `delete` on typed
arrays (`delete arr.length` / `arr["length"]` — the hole argument
only ever gated element slots). Sweep: passTotal 30150 → **30171
(+21)**, bug +65, incompatible −86, trAccepted +86 — conservation
exact; **zero pass regressions**; forward moves all in the heritage
family (class/subclass, sm/class, delete, super). Gate predicate
**238 unattributed clusters / 2988 cases / register 2 · 616 /
residue 774 · 1005 / core 4609**.

**Prior @ `c589dbb2`** (2026-08-15, rotation 406 — the
capturing-nested-class RFC closed out and function values grew a real
prototype chain. Blade 6 (393-01): a class expression outside a class
body now lands NEXT TO its use site (parse_stmt wrapper drains a
local synth buffer; top level and class bodies keep their old
splices), so the nested-class machinery decides its fate — the
silent-wrong "warning + ReferenceError" shape became either a working
ES5 lowering or a loud named decline; the user's own alias binding
mints unique alongside the class. 405-01 all three faces: a closure
carries a user [[Prototype]] link on its lazy expando dynobj (the
same \x00proto simulation entry — set/get/member-get/method-dispatch/
`in` all walk it), the extends lane links the class side with
`Object.setPrototypeOf(D, P)` and admits static-carrying parents,
super resolves through `es5_ctor_forward` past ctor-less middles
(whose synthesized rest forwarders the promotion ABI bar refuses),
and a static body's `super.m` reads through the parent class. 405-06:
Map/Set/WeakMap/WeakSet prototype methods brand-check a rebound
receiver (15 test262 cases forward). 406-02, the one blade-6
regression family (4 cpn computed-property-name-from-await cases):
computed static fields route through the lane behind a program-unique
name census, and three adjacent holes closed (capture check now walks
the static key; index-obj joined the receiver-safe shapes; any
index-get grew a Closure arm). Sweep: passTotal 30114 → **30131
(+17)**, bug −23, trAccepted −6 — conservation exact; **zero pass
regressions** end-state (the 4 mid-rotation ones repaired same
rotation). Gate predicate **240 unattributed clusters / 3081 cases /
register 2 · 617 / residue 776 · 1007 / core 4705**.

**Prior @ `ad6b3067`** (2026-08-15, rotation 405 — three fronts moved.
The capturing-nested-class lane took `extends` for a static-free routed
sibling (RFC 20260814 blade 5 knife 1: ES5 inheritance shape, implicit
rest-forwarding ctor, the static-init wrapper's receiver argument
joining the receiver-safe use shapes). A GENERIC class's instances
became first-class on the any lane (404-01: the named class_layouts
walk no longer SKIPS a class with no alias sid — that skip had shifted
every later row one slot left; each mono factory mints its own tag so
two same-shaped generic classes stay two classes; method rows
re-target at the `__cmany_` twin-any instance under a new recv-first
flags bit; `instanceof` ORs a runtime row-name identity check into the
constant descendant chain). And the stage-3 upsert family landed on
both lanes (383-04: `Map.prototype.getOrInsertComputed` +
`WeakMap.prototype.getOrInsert`/`getOrInsertComputed` as the
peek→call→set composition that IS the spec's late-set semantics).
Sweep: passTotal 30085 → **30114 (+29)**, bug −28, incompatible −1,
trAccepted +1 — conservation exact; all 36 verdict moves are the
upsert family, two of them pass→bug de-inflation (the pre-existing
passes rode "method missing → TypeError" coincidence; the real gap is
the brand check on rebound collection methods, registered 405-06).
Gate predicate **240 unattributed clusters / 3078 cases / register
2 · 617 / residue 774 · 1004 / core 4699**.


**Previous @ `a9cfa97d`** (2026-08-14, rotation 401 — the any-lane
this/receiver channel closed out as a group: six registry entries
(398-10 / 398-05 / 400-01 / 401-01..03) across six knives, every one the
same pairing shape — a checker admit whose record the lowering side
already keys on, with ZERO new runtime kernels. An any-typed HOF callback
routes the typed-Array call through the any lane (the callee's `Any`
record is the cluster-#4 pairing); the as-any spelling of a named
this-reading callback peels to the existing `__fwdrecv_` forwarder; a
this-reading fn-expr binding gained the `hof_cb_arg` use-shape (escaping
proof family); an any-objlit FIELD holding a named this-reading fn now
binds the holder (every consumer of a dynobj field is an any-lane call
path); a static method read through the any lane answers
`.length`/`.name` (the `__sm_` body bakes a second registry row against
its adapter fid, the `__cm_` mirror); and a closure nested in a
COMPOSITE initializer may capture its own binding (the recursive lane's
ordinary-binding box + a checker pre-declare at the annotation's type).
findLast / findLastIndex / flatMap joined the any-callback allowlist
after a kernel audit; sort / toSorted stay loudly rejected (in-place
any-lane write into a typed block). Sweep @ `a9cfa97d`: passTotal 30078
→ **30083 (+5)**, pass ±0, `passNegative` +5, bug −3, incompatible 10337
→ **10335 (−2)**, trAccepted +2 — conservation exact (`+2 == 5 − 3`).
Verdicts joined: 14 differing, zero only on one side: 5 forward
pass-negatives are the rotation-400 [In] knife landing (in-branch-2
back in place as predicted, plus four siblings), `sm/class/methodName`
went bug → pass, five bug-bucket cases started running (progress shown
as bugs), and exactly 1 regression — `accessor-name-computed-in`, pass →
parse error: the [In] gate refused `in` inside a literal, but §13.2.4/5
spell every literal element, property value, and computed key as
`[+In]`. Fixed the same rotation (`7dd4130f`, after the sweep,
gate-verified): both literal entries reset `in_for_init` the way the
paren reset does. The remaining [+In] re-entry surfaces (call Arguments
/ index brackets — the `2nd-param-in` movement) are registered 401-04.
Gate predicate **240 unattributed clusters / 3084 cases / register 2 ·
619 / residue 772 · 1002 / core 4705** — flat, with the movement inside
the accepted buckets.)

**Previous @ `989d4b7b`** (2026-08-14, rotation 400 — the rotation that worked
through the 398/399 registry: six entries closed across five substrate
knives. The fn-expr receiver channel gained the array-literal element
position and the bare (unannotated) class-member return — the latter fixed
at the root by SEEDING the return annotation to `any` (the FnDecl's
`return_type` is the single source every downstream reader consumes)
rather than widening the collector, which rotation 399 had measured wrong.
The twin double-count was then audited across every by-name census:
two were count-sensitive and fixed (`certain_bindings` — a method-scope
`p.then(handler)` silently took the method's receiver — and the alias
census, which needed the source-vs-copy split because the twin's clone is
a DISTINCT lifted closure), three were proven insensitive (bool existence,
`is_empty`, set dedup). A static method reached through the any lane now
rides its `__smany_` twin with the receiver — the miss was in the dynobj
own-entry and [[Prototype]]-chain arms, where an INHERITED static
resolves. And a ternary over two different concrete scalars joins to Any,
landed as the checker/lowering pair after the checker half alone measured
silently wrong (raw pointer bits through the other branch's slot type) —
which also un-crashed the class-nested spelling. Sweep @ `989d4b7b`:
passTotal 30076 → **30078 (+2)**, pass +3, `passNegative` −1, bug +2,
incompatible 10341 → **10337 (−4)**, trAccepted +4 — conservation exact
(`+4 == 2 + 2`). Verdicts joined: 53174 vs 53174, **5 differing, zero
only on one side**: 3 forward (the ternary knife), 1 newly-running-but-
wrong, and exactly 1 regression — `conditional/in-branch-2`,
`pass-negative → bug`, which is WATER SURFACING, not a loss: the §13.13
`[In]` grammar parameter was never implemented on the ordinary `in` arm,
and that negative case was being caught by the ternary TYPE reject, by
coincidence and in the wrong phase. Fixed the same rotation
(`50b780bf`, after the sweep, gate-verified): the `in_for_init` gate the
private-name arm already consulted now guards the ordinary arm, the
ternary THEN branch lifts to `[+In]` per spec, and the reject happens at
parse where it belongs. Gate predicate **240 unattributed clusters / 3084
cases / register 2 · 619 / residue 773 · 1002 / core 4705** — core down 4
with every case nameable.)

**Previous @ `16256297`** (2026-08-14, rotation 399 — four receiver-promoting
knives on the fnexpr-`this` channel, and then the regression they caused,
which is the part worth reading. A function expression binds `this` at the
call site (§10.2.1.2); inside a class it was being handed the enclosing
method's receiver, i.e. arrow semantics. The root was not the desugar that
rewrites `this` — three of the four host shapes were already correct — but
the by-name census that proves a binding is declared exactly once:
`desugar_classes` clones every `this`-using method body into a
receiver-polymorphic `__cmany_` twin, so a `const` written ONCE was counted
TWICE and the promote declined. Fixed, plus the class-member `return` face
and the `this.<any field>` store face. **Then the closing sweep caught what
five green gates could not**: §13.1's "undeclared private name" is a
PARSE-phase early error, but tr implemented it in the checker, which
recognizes the mangled name only on a `ClassRef`-typed receiver — so a
parse-phase early error hung on the static type of `this`, and promoting
`__this` to `any` erased it. Four negative cases went `pass-negative →
bug:negative-phase-mismatch`. Moving the decision to where the reference is
resolved (`parser/private_refs.rs`) is both the spec phase and the only
position no downstream typing can erase. Sweep vs rotation 398: passTotal
30062 → **30076 (+14)**, `passNegative` 4106 → **4120 (+14)**, bug 12771 →
**12757 (−14)**, `pass` / `passNoOracle` / `incompatible` / `trAccepted` all
unmoved — conservation exact (`0 == 14 + (−14)`). Verdicts joined line by
line: 53174 vs 53174, **14 differing, zero only on one side, zero pass
regressions** — every one forward, and every one nameable: the six
`invalid-names/*-bad-reference` variants per class form plus
`grammar-privatename-in-computed-property-missing`, all now refused at parse.
The four cases that regressed mid-rotation are back at `pass-negative`.
Gate predicate **240 unattributed clusters / 3084 cases / register 2 · 619 /
residue 776 · 1006 / core 4709** — unmoved, because everything that moved
moved inside the `bug` bucket rather than out of `incompatible`.)

**Previous @ `8216cbdc`** (2026-08-14, rotation 397 — one gap in one
whitelist was holding four separate faces shut. A `this`-using function
expression only gets its receiver promoted when every use of its binding
is a shape the promoted ABI survives, and two positions were missing:
`return <name>` (so a class factory — `function make(b){ class C {…}
return C }` — was rejected whole) and the target argument of
`Object.defineProperty` (so a static class member could not be installed
non-enumerably, and static accessors and computed static names were
declined outright). Both landed, then the lane consumed them, then a
computed accessor fell out as a decline removal, then a class sentinel
joined the capture filter. Sweep: passTotal 30041 → **30061 (+20)**, pass
+20, `passNoOracle` unmoved (external oracle on every one, so not water),
bug 12768 → **12772 (+4)**, incompatible 10365 → **10341 (−24)**,
trAccepted +24 — conservation exact (`24 == 20 + 4`). Verdicts joined line
by line: 53174 vs 53174, **28 differing, zero pass regressions**. The 20
forward name their knife: 18 are `derived-cls-direct-eval-contains-
superproperty` variants plus `super/prop-{dot,expr}-cls-val-from-arrow`
(the sentinel filter — `__proto_<C>` is the super base, and an arrow or an
eval-inlined body was collecting it as a capture), 2 are
`accessor-name-{inst,static}/computed-err-unresolvable` (the computed
accessor). Four more now RUN and answer wrong rather than being rejected
(three `dynamic-import/import-attributes/2nd-param-*`, one
`Function/has-instance-jitted`) — new exposure, not regression. Gate
predicate **240 unattributed clusters / 3084 cases / register 2 · 619 /
residue 776 · 1006 / core 4709** — cases and core both down 22/24 with
every case nameable; the cluster count holds because the 20 were spread
across five existing clusters rather than emptying one.)

**Previous @ `20229614`** (2026-08-14, rotation 396 — the rotation that moved
test262 by exactly nothing, and says so. Six knives on the
capturing-nested-class lane and the parser's `this` scoping: a static
method saying `this` now routes, a class the hoist renames takes its
static-`this` recording with it, an instance accessor rides the lane, an
instance member of a lowered class is non-enumerable, and a `function`
nested in a static body binds its own `this`. Sweep: **every bucket
identical to the previous one** — passTotal 30041, pass 25084, bug 12768,
incompatible 10365, trAccepted 42809, conservation trivially exact.
Identical aggregates do not prove nothing moved, so the verdicts were
joined line by line: **53174 vs 53174, zero differing lines** — no
forward, no regression, no cancelling pair. The knives land on surfaces
test262 barely exercises (nested classes that capture, and the lane's own
lowered shape); the gain is recorded on conformance instead, 2870 →
**2875**, five bun-exact fixtures, two of which fix SILENT wrong answers
(a renamed class's `typeof this` answered `"undefined"`; a lowered
instance method was enumerable, so `for…in` listed it). Gate predicate
**240 unattributed clusters / 3106 cases / register 2 · 619 / residue 777
· 1008 / core 4733** — the 2-case shift is wording, not capability: the
verdicts are identical and the new decline message
(`it has a static getter or setter`) simply re-clusters.)

**Previous @ `ec85557b`** (2026-08-14, rotation 395 — computed class member
names reach the capturing-nested-class lane, plus three defects the
probes turned up that were not the task. §15.7.14 evaluates each
ComputedPropertyName once, in element order, at class-definition time,
so the ES5 lowering emits the keys first and the members read the
binding — the same `__ccmk_<C>_<n>` name the parser had already baked
into the constructor prefix for a computed instance FIELD. The
prerequisite was a hole of its own: the hoist's capture check walked the
constructor, the methods and the static inits, and a computed key is in
none of them (it lives in a side table), so `{ const k = "z"; class K {
[k]() {…} } }` read as capture-free, lifted to the top level, and
answered a warning plus a wrong answer at run time. The fixture — not
the probe — then refuted the implementation: `class_computed_keys` is
keyed by class NAME, and three functions each declaring `class K` share
one entry set, so ownership had to be read back off the class itself.
Two more, unrelated to the lane and both hand-written source: a computed
key has no static name to put in an inferred return shape, so
`JSON.stringify(mk("a"))` serialized `{"__computed_0__":0,"z":0}` while
`r["a"]` answered 1 (silent); and an `as` suffix hid a fn-expr from
receiver promotion on both the declaration and the use side. One
widening was measured and REVERTED — routing static methods whose `this`
only reads turned a loud refusal into a wrong answer; chasing WHY found
the fourth defect, also silent and also hand-written source: a method
stored on a FUNCTION value was invoked with the function's props bag as
`this`, so `K.self() === K` answered false and `typeof` answered
`"object"` while every `this.<name>` read answered correctly, because
the properties are in that bag. Sweep: passTotal
30035 → **30041 (+6)**, pass +6, `passNoOracle` unmoved (so the gain is
oracle-backed, not water), bug +4, incompatible −10, trAccepted +10,
conservation exact, **zero pass regressions**; two cases LOST acceptance
(`class/accessor-name-{inst,static}/computed-err-unresolvable`, from
`bug:exit 1` to a loud refusal — they were wrong answers before, and the
capture check now sees their computed key). Gate predicate **240
unattributed clusters / 3108 cases / register 2 · 619 / residue 777 ·
1006 / core 4733** — clusters and cases both down, and this time from
capability rather than from wording.)

**Previous @ `23c828d4`** (2026-08-14, rotation 393 — the rotation where the
probe refuted the task. The plan said "`with` body class declarations —
write a probe first"; the first probe, `{ let a = 7; class K { m() {
return a } } }` with **no `with` in it**, answered `internal: ClassDecl
reached check.rs`. The real blocker is that a nested class cannot capture
a local at all — `hoist_nested_classes` lifts only capture-free ones —
and the `with` binding is a block-scoped `let`. So the class face landed
as an EXACT verdict rather than a lifted refusal: a closed class runs,
anything reading a name the object could supply is refused naming the
part that carries it. Six of the eight knives fixed **existing wrong
answers**, three of them silent, and half were not `with` bugs at all:
a class body is strict code so `with` inside one is a §14.11.1
SyntaxError (bun cannot even transpile that shape — node was the
oracle); the globals the parser writes as machinery
(`Object.__forinKeys`, `String(sub)` for `${…}`, `Promise.resolve(ns)`)
were being answered by the `with` object, which made every template
substitution in a `with` body hijackable and every for-in in one throw;
a guard arm's clone was losing every side-table marker it copied; a
destructuring `var` was not a `var` (`{ var { a } = src } a` answered
`unknown identifier`); and a `switch` on an `any` scrutinee was refused
by the checker — where widening the checker ALONE turned the loud
refusal into a silent wrong answer, because the per-case compare was a
raw integer compare of a boxed word against a bare one. Sweep: passTotal
30014 → **30035 (+21)**, pass +19, bug +2, incompatible −23, trAccepted
+23, conservation exact, **zero pass regressions**. The 12 cases that
moved `incompatible → bug` are programs tr now ACCEPTS and which fail on
the real gap behind the spurious refusal — five on `Proxy`, the rest on
eval semantics. Gate predicate **241 unattributed clusters / 3123 cases
/ register 2 · 619 / residue 772 · 1001 / core 4743** — clusters and
cases both down.)

**Previous @ `0a2fcd56`** (2026-08-14, rotation 392 — `with` (§14.11) closed
out, and with it three silent wrongs of one shape: **the rule the pass
wrote down was not the rule the pass implemented, because its walk did
not reach every place the shape can sit.** A function EXPRESSION's body
is a `Vec<Stmt>` hanging off an arena expression, not a statement child,
so a `with` written there was never rewritten at all; an identifier that
IS a statement's whole expression (`return x` / `if (x)` / `throw x`) had
no parent to announce it, because `collect_expr` recorded only children;
and `delete <bare name>` never reached the desugar because the triage
that folds it to a constant ran FIRST in the prelude — its refusal arm
was dead code while `with (o) { delete x }` answered `true` and removed
nothing. **None of the three is visible to the conformance gate**: the
programs ran and printed a wrong answer. 刀 4 (nested function bodies)
then cost nothing where the RFC predicted machinery — the binding is an
ordinary block-scoped `let`, so the guards capture it like any closure
captures any outer binding — and everything where it did not: a nested
function's `var` / params / `arguments` shadow while the with body's own
`var` does not, so one flat binder set could not answer both and `bound`
became a `Scope` chain. The `var` initialiser and `for (var i = …)` both
needed only for the statement to stop being ONE statement. Two fixes with
no `with` in them fell out of writing the fixtures: a refused `delete`
throws only in strict code (§13.5.1.2 step 5 — the kernel had shipped
both flavours all along and the lowering always picked the throwing one),
and **a refused `with` printed a bare `error:`, which every
stderr-classifying harness reads as an uncaught throw** — so each
declined program was counted in `trAccepted`. Sweep: passTotal 29988 →
**30014 (+26)**, pass +27, bug −76, incompatible +50, trAccepted −50,
conservation exact. **The −50 is water leaving, not a regression**: 56
cases moved `bug → incompatible` because the mis-prefixed refusals were
replaced by real downstream verdicts that carry a prefix. One pass
regression, the registered per-function-strictness divergence (a
`"use strict"` function inside a sloppy script now answers `false` for a
refused delete). Gate predicate **242 unattributed clusters / 3149 cases
/ register 2 · 620 / residue 769 · 997 / core 4766**. The rotation also
made the next gap visible: cluster #7, **60 cases in one directory**,
is `delete` on a `Struct` receiver — the real blocker behind the
compound-assignment `with` family, which the mis-prefixed refusal had
been hiding.)

**Previous @ `ae19d1f6`** (2026-08-13, rotation 389 — `instanceof` never
asked `@@hasInstance`. §13.10.2 step 2 is a lookup the compile-time lane
structurally cannot do: it answers class membership from a heap tag, so
a target that names no class folded to `false` while its handler sat
unread. Measuring nine spellings against bun split the gap along two
orthogonal axes — the target's VALUE shape (class / callable / plain
object) against its SYNTAX shape (bare name / general expression) — and
every silent wrong landed in the bare-name column, so the parser did not
have to move at all. Four knives: a kernel running the operator in spec
order (non-object throws, GetMethod, Call with `this` bound to the
target, ToBoolean, and the existing OrdinaryHasInstance walk when no
handler is found), placed ahead of every static fold **including the
operand-type early-outs**, since a handler decides the answer for a
primitive V too; the callable lane, whose handler can only be installed
by `defineProperty` because `Function.prototype[Symbol.hasInstance]` is
non-writable; a child-module split to make room; and a class's own
`static [Symbol.hasInstance]` (§15.7), gated on a compile-time check that
never fires for an ordinary class and walks `class_parents` so an
inherited handler answers too. **A fifth knife fixed a regression only
the sweep could see**: all four gates were green, but `box_to_any`'s
match ends in a panic, and a panic in the lowerer rejects the whole
program — handing it a `FnSig` operand turned four cases from `bug` into
`incompatible`, i.e. tr stopped compiling programs it used to run. The
check has to be on both sides; `genFn instanceof GeneratorFunction`
fails on the LEFT one. Sweep: passTotal 29944 → **29946 (+2)**, bug −2,
incompatible and trAccepted both back to baseline, conservation exact
(0 = +2 + −2). The verdict diff moves **four cases total**: three
`bug → pass` on the target surface, and one `pass → bug:exit 138` that
was **proven pre-existing** by rebuilding rotation 388's HEAD — the
guarded run was already 139 there, so the layout change only made a
latent UAF surface on the `tr run` path. Gate predicate **243
unattributed clusters / 3177 cases / register 2 · 626 / residue 763 ·
986 / core 4789** — identical to 388, as it should be: these knives
moved `bug`↔`pass`, not the incompatible face. The syntax half — the
right-hand side accepting a general expression — is measured and filed
as `.claude/rfcs/20260813-instanceof-general-rhs/rfc.md`, and needs no
new runtime code.)

**Previous @ `e0b3c267`** (2026-08-13, rotation 388 — the replacer rotation
387 refused, actually built, plus a wider `undefined` fold it turned up.
§25.5.2.2 step 3's `Call(replacerFunction, holder, «key, value»)` and
§25.5.2.1 step 4.b's PropertyList are both served now: the walk threads
the spec's state record instead of a bare gap slice, and applies the
replacer at every property position — the synthetic `{ "": value }`
root wrapper, array elements under their index, dynobj entries, both
struct field lanes. Measured order: step 2's toJSON runs FIRST (bun
hands a `Date` field to the replacer as its ISO string), so the Date leg
moved into the shared hook. A slot-2 callable, array, or `any` takes the
call out of the static unfold into a new kernel — which also closes the
`Any` residual hole 387 recorded, since callability is now tested at run
time instead of the value being dropped. `this` inside a replacer
resolves too (the kernel was already passing the holder; the compile-time
census entry was missing). **The second finding is not a JSON bug**:
verifying the MDN spelling of a key-dropping replacer showed
`cond ? undefined : anyValue` answering `null` — an `undefined` branch is
a compile-time ConstPtrNull and the mixed-Any widen boxed it with the
non-expr-aware box. The ternary's own wedge exists for exactly this but
only fires when NEITHER side is Any; the same plain box sits in `&&`/`||`.
The contrast that pinned it: the statement spelling of the same function
was already right. A fifth knife made a top-level `undefined` or callable
answer the undefined VALUE (`JSON.stringify(undefined)` had been the
STRING "null"; a callable was a loud reject). Sweep: passTotal 29929 →
**29944 (+15)**, bug +14, incompatible −29, trAccepted +29, conservation
exact (+29 = +15 + +14). All 29 moved cases came out of
`incompatible:type error` — **15 to pass, 14 to bug (they run now and
fail deeper), zero pass regressions**. Gate predicate **243 unattributed
clusters / 3177 cases / register 2 · 626 / residue 763 · 986 / core
4789** — both numbers DOWN against 387's 244 / 3207, and the 29-case
replacer cluster 387 named is gone. Build determinism 44/44 N=12.
Recorded unfixed: `typeof JSON.stringify(x)` still const-folds to
"string" and `=== undefined` to false, because the checker types the call
`String` and cannot yet spell "string or undefined" — the value is right
everywhere, only the static type is not.)

**Previous @ `de2b9387`** (2026-08-13, rotation 387 — a silent wrong found
while extending the `__this` slot table. `JSON.stringify`'s lowering
evaluated slot 2 and dropped it, so a written replacer did not fail: it
produced the unfiltered serialization and looked like a pass. Measured
against bun, `stringify({a:1,b:2}, (k,v)=>typeof v==="number"?v*100:v)`
answered `{"a":1,"b":2}` for `{"a":100,"b":200}`, and `["a"]` answered
the whole object. Invisible to every gate we run — the output is
well-formed JSON, and an identity replacer agrees with it exactly. A
replacer whose CHECKED TYPE is one §25.5.2 step 4 would consult (a
callable or an array) is now refused until §25.5.2.1's PropertyList and
§25.5.2.2 step 3's `Call(replacerFunction, holder, «key, value»)` are
served. The first cut refused everything not syntactically
`null`/`undefined` and the gate priced it immediately:
`stringify(42, step("t1"))` passes a STRING there, which step 4
discards, so ignoring it is the spec's own answer — the bar had to move
from the spelling to the type. Also: `asyncFn().then(fn-expr)` and the
`Array.fromAsync` mapfn joined the no-receiver slot table. Sweep:
passTotal 29932 → **29929 (−3)**, and the sign is the point —
**+2 genuine forward** (`fromAsync/thisarg-omitted-{sloppy,strict}`)
against **5 coincidence-passes removed**. All five were verified case by
case, not assumed: each ran on data where the replacer is a no-op
(`stringify(o,["p"])` where `p` is the only key; three more where
applying vs ignoring produce byte-identical strings), and
`stringify-replacer-with-array-indexes` never ran its in-replacer
assertions at all. **True pass regressions: 0.** bug −22, incompatible
+25, trAccepted −25, conservation exact (−25 = −3 + −22). Gate predicate
**244 unattributed clusters / 3207 cases / register 2 · 626 / residue
762 · 985 / core 4818** — the cluster count went UP by one against
rotation 386's 243 / 3180, which is the "先涨后跌" shape the protocol
warns about: refusing the replacer turned an invisible hole into a named
29-case cluster. Build determinism 44/44 N=12; Guard Malloc 2545/2548
clean, 0 true hits. Second finding, from measuring before writing: the
21 `test/language/statements/with` cases filed under `unknown ident
__this` are MISFILED — `with` has no `Stmt` variant at all, and in
sloppy `.cts` the parser reads it as an identifier, so the `__this`
message is just the first error. Binding `this` there would not move one
of them. A fifth knife let the handler slot be reached through a `const`
name (`const f = function () {…this…}; p.then(f)`), and its sweep moved
**zero** cases with every verdict byte-identical — predicted before it
ran, because those test262 cases spell the binding `var` and the census
requires `const`. It is recorded as a real-code improvement, not a
metric one.)

**Previous @ `4a0d6c1e`** (2026-08-13, rotation 383 — the own-property
surface a builtin prototype was answering one key at a time but could
not enumerate. `Map.prototype.hasOwnProperty("entries")` said true while
`getOwnPropertyNames(Map.prototype)` said nothing; the ownership table
only had the "does it own THIS key" direction, which is all a read or a
gOPD needs. Adding the other direction moved Map 0 → 13, Set 0 → 17,
Array 1 → 40, Date 0 → 47, Object 1 → 12 — bun-identical once sorted.
Around it: `[Symbol.iterator]` installed as a REAL entry on Map / Set /
String prototypes, aliased to the same interned cell the instance table
hands out (§24.1.3.14's "the initial value IS the entries function");
`Map.prototype.get = f` no longer lands enumerable (§10.1.9.2 step 2 —
it is an overwrite of a synthesized own property, not a creation); and
the namespace stand-ins joined the two receiver tables that admit a
symbol key and a delete. Sweep: passTotal 29901 → **29924 (+23)**,
pass **+22**, passNoOracle **+1**, passNegative **0**; bug **+2**,
incompatible **−25**, trAccepted **+25**, conservation exact
(+25 = +23 + 2). **Zero pass regressions**; 33 verdict movements, all
forward. Gate predicate **243 unattributed clusters / 3183 cases /
register 2 · 630 / residue 763 · 985 / core 4798** — every one of the
five improves on rotation 382's 244 / 3204 / 2 · 633 / 763 · 986 /
4823. Build determinism 44/44, N=12. The lesson worth carrying: a
release-only SIGTRAP rode in on the Array.prototype leg and the
conformance gate could not see it — `target/iter/tr` ran the same source
exit 0. Only the sweep's per-case verdict diff caught it, as a single
`pass-no-oracle → exit 133`. The narrowing showed the ENTRY is fine
(installing the identical one from user code crashes nothing) and the
MINT-TIME path is not, so that one wire came back out and the kernel it
needed stayed in.)

**Previous @ `b60b6dbe`** (2026-08-12, rotation 377 — the strict-code
refusals rotation 376's per-function bit made answerable, plus the
position that bit was never armed in. §13.1.1 `eval` / `arguments` in
every binding position (declarations, parameter lists — which also
gained the goal half they never had — catch parameters, and a
function's own name, judged by its own body's directive); §13.1.3 the
same pair as an assignment target, through the one judge every
assignment and update form already shared; §14.11 `with`, both halves —
refused outright in strict code, and refused a Declaration as its body
in sloppy. The missing position: a TOP-LEVEL `"use strict"` armed
nothing, so a sloppy script's directive neither refused a reserved
binding nor made its functions' detached `this` undefined — one bit,
three consumers, all three now answer. Sweep: passTotal 29683 →
**29805 (+122)**: pass **+11** (true forward), passNegative **+111**
(negatives refused at parse phase where the spec asks), passNoOracle
**0**; bug **−117**, incompatible −5, trAccepted +5, conservation
exact (+5 = +122 − 117). **Zero pass regressions.** Gate predicate
**246 unattributed clusters / 3301 cases / register 2 · 654 / residue
761 · 983 / core 4938** — the four numbers hold because the movement is
entirely bug→pass on cases tr already accepted; the `incompatible` face
they measure was not touched. Build determinism 44/44, N=12. TWO
lessons worth carrying: (1) `new Function`'s §15.2.1 early errors were
read off a SUCCESSFUL parse, so each new parser refusal silently
demoted them to the honest-reject arm's runtime TypeError — twice in
one rotation, same fixture; the repair moves the parameter-name check
ahead of the parse and teaches the failure arm to ask whether the text
parses when it is not strict. (2) the gate was GREEN for the with-body
knife and the sweep still found a real pass regression
(`with ({}) let \n x = 1` — §13.16 restricts only `let [`), which the
loop-body rule had already exempted in a helper named for that very
test262 family; the two now share it instead of restating it.)

**Previous @ `868507c6`** (2026-08-12, rotation 375 — the 79-case
`unknown ident __this` cluster: a fn-expr body's `this` desugars to a
`__this` the promote pass only admitted at face positions. Knife D
lands the §10.2.1.2 step-6 callee-side prologue (`__this = __this ??
globalThis` under the sloppy goal, skipped when the directive
prologue says "use strict") — sloppy detached calls now bind the
global object, fixing four standing silent-wrongs (forEach / filter /
some no-thisArg callbacks and `f.call(undefined)` answered undefined
where bun answers globalThis). Four new receiver-safe faces:
NewDynamic inline callee (`new (function () {…this…})`), the throw
operand (any-shaped exception channel), string-pattern
replace/replaceAll callbacks (the str kernel gained the recv-first
argv shift), and inline fn-exprs in explicitly-`any` param slots —
which is what admits the `new Promise(executor)` spelling. The
cluster drops 79 → **68**. Sweep: passTotal 29634 → **29625 (−9)**:
pass **+9** (true forward), passNoOracle **−22 +4 = −18** — the 18
are 10.4.3-1-{36..44}{-s,gs}, nested functions inheriting an
enclosing "use strict" that the body-local directive probe cannot
see; same root as L3b 374-00 (no per-function strictness bit), now
the next rotation's lead item. bug +17, incompatible −8, trAccepted
+8, conservation exact (+8 = −9 + 17). Gate predicate **246
unattributed clusters / 3301 cases / register 2 · 657 / residue
762 · 985 / core 4943**. Build determinism 44/44, N=12. NOTE: bun
answers `object` for a per-function-strict detached `this` in .cts —
its transpile layer appears to drop the directive; the 10.4.3 family
is bun-skip no-oracle so scoring rides tr's own asserts, spec is the
truth source.)

**Previous @ `6d622249`** (2026-08-12, rotation 373 — class instances
grow dynamic members on the WRITE side: the checker's field-miss on
a ClassRef receiver admits an expando definition (§10.1.9.2; the
lowering boxes the receiver into RFC 20260714's +24 expando dict
with its non-extensible gate), a `__priv_` name resolving to a
private METHOD takes a static always-throw lane (§13.15.2 PutValue →
PrivateSet TypeError) and a getter-only private accessor throws
through the runtime accessor kernel — the 113-case `no field on
Struct` cluster drops to **13**. The same knife cleared a
pre-existing silent-wrong: every class/struct member store now marks
its fn may-throw, so a frozen instance's method-body store
PROPAGATES its TypeError instead of being pruned at the caller.
**WeakMap / WeakSet / Date join the exotic-subclass table**
(rotation-371 pattern: weak twins ride the shared iterable kernel
and the Map|Set default-ctor synthesis group; Date mints at now,
`super(v)` reuses the §21.4.2.1 value ladder; ctor-less Date stays
loud pending the real-argc face). Multi-component `new Date(y, m,
…)` components now run ToNumber (§21.4.2.1 step 5). A class
expression inside a class body may extend its enclosing class
(field flattening became a dependency-ordered fixpoint; the
declaration-order cluster 96 → **78**). Sweep: passTotal 29464 →
**29515 (+51)**, bug +40, trAccepted +91 / incompatible **−91**,
conservation exact (91 = 51 + 40). True pass regressions **0**.
Gate predicate **250 unattributed clusters / 3445 cases / register
2 entries · 732 attributed / residue 766 · 990 / core 5167** — all
three headline numbers down (252 → 250, 3543 → 3445, 5256 → 5167).
Recorded boundaries: 373-00 (public method-shadow write stays loud —
static dispatch can't see an expando shadow), 373-01 (private-method
compound assignment blocked on the mixed-binop coercion surface),
373-06 (construct from a runtime class value), 373-07 (class-expr
method bodies capturing enclosing locals). Conformance gate 2768 →
**2772/0/4** (+4 fixtures). Build determinism 44/44 (N=12).)

**Previous @ `c5182f07`** (2026-08-12, rotation 372 — the two headline
clusters both break: **dynamic spread call arguments** (§13.3.8.1,
the 91-case top-2) land as a runtime lane on every tier that owns a
boxed-adapter channel — a spread-carrying call materializes its full
argument list into one Array<Any> (spread sources walk the unified
iteration protocol: typed arrays bridge, strings iterate per code
point, custom @@iterator resolves at runtime, IteratorStep errors
stay catchable) and routes to thin dynamic-argc kernels
(`__torajs_any_call_spread` / `__torajs_any_method_call_spread` /
`__torajs_super_builtin_method_spread`) that enter the existing
dispatch tails; the `.apply` literal-argArray form re-enters the
same lane, and `super.m(...rest)` reaches the builtin-heritage
re-dispatch. The static expanders stay the semantic owners of
known-arity shapes (a gate round proved a too-early wrap axis
hijacks them — retired same rotation). **Sloppy-goal bare-name
`delete`** (the 103-case top-1) resolves per compile goal: the
parser emits a plain Delete{Ident} node and a prelude gate answers
§13.5.1.1 (strict SyntaxError) or §13.5.1.2 statically (declared
bindings and non-configurable globals false, unresolvable true). A
pre-existing silent-wrong also cleared: apply_spread's index-read
expansion trimmed arguments-carrying callees to declared arity
(`f(...s)` on a 0-param fn answered length 0). Sweep: passTotal
29396 → **29464 (+68)**, bug +26, trAccepted +94 / incompatible
**−94**, conservation exact (94 = 68 + 26). True pass regressions
**0**; the +26 bug is the unlocked surface's next stratum honestly
entering the truly-run set (spread through private class methods,
delete edge shapes, Promise.any resolve probes). Gate predicate
**252 unattributed clusters / 3543 cases / register 2 entries · 733
attributed / residue 761 · 980 / core 5256** — both headline
numbers down (253 → 252, 3608 → 3543). Recorded boundaries: L3b
372-00 (named-FnDecl spread beyond static expansion needs a
real-argc ABI face), 372-01 (dynamic-argArray `.apply` silent-wrong
rides the same face), 372-02 (super(...args) spread + class-ctor
arguments face). Conformance gate 2764 → **2768/0/4** (+4
fixtures). Build determinism 44/44 (N=12).)

**Previous @ `d9b8daf4`** (2026-08-12, rotation 369 — the console
surface closes four knives deep behind RFC 20260812-console-sink:
the print/inspect family streams through a torajs-io current-sink
indirection (STDERR LineBuf twin + putc_out/write_out + a
drain-on-switch pair that keeps `2>&1` caller order),
`console.error` / `console.warn` route EVERY runtime type to stderr
through the one `emit_console_print` gate (the four per-type `_err`
intrinsics retire; the fixture caught `print_i64`/`print_bool`
raw-writing fd 1 past the sink — the io doc claiming they route
through putc was stale), the stderr pair becomes first-class
ns-static cells, and the console namespace object mints as a
singleton joining the globalThis fill (MISSING_KNOWN 11 → 10, Web
IDL `[object console]` badge). test262: **zero movement — verdicts
byte-identical across all 53174 cases** (passTotal 29307 / bug
12731 / incompat 11136 / gate predicate 262 clusters · 3686 cases,
all unchanged): the console surface is a conformance-gate face
(stdout+stderr split byte-parity vs bun), not a test262 face —
test262 never asserts console stream routing. Conformance gate
2752 → **2755/0/4** (+3 fixtures: types / cell / singleton). One
new recorded boundary probed honest: an ns-alias member CALL
(`const c = console; c.log(x)`) rejects loud in the same family as
`const m = Math; m.max(1,2)` — registered in plan-state L3b with
the dynamic cast as the working escape hatch. Build determinism
44/44 (N=12).)

**Previous @ `e96477f7`** (2026-08-12, rotation 368 — the globalThis
surface closes five knives deep: the G3 descriptor probe finds the
gOPD global arm already working (verifyProperty-shape reads and
aliased-spelling mutation probes byte-match bun, closing the L3b
entry without code); MISSING_KNOWN shrinks 19 → 11 — parseInt /
parseFloat reuse the Number.* cells (§21.1.2.12/.13 same-object
clause, identity free), isFinite / isNaN get their own
ToNumber-coercing cells, and the §19.2.6 URI quartet lands whole: a
real Encode/Decode kernel pair in torajs-str (uri.rs — code-unit
walk over both encodings, strict escape-run parsing with from_utf8
as the overlong/surrogate/range judge, decodeURI preserving a
reserved escape's original spelling), URIError as a real runtime
raise (torajs-throw slot 6 + factory registration + implied class
injection + throw-info marking), then value cells + fill-list
entries completing both call forms. The eight global function
properties also open as bare VALUES (checker types the read
concretely, ident lowering mints the interned cell, a let-alias
registers variadic for the boxed dual entry); under-arity alias
calls and the HOF-callback position stay recorded loud boundaries.
Sweep: passTotal 29137 → **29307 (+170)**, bug +8, trAccepted +178
/ incompatible **−178**, conservation exact (178 = 170 + 8).
Forward 170: decodeURIComponent 51, decodeURI 50, encodeURI 26,
encodeURIComponent 26 (the former top-1 unattributed cluster clears
whole), global/isFinite/isNaN/parseFloat/parseInt 13, sm 2, Array 1.
The +8 bug is the non-constructor family (`new parseInt()`-shape
cases) honestly entering the truly-run surface. True pass
regressions **0**. Gate predicate **262 unattributed clusters /
3686 cases / register 2 entries · 763 attributed / residue 765 ·
989 / core 5438** — both headline numbers down (268 → 262, 3862 →
3686). Build determinism 44/44 (N=12). Conformance gate 2747 →
**2752/0/4**.)

**Previous @ `9ca6ca25`** (2026-08-12, rotation 367 — the @@toPrimitive
protocol hook lands (§7.1.1 steps 1.a-1.c: GetMethod through the
symbol walk, hint delivered by name, object answers refuse, nullish
falls through to OrdinaryToPrimitive) plus the §21.4.2.1
`new Date(value)` kernel (Date copies [[DateValue]], objects run the
no-hint ToPrimitive, string answers PARSE); `delete` gets the
void-fold disambiguation and the String-receiver §10.4.3 arm; a
dynamic function's sloppy `this` binds to the G2 globalThis singleton
per §10.2.1.2 (token-gated: strict prologue and nested-function
bodies keep the loud reject), which made the stock fnGlobalObject.js
harness portable — 130 include cases enter the truly-run surface.
Registers: SR-2 (source-phase-imports proposal, 93 cases) with the
new `test262-feature` predicate kind. S5.6 recon corrected the stale
roadmap entry (global object shipped 60 rotations ago); residue in
plan-state L3b. Sweep: passTotal 29048 → **29137 (+89)**, bug
**+53**, trAccepted +142 / incompatible **−142**, conservation exact
(142 = 89 + 53). Forward 89: annexB eval-code 14, String trim 9,
instanceof 9, Function/prototype 8, equals 6, the whole
Date/value-symbol family. The +53 bug is the honest inflow of
unlocked fnGlobalObject sloppy cases (annexB eval/global-code 64,
Function/prototype 30) now truly running. True pass regressions
**0**. Gate predicate **268 unattributed clusters / 3862 cases /
register 2 entries · 763 attributed / residue 766 · 991 / core
5616** — both headline numbers down (270 → 268, 4023 → 3862). Build
determinism 44/44 (N=12). Conformance gate 2744 → **2747/0/4**.)

**Previous @ `03200808`** (2026-08-11, rotation 366 — the sign-off
delegation lands and the subset-decision register goes live: SR-1
attributes the 752 noStrict-flagged core cases (sloppy-only surface —
tr's TS-module surface is always strict, same as bun's `.ts` face),
`cluster_incompat.py` applies register predicates mechanically, and
the gate predicate re-baselines **293 → 270 unattributed clusters /
4764 → 4023 cases** with cause and magnitude recorded. Substrate: the
Function/bind family fell — the bind synth lane's `->any` capture lie
on unannotated-ret closure bindings (silent garbage tags) routes to
the runtime kernel; bound functions construct per §10.4.1.2
(boundArgs ++ args, newTarget→target recursion, bound-over-bound
unwinds); `arguments` inside constructor arguments joins all three
walks (Expr::New/NewDynamic arms); `fn as any` joins the forwarder
wrap axes. One knife REVERTED by the sweep: the forwarder recv-first
public face lost 128 passes (gen-argv `[...arguments]` swallowed the
receiver; strict-this lanes misaligned) — bisected, reproduced,
reverted same rotation; receiver-through-forwarder is registered as
its own caller-audited knife. Sweep: passTotal 29032 → **29048
(+16)**, bug **−16**, trAccepted / incompatible flat, conservation
exact. Forward moves: Function/prototype/bind 14 (the whole probed
family), DisposableStack newtarget-abrupt 2; true pass regressions
**0**; one bug-internal move (bind 4-5 exit3→exit1). Crash counts
flat (139=39, 138=64, timeout=5). Gate predicate **270 unattributed
clusters / 4023 cases / register 1 entry · 752 attributed / residue
760 · 983 / core 5758**. Build determinism 44/44 (N=12). Conformance
gate 2740 → **2744/0/4**.)

@ `c025fd3f` (2026-08-11, rotation 365 — five knives on the
`identifier arguments` bug bucket, all t262-harness-driven shapes. The
harness shadow poison closed: `__t262_throwsAsync`'s param `func`
(captured by its own inner closure) killed every same-named top-level
argv binding — the knife-7 bind-receiver gate on the shadow-aware kill
walk lifted (its rest-tail-mint precondition now exists), which
exposed the second half of the old 27-SIGSEGV shape (a promoted
top-level binding resolves through the globals fallback, missing the
let-decl variadic registration) — closed by the closure-local lane's
`global_argv_face_binding` boxed route. The boxed-only admit template
(no site can name the fn → no lane can reach the old signature → the
universal boxed adapter is the only entry) landed three instances:
zero-site objlit field closures (the Date `arg-to-number` valueOf
idiom, this/value head-shape split), boxed-face store positions
(`spy[Symbol.toPrimitive] = fnexpr`), and the fn-arg track (an
argv-face fnexpr passed as a user fn's annotated param, the param's
direct calls registered variadic via the new per-fn-keyed
`ast.argv_boxed_params`). Plus a fix-grade double silent-wrong: Date
setter `? ToNumber(arg)` abrupt completions were dropped AND the
garbage wrote [[DateValue]] — decode_fields now aborts pre-write.
Sweep: passTotal 28945 → **29032 (+87)**, bug **−89**, trAccepted −2,
incompatible +2 (two reduce cases surfacing their checker reject),
conservation exact (−2 = +87 − 89). Forward moves all direct-hit:
Date/prototype 34, Array/prototype 33, for-of 5, assignment 5,
defineProperties 5, String 4, keys 1; true pass regressions **0**;
crash counts flat (139=39, 138=64, timeout=5). Gate predicate **293
clusters flat / 4764 cases flat / residue 764 / 994 / core 5758**.
Build determinism 44/44 (N=12). Conformance gate 2735 → **2740/0/4**
(+5 fixtures, chain zero-red).)

**Previous @ `1e1c6083`** (2026-08-11, rotation 358 — five blades on the
argc/repr seam. Objlit-this receiver fix: a method body reading both
`this` and a user param split its literal into a width-twin sid (the
method fn's analyzed F64 ret vs the `__ObjLit_<n>` TypeDecl's static
I64 parse), so `is_objlit_method_slot`'s alias-sid equality missed and
the callee read `k` as the receiver (SIGSEGV) — fixed by joining the
literal onto its nominal class (constructor glue mirroring `__new_C`,
`nominal_alias_names` now includes `__ObjLit_*`, and pass 0.5's
TypeDecl fill widens fn-sig faces by field key), which also fixes the
sibling `this.other()` wrong-register-lane read. S3.5: universal
beyond-arity admit per ES §13.3.6.1 — the checker's general arity
gate now evaluates extras for type errors then truncates, retiring
the name-keyed length-only closure wedge; struct-method dispatch
emitters skip extras past the declared list (evaluated, released,
never in argv — the S1 argc slot already carries the real count).
Two sibling repr holes: a fn-sig-annotated binding initialized from a
closure-repr IDENT or from a struct's fn-typed FIELD read now keeps
the cell repr (both used to call_indirect the cell header, EXIT=138).
Escaped-closure forward face: length-only env-first bodies read the
S1 hidden argc with NO binding-chain admission — container-stored /
returned / passed-along closures (previously loud-rejected) now
answer the true count. Sweep: passTotal 28881 → **28901 (+20)**, bug
+10, trAccepted +30, incompatible **−30**, conservation exact (+30 =
+20 + 10). Verdict moves all forward: incompat→pass 8, incompat→bug
22 (newly reachable cases exposing their true holes), bug→pass 12;
true pass regressions **0**; crash/timeout counts flat (138=64,
139=39, timeout=67). Gate predicate **296 clusters flat / 4813 cases
(−13) / residue 756 / 982 (−1/−2) / core 5795 (−15)**. Build
determinism 44/44 (N=12). Conformance gate 2701 → **2705/0/4** (+4
fixtures + 1 fixture extension, chain zero-red).)

**Previous @ `f8146176`** (2026-08-10, rotation 356 — the two orthodox
blades from r355's regression triage landed, plus the S3 opening.
Self-tail-call elimination (egraph SELF_TAIL_CALL, RFC
20260810-self-tail-call): `return f(args)` self-recursion through a
named-fn-expr's self slot rewrites to a parameter-rebind loop behind
a runtime cell==env guard — O(1) stack at 10M depth, rc protocol
loop-carries the params (pre-block retain / per-ret release /
inc-new-dec-old handoff through parallel-move temps), and the
"ok-block decs are exactly call args" match doubles as the
scope-drop exclusion. AAPCS64 stack args: the 9th+ register-class
argument travels through an outgoing area carved into the caller's
frame bottom, with a shared classify_lanes on both sides and a
prologue copy-in on the callee side — the old 8-GPR ARG_RET OOB
panic face is gone. S3.1+S3.2 (indirect-argc-abi): env-first faces'
`arguments.length` now reads the S1 hidden argc; the iife tier's
param injection + call-site prepend retired in the same cut (the
A-station counts every user-position argument, so the prepended
literal inflated the hidden argc by one — the Phase A step plan was
wrong about these being separable), length-WRITING bodies ride a
synthesized writable `__torajs_argc_len` local, and the this-first
method face stays on the injected param. Sweep: passTotal 28857 →
**28881 (+24)**, bug −14, trAccepted +10, incompatible −10,
conservation exact (+10 = +24 − 14). Forward 24: tco-* ×16 (the 14
r355 regressions all recovered + 2 pre-existing), for-of arguments
×2, gen/async-gen-method dflt-params ×6 (≥9-param cap unlock). True
pass regressions **0**. Remaining tco exit-139: 9 (switch /
conditional / call-args / for-lhs / for-var / try-catch SSA
variants the v1 shape match skips — L3b). Gate predicate **296
clusters (−1) / 4825 cases / residue 757 / 984 flat / core 5809
(−10)**. Build determinism 44/44 (N=12). Conformance gate 2695 →
**2698/0/4** (+3 fixtures; one 2-fail link — the length-write and
method-argv faces — fixed same rotation).)

**Previous @ `380172de`** (2026-08-10, rotation 352 — the promise
iterator-interleave RFC landed whole, I1-I4 in six substrate commits
plus a file-cap refactor. Promise cells grew a lazy expando props
bag (layout 32 → 40; defineProperty on a promise receiver was a
silent no-op before), the per-element `then` GET observation reads
it (data + accessor getters), and all four dyn combinators now
iterate the spec's way: the loop probes each promise element for a
user `then` override, activates a growable fan-in block on the
spec's remaining-sentinel protocol, hands the override a freshly
minted resolveElement / rejectElement pair (per-mode entry matrix),
and a then GET/INVOKE throw closes the iterator once and rejects —
which is what unhangs the infinite-iterable invoke-then-error-close
family. fromAsync's mapped form iterates per element likewise
(mapfn throw / rejected element close). The probe chain also caught
a latent codegen double-spend: an object-literal field initialized
from a BORROWED binding (closure captures are registered
moved-at-birth) bare-stored the env's stake — `return {value:
captured}` from a nested fn freed the captured cell on the first
call, and for-of over such literals rode the same hole; borrowed
bindings now always inc into the field. Sweep: passTotal 28818 →
**28820 (+2)**, bug +4, trAccepted +6, incompatible **−6**,
conservation exact (+6 = +2 + 4). tr-timeout **11 → 5**: fromAsync
mapfn ×2 went timeout → PASS; the four Promise
invoke-then[-get]-error-close cases went timeout → bug:no-oracle
exit 1 — the interleave itself works (close runs once, the
assertion passes), the nonzero exit is the unhandled-rejection
fatal host semantic, byte-identical with bun's (test262's official
host is non-fatal there; registered L3b as a host-semantics
census). Gate predicate **304 clusters flat / 4956 cases (−6) /
residue 763 / 997 flat / core 5953 (−6)**. True pass regressions 0
(the verdict diff is exactly the six timeout conversions). Build
determinism 44/44 (N=12). Conformance gate 2675 → **2685/0/4**
(+10 fixtures, chain zero-red).)

**Previous @ `76c7061a`** (2026-08-10, rotation 351 — the arguments
exotic object's whole reflective face landed in seven knives.
`FLAG_ARR_ARGUMENTS` (Tag::Arr-private bit 1 — the survey flipped
the planned bits 10/11, which are the elem-kind field) stamps the
materialized `__torajs_arguments` cell via a synthetic mark call
both desugar lanes append; the keyed readers gate on it. The
`length` face carries §10.4.4's plain-data attributes (gOPD
configurable true; delete leaves the element-domain hole tombstone
under the "length" key — every enumerator already skips holes;
hasOwnProperty and the any-lane member read see the tombstone; the
direct `delete arguments.length` spelling routes as a keyed delete).
`callee` is the %ThrowTypeError% accessor across all three touch
positions (read rewrites to a thrower call; write evaluates the RHS
then throws — §13.15.2 order; delete throws per §13.5.1.2 step 3.a;
the thrower joins the may-throw analysis — without it a pending
TypeError strands across the fn boundary, the S10.6_A3_T4 SIGSEGV,
caught by the mid-rotation sweep and fixed same rotation), answers
the interned thrower pair through gOPD (get === set), and joins the
hasOwnProperty arm. The [[Prototype]] answers %Object.prototype%
(§10.4.4.7 step 2) — whose comparison target exposed an independent
hole: INLINE `Object.getPrototypeOf({})` read a dynobj header field
as a class tag through the Obj arm (an empty struct layout is a
dynobj at runtime); empty-layout receivers now ride the runtime
classifier. `delete arguments[i]` rides the materialized array
(mutation walk + rewrite both learned Delete). Sweep: passTotal
28782 → **28818 (+36)**, bug −36, incompatible flat, conservation
exact (0 = +36 − 36). +36 pass all by-name in the arguments /
delete families; +1 pass-no-oracle (ThrowTypeError
unique-per-realm) against −2 de-dilution (10.6-12-1 / S10.6_A3_T4:
noStrict sloppy callee-write round-trips were passing on the
expando-store accident; strict throw semantics is correct). Gate
predicate **304 clusters / 4962 cases / residue 763 clusters 997
cases / core 5959 — all flat** (the whole yield is bug→pass, which
never touches the incompat census). Crash triple: exit-139 by-name
+1 (sm/function-caller-skips-eval-frames unlocked from its compile
reject into a deep caller-eval combination — pending throw across
the eval boundary, L3b; minimal reproductions all clean),
tr-timeout 11 flat, exit-138 3 flat. Build determinism 44/44
(N=12). Conformance gate 2668 → **2675/0/4** (+7 fixtures, chain
zero-red. Rotations 348-350 landed between this stamp and the
previous one — sparse-grow Phase 1 and the delete/class knives —
their numbers live in plan-state / the handoff chain.)

**Previous @ `4d626200`** (2026-08-10, rotation 347 — four knives off
the recorded queues. The yield-into temp binds inside generator
EXPRESSION bodies (free_vars treated it as expression-only, so
every expression-position yield "captured" its own temp and the
hoist panicked — the 20-case yield-spread/dstr family). Rest params
materialize through the any-lane boxed adapter (argv[fixed..argc]
collects into a fresh Arr<Any>; both forwarder synthesis sites now
spread-forward the rest param so apply_rest_args does not re-pack —
the [null,true,null] family that blocked fromAsync). Promise.allKeyed
/ allSettledKeyed land end-to-end (await-dictionary: null-proto
keyed result, §10.1.11.1 order, records translated into real
{status, value|reason} dynobjs — sidestepping the recorded
record-identity blindness; bun lacks the proposal so the fixture
carries an .expected oracle). And `og instanceof G` reaches a bare
top-level FnDecl through its __forward canonical cell (§7.3.22
against the same fnprops cell the construct kernel links). The
first gate run flushed out a pre-existing promise-pool double-drop
(unobserved rejected combinator promise underflows; the freed cell
recycles under the microtask queue — full evidence dossier in
plan-state, next knife). Gate chain 2654 → 2658/0/4 zero-red (+4
fixtures). Sweep passTotal 28678 → **28707 (+29)** / bug +7 /
trAccepted +36 / incompatible **−36**, conservation exact. One pass
regression re-baselined as de-watering: the old Reflect.apply pass
rode the broken rest-adapter's garbage read; the honest collection
exposes closure length-accessor-define as silently ignored
(recorded). exit-139 31 (+1 — a newly-runnable allSettledKeyed case
hitting the recorded pool UAF); exit-138 3 flat; tr-timeout 36
flat. Gate predicate: 305 clusters (flat) / 5078 cases (−38) /
core 6069 (−36).)

**Previous @ `9379c782`** (2026-08-09, rotation 346 — the `__this`
cluster and the empty-struct family fall through one root cause:
function-this stops riding the free-var walk. A nested `function`
(and a marked fn-expr / objlit method) binds its own `this`
(§10.2.1.1), but the walk reported the body's `__this` as free in
the enclosing scope — every argument-position fn-expr enclosing a
FnDecl-this lifted with a phantom capture its scope cannot supply
(the whole 99-case unknown-`__this` cluster). Three boundaries land
in `free_vars` (FnDecl / marked-fn-expr / objlit-method bodies bind
`__this`; a lifted function-this closure's `__this` capture is the
promote protocol's marker, not a lexical need, and stops riding
into encloser snapshots), the promote candidate walks recurse
through every compound-statement body via a shared spine
(`stmt_nested_lists` — the capturing-lane `const` inside
desugar_async's Try never reached the candidate set), and a seventh
receiver-safe face position admits fn-exprs returned from objlit
method/accessor bodies (any-lane consumption, every call path
flag-aware — the old capture-supply path was receiver-correct only
by coincidence). Alongside: zero-field struct member reads answer
Any (the `var o = {}` apply-thisArg shape, 91-case cluster);
Undefined-typed keys join the property-key domain
(ToPropertyKey(undefined) = "undefined", the 69-case dstr harness
cluster); uninitialized static fields become real mutable slots
(two stale gates, the 68-case rs-static-privatename family); and
Array.fromAsync constructs through a constructor this-value (RFC
20260808 B6 knife 4 — readonly-elements now completes end-to-end).
One sweep-caught regression (`0, { yield } = {}` phase mismatch)
fixed same-rotation at the §13.15.1 gate. Gate chain 2647 →
2654/0/4 zero-red (+7 fixtures). Sweep passTotal 28416 → **28678
(+262)**, incompatible 12165 → 11832 (−333), conservation exact
(+333 = +262 pass + 71 bug), zero pass regressions. Gate predicate:
305 clusters (−7) / 5116 cases (−319) / core 6105 (−333) — both
numbers down together again.)

**Previous @ `68b66460`** (2026-08-09, rotation 345 — cluster #2
dissolves and the fn-expr `this` family closes four layers deep.
G2.5 narrows the globalThis mutation gate from all-property-writes
to builtin-name overrides only: expando stores / updates / deletes
(member, string-literal index, computed key) ride the singleton's
existing dynobj lanes — a bare read of an expando name is already a
compile-time unknown-identifier reject, so the write/read pair
cannot diverge silently; `globalThis.Array = x` stays loud. The
whole 113-case "assigning to / deleting a property of globalThis"
cluster dissolves to ZERO. The `__this` family (102 cases) gets its
knife chain: explicit-`any` param argument positions admit (the
any lane honors FLAG_CLOSURE_RECV_FIRST on every call path);
generic type-param slots admit behind a greatest-fixpoint call-free
proof over every same-name use in the program (eq / typeof / `!x` /
explicit-any let-init / recursive safe-param argument are harmless,
anything else refutes — and the any-let walk must reach every
body-owning stmt shape, the if-branch decl in sameValueCheck caught
the FnDecl/Block-only first cut); the argv face survives the same
argument escapes (escape-store's twin, so this+arguments fn-exprs
materialize through the boxed dual entry); `o instanceof C` gains a
fn-value lane (§7.3.22 OrdinaryHasInstance via a runtime prototype
walk) — which flushed out that `fn_prototype_pair` minted a FRESH
prototype on every call because its probe lived in the member-get
callers only, so construct products diverged from the canonical
cell (probe now lives in the mint itself); and `new C()` callees
join the construct-channel shapes. The Array/from/iter-cstm-ctor
t262 shape passes all asserts end-to-end. Gate chain 2641 →
2647/0/4 zero-red (+6 fixtures). Sweep passTotal 28377 → **28416
(+39)** / bug +37 / trAccepted **+76** / incompatible **−76**,
conservation exact (+76 = +39+37; the unlocked cases split between
direct passes and honest next-blocker bug entries — import-defer
semantics, `with`, FnDecl-this). **Zero pass regressions** (246-line
diff, all forward). exit-139 30 flat / exit-138 3 flat / tr-timeout
35 flat — no new crash surface. Gate predicate **312 clusters (+3,
the known unlock-exposes-signatures shape) / 5435 cases (−72) /
residue 767 clusters 1003 cases / core 6438 (−76)**, register still
empty. Build determinism 44/44 (N=12), zero compile failures.
Recorded next: the `__this` residue (100 cases) is dominated by
**FnDecl-this** — `function MyArray() { this.length = 4 }`
constructed via `Array.fromAsync.call(MyArray, …)` — a separate
shape family the fnexpr passes never touch; plus Any-binding rhs
instanceof, and the NewDynamic-callee promote admit's checker face.)

**Previous @ `58ae9866`** (2026-08-09, rotations 343+344 — the B6 tail
closes, then the dynobj backing-store split lands. Rotation 343 gave
the generator family its only real inheritance point
(%Iterator.prototype% true symbol-keyed own entries installed at
tag-15 mint; kind-1 gen_proto carries §27.1.6.1 [@@asyncDispose]) and
pinned SuppressedError.length to 3; its survey found the root cause
behind the r332 error/proto cluster and the r342 at-exit SIGSEGVs:
dynobj resize freed the old block while every other owner
(`__proto_<C>` global, classmeta registry, instance chains, aliases)
still pointed at it. Rotation 344 knife 1 split the header from the
backing store (CPython ma_keys shape, RFC 20260809-dynobj-store-split
A+B+C+D in one atomic cut): the 32B header cell address is immortal,
resize swaps the store pointer under it, the whole owner set stays
valid across growth — the r6.ts at-exit crash repro exits 0, churn
RSS flat, bench ratios flat. Knives 2-3 land the unblocked residue:
the dispose alias pairs + @@toStringTag entries (store-split-gated
own writes), and the Error-family message-ToString knife (§20.5.1.1
step 3 — message: any, single-point coercion in the root ctor).
Knives 4-6 clear a bug-327/async family: `Math.sin` as a VALUE into a
fn-typed param now wraps in a `__forward_ns_*` closure (the
dispatcher cell's fn_addr is the RFC B4 loud reject, so the `__cls(`
typed lane needs a real closure; the forwarder signature parses from
the receiving slot's own annotation); desugar_async's top-level index
scan becomes a depth-first walk so NESTED async fn decls finally get
their Promise wrap; and rewrite_returns_for_async gains its missing
ForOf arm (a `return` inside an async body's for-of leaked its bare
value). Gate chain 2631 → 2641/0/4 zero-red (+10 fixtures across the
two rotations). Sweep passTotal 28196 → **28377 (+181)** / bug
**−16** / trAccepted +165 / incompatible **−165**, conservation
exact; **zero pass regressions** (343: 4-line diff; 344: 716-line
diff all forward). exit-139 34 → **30 (−4 net)** — the store split
keeps cashing in; exit-138 3 flat, tr-timeout 35 flat. Gate predicate
**309 clusters (−2) / 5507 cases (−157) / residue 768 clusters 1007
cases / core 6514 (−165)**, register still empty — both axes fell
together again. Build determinism 44/44 (N=12), zero compile
failures. Recorded next: typed-receiver `Object.hasOwn(new Error(),
"message")` sentinel-blind face, @@toPrimitive numeric-coercion
dispatch, script-mode distinction, early_redecl fn-expr-body blind
spot, promise micro-tick ordering.)

**Previous @ `b02e97d0`** (2026-08-09, rotation 342 — DisposableStack /
AsyncDisposableStack land as injected TS-source builtins (RFC
20260809 B5 + the B6 core faces, five substrate commits, gate chain
2626 → 2631/0/4 zero-red). The opener is an rc knife the injection
itself flushed out: a nominal-struct `any`-field read handed out a
bare-Load borrow while the field-assign lane drop-olds the slot's
stake (chunk 563 slot-owns contract), so `const st: any = this.__s;
this.__s = []` freed the entries chain under a live binding and the
at-exit cycle drain segfaulted three layers away — the read now
mints owned (payload +1, `owned_member_reads`), the rotation-324
contract extended to the last unconverted read lane; underflow
census 0 maintained, 300k-churn AOT RSS flat. The classes are plain
TS parsed via `parse_into` (the `desugar_using` HELPER_SRC
convention), demand-gated, front-spliced, with `{v,d,k}` dynobj
entry records and injected async walk helpers that ride the
ordinary async desugar — knives 1-4 of rotation 341 carry the whole
implementation with zero new runtime surface. B6: SuppressedError's
three ctor params go optional (AggregateError keeps its required
face — §20.5.7 iterates errors); %Iterator.prototype%[@@dispose]
reifies on MapIter / ArrIter / IterHelper (GetMethod-return
semantics: no-op for Map/Set/Array iterators, an Iterator Helper
closes through its own return under the executing gate), making
iterators real `using` resources. Sweep passTotal 28042 → **28196
(+154)** / bug **−147** / trAccepted +7 / incompatible −7,
conservation exact; **zero pass regressions** (432-line verdicts
diff all forward). exit-139 32 → 34: +3 −1, all inside the
newly-runnable DisposableStack family — a non-underflow at-exit
drain corruption (detector reports zero, release AOT clean, iter
spawn-child crashes in mark_gray); minimal repro = top-level `var`
stack + use + defer + double dispose, recorded for the collector
chase. Recorded next: generator [@@dispose] (the right altitude is
a real symbol-keyed entry on the %Iterator.prototype% dynobj — the
gen proto chain already reaches it), async-gen [@@asyncDispose],
prototype[@@dispose]/.dispose function identity, @@toStringTag
prop-desc, SuppressedError length/ToString faces.)

**Previous @ `6aa2a36d`** (2026-08-09, rotation 341 — `using`
declarations land end-to-end (RFC 20260809, five knives, gate chain
2622 → 2626/0/4 zero-red). Knife 1: the parse-reject upgrades to a
real `Stmt::UsingDecl` and a prelude `desugar_using` pass rewrites
every resource scope to the textbook dispose-stack try/catch/finally
(reverse order, null/undefined skip, bind-time method read,
SuppressedError aggregation, for-init loop-exit timing) over
parse_into-injected helpers; computed-key object literals now answer
Any at every position, unblocking `return { [Symbol.dispose]() {} }`
from the struct-lane panic. Knife 1b: the anylane recv promotion
gains the (g) computed-key leg — a this-using method shorthand in
such a literal had kept a nominal `__this` and written its receiver
onto a struct-unbox copy (`this.x = 99` did not transmit; dispose
identity broke). Knife 2: `await using` parses wherever `await` is
legal and disposes through an injected async helper pair
(@@asyncDispose first, @@dispose sync fallback un-awaited, null
binding still one tick), awaited in the finally via the parser's own
`.value`-read await spelling. Knife 3: `for ([await] using x of …)`
heads wrap the body in a per-iteration UsingDecl (spec
dispose-at-end-of-each-iteration for free); for-in heads and
single-stmt-body positions reject per the negative family; the
`{ async [key]() {} }` stub-drop dies. Knife 4: the class computed
Symbol.<x> key fold narrows to exactly `Symbol.iterator` — every
other symbol chain reifies under the real Symbol cell, so
`c[Symbol.dispose]` / `C[Symbol.asyncDispose]` finally resolve.
Sweep passTotal 27910 → **28042 (+132)** / bug +80 / trAccepted +212
/ incompatible **−212**, conservation exact. The six "regressions"
are all coincidental passes evaporating: four script/eval-top-level
negatives that only passed because using used to parse-reject
(script-mode distinction is the real gap, recorded), two
redeclaration negatives now exposing the early_redecl fn-expr-body
blind spot (recorded). using slice 0 → 55/78, await-using 0 → 48/94;
the former top-1 `using` cluster (138) leaves the board. Gate
predicate **311 clusters (flat) / 5671 cases (−206) / residue 771
clusters 1015 cases / core 6686 (−212)**, register still empty.
Recorded for the next knives: DisposableStack/AsyncDisposableStack
injection (B5, 178 unknown-ident cases; its class-computed
prerequisite fell this rotation), @@toPrimitive numeric-coercion
dispatch, method-body self-reference, nested-async-fn-decl await
crash (exit 138, pre-existing), SuppressedError optional-params
checker face.)

**Previous @ `5894bd1e`** (2026-08-09, rotation 340 — Array.from's
whole call surface closed in three knives (RFC
20260808-construct-channel B6, the trunk's last blade). Knife 1
gives the detached value a real §23.1.2.1 any-tier kernel
(torajs-anyvalue/array_from.rs: step-2 IsCallable before iteration,
the unified iterable/array-like cascade, the exact «kValue, k»
mapfn call shape with thisArg on the receiver channel, per-index
Get/map interleave) — the loud reject retires. Knife 2 opens the
ns-static RECEIVER channel: Array.from / fromAsync cells carry
FLAG_CLOSURE_RECV_FIRST (keyed off the dispatch table), every
receiver-honoring caller prepends the thisArg in argv[0], and the
kernel's step-4 split constructs through C — Construct(C) for
iterables, Construct(C, «len») for array-likes, elements through
the species define-semantics store. Same channel:
__torajs_reflect_construct grows the plain-fn Closure arm (the
pre-B1 path only knew the class registry, so
Reflect.construct(function(){}) always threw); fnexpr-this routed
promotion admits construct-channel argument positions (whitelisted
callees) and equality operands (the t262 identity-assert form);
the checker types a reified ns-static's .call/.apply/.bind as
any-dispatched and the fn-call-value this-drop replay lets those
through to the runtime cell. Knife 3 converges the typed lowering:
the array-like struct arm's undefined-filled stub retires (index
properties really read), a SHARED checker/lowering predicate
routes the mapFn escape shapes (non-fast-lane source /
non-Function mapfn / explicit thisArg) to the new
__torajs_array_from_dyn kernel while Str/Array/Set + Function
2-arg keeps the typed devirt loop, and a nullish source compiles
into the runtime step-5 TypeError. fromAsync's ~29
stdout-mismatch cases were re-attributed to the
collect-then-settle snapshot vs the spec's per-element async
interleave (iterator liveness) — a promise-kernel structure blade,
registered L3b; the receiver argv offset landed with knife 2.
Sweep: passTotal 27900 → **27910 (+10)**, bug +7 (Array.from
shapes now run to their honest failures), incompatible −17,
conservation exact (17 = 10 + 7). All 18 moves forward, ZERO
regressions: 10 straight to pass (items-is-null-throws /
iter-cstm-ctor-err / mapfn-is-not-callable / mapfn-is-symbol /
this-null / source-object ×5), 7 incompatible → bug (running,
behavior-diff residue), and iter-map-fn-err's tr-timeout resolved
(36 → 35). Gate predicate **311 clusters (flat) / 5877 cases (−9)
/ residue 774 clusters 1021 cases / core 6898 (−17)**, register
still empty. Conformance gate 2618 → **2622/0/4** (+4 fixtures,
zero red gates). exit-139 32 / exit-138 3, both flat by name.)

**Previous @ `6a724884`** (2026-08-09, rotation 339 — the species
second key landed in four knives and the create-species family
flipped. Knife 1 merges the two receiver collectors into ONE walk
(`collect_props_receiver_binding_names`): a name admits when its
EVERY declaration is a runtime-props shape (`any` / `T[]`
annotation, bare `{}` or array-literal init) — the retired pair of
separately-walked collectors each classified the other's shape as
"other", so a t262 harness helper's `const a: any` and the case
body's `var a = []` under the same name killed each other and the
B2 store arm never admitted `a.constructor[Symbol.species] = Ctor`.
Knife 2 lets `Ctor.prototype` reads (create-species.js's assert
shape) survive both guards — the arguments kill walk (exemption
gated on an escape-store use after a same-rotation regression fix:
the fn-ctor desugar SYNTHESIZES `F.prototype` reads, and the
ungated exemption floated store-free fn-expr ctors off their
static-argv channel — fnexpr-ctor-args-001, caught by the gate,
bisected, fixed in `70a3de80`) and the knife-2 argv-contention bar
(now keyed on an actual direct call in the mixed set). Knife 3
hands the species Construct each family method's real
ArraySpeciesCreate length (§23.1.3: concat/filter/flat/flatMap 0,
map len, slice count, splice actualDeleteCount — the receiver-len
shortcut fed filter 7 where the spec says 0). Knife 4 switches the
concat derive / transplant element store to DEFINE semantics
(§23.1.3.1.1 CreateDataPropertyOrThrow: a configurable
non-writable entry redefines, a non-configurable one refuses).
Sweep: passTotal 27881 → **27900 (+19)**, bug −14, incompatible
−5, conservation exact (+5 = +19 − 14). All 19 moves forward, ZERO
regressions: create-species.js ×5 (incompatible → pass), concat
create-species-with-non-* ×4, target-array-with-non-* ×8 across
filter/flat/flatMap/map/slice/splice (the define-semantics
transplant dividend), create-species-neg-zero ×2. The
create-species spot family ends 33 pass / 2 bug (both Proxy-faced)
/ 6 incompat from rotation 338's 22 / 8 / 11. Gate predicate **311
clusters (flat) / 5886 cases (−5) / residue 779 clusters 1029
cases / core 6915 (−5)**, register still empty. Build determinism
44/44 (N=12). Conformance gate 2614 → **2618/0/4** (+4 fixtures;
one red gate mid-rotation, bisected and fixed same rotation).)

**Previous @ `049039cd`** (2026-08-08, rotation 338 — the
builtin-singleton value-position axis closed in five knives, all
riding one generalized mint (`fill_ns_methods` + `intern_singleton`).
JSON and Reflect become first-class namespace objects (interned
dynobj singletons pre-filled with their ns-static cells; parse /
stringify and get / has / ownKeys / construct join the table over
the existing any-lane kernels — the member-get pair with the
accessor sentinel, the in-op walk, the Names own-keys walk,
`__torajs_reflect_construct`; toString badges answer `[object
JSON]` / `[object Reflect]` by pointer identity). The global `eval`
becomes a first-class value (checker admits the ident as Any; an
interned cell keyed under globalThis carries the real §19.2.1
name / length / typeof / identity, while the call-through-a-value
face stays the recorded loud TypeError — tr performs no runtime
evaluation and direct literal calls keep compiling through the
desugar_eval prefix). The checker admits expando writes on exactly
the singleton-backed namespaces (`Math.length = 1; Math[0] = 1` —
ordinary dynobj stores, unlocking Math as an array-like receiver
for the generic Array.prototype methods), and the globalThis fill
gains all three new identities. The factory-adapter predicate
widens for aliased/detached Reflect.construct (method NAME arms a
call; a bare member read arms too). Sweep: passTotal 27733 →
**27881 (+148)**, bug +81 (eval-shape programs now run to their
honest assertion failures), incompatible −229, conservation exact
(229 = 148 + 81). GAINED 150 by dir: Array/prototype 34 (the
Math-as-receiver family), Object create/defineProperties/
defineProperty 51, Reflect 16, JSON 10, class 8, rest singles.
**LOST 2 — both honest de-dilution**: postfix-increment/eval.js and
postfix-decrement/eval.js passed-negative on the accidental
`unknown identifier eval` compile reject; the real subject (strict
`eval++` as an assignment-target early SyntaxError, §13.4.2.1) is
now exposed as incompatible. Gate predicate **311 clusters (+2) /
5891 cases (−196) / residue 779 clusters 1029 cases / core 6920
(−201)** — the +2 with a −196 case drop is the unlock-exposes-new-
signatures shape; the `unknown identifier eval` cluster leaves the
top ranks. Crash triple unchanged (exit-139 32 = 32 by-name,
tr-timeout 36, exit-138 3). Build determinism 44/44 (N=12). A sixth
knife then landed the first key to the species family: an
arguments-touching fn-expr STORED into a boxed-face position (the
fnexpr-this B2 store roots) joins the argv face instead of killing
the binding chain, and leaves the static face whose direct-site
fold would materialize an empty `arguments` against the runtime
call the store implies; the fnexpr-this promote now inserts
`__this` AFTER the injected argc/argv slots — the pre-fix first
position made the boxed adapter unbox the argv pointer as a user
param, a SIGSEGV on the this+arguments combination that IS
create-species.js's primary spelling (probe now answers bun's
`1 true 1`). The re-swept verdicts @ `049039cd` are IDENTICAL to
`6a2b2adc` line-for-line — the t262 species family still waits on
the harness same-name collector mutual-kill fix (handoff 337 §4-④),
so the knife's yield is the crash-face cure plus the
infrastructure. Conformance gate 2608 → **2614/0/4**, six green
gates, +6 fixtures.)

**Previous @ `100d47f4`** (2026-08-08, rotation 337 — the
construct-channel species face closed, RFC 20260808 B2-B5 in four
knives. B2 admits a this-carrying fn-expr escaping into the species
slot (store arm `a.constructor[k] = fn` on any/array roots plus
`.prototype` reads as the third receiver-safe use shape); B3 hands
the @@species construct product back to the dispatcher — concat
derives into it directly, the other six family methods run their
default kernel and TRANSPLANT its elements across (length only where
the spec has a Set step: slice / splice — map / filter / flat /
flatMap CreateDataProperty only, bun-verified); knife 4 + B5 close
the `extends Array` inheritance face (the class object answers
ITSELF to an own-miss species read via a CTOR-registry mark, and
`program_constructs_from_value` arms factory adapters on the
`X.constructor = …` assign shape). Sweep: passTotal 27725 →
**27733 (+8)**, bug +12 (species product integrity shapes unlocked
into honest bug classification), incompatible −20, conservation
exact (20 = 8 + 12). GAINED 8 = species non-extensible-target throw
observability across all seven methods; **LOST 0**. Gate predicate
**309 clusters (flat) / 6087 cases (−20) / residue 780 clusters
1034 cases / core 7121 (−20)**, register still empty. Crash triple
unchanged (exit-139 32 = 32 by-name, tr-timeout 36, exit-138 3).
Build determinism 44/44 (N=12). Remaining species residue: the
create-species.js primary spelling still rides two pre-existing
gaps — the lifted-fn-expr `arguments` materialization hole (escaped
closure values never join the argv face) and one further checker
reject; the exotic default ctor still drops its len argument to
super.)

**Previous @ `685ff385`** (2026-08-08, rotation 336 — the eval cluster
re-attribution plus five knives. The B-layer artifact-size hard
prerequisite got measured first (embed upper bound ≈ 4.4 MB of
compiler machine code — torajs_core 2.73 + egraph 0.26 + codegen 0.07
+ std support — against a 2.27 MB hello baseline, pay-for-use so an
eval-free program pays nothing; method: per-crate `nm` adjacent-symbol
delta over a release-vanilla build, written down in RFC
20260807-eval/cluster-reshape-20260808.md). The 278-case `unknown
identifier eval` cluster then re-attributed AGAINST the r335 framing:
its main component was never the variable-argument B layer but 138
strict-reachable STRING-LITERAL calls the A layer missed — sources
whose final statement is a control-flow statement, wanting the §8.4
UpdateEmpty completion machinery. Knife 1 desugars exactly that (per
breakable/labeled statement V domains seeded undefined, if/try reset,
cross-domain jumps carry the innermost V; a label glued to its loop
stays one domain so ssa_lower's `continue l` contract holds). Knives
2+3 propagate named compile-time string sources into eval argument
position under fully static proof (const_string-foldable init, single
declaration program-wide, never assigned, no binding form reuses the
name, inert statement-list prefix — where "inert" means no call-like
node, so the test262 harness prelude of call-free assignments and
`class Test262Error extends Error {}` survives). Knives 4+5 land RFC
20260808-construct-channel B1: IsConstructor now answers true for a
closure cell with FLAG_FN_PROTO (§10.2.4: owning `.prototype` and
owning [[Construct]] are the same forms), and
`__torajs_anyv_construct` grows the §10.2.2 base-kind arm — fresh
dynobj `this` linked to the callee's `.prototype`, dispatch through
invoke_with_this, object completion overrides. The gate caught knife
4 stamping too wide (`new (Error.isError)()` constructed — the
`__forward_` arm marked hoisted `__sm_`/`__cm_` methods as
constructible; §10.2.5 says methods never run MakeConstructor) and
knife 5 narrowed it. Conformance gate 2601 → **2604/0/4** (+2
fixtures, five green gates after one caught-and-fixed red); build
determinism 44/44 (N=12). Against r335: passTotal 27637 → **27725
(+88)**, bug 12530 → 12553 (+23), trAccepted 40167 → **40278 (+111)**
/ incompatible 13007 → **12896 (−111)** — conservation exact (111 =
88 + 23). GAINED 89 by dir: the cptn family ~68 (switch 16, try 11,
if 7, for-in 6, for 6, while 4, do-while 4, labeled 2, for-of 2,
statementList 6, eval-code/direct 2, comma 1), S10.2.3 7, sm singles
2. LOST 1 — and it is an honest de-dilution: 15.3.5.4_2-19gs passed
because tr's "cannot construct a closure" TypeError happened to
satisfy an assert.throws(TypeError) whose REAL subject is strict
`caller` poisoning; B1 removed the accidental source and the true gap
(Function.prototype.caller strict poison, L3b) now reads as the bug
it is. Crash flat (exit-139 list comm-identical 32=32 — r335's "30"
was a counting-lens difference, not a delta; tr-timeout 36 / exit-138
3). Gate predicate **309 clusters (flat) / 6107 cases (−115) / residue
780 clusters 1034 cases / core 7141 (−111)**, register still empty.
The eval cluster drops 278 → **154 (−124)** and is no longer a
dominant #1 (154 vs `using` declarations 138): its residue is
staging/sm 47 (mostly sloppy-only), class multiple-evaluations
(noStrict-heavy B layer), and the ~41 bare-value uses that belong to
the builtin-singleton value-position reify axis, not to eval.)

**Previous @ `3a88f594`** (2026-08-08, rotation 335 — five knives across
three fronts: the r334-logged silent-wrong (a `throw` inside an
object-literal method body was swallowed — the struct-method-dispatch
lane's two CallIndirect emitters carried no throw_check; fn-valued
fields, arrow fields, accessor faces and user toJSON all rode the same
hole), fnexpr-this knife 7 (`F.prototype.m = function () { …this… }` —
the test262 fn-constructor idiom — joins the face family, cutting the
`__this` cluster 160 → 101), and RFC 20260808-json-parse-any, all four
blades: an any-lane `JSON.parse` runtime kernel (full ECMA-404
recursive descent into NaN-boxed values; expression-position calls and
`any`-annotated slots previously had NO lane at all), the §25.5.1.1
reviver walk (children-first, ES key order, undefined deletes,
holder-as-this through the boxed ABI), and the reviver slot as a
fnexpr-this face. The mid-rotation sweep caught a +2 SIGSEGV signature
that decomposed into a GENERAL closure defect: a bare `return;` in a
mixed-return fn emitted `Ret(None)` from an Any-ret body — the caller
read a garbage register as a NaN-box and the rc teardown crashed;
knife 5 synthesizes the boxed undefined (§10.2.1.4) and also gates the
reviver's delete on §10.1.10 step 4 (non-configurable refusal).
Conformance gate 2596 → **2601/0/4** (+5 fixtures, five green gates);
build determinism 44/44 (N=12). Against r334: passTotal 27563 →
**27637 (+74)**, bug 12475 → 12530 (+55), trAccepted 40038 → **40167
(+129)** / incompatible 13136 → **13007 (−129)** — conservation exact
(129 = 74 + 55). GAINED 72 by dir: JSON 45, Set 6, staging/sm 4,
String 3, WeakMap 3, WeakSet 3, Map 2, eval-code 2, singles 4
(passNoOracle 754 → 756, passNegative flat). Regressions **zero**;
crash flat vs r334 (exit-139 30 / tr-timeout 36 / exit-138 3 — the
mid-sweep +2 both root-fixed by knife 5). Gate predicate **309
clusters (−1) / 6222 cases (−112)**, residue 778 clusters 1030 cases,
core **7252 (−113)**. Newly measured for L3b: bare namespace idents as
values (`arr.every(cb, JSON)`, `Object.getPrototypeOf(JSON)`) reject
at ~50 cases — needs namespace-singleton reification; the
species/`Array.from.call` construct channel (shape B of the `__this`
survey) stays the other big fn-value gap.)

**Previous @ `eec696c9`** (2026-08-08, rotation 334 — SuperProperty
substrate + the eval×super context gate; also stamps rotations 332-333,
which updated plan-state only. r332 (`b60554b0`, five knives:
super-adjacent class fixes) closed at passTotal 27355. r333
(`d28dbf5e`, seven knives: the eval A-layer compile-time-text axis —
literal concat folding, completion-value IIFE, Function-ctor literal
synthesis, value-position SyntaxError, zero-arg, indirect closed
completion, orphan-Call overwrite) closed at **27540 (+185)**, the
largest single-rotation gain of the phase: class/elements
eval-in-field-init 108, gate predicate 313 clusters / 6352 cases /
core 7378. r334 lands the missing half of `super`: the parser accepts
`super.x` / `super[k]` as `__superbase__`-marker reads desugared per
§13.3.7 (getter dispatch / super-base member read / write-to-this),
static-context `super.m()` dispatch, `super(...spread)`,
dynamic-function strict early errors (§20.2.1.1 creation-time
SyntaxError), and base-class instance super resolving to
%Object.prototype% (§10.2.4). The mid-rotation sweep then caught the
collateral: unconditional `super.x` parsing had broken 36 cases whose
correct behavior WAS the parse failure — indirect eval containing
super must throw SyntaxError before running a statement (§19.2.1.1),
and bare super outside member bodies must fail at parse phase. The
closing knife (`eec696c9`) makes position part of the grammar again:
a `super_prop_allowed` parser flag (true across class bodies and
object-literal method bodies, false in ordinary function bodies,
arrows inherit; `super.#x` always rejected), and eval text parsed
with the call site's §19.2.1.1 steps 4-6 home verdict
(`walk::class_owned_exprs`), so illegal sources fail parse and ride
the existing step-12 throw carriers — nothing in the source runs.
Conformance gate 2589 → **2596/0/4** (+7 fixtures, six green gates);
build determinism 44/44 (N=12). Against r333: passTotal 27540 →
**27563 (+23)**, bug 12485 → 12475 (−10), trAccepted 40025 → 40038
(+13) / incompatible 13149 → **13136 (−13)** — conservation exact
(13 = 23 − 10). The +23 is all `pass`-family (passNoOracle 754 /
passNegative 3957 both flat — zero dilution): super 6 (eval-code
super-prop family — 9 of that directory's 10 now green), class 5,
Function strict early errors 5 (the r333 regression family root-fixed,
plus 15.3.2.1-11-{1,3,5}-s), with/object/new.target/staging singles.
Regressions **zero** (the 36 mid-sweep LOSTs all recovered);
crash/timeout flat (exit-139 31 / tr-timeout 36 / exit-138 3). Gate
predicate **310 clusters (−3) / 6334 cases (−18)**, residue 780
clusters 1031 cases, core **7365 (−13)**. Newly logged silent-wrong
(pre-existing, L3b): a `throw` statement inside an object-literal
method body is silently dropped — the method call site appears to
miss pending-throw propagation.)

**Previous @ `80753982`** (2026-08-08, rotation 331 — `new` +
`arguments`: the construct site's full argument list finally reaches
the body. `new H(1,2,3)` on an arguments-reading function answered
`arguments.length` 0 — the `__fnctor_` factory forwarded by naming
each declared param (surplus checker-admitted then dropped), and the
static-argv face only ever saw the factory's own 0-arg direct call.
Knife 1 reshapes the factory's params to EXACTLY the uniform
construct-site argc (trailing `__ctor_extra_*` for surplus,
truncation for under-filled — forwarding the declared tail let the
T-28 pad count undefined into the length) and opens knife 3a's
`__this` door (receiver excluded via this_slots; length-only bodies
qualify there since the T-31 tier never takes them). Knife 2 admits
constructed fn-expr BINDINGS: the lifted `__closure_N` carries
`__env` (env_fns → KeepLoud) and its sites hide behind the binding
name, so a new collector maps binding-name uses (direct calls ∪
`.prototype` reads) onto the lifted fn, and snapshot_fn_params
learns the `__env`-then-`__this` double prefix — unlocking the
S13.2.2_A5_T2 shape. Knives 3-5 clear three of rotation 330's four
logged typed-lane defects: `in` on a typed array boxes every
non-numeric key to the Any face (a blind coerce_to_i64 rejected Any
and would have mis-keyed Str/Bool), the mono deep-clone gains its
three missing variant arms (Delete / NewDynamic / Elision — a
variants-vs-arms comm found exactly these), and index-assign under
an Any key on an Array receiver rides the keyed set kernel (write
mirror of the read-side arm; the using-syntax family's
`var using = [], x = 0; using[x] = null`, where a `var` binding
reads as Any). Knife 6 is the closing debt-clean (the face-exclusion
scan moves to the walkers sibling). Conformance gate 2573 →
**2578/0/4** (+5 fixtures, six green gates); build determinism 44/44
(N=12). Against the r330 sweep (`db28985d`, whose numbers lived only
in plan-state — this entry closes that gap): passTotal 27283 →
**27326 (+43)**, bug 12326 → **12382 (+56)**, trAccepted 39609 →
**39708 (+99)** / incompatible 13565 → **13466 (−99)** —
conservation exact (99 = 43 + 56). The +43 is all `pass`
(passNoOracle 735 / passNegative 3957 both flat — zero dilution):
~26 on the arguments/mono face (the Array-HOF `-c-ii-5`
callback-deletes family, class/function `params-dflt-*-ref-arguments`,
S13.2.2_A5_T2/A6_T2, arguments-object mapped-delete), ~9 on the
in-op face (S12.6.3_A5, Math atan2/max/min, S15.4_A1.1 T6-8,
sm regress-634210), ~8 on the index-assign face (using-syntax,
for-of arguments-mutation, Object/keys). Regressions **zero**;
crash/timeout flat (exit-139 31 / tr-timeout 36 / exit-138 3).
Gate predicate **311 clusters (−4) / 6674 cases (−93)**, residue 774
clusters 1021 cases, core **7695 (−89)** — both headline numbers
down. eval stays #1 at 597; the bind-family residue (11 cases) has a
scouted unified path (the runtime bound cell is already §20.2.3.2
one-to-one with a boxed-ABI entry; what's missing is the value-argv
face's `.bind`-receiver exemption, synth_bind yielding to the kernel
lane for arguments-touching targets, and the RECV_FIRST marking —
logged as its own RFC-sized unit).

**Previous @ `abb45913`** (2026-08-08, rotation 329 — the PC=1 latent
linker bug is closed, and its "1" was argc all along. The layout
phase's `sizeofcmds` counted no `__DATA,*` section_64 headers while
the emit wrote them all; `text_file_offset` is that undersized sum
rounded to a page, so the undercount lived on page-round-up slack
until rotation 328's cross-CGU static pulled two more data sections
past the boundary. `pad_to` no-ops on an overrun, every `__text` byte
shifted 16 while every recorded address stayed put, and LC_MAIN's
entry executed the PREVIOUS function's epilogue — popping argc off
the start-up stack into (fp, lr) and ret'ing to PC=1. Fixed by
single-sourcing the count (`count_data_section_64s` reuses the data
phase's own walk) plus an emit-side hard gate (LC end ≤
text_file_offset, or a loud link-time panic). Rotation 328's reverted
globalThis-singleton knife relanded on top of the fix and its
formerly-red gate ran green; the GOT_LOAD relaxation's debug-only
encoding assert got hardened to a real error the same session. Then
two S5.6 knives: a this-reading fn-expr as an iterator-helper
callback (both halves existed — the RECV_FIRST channel and §27.1.4's
Call(cb, undefined) — neither wired here; the receiver test is the
desugared factory's `__Gen_*` return type), and the self-transform
reassign `iter = iter.filter(cb)` joining the mutable-let widen with
factory-table and self-receiver classifiers. Conformance gate 2568 →
**2571/0/4** (+3 fixtures, five green gates); build determinism 44/44
N=12): passTotal 27216 → **27276 (+60)**, bug 12294 → **12309 (+15)**,
trAccepted 39510 → **39585 (+75)** / incompatible 13664 → **13589
(−75)** — conservation exact (75 = 60 + 15). The +60 splits 17 pass
(15 of them `Iterator/prototype` — the cb-this and widen knives) +
43 passNoOracle (+1 passNegative), and the 43 were audited per the
no-dilution rule: all `eval-code/direct` arguments-family cases whose
sole blocker was reading `globalThis.arguments` — the relanded
singleton — with assertions spot-verified live (fn-decl typeof and an
undefined-vs-undefined global read, both genuine); bun fails these
itself, hence the bucket. Regressions **zero**; crash/timeout flat
(exit-139 31 / tr-timeout 36 / exit-138 3). Gate predicate 317
clusters / **6798 cases (−78)**, core **7807 (−75)**.

**Previous @ `34d22b5c`** (2026-08-07, rotation 327 — six knives on
indirect eval, the strict-face entry point r322's decomposition
named. `(0, eval)("…")` — the comma spelling, 263 of 311
indirect-shaped blocked cases — now resolves wherever the answer is
exact: closed single-expression sources collapse anywhere (free-vars
empty), identifier-bearing ones collapse at top-level positions under
script framing (two soundness gates: per-slot function-body positions
reconstructed by `walk::fn_owned_exprs`, and a nested-lexical-name
veto for the one shadowing direction that would be silent-wrong),
empty-completion sources collapse to `undefined`, top-level statements
inline as UNSEALED blocks (`var` hoists out per sloppy global
semantics, `let` stays contained), a `"use strict"` prologue flips the
source strict (seals, and its dead declarations collapse to the
directive string — the completion value, bun-confirmed), the Script
goal rejects `import`/`export` and orphan `continue`/`break`/`return`
as evaluation-time SyntaxErrors, and non-string literal arguments
return themselves per §19.2.1.1 step 2. The first knife's
empty-completion predicate had three holes, closed in knife 3: a
literal expression statement is value-producing (`eval("1; 2;")` is 2,
not undefined — now honestly rejected in value position), a truthy
literal loop condition never terminates, and a truthy `if` completes
with its taken branch. Against the r326 sweep (`713d83c8`): passTotal
27108 → **27166 (+58)**, bug 12163 → **12262 (+99)**, trAccepted
39271 → **39428 (+157)**, incompatible 13903 → **13746 (−157)** —
conservation exact (157 = 58 + 99). Regressions **zero** (one
`pass → tr-timeout` on a heavy unicode property-escapes case refuted
by standalone rerun 4/4 pass — an external `cargo test` was saturating
the machine during the sweep). Gate predicate **316 clusters / 6959
cases** (clusters flat, cases −156), core **7964 (−157)**. The eval
cluster fell 763 → **597** and stays #1; `__this` 257 / globalThis 216
next. Water ledger: `var-env-var-strict`'s pass was two defects
cancelling (prologue ignored × `in this` blind to top-level bindings)
— the prologue knife turned it real; the annexB indirect passes (3 +
2 no-oracle) ride the registered block-fn-leak family and stay on the
ledger. Core `language/eval-code/indirect`: 59 blocked → 20 pass.)

**Previous @ `b6e7420b`** (2026-08-07, rotation 324 — five knives on the
reflection / assignment-value / refcount seam, four of them silent.
`Object.entries(new Error("m"))` answered 3 where `Object.keys`
answered 0 (the compile-time struct unfold cannot express
non-enumerable own slots; Error-family receivers now ride the runtime
own-walk the `any` lane always used, family test walking `extends` to
the injected classes). `(TypeError.prototype as any).x = 1` — one
line — underflowed %Error.prototype%'s refcount: an owned any-receiver
box had no release site anywhere on the member-write path, and the
stranded +1 cut the error-prototype cycle in half at the at-exit
drain, the two halves tearing each other down (rc_underflow census
32 → 29; the guess "that would be a leak, not an underflow" is exactly
what had hidden it). `b = (o.k = [1,2,3])` left `b` holding integer 0
on five member-assign lanes (§13.15.2: the value is the rhs;
transfer-or-share ownership contract, settled by SSA A/B diff after
two paper ledgers both collapsed). `o.x = [7,8]` read back EMPTY — the
dynobj lane never stamped the elem-kind chain a typed array's NaN-box
needs (chunk 621's struct-field lesson, one lane over). And
`c = (t[0] = [4,5])` answered right but underflowed at teardown — the
index half of the assignment-value contract rotation 323 explicitly
left as a borrow. Conformance gate 2542 → **2546/0/4** (+5 fixtures,
zero red across five gates); build determinism 44/44 (N=12)):
passTotal 27102 → **27107 (+5)**, bug 12169 → **12164 (−5)**,
trAccepted 39271 flat / incompatible 13903 flat — conservation exact
(0 = +5 + −5); `passNoOracle` flat at 683, no dilution. Gate predicate
**316 clusters / 7115 cases**, core **8121 — all four numbers flat**,
as expected for memory/value-contract work that opens no language
surface. Regressions **zero** (5 verdict moves, all forward:
`dstr/array-rest-put-prop-ref` twice — the member-assign value knife
verbatim — `fields-asi-1` twice, and `includes/tolength-length` off
the array-length lane now answering its value). Crash/timeout flat
(exit-139 31 / tr-timeout 36 / exit-138 3). Session note: this
rotation began on a stale context (rotations 309-323 summarized away)
and was saved by reproduce-first discipline; the working tree carries
four uncommitted DIAG files (the rc-underflow census instrumentation)
that the underflow knives depend on — left in place deliberately.

**Previous @ `38cfbc76`** (2026-08-07, rotation 322 — eval, measured
before it was designed. The reflex says an AOT compiler cannot have
eval; the decomposition (`.claude/rfcs/20260807-eval/`) says 79.5% of
the eval-blocked cases pass a compile-time-known literal, so five
knives parse the literal at the call site and lower it through the same
pipeline as every other statement: statement-position inlining as a
sealed Block (§19.2.1.1 strict branch — `var` does not escape),
expression-position function bodies (`Expr::ArrowFn` lives in the expr
arena no statement walk visits), single-expression sources collapsed in
place (exact per §14.5.1, reaches value position), declaration-only
sources collapsed to `undefined` (declarations complete with empty, and
strict-eval bindings die unobserved — with end-of-input ASI per §12.9.1
rule 2, which tr's parser had never met because files end in newlines),
and SyntaxError at evaluation time for a literal that does not parse.
Plus the build determinism gate `hardev/autorun/build_determinism.sh`
as close-step 0c, green on both HEADs it ran at): passTotal 26891 →
**27101 (+210)**, bug 11827 → **12170 (+343)**, trAccepted 38718 →
**39271 (+553)**, incompatible 14456 → **13903 (−553)** — conservation
exact (553 = 210 + 343). Regressions **zero** (53174-line verdict diff:
no `pass` moved to anything else; timeout 36 / exit-138 3 flat).
**One newly-exposed crash**, listed singly: `for/scope-body-var-none.js`
went `incompatible:type error → bug:exit 139` — the eval knife let it
compile for the first time and it dies at runtime on the
closure-capture × var-hoist × for-head-probe combination (exit-139
count 30 → 31; registered in plan-state L3b). Gate predicate **316
clusters / 7115 cases** (+5 / −563), core **8121 (−553)** — the
predicted count-up-cases-down shape: the eval cluster fell 1433 →
**763** and newly-running cases surfaced their next signatures.
**Water ledger (no-metric-inflation)**: 26 of the +34 in `passNoOracle`
are `annexB/language/eval-code` cases asking for sloppy Annex B
function hoisting and passing because tr's block-level function
declarations leak (`{ function f(){} }` leaves `f` visible; bun does
not) — an inherited behaviour of `nested_fns.rs` (deliberately
implements B.3.3), not of the eval pass. Registered, documented in the
pass, and expected to fall back out if/when the language-mode question
is settled — that question (strict vs sloppy surface) is takagi's per
CLAUDE.md. Top clusters at that point: eval 763 (17 dirs) / `__this` 256 (21) /
globalThis ~216; top 400 = 90.4%.**

**Previous @ `a3f0ce09`** (2026-08-06, rotation 313 — five knives across
two surfaces, `delete`'s three gates and object rest's two halves.
Three of the five removed a gate that had no missing implementation
behind it: the numeric property key was already coerced by the
lowering, and the object-rest omit walk was already in the dynobj
lane. A gate's stated reason can go stale after the implementation it
described arrives, so the test is what the blocked path does today,
not what the comment says): passTotal 26439 → **26685 (+246)**,
bug 11824 → **11807 (−17)**, trAccepted 38263 → **38492 (+229)**,
incompatible 14911 → **14682 (−229)** — conservation exact
(229 = 246 + −17). Unlike rotation 312, whose gain was entirely
`bug → pass` and left the clusters untouched, **229 cases left
`incompatible` here**, so the v1.0 curve moved. Gate predicate
**317 clusters / 7906 cases** (−3 / −230), core **8891 (−222)** —
clusters and cases moving DOWN together, as in rotations 227 and 310.
Regressions **zero**: no `pass` moved to anything else, and timeout
(98) and crash (33) counts are both flat. Fifty-four cases went
`incompatible → bug` — newly-running programs whose answers are now
being judged for the first time, which is what forward motion looks
like at this boundary, not regression. Top clusters unchanged: eval
1433 / `__this` 253 / globalThis 216; coverage curve top 25 = 43.1%,
top 100 = 70.2%.

**Previous @ `b00e9bea`** (2026-08-06, rotation 310 — six knives on one
question: where does a value live. A class instance's own properties
live in a dict the compile-time layout cannot see; a scalar
`T | null` had nowhere at all to put the null; and
`getOwnPropertyDescriptors` had no lowering, which on being built
revealed that the descriptor kernel underneath has no arm for a
string receiver at all): passTotal 26422 → **26432 (+10)**, bug
11832 → **11831 (−1)**, trAccepted 38254 → **38263 (+9)**,
incompatible 14920 → **14911 (−9)** — conservation exact
(9 = 10 + −1). The +10 is entirely in the oracle-backed `pass`
bucket, `passNoOracle` flat at 639, so none of it is dilution. The
verdict diff is ten lines, all under
`Object/getOwnPropertyDescriptors/`: eight forward from
`incompatible:not yet supported` to `pass`, one newly-running case
that now fails (`order-after-define-property`, exactly the residual
the defineProperty knife declared — redefining a DECLARED field needs
per-field attribute storage the layout has no room for), and one bug
whose exit code changed (the proxy face is unimplemented). Gate
predicate **320 clusters / 8136 cases** (−1 / −9), core **9113 (−9)**
— clusters and cases moving DOWN together, as in rotation 227.
Regressions **zero**: no `pass` moved to anything else. The other
five knives fixed real defects (six fixtures byte-equal with bun, all
wrong before); two of them show up here, the rest are outside this
metric's window — neither dilution nor regression. Top clusters
unchanged: eval 1433 (26 dirs) / `__this` 253 (20 dirs) / globalThis
216 (14 dirs); coverage curve top 25 = 42.6%, top 100 = 70.6%.

**Previous @ `4b2659f7`** (2026-08-05, rotation 308 — five knives, four
of them silent-wrong, three of them the same shape: the checker had
already worked the answer out and the runtime was not reading it. A
heterogeneous `Promise.all` decoded every slot of its result array
through the FIRST element's repr (a string surfaced as its pointer,
`true` as 1), while the checker types that call `Promise<Array(Any)>`
and hands the element form down as `target_repr`; the same for
`allSettled`, one level in, at the value slot of each `{status, value}`
record; and half the binary operators refused an object operand
outright — `t * 2` ran while `t + 1` was a compile error for the same
`t` — though the any-lane kernels have always carried the whole
ToPrimitive walk, because an `any` operand uses them. Plus a rest
parameter that was an ALIAS of the caller's array (§10.4.2 wants
CreateArrayFromList, so `g(...arr)` pushing to `xs` grew `arr`), and
`{ *[Symbol.iterator]() {} }` — the ordinary way to write an iterable —
falling between two parser arms, each of which declined for a locally
correct reason): passTotal 26340 → **26422 (+82)**, bug 11881 →
**11832 (−49)**, trAccepted 38221 → **38254 (+33)** / incompatible
14953 → **14920 (−33)** — conservation exact (33 = 82 + −49); the +82
is entirely in the oracle-backed `pass` bucket, `passNoOracle` flat at
639, so none of it is dilution. Gate predicate **321 clusters / 8145
cases** (−6 / −30), core **9122 (−33)** — clusters and cases moving
DOWN together, as in rotation 227. Regressions **zero** (82 verdict
moves, all forward), no new timeouts or crashes (exit 139 at 30,
tr-timeout 36, exit 138 at 3, all flat). Attribution: 60 of the 82 are
`class/elements` cases that collect six async methods through
`Promise.all([...]).then(...)` — the fan-in lane the first knife fixed,
previously `bug:exit 1`, which is the TypeError that lane threw; ~12
are operator cases (greater-than, less-than-or-equal, addition,
bitwise, exponentiation); one is `Promise/allSettled`; one is
`computed-property-names/object/method/generator.js`, the parser knife
named exactly. Top clusters unchanged: eval 1433 / `__this` 253 /
globalThis 216. Note the span: this delta is measured against rotation
307's sweep (`f3af5bb4`), while the previous entry below is rotation
303 — rotations 304-307 recorded their sweeps in plan-state and
handoff without refreshing this section.

**Earlier @ `e6e616f5`** (2026-08-05, rotation 303 — a generator's
lifted locals keep the types their initializers say they have. The
let-lift moves every `let` in a generator body to a field of the
synthesized `__Gen_*` class so the binding survives a yield boundary,
and an unannotated one was pinned to `number` — right for a loop
counter, wrong for everything else a body can hold, so `function* g()
{ const xs = [1, 2]; }` did not compile at all. Four knives: the sniff
itself (`infer_expr_ann_with`, seeded with the generator's params and
each local as it lands); an arrow held in a local, which the shared
sniff cannot answer here because `lift_arrow_fns` has not run — plus
what that arrow captures, since the param rewrite did not descend into
arrow bodies, the same omission `nested_fns_idents` carried until r302;
`new C()`, a call to a `function*` (whose declared type describes what
it YIELDS, not what calling it answers) and `undefined`; and a call
that returns nothing, which needed `box_to_any` to grow the `Void` arm
that `check_stmt_let_decl` chunk 618 had already argued for one layer
up. This is RFC 20260805 D0, the prerequisite the reverted async
state-machine commit found): passTotal 26327 → **26340 (+13)**, bug
11881 flat, trAccepted 38208 → 38221 (+13) / incompatible 14966 →
**14953 (−13)** — conservation exact (13 = 13 + 0); gate predicate
**327 clusters / 8174 cases** (−1 / −15), core 9155 (−13);
regressions **zero** — 13 verdict moves, every one
`incompatible:type error → pass` and every one a
`generators/scope-paramsbody-var-*` case, tr-timeout 36 flat. Note the
span: rotation 302 never reached its sweep (the session was cleared
mid-rotation), so this delta covers r302's five commits as well —
which contributed no test262 movement, being a revert plus three
narrow correctness fixes. Top clusters unchanged: eval 1433 /
`__this` 253 / globalThis 216.

**Earlier @ `4b3c3ee5`** (2026-08-05, rotation 301 — the promise
combinator family closes out and one leak underneath it: an
all-rejected `Promise.any` answers a real AggregateError built through
torajs-throw's factory registry (the call site implies the class the
way bigint division implies RangeError); `Promise.any` gets its fan-in,
the last of the four, sharing `all`'s block read in a mirror; `.finally`
uses what its handler returns — a checker wedge admitting any return
plus the ret-repr word plus a waiting job, where before every non-void
return was a COMPILE reject; an empty iterable settles synchronously
like bun instead of a microtask late, found by probe while writing the
first knife and belonging to the whole family; and the leak the last
rotation measured without attributing — `new Promise(executor)`
stranding ~885 bytes per call — turns out to be `pack_any_argv` never
releasing an owned `Any` argument, so the executor's two settle
closures leaked with the envs that hold the cell): passTotal 26321 →
**26327 (+6)**, bug 11886 → 11881 (−5), trAccepted +1 / incompatible
14967 → **14966 (−1)** — conservation exact (1 = 6 + −5); gate
predicate **328 clusters / 8189 cases** (flat), core 9168 (−1);
regressions **zero** — 7 verdict moves, all forward (6 `Promise/any/*`
bug→pass, 1 `Promise/allSettled/resolved-then-catch-finally.js`
incompatible→bug as the `.finally` wedge unlocks it past the checker),
tr-timeout 36 flat, exit-138/139 crashes 34 flat. Top clusters
unchanged: eval 1433 / `__this` 253 / globalThis 216;
harness-includes 5935 (39.7%), type error 4490 (30.0%).

**Earlier @ `8e7e683e`** (2026-08-04, rotation 296 — the method-rebind
RFC lands whole: the receiver-polymorphic cmany twin (a class method
read off its instance and re-bound to a foreign receiver runs the twin
body through the any-member lane instead of silent-wrong slot reads —
`f.getM().call({v:42})` answers 42), the reified method face guarding
its receiver by exact class tag, any-member postfix incr/decr composing
GetV → step kernel → member-set, and the checker's terminal member miss
on a class-instance receiver answering undefined per §10.1.8.1 instead
of rejecting — with the undeclared-private-name carve-out (`this.#x`
unresolved stays an early SyntaxError; the mid-rotation sweep caught 40
pass-negative cases regressing to phase-mismatch and the fix landed
same-rotation), plus the five file-size debt splits from the close-out
audit): passTotal 26170 → **26194 (+24)**, bug 11919 → 11963 (+44 —
member-miss unlocks running to runtime), trAccepted +68 / incompatible
15085 → **15017 (−68)** — conservation exact (68 = 24 + 44); gate
predicate **331 clusters / 8235 cases** (flat / −67), core 9219 (−68);
regressions **zero** in the final sweep (the 40-case private-name
family verified restored), tr-timeout 36 flat. Forward 24:
Iterator/prototype 11, class family 5, postfix incr/decr 2, scattered.
Top clusters: eval 1433 / `__this` 253 / globalThis 216;
harness-includes 5935.

**Previous @ `0e556523`** (2026-08-04, rotation 295 — fn-value
semantics three ways plus the width-analysis root fix: container
poison reaching scalars, fn-receiver store wraps, fnprops delegating
to the canonical cell, and generator-expression bodies threading the
user `this` through `__genrecv`): passTotal 26099 → **26170 (+71)**,
bug +13, trAccepted +84 / incompatible **15085 (−84)** — conservation
exact; gate predicate 331 clusters / 8302 cases, core 9287 (−83);
regressions zero, tr-timeout 36 flat. Forward 71 ≈ the destructuring
family (~67) through the patched-iterator runtime-protocol lane.

**Previous @ `9ee27236`** (2026-08-04, rotation 294 — the nested-class
hoist plus the iterator-close and let-widen knives, five knives all
green through the gate chain 2419 → 2422: capture-free ClassDecls
nested in fn / fn-expression / block bodies hoist to the top level
(the desugar_classes pre-pass, α-renaming on collision — the same
strategy the parser already applied to class expressions); the lazy
and eager Iterator helpers close their underlying UNDER the stashed
pending throw so a user `return()` actually runs (proposal 2.1.5 /
IfAbruptCloseIterator); a mutable unannotated `let` reassigned a
value of a different syntactic family types `any` from declaration
(RFC 20260804-mutable-let-widen, the third shared init-eid set in
the dynobj_degrade family); for-of/for-in bodies reject lexical
declarations like every other single-statement position, with the
sloppy `let \n x` ASI-identifier spelling exempted across all seven
positions). passTotal 26043 → **26099 (+56)**, bug 11874 → 11906
(+32, unlock shape), trAccepted +88 / incompat **15169 (−88)** —
conservation exact (88 = 56 + 32); gate predicate **331
clusters / 8382 cases** (−2 / −64), core 9370 (−71); regressions
**zero** (all three pass verdicts diffed; the one blade-4 regression
was caught by the mid-rotation sweep and fixed same-rotation by
blade 5); one new tr-timeout (36 total — a hoist-unlocked TDZ
self-reference loop, registered).

**Previous @ `b6940559`** (2026-08-04, rotation 293 — the fn-value
store-site blind-spot sweep plus Reflect.construct, four knives all
green through the gate chain 2414 → 2419: the NewDynamic CALLEE and
a lifted arrow's untyped params reach the wrapped-closure lane
(`new Error.isError()` answers the spec TypeError through the
construct kernel's conservative IsConstructor; the lazy-Iterator
`iter => iter.map(fn)` family boxes); the collector's walk gains the
arms it never had — Stmt::ForOf/ForOfSplitIter bodies were entirely
invisible, dynobj member-assign stores and the for-in head hoist
wrap their fn-name operands; §28.1.2 Reflect.construct lands three
layers deep (kernel with both IsConstructor gates + unconditional
CreateListFromArrayLike + newTarget [[Prototype]] re-wire, checker
arm, lowering route; the factory-adapter synthesis predicate now
arms on its call shape — the 136-case single-API cluster), and
as-cast receivers peel to reach the degrade pass (the layout-[]
family): sweep passTotal 26024 → **26043 (+19)** / bug 11761 →
11874 (+113 — Reflect.construct unlocks run to runtime and expose
their signatures) / trAccepted +132 / incompatible 15389 → **15257
(−132)**, conservation exact (132 = 19 + 113); gate predicate **333
clusters / 8446 cases** (flat / −65), core 9441 (−70); regressions
**ZERO** (verdicts diff: 0 cases left pass), new timeouts/crashes
zero (tr-timeout flat 35). Previous stamp below.

**Prior @ `7b756e00`** (2026-08-04, rotation 292 — the fn-value /
iterator-protocol residue face, seven knives all green through the
gate chain 2409 → 2414: iterator helpers cache the next method at
construction per GetIteratorDirect (helper cell grows a next slot,
48 → 56 B — the 8 get-next-method-only-once tr-timeouts whose
accessor minted a fresh generator per step all clear, tr-timeout
45 → 35); the collector's blanket generator-factory wrap exclusion
(predating the G2 forward-cell reflection faces) drops across four
axes — Object/Reflect namespace args except getPrototypeOf's
kind-exact fold, any-receiver member-call args, member-call
receivers, unswallowed apply/bind receivers — plus the let-init axis
admits hoisted genexpr inits (fn-name-gen family, NamedEvaluation
answers through the existing registry); the apply/Reflect.apply
kernel reads non-Arr objects array-like per CreateListFromArrayLike
(a function argArray answers its virtual length — the 15.3.4.3-*-s
family), with the mid-rotation sweep catching a Symbol cell riding
the new lane (argarray-not-object regressed pass → bug; fixed
same-rotation, Symbol + BigInt join the primitive gate);
`new Object(value)` rewrites to the call form (§20.1.1.1 construct
IS call) and the collector gains construct-arg marking plus a
NewDynamic arm that form's args never had: sweep passTotal 25932 →
**26024 (+92)** / bug 11831 → 11761 (−70) / trAccepted +22 /
incompatible 15411 → **15389 (−22)**, conservation exact (22 = 92 −
70); gate predicate **333 clusters / 8511 cases** (−1 / −24), core
9511 (−22); regressions **ZERO** (verdicts diff: 0 cases left pass);
box_to_any FnSig cluster 14 → **2** (Error.isError namespace-static
fn value + one staging shape). Previous stamp below.

**Prior @ `0f8f8257`** (2026-08-03, rotation 288 — the rotation-287
heisenbug root cause + the dynamic-import parse face, four substrate
knives: `__torajs_anyv_await`'s identity arm (non-promise cell) now
mints the +1 stake its lowering call site releases — pre-fix `await
<owned non-promise cell>` freed the operand's only stake and handed
back a dangling pointer, which is the whole release-only /
cross-machine / instrument-to-flip heisenbug (async-gen `yield*`
awaits its step object; the UAF read of the freed cell's tag byte is
what the layout decided; Guard Malloc turned it into a deterministic
crash and an lldb refcount watchpoint on the dev box gave the
alloc → dec-to-free → UAF-inc timeline); `import defer` parses as a
contextual keyword (`defer` + `*` only; eager semantics layered) and
a default-binding clause now requires the `,` before a namespace /
named clause; statement-position `import("...")` is an expression
per §13.3.10 and rewrites to `Promise.resolve(__dyn_ns_<n>)` so
`.then()` chains and `await import(...)` unwraps; a CallExpression
is not a valid assignment target (§13.15.1 early error — closes the
15 negatives the ImportCall dispatch exposed, which had passed on an
unrelated parse error): sweep passTotal 25486 → **25568 (+82)** /
bug 11692 → 11735 (+43) / trAccepted +125 / incompatible 15996 →
**15871 (−125)**, conservation exact (125 = 82 + 43); gate predicate
**336 clusters / 9001 cases** (−2 / −144), core 9993 (−125, first
time under 10k); regressions: **ZERO** (the rotation-287 regression
case is back to pass — root-caused, fixed, and its 11 variants +
minimized repros run clean under Guard Malloc). The 82 forward moves:
dynamic-import 71 / assignmenttargettype 6 / logical-assignment 3 /
top-level-await 1 / the heisenbug class case 1.

**Prior @ `c0e82d13`** (2026-08-03, rotation 287 — the generic
array-like / method-value face, four substrate knives + one harness
knife: Array.prototype.flat / flatMap join the runtime's
"intentionally generic" array-like arm (has-gated walk, one-level
spread through arr_extend_any, the shared flat-depth kernel); Array
receivers join the method-value reify family (ARRAY_PROTO_TAG mint
on both mirror sides — `[].flat.call(arrayLike)` /
`[].flatMap.call(arrayLike, fn)` / first-class `const m = arr.map` /
bind); flat accepts a RUNTIME depth operand (ToIntegerOrInfinity in
a new kernel shell: null → 0, numeric strings parse, Infinity
saturates, Symbol throws); the test262 runner stages sibling
`*_FIXTURE.js` files beside the assembled case (module-import
fixtures resolved against the temp dir and always missed — worker
slots own temp subdirectories now, staged bytes salt the bun-oracle
key); bare named exports (`export { a, b as c }`) resolve through
both import shapes, and injected lib decls reset their source spans
to the (0,0) sentinel (they indexed the LIB file's text while every
consumer slices the MAIN file's — out-of-bounds panic when the main
file is shorter): sweep passTotal 25441 → **25486 (+45)** / bug
11680 → 11692 (+12) / trAccepted +57 / incompatible 16053 →
**15996 (−57)**, conservation exact (57 = 45 + 12); gate predicate
**338 clusters / 9145 cases** (−9 / −24, both axes down), core
10118 (−57); regressions: **ONE** —
`test/language/statements/class/async-gen-method/yield-star-next-then-non-callable-symbol-fulfillpromise.js`
pass → bug:stdout-mismatch, a release-profile-only layout-sensitive
latent bug (bisects to the flat-runtime-depth commit whose only
global effect on this case is an intrinsics-table FuncId shift; the
iter profile passes on every commit; inserting two console.log
lines flips release to pass; its 11 same-semantics variants all
pass) — root-cause investigation registered in plan-state L3b. The
46 forward moves land in the knives' faces: flat 6 / includes 6 /
find family 8 / flatMap 2 / dynamic-import 10 (fixture staging
unlocked string-literal `import()` cases) / import-defer syntax 2 /
misc array-generic 12.

**Prior @ `936712e4`** (2026-08-03, rotation 286 — the annotated-
callback + flatMap face, seven substrate knives + one debt extract:
a receiver-matching `{elem}[]` / `Array<{elem}>` annotation on the
§23.1.3 srcArray callback slot normalizes to the kind-aware `any[]`
view (all 8 HOFs accept fully-annotated signatures; mismatched
spellings stay loud, matching tsc); flatMap identity over nested int
arrays read i64 slots as f64 denormals — the result-key now marks
the callback's ret point as container evidence so the guarded/nested
width edges activate (silent-wrong closed); a void flatMap callback
pushes boxed undefined per §23.1.3.11 step 8.d; nested-array callback
inners admit on the hetero lane; flatMap's callback rides the full
spec arity (elem, index, srcArray); flatMap joins the knife-4 thisArg
protocol with a runtime IsArray spread arm (an Any-returning callback
branches spread-vs-scalar per element at runtime); the TS
this-parameter (`function f(this: T, …)`) parses as a type-only first
param across fn-decls / fn-exprs / ctors: sweep passTotal 25439 →
**25441 (+2)** / bug 11680 flat / trAccepted +2 / incompatible 16055
→ **16053 (−2)**, conservation exact (2 = 2 + 0); gate predicate
**347 clusters / 9169 cases** (flat / −1), core 10175; regressions:
**ZERO** — both forward moves land in the flatMap knives' faces
(depth-always-one / thisArg-argument). The rotation's main yield is
TS-annotation-surface (conformance 2375 → 2382, +7 fixtures) which
test262's pure-JS corpus barely samples.

**Prior @ `b4440fbd`** (2026-08-03, rotation 285 — the HOF-callback
face, five substrate knives + one mechanical extract: filter's keep
test folds through ToBoolean (§23.1.3.7, the trio-kernel gap the
rotation-284 predicate fix left); a NAMED fn callback receives the
HOF thisArg through a receiver-first `__fwdrecv_` forwarder riding
the knife-4 protocol unchanged (root cause of the 22-case rotation-284
honest re-baseline dip — all recovered); map callback return
polymorphism at full spec arity with thisArg; reduce hetero returns
ride an Any accumulator lane (empty-walk seed passthrough
silent-wrong closed); the predicate-family "boolean" contextual seed
yields to the body's sniffed return (arrow faces): sweep passTotal
25360 → **25439 (+79)** / bug 11736 → 11680 (−56) / trAccepted
+23 / incompatible 16078 → **16055 (−23)**, conservation exact
(23 = 79 − 56); gate predicate **347 clusters / 9170 cases** (−1 /
−15); regressions: **ZERO** — all 79 forward moves land in the five
knives' target faces (map 22 / every 14 / forEach 14 / some 14 /
filter 8 / reduce family 5 + 2 spillover); crash count 34 flat.
Rotation-284 interim @ `882462f3`: passTotal 25224 → 25360 (+136) /
incompat −74, gate predicate 348 / 9185, the 22-case every/some
thisArg dip honestly re-baselined (recovered this rotation).

**Prior @ `f76e0d15`** (2026-08-02, rotation 283 — five substrate
knives: reassigned non-Copy params copy in an owned stake (the
default-param guard's compile-time `moved` clear over-released the
caller's borrow — root cause of the seven r282 gen dstr-default pass
regressions, all recovered; e1f1b319's cell resize was only the
size-class shift that made the stale IteratorClose write land on a
live cell); zero-arg `Function()` / `new Function()` answer an empty
function; typevar inference joins Array(Any) with typed arrays
structurally (149-case dstr-rest cluster); generator-lifted
destructure temps ride the any lane instead of the "number" lift
fallback (366-case for-await-of dstr cluster); nested-fn hoisting
runs to a fixpoint (120-case async-`$DONE` cluster): sweep passTotal
24800 → **25224 (+424)** / bug 11676 → 11798 (+122) / trAccepted
+546 / incompatible 16698 → **16152 (−546)**, conservation exact
(546 = 424 + 122); gate predicate **351 clusters / 9245 cases**
(−15 / −515, both axes down); regressions: **zero semantic** (the
single pass→timeout flip, `new Array(4294967295)`, re-runs clean
solo — load-state flake). Interim rotation-282 sweep @ `e34a7fa9`
(struct-dynamic-props blades 1-3 + computed class fields) had moved
passTotal 24725 → 24800 / incompat −148 and was not recorded here —
folded into this entry's baseline.

**Previous @ `887e6422`** (2026-08-02, rotation 281 — five early-error
knives closing the rotation-280 regression ledger plus the whole
await-context face: yield as an assignment/update target rejects
(§13.15.1 — `(yield) = v`, `(yield)++`, `++(yield)`, dstr slots);
accessor arity early errors (§15.4.1 — getter zero params, setter
exactly one non-rest, class + object-literal faces); yield/await
inside formal parameter lists reject for every function kind
(§15.1.2/§15.8.1, new in_formal_params flag — fixes the silent
hoist-past-params of `function f(x = yield)`); `using` / `await
using` declaration heads get a loud not-yet-supported reject plus
`[...x,] = src` and `[arguments]/[eval] = src` dstr early errors;
await outside async bodies / module top level rejects at parse
(new await_allowed flag threaded through every function-like body
incl. static blocks): sweep passTotal 24683 → **24725 (+42)** /
bug 11748 → **11603 (−145)** / trAccepted −103 / incompatible
+103; gate predicate **366 clusters / 9905 cases** (−1 / +101 —
bug-bucket cleanse shape: the case-axis rise mirrors 145 silent-
wrong accepts becoming loud rejects); forward 63 (all
pass-negative) / 21 regressions (17 using-family coincidental
passes now loud rejects, 6 sloppy `{eval, arguments} = {}`
no-oracle cases refused by the always-strict surface), zero new
timeouts / crashes.

**Previous @ `2563958c`** (2026-08-02, rotation 280 — five knives:
§15.7.1 class-field early errors ('constructor' in every literal
spelling, `#constructor` in every ClassElementName position,
ContainsArguments over field initializers) + async-modifier
lookahead admits computed/literal names; expression-position
`yield` lands via parse-time hoisting to `__yx_` YieldInto temps
(top-#4 cluster, 171 cases — conditional positions and
pre-yield side effects reject loudly, untyped locals reading a
temp lift as `any`); comma-operator expression statements desugar
to sequential statements (cluster #15, 119 cases, for-init comma
included); bare class fields terminate by ASI at a line break
(cluster #13, 122 cases); parser.rs 504-line breach cleaned into
parser/cursor.rs): sweep passTotal 24500 → **24683 (+183)** / bug
11706 → 11748 / trAccepted +225 / incompatible 16968 → **16743
(−225)**; gate predicate **367 clusters / 9804 cases** (+3 / −225
— unlock-exposes-new-signatures shape, case axis down big);
forward 205 / 22 regressions (all metric-hygiene: `(yield) = 1`
and getter-with-params now parse where the old wrong parse error
masked missing early errors — AssignmentTargetType, accessor
arity, param-default yield coverage, `using` statement-head
reject; four thin knives specified in handoff), zero new
timeouts / crashes.

**Previous @ `72ac3231`** (2026-08-02, rotation 279 — RFC
20260802-class-computed-member, four knives + one fix-up: class
member names accept string / numeric literals and whole-literal
computed keys (parse-time fold, `__sym_` narrowed to Symbol-headed
chains, silent misinstall of `[k]` under `__sym_k__` replaced by
the runtime lane); runtime computed member names install under
their evaluated keys at the class-decl position (ToPropertyKey with
Symbol pass-through and throw propagation, get/set merge via
single-face present bits, sentinel names fenced out of every reify
sweep, struct_method miss chain probes the class prototype chain);
struct-receiver keyed access rides the any lane and symbol keys
walk the class prototype chain (instance-side symbol-keyed read +
call, subclass chain included); un-annotated setter params force
`any` on accessor faces (None-ann Type::Void sig dropped the
AccessorPair set face — the accessor-name-static literal-numeric
family's exact assert surface): sweep passTotal 24405 → **24500
(+95)** / bug 11652 → 11706 / trAccepted +149 / incompatible
17117 → **16968 (−149)**; gate predicate **364 clusters / 10029
cases** (−12 / −130, both axes down); forward 113 / 18 regressions
(all metric-hygiene shape: 14 negative cases that used to pass on
the wrong parse error now expose two missing early errors, 4
no-oracle `static async *[k]` lookahead breaks — all registered in
L3b), zero new timeouts / crashes.

**Previous @ `6ecef1a7`** (2026-08-02, rotation 276 knife 7 = RFC
20260802 D3a on top of the `a3a45e66` stamp below — a `return`
inside a generator's try/finally body (or nested catch body) now
routes through every enclosing finally copy before completing,
outside-in, via a frame stack + placeholder-patched gotos into a
third F duplicate): sweep is **verdict-identical to `a3a45e66`** —
passTotal 24323 / bug 11718 / trAccepted 36041 / incompatible 17133
/ gate predicate 377 clusters / 10173 cases, zero moves in either
direction. The body-return face has no direct test262 coverage of
its own (the remaining GeneratorPrototype/return try-finally-*
cases need the `return()` METHOD injection — registered as D3b);
the knife's semantics are held by fixture gen-try-011 (early return
through a yielding finally, nested chain outside-in, return-in-
catch, bare return + finally-return override, all bun byte-equal)
and it unblocks D3b, which reuses the same return-copy entries.

**Previous @ `a3a45e66`** (2026-08-02, rotation 276 — six knives, RFC
20260802-generator-try-region C0-D2 + one default-param knife:
generator try/catch/finally exception regions in the regenerator
tryEntries shape (monotonic state allocation makes the numeric range
[try_entry, region_end] equal lexical containment, so nested regions
and inner-catch-rethrow-to-outer come free); the dispatch if-chain
wraps in a region-routing try/catch only when regions exist;
GeneratorPrototype.throw injects the stashed error at the suspended
state via a next() delegate; try/finally lowers by per-exit-path
duplication (javac shape) with a conservative fallback walker;
catch+finally decomposes into the standard nesting; and V2b
expression-default materialization extended to arrow / function-
expression values still in the expr arena (the dflt-params
ref-prior families' unknown-identifier root cause)): passTotal
**24323** (+53), bug **11718** (+28), trAccepted **36041** (+81 —
conservation exact: +81 = +53 + 28), incompatible **17133** (−81),
core **11252** (−81). Gate predicate: **377** clusters of ≥ 4
holding 10173 (−4 clusters, −82 cases — both axes down), residue
817 / 1079. Forward 53 (GeneratorPrototype/throw all 13, yield 9,
function 7, object 7, arrow-function 6, AsyncGeneratorPrototype/
throw 4, GeneratorPrototype/return 4, async-function 2,
async-arrow 1); the 35 incompatible→bug moves are the unblocked
faces running into real semantic residue (yield 21, Generator/
AsyncGenerator return 4+4 — the D3 finally-return face, for-of 4,
AsyncFromSync 2). **Zero pass regressions, zero new timeouts, zero
new crashes.** Top clusters: eval 1411 / `__this` 267 /
dynamic-import 250.

**Previous @ `0fd3f1d7`** (2026-08-02, rotation 275 — six knives:
JSON.rawJSON / isRawJSON runtime kernels + frozen [[IsRawJSON]]
carrier spliced verbatim by the any-lane stringify; the
fromAsync / Promise.try / rawJSON-pair ns-static reflection rows
(detached fromAsync falls to ArrayCreate, plus the §10.4.2.2
length > 2³²−1 RangeError gate replacing a ~2³¹-index hang);
default-parameter TDZ (bare self/later-param default reads throw
ReferenceError at call time — plain async functions skipped, their
existing channel already answers the §15.8.4 rejection, caught as 7
sweep regressions and fixed same-rotation); array rest slots accept
nested binding patterns (`[...[x, y]]`); numeric / string-literal
property keys in object binding patterns (`{ 0: v, length: z }` via
length-guarded index reads)): passTotal **24270** (+212), bug
**11690** (−24), trAccepted **35960** (+188 — conservation exact),
incompatible **17214** (−188), core **11333** (−188). Gate
predicate: **381** clusters of ≥ 4 holding 10255 (−4 clusters,
−189 cases), residue 816 / 1078. **Zero pass regressions vs
`a28a5177`.** Top clusters unchanged: eval 1387 / `__this` 267 /
dynamic-import 250.

**Previous @ `a28a5177`** (2026-08-02, rotation 274 knife 7 on top of
the `3db6224b` stamp below — the any-lane `Promise.resolve` adopts a
boxed %Promise% cell via the new `__torajs_promise_resolve_any`
kernel instead of double-wrapping it, closing the Promise.try
flatten face; re-sweep moved exactly the 3 Promise/resolve
S25.4.4.5 adopt-semantics cases bug→pass, nothing else): passTotal
**24058** (+3), bug **11714** (−3), trAccepted **35772** (flat —
conservation exact), incompatible **17402** (flat), core **11521**
(flat). Gate predicate: **385** clusters of ≥ 4 holding 10444,
residue 816 / 1077 — all flat. Zero regressions. Known residue:
native Promise.try resolves through a resolve function (+1
thenable-job tick) while the desugar rides the static, so
cross-chain interleaving can differ; per-chain output is identical.

**Previous @ `3db6224b`** (2026-08-02, rotation 274 knife 6 on top of
the `49aa4ebb` sweep below — ES2025 `Promise.try` desugared to an
immediately-invoked try/catch arrow over the resolve/reject statics;
re-sweep moved exactly the 4 core-semantics Promise/try cases
incompatible→pass, nothing else): passTotal **24055** (+4), bug
**11717** (flat), trAccepted **35772** (+4 — conservation exact),
incompatible **17402** (−4), core **11521** (−4). Gate predicate:
**385** clusters of ≥ 4 holding 10444 (flat clusters, −4 cases),
residue 816 / 1077. Zero regressions. The 8 remaining Promise/try
cases are the reflection face (length / name / prop-desc / ctx-*) —
they need `Promise.try` as a readable function object, the
builtin-method-value family.

**Previous @ `49aa4ebb`** (2026-08-02, rotation 274 — the top-level
global promotion gap family, five knives on the shared
`any_promote_init` / `arrlit_literal_elem_ann` verdicts: empty
object literals (`var obj = {};`, the dominant test262 shared-
fixture idiom), data-only literals with null / nested / array
fields, undefined-valued fields, empty array literals (`var xs =
[]` → `any[]` over the K.6 fast path), and mixed data-literal
arrays falling back to `any[]`; method-carrying literals and
runtime-expression members stay fenced behind the tb2 guard):
passTotal **24051** (+64), bug **11717** (−39), trAccepted
**35768** (+25 — conservation exact: +25 = +64 − 39), incompatible
**17406** (−25), core **11525** (−25). Gate predicate: **385**
clusters of ≥ 4 holding 10448 (−2 clusters, −25 cases — both axes
down together), residue 816 / 1077. Forward 64, zero regressions;
15 bug→incompatible moves are timeouts/exit-3s turned into loud
refusals, and 6 incompatible→bug:exit-1 are newly-exposed
defineProperty/defineProperties 15.2.3.x cases whose `var obj = {}`
prelude now runs into the real semantic face. New crashes: 0.

**Previous @ `a4be9596`** (2026-08-01, rotation 273 — five knives:
the §15.1.1 "use strict" × non-simple-params early error at all 8
params+body parse sites; method VALUES delivering real argv through
the boxed adapter into injected `__torajs_real_argc`/`__torajs_argv`
(zero-Ident admission); escaped object-literal methods joining the
argv face with escape-vs-static resolution; object-literal generator
methods riding the GEN_ARGV_PARAM channel through their `__forward_`
relays; static methods (`__sm_`) joining the method static face plus
the devirt receiver-slot fix): passTotal **23987** (+122), bug
**11756** (−118), trAccepted **35743** (+4 — conservation exact:
+4 = +122 − 118), incompatible **17431** (−4), core **11550** (−4).
Gate predicate: **387** clusters of ≥ 4 holding 10473 (flat
clusters, −4 cases), residue 818 / 1077. Forward 122, zero
regressions: 25 rest-param-strict-body / use-strict-with-non-simple
negatives (knife 1), 56 arguments-object (48 cls-decl/expr private +
escape families, 8 obj gen-meth), and the rest across the
statements/expressions static-gen and method-escape families. New
crashes: 0.

**Previous @ `d9831e8f`** (2026-08-01, rotation 272 close — knife
F on top of the `a08ff268` sweep below: value-return fns that can
fall through now answer undefined per §10.2.1.4 instead of trapping
on an `unreachable` tail; re-sweep moved exactly 2 verdicts,
10.6-10-c-ii-1/2 `exit 133` → loud `exit 1` (the pre-existing crash
those relabels pointed at is closed; every bucket total and the gate
predicate are byte-identical to the stamp below).

**Previous @ `a08ff268`** (2026-08-01, rotation 272 — six knives:
untyped rest param → implicit `any[]`, the rest-aware static-argv
face, the whole face going unmapped for module-strict semantics
with FoldTo re-admitted as the mutation-free snapshot, the mutating
FoldArity ride, and object-literal methods joining the face):
passTotal **23865** (+10), bug **11874** (−8), trAccepted **35739**
(+2 — conservation exact: +2 = +10 − 8), incompatible **17435**
(−2), core **11554** (−1). Gate predicate: **387** clusters of ≥ 4
holding 10477 (flat clusters, −6 cases), residue 818 / 1077.
Forward 23 (meth-args-trailing-comma ×4, mapped ×6 + unmapped ×2,
rest-parameters ×2, 10.5-1gs, 10.6-10-c-ii-1-s). Regressions 13,
all attributed: 6 rest-param-strict-body pass-negatives were
metric-water (the mandatory-ann refusal rejected for the wrong
reason; knife A accepts rest params so they now surface as
negative-phase-mismatch — the real §15.2.1 non-simple-params +
use-strict early error is a registered small knife), 5
sloppy-mapped asserts (10.6-10-c-ii-1/2, mapped/ ×3) correctly fail
under the unmapped semantics (bun fails them too — module code is
strict; 2 of them additionally trip a **pre-existing**
fall-through-return crash, bisected to the rotation-271 baseline,
registered), plus 2 async variants of the same families. New
crashes beyond those 2 relabels: 0.

**Previous @ `36f97ddd`** (2026-08-01, rotation 271 — five
arguments-face knives off the trailing-comma census: uninit-let
orphan tombstone, forwarder-relay argc-vote exclusion, fn-value
alias devirtualization, spread-site face disqualification, and the
unmapped-arguments mode for default/rest/destructured params):
passTotal **23855** (+28 vs the rotation-270 census @ `e2f482ab`),
bug **11882** (−28), trAccepted **35737** (flat — conservation
exact: 0 = +28 − 28), incompatible **17437** (flat), core **11555**
(flat). Gate predicate: **387** clusters of ≥ 4 holding 10483,
residue 816 clusters / 1072 cases. 95 verdict transitions; the
arguments-object directory's non-pass fell 204 → ~160 (the whole
declare-then-assign func-expr / cls-static-meth / gen-func-expr /
unmapped-via-params families now pass; 6 class-elements
init-err-contains-arguments negatives flipped to pass-negative; 3
Array HOF cases moved incompat → honest bug:exit). **One
pass-loss**: RGI_Emoji_Modifier_Sequence pass → tr-timeout, standalone
rerun 0.04s exit 1 — a 10-worker scheduling blip, not a code
regression. New crashes 0. Registered residue: private-method
getter-return + obj-literal method escapes need the method-value
argv ABI (runtime tier, RFC knife 4); cls async-gen meth ~37 rides
the same; untyped rest params still die at mandatory-ann.

**Previous @ `d3ec34a2`** (2026-08-01, rotation 267 — seven
blades: Reflect.defineProperty data + accessor halves, Reflect.apply,
generic-callback parameter elision, RegExp-called-as-function, the
new-RegExp-with-non-string-pattern crash fix, and object spread with
an any-typed source): passTotal **23459** (+293), bug **11868**
(+49), trAccepted **35327** (+342, conservation exact = +293 + 49),
incompatible **17847** (−342), core **11943** (−310). Gate
predicate: **388** clusters of ≥ 4 holding 10874 (clusters −4,
cases −305 — both numbers down), residue 814 clusters / 1069 cases.
**Zero regressions, zero new crashes; 2 new timeouts** — the
unicode-10.0.0 identifier class pair moved not-yet-supported →
tr-timeout inside the incompatible bucket (the spread blade's
Any-literal dynobj-lane split lets a previously fast-refused shape
keep compiling; registered). 293 forward transitions. The R5a blade
threaded `throw_on_refusal` through the whole define chain (dynobj +
torajs-arr, dual extern shells, three new soft shells); R5b closed
the accessor half; R6 reused the `apply_list` kernel with the
§28.1.1 no-nullish-amnesty delta. The mid-rotation sweep @
`d28cb673` caught 4 new exit-139 crashes (the RegExp call→construct
rewrite exposed RegExp-object patterns crashing the Str-shaped
compile kernel) — fixed same rotation by the
`__torajs_regex_compile_any` kernel (§22.2.3.1 shape dispatch:
RegExp copies source/flags, else ToString), which also closed the
pre-existing `new RegExp(any)` silent-death gap. The spread blade
(`{...anySrc}`, cluster #4's 252 cases, dirs=2) runs §7.3.25
CopyDataProperties at run time — the own-enumerable walk shared
with Object.assign, pointer-slot form, which also fixed
`copy_keys`' stale-box re-read after a dynobj resize. `expected N
argument(s)` direct-call arity (372, untouched by the generic-unify
elision blade — different signature) is the next addressable
cluster after eval (1385, awaiting takagi).

**Re-derived @ `49d94689`** (2026-08-01, rotation 268 — four
blades: direct-call over-arity trailing-ignore, the member_set /
ns_static pre-split, Reflect.set Set-a, and the wide-struct
compile-time perf triple): passTotal **23517** (+58), bug **12076**
(+208), trAccepted **35593** (+266, conservation exact = +58 +
208), incompatible **17581** (−266), core **11699** (−244). Gate
predicate: **388** clusters of ≥ 4 holding 10628 (clusters flat,
cases −246), residue 815 clusters / 1071 cases. **Zero
regressions, zero new crashes, zero new timeouts**; 58 forward
transitions; the unicode-10.0.0 identifier pair moved tr-timeout →
honest not-yet-supported (the perf blade cut its 21s checker/egraph
stall to 10.7s, back inside the sweep budget). The over-arity blade
retired the former #2 cluster (`expected N argument(s), got N`, 372
— out of the top 12 entirely); its admitted programs largely land
in the bug bucket for now (arguments-object content and
callback-this divergences — the +208), which is the honest ledger:
they compile and run where they previously refused. Reflect.set
rode the R3-style parameterization across the whole member_set
chain (five receiver arms + the §10.1.9.2 inherited walk,
`Option<i64>` form); the Reflect namespace now lacks only
`construct` (133, newTarget + Proxy deep water) and the 4-arg
receiver form of `set` (Set-b, registered). Next addressable after
eval (1386, awaiting takagi): `__this` 267 (surveyed — alias/ctor
positions need an RFC-level design, adjacent to construct) and
dynamic-import LParen 250.

**Previous @ `c44a5d10`** (2026-08-01, rotation 266 — the Object
member cluster survey plus seven blades: Promise combinator statics
as values, Reflect gOPD / getPrototypeOf / preventExtensions /
isExtensible / deleteProperty / setPrototypeOf, Array.of as a value,
RegExp.escape): passTotal **23166** (+100), bug **11819** (+87),
trAccepted **34985** (+187, conservation exact = +100 + 87),
incompatible **18189** (−187), core **12253** (−118). Gate
predicate: **392** clusters of ≥ 4 holding 11179 (clusters +3
unlock-exposed, cases −120), residue 816 clusters / 1074 cases.
**Zero regressions, zero new timeouts, zero new crashes** — 100
forward transitions (86 incompat→pass, 12 →pass-no-oracle, 2
bug→pass). The `no member X on Object("X")` family fell 376 → 187
(the Promise-combinator `.call` shapes, five Reflect methods, the
Array.of / RegExp.escape reflection faces); the remaining 187 is
mostly un-reified Promise statics (48: try / withResolvers /
allSettled-keyed proposals) and Function statics (23). The
Reflect.deleteProperty blade parameterized the OrdinaryDelete
kernel's refusal throw (`any_prop_delete_impl` + soft shell — the
§13.5.1.2 strict TypeError belongs to the delete expression's
caller, not OrdinaryDelete); Reflect.defineProperty / apply / set /
construct are surveyed and filed in RFC `20260801-reflect-namespace`.

**Previous @ `38d1b8ca`** (2026-08-01, rotation 265 — the Iterator
ClassRef member blade plus four early-error blades): passTotal
**23066** (+626 vs rotation 264's 22440), bug **11732** (−340),
trAccepted **34798** (+286), incompatible **18376** (−286), core
**12371** (−286). Gate predicate: **389** clusters of ≥ 4 holding
11299 (clusters +2, cases −296 — unlock-exposed signatures, read
with the case count), residue 815 clusters / 1072 cases.
Conservation exact: +286 = +626 − 340. Blade 1 implemented the
iterator-global RFC's written-but-unimplemented §3.3 SSA face: the
eleven §27.1.4 helper names on iterator-by-construction TYPED
receivers (generator ClassRef / extends-Iterator heirs / MapIter /
ArrIter) answer Any and ride the any-lane dispatcher — the ClassRef
incompat family fell 486 → 229. Blades 2-5 are the early-error
family's next four clans (negative-phase-mismatch 1128 → 748):
block/switch-CaseBlock redeclaration (raw-AST pass + parser
dup-default), lexical decls in single-statement positions, `delete
o.#x` (parse-time, keyed on the `__priv_` mangling), and escaped
`await`/`yield` spellings (RESERVED_WORDS is escaped-only — literal
spellings untouched). 380 mismatches turned PassNegative, 205
ClassRef type-errors turned pass. Regressions: **8**, two roots,
both honest — 4× `let`-with-newline (noStrict ASI shapes bun
accepts: tr parses `L: let\nx=1` as a decl and the new
single-statement check now rejects it; the real gap is the ASI
`let`-as-identifier face, registered) and 4× escaped-await in
script-goal positions (bun rejects these files identically —
the old passes were strict-semantics accidents, no-metric-inflation
evaporation). New crashes **0**; 6 new timeouts are unlock-exposed
Iterator cases hitting the recorded next-method-not-cached boundary
(RFC blade 2a). Gate 2267 → **2270**/0/4 zero-red across five
substrate commits.

Previous census @ rotation 264 `5506154f` (the
undeclared-write strict lane + var-in-nested-block hoist fix +
numeric-separator early errors + @@split face, then a two-commit
regression repayment: stale write-mark self-heal + ToNumber/ToString
pending-throw checks + harness compareArray de-genericization):
passTotal **22440** (+201 vs rotation 263's 22239), bug **12072**
(+322), trAccepted **34512** (+523), incompatible **18662** (−523),
core **12657**. Gate predicate: **387** clusters of ≥ 4 holding
11595 (−3 clusters, −486 cases vs 263), residue 807 clusters / 1062
cases. Conservation exact: +523 = +201 + 322. The mid-rotation sweep
@ `a02973c6` caught 4 pass regressions; 3 roots fixed same rotation
(`0c8d1bb2` checker mark self-heal + coercion-kernel throw checks,
`5506154f` harness `Array<T>`→`any` — the checker now honestly
answers Any for `.split(anySeparator)`, which the generic harness
signature rejected along with 220+ pre-existing cases). Final sweep
keeps **2 regressions** (RegExp lookBehind/captures-negative +
named-groups/duplicate-names-split), one shared pre-existing root
now registered: monomorph specializes an `any` param to `Arr<Str>`
per call site and the typed str-index lane has no value for a NULL
capture slot — needs the match/split return-type honesty RFC
(plan-state L3b rotation-264 #7). New crashes/timeouts **0**.
symbol-dispatch method-face family: @@match / @@search / @@replace
over Any-typed patterns, plus the computed-key member-store
fn-expr-this face): passTotal **22196** (+10), bug **3372** (±0:
+2 search true-runs − 2 get-err bug→pass), trAccepted **25568**,
incompatible **27606**, core **15367**. Gate predicate: **426**
clusters of ≥ 4 holding 14209 (cases −9), residue 870 clusters /
1158 cases. Conservation exact: +10 = +10 + 0. True regressions
**0**; crashes 22 / timeouts 96 both unchanged. The cstm-* family
went 9/21 → 18/21 exit-0 across match/search/replace (all three
`invocation` cases stay behind the `arguments` wall).

**Re-derived @ `2ee51cef`** (2026-07-31, rotation 261; re-swept at
the final HEAD after the iterator-helper executing-gate fix-up):
passTotal **22186** (+8), bug **3372** (+6), trAccepted **25558**,
incompatible **27616**, core **15377**. Gate predicate: **426**
clusters of ≥ 4 holding 14218 (cases −16; clusters +1 — the
non-strict-this family the fnexpr knives unlocked now runs to its
own signature), residue 870 clusters / 1159 cases. Conservation
exact: +14 = +8 + 6. True regressions **0** — the mid-close sweep
@ `69e46bbd` caught 6 pass-losses (zip/zipKeyed
suspended-start-iterator-close-calls-* + options-padding), all one
root: return() must not HOLD the executing flag across
IteratorCloseAll (§27.1.4.x sets completed first); fixed same
rotation, all six re-verified.

**Re-derived @ `b53d226b`** (2026-07-29, rotation 244; re-swept at
the final HEAD after V4 knife 1): passTotal **19965** (+392), bug
**2374** (net +173 vs `6397d916`'s 2354: V2b turned a −153 family
into passes, then V4 knife 1 moved +173 whole-program rejects into
runtime bug-classified — those cases now RUN to their `.name`
assert), trAccepted 22339, incompatible **30835**, core **18585**.
Gate predicate: **495** clusters of ≥ 4 holding 17303 (cases −411,
core −412 over the rotation; clusters flat). Conservation exact
across both sweeps: +239 = +392 − 153, then +173 = 0 + 173. True
regressions **0** — 4 pass-losses at the mid sweep, all
`async-gen-meth-dflt-ary-init-iter-get-err-array-prototype`
(dflt / static-dflt × expressions / statements), the same
honest-fail family as rotation 243's pair (receiver-guard TypeError
coincidentally matching a GetIterator-on-deleted negative oracle);
the final re-sweep is verdict-stable (0 regressions, 0 forward).
Timeout 32 → 28. Forward 396 at the mid sweep: 302 in the two class
dirs + 21 object + 50 generators/async-generators + 14
Array/prototype — RFC 20260729-fn-value-any's V2b
(`e72e64b5`+`f0924087`: expression parameter defaults materialize in
the callee body per §9.2 — they had NEVER worked: solo-name shapes
whole-program-rejected, same-name shapes silently bound null), V1b
(`515f7a06`: ctor-init receivers lose the typed-ident wrap
exemption), the §9.2 optional-param undefined fix (`ba213d72`), and
V4 knife 1 (`b53d226b`: destr-slot `__genexpr_*` defaults wrap —
the fn-name-gen template family). `box_to_any FnSig` 355 → **118**
(remaining: Function.prototype call/apply/bind 22 + annexB
block-decl 12 + scattered; the fn-name-gen 186 now runs, gated on
the NamedEvaluation `.name` thread — registered follow-up). Next
walls: #1 eval (RFC awaiting sign-off), #2 fn-value-any
NamedEvaluation thread + dynobj-proto builtin Array method dispatch,
#3 `arguments` ~430. Previous stamp below.

**Re-derived @ `6397d916`** (2026-07-29, rotation 243): passTotal
**19573** (+999), bug 2354, trAccepted 21927, incompatible
**31247**, core **18997**. Gate predicate: **495** clusters of ≥ 4
holding 17714, plus 917 of ≤ 3 holding 1283. Both numbers down
together (496 → 495 clusters, 18953 → 17714 cases, core −1242).
Conservation exact: ΔtrAccepted +1242 = Δpass +999 + Δbug +243.
True regressions **0** — the verdict diff shows exactly 2
pass-losses (`async-gen-meth-ary-init-iter-get-err-array-prototype`,
expressions + statements mirror pair), both honest-fail exposure:
the old pass rode the receiver-guard TypeError coincidentally
matching a GetIterator-on-deleted negative oracle, and V2a's
bare-call drive surfaced the real missing face (array destructuring
does not consult a monkey-deleted `Array.prototype[Symbol.iterator]`
— registered). **The mid-close sweep @ `b845d957` caught 85
`async-gen-meth-static-*` passes broken by V2a** (a static forwarder
dropping `this` broke the `__sm_<C>__<m>` static-member alias chain
into whole-program rejects); fix-up `6397d916` narrows the
receiver-free relaxation to instance methods and this stamp is the
re-swept final. Timeout 31 → 32 (+1, machine-load edge). Forward
1001: 628 in the two class dirs + 200 for-await-of + 124
async-generator + 41 object — the async-iteration walls fell
together: for-await-any (`4d470616`) + F3 async yield* (`4982ffb9`)
closed RFC 20260728-gen-forof-yieldstar entirely (`yield*` refusal
and for-await-any signatures both at **0**), and RFC
20260729-fn-value-any V1 (`2e4f56d2`, fn-name args wrap on untyped
member-call receivers — the `.then($DONE, $DONE)` harness tail that
gated every async t262 case) + V2a (`b845d957`, detached
receiver-free generator-method bare-calls) opened the class
dflt-params template family (`box_to_any FnSig` 720 → **355**).
Next walls: #1 eval 735 (RFC awaiting sign-off), #2 fn-value-any
V2b (expression-default bare-call materialization — the remaining
355 + class residue), #3 `arguments` ~430. Previous stamp below.

**Re-derived @ `8dadd71a`** (2026-07-29, rotation 242): passTotal
**18574** (+51 over rotation 241's 18523 — that rotation's sweep @
`04655b9f` ran but its census stamp was skipped; its +1 is folded
into this baseline), bug 2111, trAccepted 20685, incompatible
**32489**, core **20239**. Gate predicate: **496** clusters of ≥ 4
holding 18953, plus 918 of ≤ 3 holding 1286. Conservation exact:
ΔtrAccepted +61 = Δpass +51 + Δbug +10. True regressions **0**; new
crash 0; timeout 28 → 31 (three `identifiers/start-unicode-*-class*`
moved not-yet-supported → tr-timeout inside incompatible — the
mid-rotation sweep at `362270e7` still showed not-yet-supported and
the only commit after touches the async-expr parser guard, so this
is machine-load edge, registered watch). Forward attribution: 45 of
51 in the two class dirs (class generator methods' for-of bodies —
the F1 state-machine ForOf arm), 6 across for-of / generators /
yield / object. Bug +10 = the registered F2 residual surfacing
(sync yield* abrupt-completion forwarding, stdout-mismatch family).
**Mid-close audit caught and reverted a 65-case inflation**: the
un-flagged `async function*` EXPRESSION form rode the F2 sync
next-drive — 65 coincidental passes (sync GetIterator TypeError
happening to match the async oracle) + 36 abrupt mis-drives; fix-up
`8dadd71a` restores the attributed refusal and this stamp is the
honest re-baseline (the intermediate sweep read 18639/2147). Next
walls: #1 eval 1076 (RFC awaiting sign-off), #2 `box_to_any FnSig`
720, #3 async yield* (F3, needs for-await-any), #4 `arguments` ~430.
Previous stamp below.

**Re-derived @ `b9a8019b`** (2026-07-28, rotation 240; re-swept at
the final HEAD `2879793f` after the size-debt refactor — verdicts
byte-identical across all 53174 cases, every number below unchanged):
passTotal
**18522** (+362), bug 2101, trAccepted 20623, incompatible **32551**,
core **20301**. Gate predicate: **497** clusters of ≥ 4 cases holding
19015, plus 918 clusters of ≤ 3 holding 1286. Clusters −3 with cases
−577 and core −572 — all three axes down. Conservation exact:
ΔtrAccepted +572 = Δpass +362 + Δbug +210. Pass regressions **0**
(the single pass-family transition is static-init-invalid-yield:
pass-negative → negative-unsupported, an HONEST correction — the old
pass rode "tr can't parse bare yield" hitting the expected
SyntaxError by accident; the real §15.5.1 non-generator-context
early error is a registered face). New crashes 0 (the mid-rotation
`2n ** any` exit-139 was recovered same-rotation by the BigInt
guard), new timeouts 0. Forward attribution: 251 of 363 in the two
class dirs (empty `;` element + setter arg_conv), 22 exponentiation
(S2.43), ~45 across for-of / for / for-await-of / compound-assign
(bare `yield;` fixture drivers + private-accessor compound). Next
walls: #1 eval 1074 (register sign-off), #2 yield* bare-ident, #3
box_to_any FnSig, #4 `arguments`. Previous stamp below.

**Re-derived @ `3fb2e527`** (2026-07-28, rotation 239; stamp
recovered by rotation 240 — the 239 close wrote the S2.37-39 entries
but skipped this census block): passTotal **18160** (+284), bug 1891,
trAccepted 20051, incompatible **33123**, core **20873**. Gate
predicate: **500** clusters of ≥ 4 holding 19592, plus 914 of ≤ 3
holding 1281. Conservation exact: +534 = +284 + 250. True
regressions 0 (12 mid-rotation forbidden-ext pass losses recovered
same-rotation by the collect double-arm fix). Cluster +7 with cases
−541: unlock exposure. Forward 285 concentrated in the two class
dirs — old cluster #1 static private (1482/4 dirs) emptied entirely.
Previous stamp below.

**Re-derived @ `c56c47ca`** (2026-07-28, rotation 238): passTotal
**17876** (+350), bug 1641, trAccepted 19517, incompatible **33657**,
core **21412**. Gate predicate: **493** clusters of ≥ 4 cases holding
20133, plus 914 clusters of ≤ 3 holding 1279. **Second rotation in a
row with both numbers falling together** (501 → 493 clusters, 20568 →
20133 cases). Conservation exact: ΔtrAccepted +457 = Δpass +350 +
Δbug +107. Pass regressions **0**; timeout 90 flat, crash 0. Forward
attribution: 318 of 334 verdict-level fresh passes sit in the two
class directories — the S2.33+S2.34 pair emptied the 457-case
ClassRef cluster out of the top list entirely; 9 for-await-of passes
came from the S2.35 promotion. En route, S2.36 closed a pre-existing
worst-shape silent wrong (inline objlit args to any-dispatched
destr-param methods died with exit 0). Next walls: #1 static private
1482/4 dirs (both parser reject sites already located), #2 eval 1070
(register sign-off), #3 yield* bare-ident 555, #4 `box_to_any FnSig`
540/12 dirs (fn-value materialization, newly in the top list), #5
`arguments` 434/16 dirs. Previous stamp below.

**Re-derived @ `591c5fd5`** (2026-07-28, rotation 237; census block
recovered by rotation 238 — the 237 close committed only the
dashboard): passTotal **17526** (+111), bug 1534, trAccepted 19060,
incompatible **34114**, core **21869**. Gate predicate: **501**
clusters of ≥ 4 holding 20568, plus 927 of ≤ 3 holding 1301.
Conservation exact: +137 = +111 + 26. Pass regressions 0. Cluster +4
with cases −139: unlock exposure (static private 1404 → 1482, yield*
525 → 555 both swelled from newly-reachable cases), the fifth
case-count fall in a row. Previous stamp below.

**Re-derived @ `7e25fd42`** (2026-07-28, rotation 236): passTotal
**17415** (+1140 over rotation 235's 16275 — the largest single-rotation
gain of the sweep era), bug 1508, trAccepted 18923, incompatible
**34251**, core **22006**. Gate predicate: **497** clusters of ≥ 4
cases holding 20707, plus 926 clusters of ≤ 3 holding 1299. The case
count fell for the fourth rotation running (22197 → 20707, −1490) with
clusters near-flat (495 → 497). Conservation is exact: ΔtrAccepted
+1518 = Δpass +1140 + Δbug +378. Pass regressions **0** (per-case
verdict diff). Timeout 24 → 28: four fresh `Array.prototype.map.call
(obj, cb)` with `length: Infinity` — the blade-5b unlock lets the cb
value through and the runtime arraylike-map loop lacks the §23.1.3.19
len RangeError guard (real bug, L3b). Crash 14 → 15 (one fresh
exit-139, `class` setter restricted-ids, L3b). Top movement — cluster
#1 (untyped fn-decl param, 1488 cases measured at entry) fell
entirely out of the top clusters to six blades: the `__this`-first
closure-shape any-default (class-method `__param_destr_N` holders +
bind_this_param-promoted fns), the full-arity §23.1.3 HOF callback
substrate ((elem, index, srcArray) formals + position seeds + lowering
argv), the any[any] keyed index kernel (read + write + struct
receiver), and the untyped-plain-fn value-use wrap whose forwarder is
the generic's all-any mono site. 378 fresh bugs are honest exposure of
callback-heavy cases now reaching the runtime. Two pre-commit full
gates ran for the wrap-class blades (handoff-235 discipline); zero
in-rotation regressions. Previous stamp below.

**Re-derived @ `b83b0f88`** (2026-07-28, rotation 235; final sweep —
the two earlier sweeps at `c7303b36` / `efae2a7e` differed only by the
keys/values/entries closure blades, +2 pass / +4 bug total, 0
regressions at every diff): passTotal
**16275** (+316 over rotation 234's 15959), bug 1130, trAccepted 17405,
incompatible **35769**, core **23527**. Gate predicate: **495** clusters
of ≥ 4 cases holding 22197, plus 949 clusters of ≤ 3 holding 1330. The
case count fell for the third rotation running (22663 → 22197, −466)
with clusters up (483 → 495) — the unlock-exposes-new-signatures shape;
the case count stays the honest axis. Conservation is exact:
ΔtrAccepted +455 = Δpass +316 + Δbug +139. Pass regressions **0**
(per-case verdict diff at all three sweeps); timeout 24 flat; crash 12 → **14** (two fresh
exit-139: `defineProperty(arr, 2^32-2, …)` boundary-index cases whose
checker wall fell and whose dense-grow alloc now SIGSEGVs — runtime
array-index-clamp face, L3b). Top movement: 310 `type error → pass` +
3 nys + 1 bug — cluster #4 (`not callable: type Any`, measured 862
cases) fell to the Any-member-callee admit (checker general tail +
any-method-call mirror gate + fn-receiver wrap axis), the Symbol()
K.3b global promotion (with the generic return-station global-borrow
retain fix), the Object/Reflect fn-arg wrap, and cluster #5's bare
top-level-this shape fell to the module-this `{}` rewrite. 212
`type → not yet supported` + 19 no-oracle mirrors are the wall one
layer deeper (mostly Object.* lanes now reached with non-struct
receivers); 133 fresh bugs net are honest exposure. Two gate
regressions were caught and fixed in-rotation before ship
(generator-family wrap severing the %GeneratorFunction% chain;
toplevel-this walker descending into objlit method bodies). Previous
stamp below.

**Re-derived @ `47b722ac`** (2026-07-28, rotation 234): passTotal
**15959** (+311 over rotation 233's 15648), bug 991, trAccepted 16950,
incompatible **36224**, core **23979**. Gate predicate: **483** clusters
of ≥ 4 cases holding 22663, plus 941 clusters of ≤ 3 holding 1316. The
case count fell hard again (23073 → 22663, −410) with clusters
essentially flat (482 → 483). Conservation is exact: ΔtrAccepted +401 =
Δpass +311 + Δbug +90. Pass regressions **0** (per-case verdict diff,
run at both the mid-rotation `665d8958` sweep and this final one);
timeout 24 and crash 12 (3× exit 138 + 9× exit 139) both flat. Top
movement: 308 `type error → pass` — cluster #4 (`__new_*`
new-on-a-function, 930 cases / 39 dirs) fell to the fn-expr
constructor blades (RFC 20260726 blades A/B), the
`new Promise(executor)` desugar, the class-instance promise admit, and
the AggregateError/SuppressedError injection; residuals are near zero
(Con 233→0, ConstructFun 126→1, Promise 216→1, AggregateError 11→0).
131 `type → not-yet-supported` is the wall one layer deeper; 90 fresh
bugs net (75 exit-1 + 14 stdout-mismatch at the mid sweep, two of them
recovered by the called-as-function blade) are honest exposure of cases
now reaching the runtime. New top gaps: #1 untyped fn-decl param 1469, #2
static private 1404, #3 `eval` 1035, #4 `not callable: type Any` 847 /
21 dirs (spotting note: the reject is check_type_of_call/general.rs's
non-Function catch-all; the exemplar shape is
`Object.prototype.hasOwnProperty.call(x, k)` — a nested-member
Any-typed callee). Previous stamp below.

**Re-derived @ `b5fb1232`** (2026-07-28, rotation 233): passTotal
**15648** (+506 over rotation 232's 15142), bug 901, trAccepted 16549,
incompatible **36625**, core **24380**. Gate predicate: **482** clusters
of ≥ 4 cases holding 23073, plus 935 clusters of ≤ 3 holding 1307.
Cluster count rose (472 → 482) while cases fell hard (23639 → 23073,
−566) — the unlock-exposes-new-signatures shape the section header
warns about; the case count is the honest axis this rotation.
Conservation is exact: ΔtrAccepted +532 = Δpass +506 + Δbug +26. Pass
regressions **0** (per-case verdict diff); timeout 24 and crash 12 both
flat. Top movement: 480 `type error → pass` + 25 no-oracle mirrors —
cluster #4 (`no member on Promise(Struct)`, 1014 cases) fell to the
`.then`/`.catch` receiver generalization + by-type await dispatch
(expression position, promise-in-any, ternary field-depth unify); 31
`type → not-yet-supported` is the wall one layer deeper, 26 fresh bugs
are honest exposure. New top gaps: #1 untyped fn-decl param 1469, #2
static private 1404, #3 `eval` 1034, #4 `new`-on-a-function `__new_*`
930 across **39 dirs** (widest span). Previous stamp below.

**Re-derived @ `410d8a9f`** (2026-07-28, rotation 232): passTotal
**15142** (+216 over rotation 231's 14926), bug 875, trAccepted 16017,
incompatible **37157**, core **24912**. Gate predicate: **472** clusters
of ≥ 4 cases holding 23639, plus 919 clusters of ≤ 3 holding 1273.
Clusters and cases fell together again (476 → 472 and 23855 → 23639).
Conservation is exact: ΔtrAccepted +226 = Δpass +216 + Δbug +10. Pass
regressions **0** (per-case verdict diff); **tr-timeout 49 → 24** — the
IteratorResult [[Get]] fix un-hung the whole iter-val-err family, 25 of
them straight to pass. Top movement: 132 `type error → pass`
(CoverInitializedName + default-guard GetV + for-await by-type), 22
`bug → pass`, 15 `parse → pass`; 20 `type → parse` + 18
`type → not-yet-supported` are the wall moving one layer deeper.
Cluster #4 (`no member on Promise(Struct)`) now 1014 cases / 7 dirs —
the for-await unlock pushed more cases onto it; its body (`.then` /
`.value` on Promise(Struct), the AsyncFromSyncIterator family) is
untouched and is the next widest reachable gap. Previous stamp below.

**Re-derived @ `98efa861`** (2026-07-27, rotation 228): passTotal
**14444** (+307 over rotation 227's 14137), bug 768, trAccepted 15212,
incompatible **37962**, core **25720**. Gate predicate: **484** clusters
of ≥ 4 cases holding 24438 cases, plus 919 clusters of ≤ 3 holding 1282.
**Both numbers fell for the second rotation running** (487 → 484 and
24755 → 24438), so the pattern from rotation 227 held rather than
reverting to the count-up-cases-down shape of 225/226. Conservation is
exact: ΔtrAccepted +326 = Δpass +307 + Δbug +19. The nine
`pass-negative → negative-unsupported` moves are S2.21, and were passing
only because the escape was a lex error. Previous stamp: `b58797fb`
(2026-07-27, rotation 227) — passTotal 14137, core 26046, 487 clusters
holding 24755. The per-group counts below are still stamped
`@ 9215301c` except where an entry says otherwise.

**Scope split.** Of 38717 incompatible cases, **12241 are post-v1.0
surface**: Temporal, TypedArray/Atomics (with ArrayBuffer and DataView,
which are its substrate), intl402, Proxy/Reflect. The split is by test
path *and* by blocking identifier — `new Proxy(...)` used as a fixture
inside an Object test is still a Proxy case. The exclusion lists live in
the script (`POST_V1_PATH` / `POST_V1_GLOBALS`), so the split is
mechanical, not judgment applied per sweep.

**core = 26370 @ `7ef9b170` (was 26476 when this section was
written). That is P-SURF's denominator.** Its shape, from the
census:

| cluster depth | core cases covered |
|---|---|
| top 10 | 29.5 % |
| top 25 | 43.3 % |
| top 50 | 57.1 % |
| top 100 | 71.4 % |
| top 400 | 91.6 % |
| clusters of ≤ 3 cases (815 of them) | 8.7 % |

(refreshed @ rotation 463 closing sweep `92343562`, core **2500**,
**167** clusters of ≥ 4 holding 1406 cases; 655 clusters of ≤ 3 hold
820 more (32.8 %), and the subset-decision register accounts for 274
across two entries. Coverage: top 10 = 14.4 %, top 25 = 25.3 %,
top 100 = 52.8 %, top 400 = 82.3 %. Rotation 463 took one incompat
signature — `argument N: expected Number, got String`, 22 cases across
7 directories — and found five builtin parameters whose SPEC STEP IS A
COERCION implemented instead as a static shape gate: `charAt`'s `pos`,
`fromCharCode`/`fromCodePoint`'s codes, `toFixed`'s digits,
`Array.with`'s index, `BigInt.asIntN`'s bits. Widening them turned up
three wrong answers on operands tr already accepted, all with the same
cause — an i64 parameter where the spec step reads a Number, so the
fact each check is about (±∞ for ToUint16, non-integral for
fromCodePoint, NaN and 2^53 for ToIndex) was destroyed before the
check. A sixth lane, `[42].includes("42")`, was not a coercion at all:
comparing by type first is the whole content of §7.2.15, so a needle
of another type is a question with an answer. The signature went 22 →
**0**; passTotal **+50, zero pass regressions**, gate 3210 →
**3215**/0/4 across five substrate commits.)

Earlier stamp @ rotation 461 closing sweep `df4076d6`, core **2574**,
**175** clusters of ≥ 4 holding 1479 cases; 653 clusters of ≤ 3 hold
819 more (31.8 %), and the subset-decision register accounts for 276
across two entries. Coverage: top 10 = 14.0 %, top 25 = 25.0 %,
top 100 = 52.5 %, top 400 = 82.6 %.

Earlier stamp @ rotation 460 closing sweep `4006d0ec`, core **2611**,
**177** clusters of ≥ 4 holding 1507 cases; 661 clusters of ≤ 3 hold
828 more (31.7 %), and the subset-decision register accounts for 276
across two entries. Coverage: top 10 = 13.9 %, top 25 = 24.9 %,
top 100 = 52.4 %, top 400 = 82.5 %.

Earlier stamp @ rotation 424 closing sweep `3800ad73`, core **4212**,
**234** clusters of ≥ 4 holding 2767 cases; 734 clusters of ≤ 3 hold
923 more (21.9 %), and the subset-decision register accounts for 522
across two entries. Coverage: top 10 = 14.8 %, top 25 = 29.5 %,
top 100 = 58.8 %, top 400 = 85.7 %.

Earlier stamp @ rotation 420 closing sweep `af31fa42`: core 4572,
238 clusters of ≥ 4 holding 2957 cases; 773 clusters of ≤ 3 hold
1004 more (22.0 %), register 611 across two entries. Coverage:
top 10 = 14.2 %, top 25 = 28.8 %, top 100 = 58.4 %, top 400 = 85.0 %. Rotation 420 took rotation 419's
registered follow-up — a class's computed member name is not
ToPropertyKey'd where the class is defined — and found that the ONE
thing §15.7.14 says about the class-definition point is the thing tr
had never implemented: the FIELD lane parked the key as an
unconverted box (so `toString` ran per construction, and never at all
for a class that is defined and not constructed); hoisting a nested
class carried its whole definition-time evaluation to the END of the
module; static field initializers and static blocks were prepended to
the head of the module instead, which ran them before every top-level
`var` initializer (silently wrong for a scalar, SIGSEGV for a heap
global); `this` in those two positions was the instance receiver, not
the class; and `super.m()` in them died at typecheck on the parser's
raw marker. Five defects, one shared cause — the definition point had
no owner. passTotal **+37, zero regressions**, gate 2997 →
**3002**/0/4 across six substrate commits.)

(previous stamp @ rotation 419 closing sweep `5c33b06c`, core **4574**,
**238** clusters of ≥ 4 holding 2961 cases; 772 clusters of ≤ 3 hold
1002 more (21.9 %), and the subset-decision register accounts for 611
across two entries. Coverage: top 10 = 14.2 %, top 25 = 28.8 %,
top 100 = 58.4 %, top 400 = 85.0 %. Rotation 419 asked rotation 418's
question — what does a typed lane do when a builtin arrives through
an `any` slot — of Date, Map, Set, Promise and Array. Date and Array
were clean; the other three gave up six defects, all of them silent:
`new Map(pairs)` nulled every value of a mixed-type pair (an
Array<Any> slot is 8 bytes, and the last slot-walk site in the
compiler still scaled it by 16 — invisible at index 0, which is why
the KEY always arrived), `String(map)` answered "NaN" (the mid-miss
sentinel is a quiet NaN and OrdinaryToPrimitive took it for a value),
`typeof map.toString` answered undefined while calling it worked, and
two await sites read a promise's settled slot in a form the cell did
not carry — one handing back the NaN box's raw bits, one rc_inc'ing
the number 1. Following the last one out found a seventh: ONE
method-valued field kept a whole top-level object literal off the
globals table, so every named function in the file saw "unknown
identifier" for it. passTotal −1, **two pass regressions, both
de-watering** — the cases passed because that binding was broken, and
the real defect they document (a class computed member name is not
ToPropertyKey'd at definition time) is now visible instead. gate
2991 → **2997**/0/4 across six substrate commits.)

(previous stamp @ rotation 417 closing sweep `bc00feb6`, core **4577**,
**238** clusters of ≥ 4 holding 2963 cases; 771 clusters of ≤ 3 hold
1000 more (21.8 %), and the subset-decision register accounts for 614
across two entries. Coverage: top 10 = 14.2 %, top 25 = 28.8 %,
top 100 = 58.5 %, top 400 = 85.0 %. Rotation 417 worked the
`unknown ident __this` cluster, which the previous rotation had
attacked by shape without first counting the shapes — the honest
count says it is not one gap but several. The largest single root was
a class whose heritage is a VALUE expression: that lane lowers the
class to an ES5 constructor function, whose `this` needs the knife-2
promotion, and three ordinary value reads of the class binding
(`typeof K`, a `var` alias, `export default K`) had no admitting use
shape. The `with` guard was leaving the node it consumed alive in the
arena, which read as a use in no recognised position and cost the same
promotion. passTotal +26, **zero pass regressions**, gate 2979 →
**2984**/0/4 across five substrate commits; the cluster's occurrence
count fell 89 → 59.)

(previous stamp @ rotation 320 closing sweep `f1c685b1`, core **8674**,
**311** clusters of ≥ 4 holding 7678 cases; 756 clusters of ≤ 3 hold
996 more (11.5 %). Coverage: top 10 = 31.2 %, top 25 = 43.9 %,
top 100 = 70.5 %, top 400 = 91.2 %. Rotation 320 worked the
builtin-prototype patch surface and two substrate bugs it turned up:
ToString of a bigint now converts directly per §7.1.17 step 7 instead
of consulting the method dispatcher — which is what let the pre-gate
whitelist finally reach `family_of`'s full domain — a step method
taken off `%GeneratorPrototype%` re-dispatches a live instance for
both kinds instead of refusing, `.constructor` is recognised as a way
to reach a builtin prototype, and the stack-write trim buffer carries
the view type it always had, which both restored a bench case that
had silently stopped compiling and closed the parent-reference leak
riding on the same missing type. passTotal +1, **zero pass
regressions**, gate 2530 → **2534**/0/4 across six substrate commits.
The `.constructor` rule shipped blunt first and cost 156 regressions
suite-wide via `assert.js`; it was narrowed in the same rotation.)

(previous stamp @ rotation 314 closing sweep `0d5be9b6`, core **8706**,
**312** clusters of ≥ 4 holding 7711 cases; 754 clusters of ≤ 3 hold
995 more (11.4 %). Coverage: top 10 = 31.0 %, top 25 = 43.9 %,
top 100 = 70.5 %, top 400 = 91.2 %. Rotation 314 worked the weak
families and the collection constructors: the WeakMap / WeakSet /
WeakRef constructors read as values (proto tags 16-18), a builtin
prototype now reads what it INHERITS from `Object.prototype` and not
only what it owns, and all four collection constructors take a
general iterable initializer through one runtime walk. +148 to pass,
zero pass regressions, gate 2508 → **2512**/0/4 across five substrate
commits.)

(previous stamp @ rotation 265 closing sweep `38d1b8ca`, core **12371**,
389 clusters of ≥ 4 holding 11299 cases. Earlier stamp @ rotation
263 `c52fd6d1`: core 13147, 390 clusters of ≥ 4 holding 12081. Rotation 263 shipped RFC
20260730-undeclared-ident (policy B, decided under takagi's
ceiling-first delegation): expression-position reads that resolve
nowhere type Any + warn on stderr and raise a catchable runtime
ReferenceError (`<name> is not defined`), per §6.2.5.5 GetValue —
including inside closures (unresolved captures pruned from env
layouts, the body read throws) and as call callees. Write positions
(`x = 1`, `x++`) stay compile rejects. The flip moved 8462 cases
out of the incompatible bucket (trAccepted 25568 → 33989,
conservation exact: +43 pass, +8419 now run and fail honestly);
core shrank 15367 → 13147 and ≥4-clusters 426 → 390. It also
evaporated 1161 wrong-reason PassNegatives (phase:parse negatives
that had "passed" because tr choked on the harness marker
`$DONOTEVALUATE` — the honest gap is the unimplemented early-error
family: block-scope redeclaration, ASI, RegExp property-escapes;
now registered). Gate 2259 → **2261**/0/4 zero-red across five
substrate commits; zero positive-case regressions.

Previous census @ rotation 259 `858e2702`: core 15467,
429 clusters of ≥ 4 holding 14354 cases. Rotation 259 chased the
concat exit-139 crash down to a GENERAL substrate bug and shipped
RFC 20260731-mono-closure-clone: a generic fn's lifted closures
(arrows / fn-exprs / objlit methods) were SHARED across every
monomorphized specialization, so the per-construction-site
`closure_captures` entry was overwritten by whichever spec lowered
last and the shared `__env_drop` walked the other specs' envs with
the wrong capture layout — a boolean slot dropped as a heap pointer
dereferences VALUE_TRUE=0x7 (minimized to 15 lines with no iterator
involvement). Fix = C++/Rust generic-lambda semantics: clone every
referenced lifted closure per spec under the spec's `$$_` suffix,
mirror the name-keyed side tables + globals signature, rewrite the
closure sites BEFORE the spec body checks (the construction-site
check_closure walk of the clone bodies is what records their inner
generic calls — a mid-rotation sweep caught the shadowing family
regressing to "unknown function" under the first ordering), and
retire the shared originals via the mono skip set. Companion scope
fix: `mutated_captured_lets` was collect_assigned_names over the
WHOLE ast, so any same-named toplevel assignment promoted an
unrelated fn's never-written param to a capture box. Three more
knives rode the same crash trail: symbol-keyed index calls on TYPED
receivers (`[1][Symbol.iterator]()` lost its base per §13.3.6.2 and
hit the reified cell's this-undefined entry; Map/Set joined the
checker's property-key domain), wrapper receivers now inherit
symbol keys through their primitive's prototype singleton
(defineProperty(Boolean.prototype, Symbol.iterator) was invisible
to Object(true); SymbolWrapper's proto mapping was also wrong —
Number's dict), and the four Iterator statics reified as ns-static
VALUES (table rows + checker arm + dispatch into the same kernels;
`Iterator.concat.length` was "no member"). That unlock exposed a
pre-existing crash: emit_drop_closure's inline dec didn't know
FLAG_STATIC_LITERAL, so releasing a non-ident ns-static temp
(`Object.getPrototypeOf(Math.max)`!) took the immortal cell's rc
to 0 and CallIndirect'd its null drop_fn — the closure drop now
tests the static bit first. Sweep: passTotal +32 / bug −16 /
trAccepted +16, conservation exact; regressions: the ONE pass-loss
is the known borderline-timeout case
(`Array/length/S15.4.2.2_A2.1_T1`, pass↔tr-timeout oscillation,
non-semantic). Gate: 2238 → **2244**/0/4 zero-red across six
substrate commits. Previous stamp: rotation 258 closing sweep
`e697516f`, core **15483**,
429 clusters of ≥ 4 holding 14371 cases. Rotation 258 root-caused
and fixed the values() mint-drop leak — NOT cycle/mmalloc: the
keys/values/entries lowering never released a literal receiver's
stake, so every 72B arr cell parked at rc=1, got scan_black'd out
of the cycle buffer and leaked permanently (mmalloc per-class diag:
C128 a=200000/f=0 = the exact 25.7MB delta; churn 32.24 → 6.59MB
flat) — and closed the iterator-global RFC's blade 5 in full:
`Iterator.concat` (eager per-item iterability check, lazy
kind-CONCAT cell, items-list ownership transfer), `Iterator.zip`
(three modes — shortest / longest with padding / strict, eager
column opens, exhausted-slot = openIters removal, longest abandons
the trailing all-padding row) and `Iterator.zipKeyed` (own
enumerable keys snapshot, per-key padding Get, object rows) sharing
one RowSink row step; derive_flattenable grew the StringWrapper
code-unit arm (§22.1.5). zip/zipKeyed have NO reference runtime
(bun/node both lack joint-iteration — probed) so their fixtures
ride the runner's .expected oracle override. A fourth fix fell out:
cm_demote's receiver-shape gate admitted Call but not As casts, so
`(expr as any).next()` with any generator present was a guaranteed
checker reject. passTotal +19 (concat / zip / zipKeyed direct hits
+ the borderline-timeout case back to pass) / bug +10 (all
progress-exposure inside the newly-unlocked buckets; one exit-139
crash single-listed: concat/throws-typeerror-when-iterator-not-an-
object) / trAccepted +29, ZERO pass regressions. Mixed-nested
array-literal inline args mis-stamp the elem-kind chain of
secondary columns (silent-wrong, any-binding path is correct) —
registered L3b. Previous stamp: rotation 257 closing sweep
`e35eef75`, core **15512**, 428 clusters of ≥ 4 holding 14397
cases. Rotation 257 finished the
iterator-global RFC's blade 4 + the blade-2c redo: `Iterator.from`
(GetIteratorFlattenable — user @@iterator wins, builtin iterables
mint their lane, anything else is its own iterator; pass-through
when already on %Iterator.prototype%'s chain, else a kind-WRAP cell
whose next/return forward), `flatMap` (inner-slot double loop,
REJECT-PRIMITIVES), the iterator cells' method-VALUE read face
(next + helper family reified on the Iterator row; the first cut's
3-fail root-caused by SPEC — `return` belongs to
%IteratorHelperPrototype% alone, never Array/Map iterator protos),
and @@iterator return-this (§27.1.2.1, unlocking spread /
Array.from over iterator cells). passTotal +10 (Iterator/from +
staging lazy-methods) / bug −2 / trAccepted +8, one pass →
tr-timeout under 10-worker load (S15.4.2.2_A2.1_T1, standalone
re-run passes — borderline-timeout noise, not a regression). A
200k-iter churn face isolated a PRE-EXISTING reachable-RSS
accumulation (~130B/iter) in the `[].values()` mint-drop path
(clean-HEAD binary reproduces; leaks-tool clean; blade-4's own
wrap / all-generator flatMap churns are flat) — registered L3b.
Previous stamp: rotation 256 closing sweep `e93c16a5`, core
**15520**, 428 clusters of ≥ 4 holding 14406 cases. Rotation 256
opened the
iterator-global RFC and landed its first four blades: the `Iterator`
global joins the proto table as tag 15 (§27.1.3 identity, abstract
ctor TypeError, `extends Iterator` via stripped heritage + prototype
chain, §7.3.22 instanceof walk; %GeneratorPrototype% now chains to
%Iterator.prototype% per §27.1.2), and `Tag::IterHelper` cells carry
the §27.1.4 helper family — lazy map/filter/take/drop, eager
toArray/forEach/some/every/find/reduce — over one
step_derived_iterator drive shared by generator instances, iterator
cells and helper chains. passTotal +17 (Iterator ctor identity /
subclassable / staging lazy-methods semantics) / bug +77 (the
Iterator bucket advanced from unknown-ident compile rejects to
next-layer semantic mismatches — the progress-exposure shape) /
trAccepted +94, ZERO pass regressions. A method-VALUE read face for
iterator cells shipped and was gate-reverted same-rotation (the
reified cell's receiver protocol isn't ready — three for-of close
regressions; redo needs the invoke_with_this contract, L3b). The
blade-2b two-layer churn caught the 2a drop glue violating the
value-drop release-one-reference contract (double-free). Previous
stamp: rotation 255 closing sweep `f8e8298a`, core **15614**,
426 clusters of ≥ 4 holding 14500 cases. Rotation 255 completed the
exotic-backed subclass blade-2 tag walk — `class C extends
Number/String/Boolean/Function/Map/Set/Promise/RegExp` all mint REAL
exotic cells on the blade-0 identity substrate (five substrate
commits, one per tag group; per-tag super() semantics: wrapper
[[*Data]] coercions, Promise's full §27.2.3.1 executor run bound to
the instance, RegExp recompile-and-swap). M5.N 322 → **247** (the
extends face itself is done; the remainder is `extends Iterator` 130
awaiting the iterator-global RFC + declared-parent ordering tails).
passTotal +20 (the subclass-builtins suite directly) / bug +12
(annexB RegExp legacy-accessors unlocked to their next layer) /
trAccepted +32, ZERO pass regressions. The RegExp churn probe caught
a real ~32B/iter leak on the way — the legacy pair API materializes
ShortStr patterns into owned cells; fixed with the borrow-safe
`anyv_cell_ptr` probe. Previous stamp: rotation 254 closing sweep
`d159c899`, core **15646**,
425 clusters of ≥ 4 holding 14534 cases. Rotation 254 closed the
`assertions` cluster outright (296 cases, nested function
declarations inside closure bodies — free-vars now hoist-binds and
walks them, and the capturing router scans the expression arena) and
landed `class C extends Array` end to end on the new exotic-subclass
substrate (FLAG_SUBCLASSED + instance side table; instanceof both
ways, getPrototypeOf, super(len), method override — M5.N 334 → 322).
passTotal +67 / bug +116 / trAccepted +183, ZERO pass regressions;
105 of the new bugs are the unlocked assertions-family cases running
into the async-verify layer (verifyProperty / $DONE reporting), the
progress shape this section describes. Rotation 253's timeout-window
regression re-passed and is closed. Previous stamp: rotation 253
closing sweep `f8c93908`, core **15829**,
432 clusters of ≥ 4 holding 14688 cases. Rotation 253 drove the
`values` cluster's real root through — mutable Arr globals promote
(K.6 close), un-annotated + nested array-literal top-level bindings
promote on a synthesized `T[]` spelling both checker and lowerer
resolve, and materialize-converted params carry an explicit `any` —
plus two pre-existing silent-wrong fixes the probes surfaced
(an Any box flowing raw into a declared boolean return slot, and the
escape analysis stack-allocating cells the K.3b promote had already
put behind a global slot). passTotal +256 / bug +2 / trAccepted +258,
258 forward moves and 2 pass regressions both case-attributed
(one timeout-window edge on `new Array(4294967295)`, one
non-trailing-default arity strictness filed with the
undeclared-ident policy RFC). Previous stamp: rotation 252 closing
sweep `53c4e80a`, core **16090**,
437 clusters of ≥ 4 holding 14955 cases. Rotation 252 closed RFC
20260730 (allSettled joined the any-lane + dyn combinator paths),
landed `class C extends Object` as base-class shape (M5.N knife 1),
and shipped Array.fromAsync's sync-source MVP + mapfn form —
passTotal +40 / bug +34 / trAccepted +74 over the rotation, all 105
verdict moves case-attributed to those three faces, zero pass lost.
Previous stamp: rotation 251 closing sweep `40085319`, core **16164**,
440 clusters of ≥ 4 holding 15029 cases. Rotation 251 ported
asyncHelpers.js for real (`needs asyncHelpers.js` 341 across 33
directories → 0), wired the fn-constructor instance prototype link
(§10.2.2 step 5), and gave Promise.all/any/race true runtime
GetIterator over dynamic arguments (RFC 20260730 knives A+B) —
passTotal +161 and bug −83 over the rotation, with the bug drop
coming from bug-bucket cases the knives converted to passes.
Mid-rotation stamp: sweep `d4163724`, core 16218, 446 clusters of ≥ 4
holding 15052. Stamp before that: rotation 250 sweep `1f21a019`, core
**16243**, 443 clusters of ≥ 4 holding 15129. Rotation 250 is the shape this section warns
about: cluster count up two while the cases in them fell 476 and core
fell 458. Two clusters left the census outright — `needs
isConstructor.js` (377 across 40 directories) went to zero and
`unknown ident __new_*` (385 across 41) went to two — and the cases they
had been blocking now run far enough to show signatures of their own.
Stamp before that: rotation 249 sweep `38eb6513`, core **16701**, 441
clusters of ≥ 4. That rotation ran the sweep twice: after its first six fixes
every figure came back identical (core 16719, same as rotation 248's
`6122f7c6`) because memory-safety and property-key correctness make
programs that already run answer right; the movement above came from
its last three, which stopped refusing programs outright. Rotation 247
read core 16724; rotation 246
read core 17144 with top-10 at 27.7 %, rotation 245 core 18112 at
24.5 %, rotation 230 core 25421 at 34.4 %. **The rotation-246 reading is not comparable to the two before
it on the oracle side**: that rotation removed a `var` → `let` source
rewrite the runner had been applying before handing cases to both
engines, so rotation 245 and earlier measured rewritten programs.) The tail is short:
1250 clusters total, but seven tenths of the mass is in 100 of them —
which is why this phase is enumerable at all. **The curve has
flattened as core shrank**: the same fixes that removed 7300 core
cases since rotation 230 removed whole head clusters, so the remaining mass is spread
wider even though it is smaller. A falling top-10 share is what
progress looks like here, not a regression.

**The groups at a glance** (core cases @ `9215301c`, each ≈ because
clusters shift under every fix; core total has since fallen 25421 →
**16701** @ `38eb6513`, so read these as proportions, not counts —
re-derive with `cluster_incompat.py` before acting on any of them):

| group | what | ≈ core cases |
|---|---|---|
| S1 | `new` on a function + constructor `this` | 1900 + cascade |
| S2 | generator / class-member syntax | 6700 |
| S3 | name resolution at ssa-lower | 830 |
| S4 | eval / arguments / with | 1400 |
| S5 | type-system boundary decisions | 2100 |
| S6 | harness ports (runner-side) | 1200 |
| S7 | the tail, as one item | the rest (~12300) |
| S8 | bug bucket — accepted but wrong | 695 (separate bucket) |

S7 holding the largest share is not a contradiction of "enumerable": it
is clusters #21–#482, each individually attributable, just not worth
naming in a roadmap until the groups above them fall.

#### S1 — `new` on a function, and the `this` that comes with it

**The single largest structural gap found.** A three-line ES5
constructor trips three of the biggest clusters at once (verified
2026-07-26 @ `9215301c`):

```ts
function Con(x: number) { this.x = x; }   // unknown identifier `__this`
const c = new Con(1);                     // unknown identifier `__new_Con`
console.log(c.x);                         // unknown identifier `c`  (cascade)
```

- [x] **S1.1** `new F()` where `F` is a function declaration, not a
      `class` — shipped `48f805d5`. The desugar emitted a call to a
      synthetic `__new_<Name>` factory that only classes ever got. The
      core `__new_*` cluster was **1138 cases across 42 directories**.
      The factory is now minted under `__fnctor_` — deliberately not
      `__new_`, which is load-bearing for classes well beyond the
      factory (`class_globals` rebuilds the whole class list by
      stripping it off FnDecl names), so borrowing it announced a class
      that did not exist
- [x] **S1.2** `this` inside a function — shipped `6972d7bd`, and it
      turned out to be S1.1's *prerequisite* rather than its follow-up:
      a function mentioning `this` was rejected **at declaration**,
      whether or not anything ever constructed it. The `__this` cluster
      was **783 cases across 55 directories**, the widest spread in the
      census. `desugar_classes` rewrites every `this` to the name
      `__this` but only class members get a parameter binding it, so
      the fix gives plain functions the same hidden receiver parameter.
      The test is exact — a body with `__this` free — so class members
      are excluded by construction rather than by prefix matching
- [x] **S1.5** A constructor returning an object keeps it (spec
      §10.2.2 step 8) — shipped `e023bc6b`. Not in the original list:
      S1.1's factory returned the receiver unconditionally, which made
      `return {y: 2}` a *silent* wrong answer, so it was closed in the
      same rotation ahead of the louder S1.4. `typeof null` being
      "object" is the trap; the check is emitted only for bodies that
      can produce a value, so ordinary constructors pay nothing
- [ ] **S1.3** `F.prototype.m = function () {…}` — the method half of the
      ES5 object model. **Not yet measured**; now measurable since S1.1
      landed, so measure before designing
- [ ] **S1.4** Indirect calls. A function whose signature S1.2 changed,
      passed as a value (`const g = F; g(1)`), cannot have its call site
      rewritten — desugar never sees it. It fails loudly rather than
      silently, which was the design intent.
      **Correction (measured 2026-07-27):** the previous entry here said
      it reports `expected 2 argument(s), got 1`, i.e. that the
      synthesized receiver leaked into a user-facing diagnostic. It does
      not. The actual message is
      `not yet supported: ssa-lower: box_to_any element type FnSig(SigId(95))
      not supported` — taking a *function value* at all is what is
      missing, which is a different and larger scope than a wording fix.
      Whatever produces the arity wording, it is not this shape.
      Not a regression: this code did not compile at all before S1.2
- [ ] **S1.6** ~~Re-measure the cascade~~ — **measured, and the theory
      was wrong.** The RFC predicted that clusters keyed on ordinary
      user names were downstream of a failed `new` and would evaporate
      once S1.1/S1.2 landed. Sweep `e023bc6b` says otherwise: **`f`
      315 → 315, `x` 171 → 171, `c` 1 → 1 — not one case moved**, while
      the actual S1 targets did (`__new_*` 1138 → 926, `__this` 783 →
      636). So these clusters have a separate root cause and are not
      cascade at all. This is exactly why the acceptance was written as
      "run the sweep and see", not "write a fixture": a theory disproved
      is worth more than a fixture passed. **Next step is to find what
      they actually are — starting from no assumption that `new` is
      involved.** Neither S1 cluster reached zero either, so some other
      shape (`F.prototype.m`, indirect calls, `this` inside a nested
      function) still feeds them; measure that too
- [x] **S1.7** What the `f`/`x` clusters actually are — **measured
      2026-07-27**, full workings in
      `.claude/tasks/2026-07-27/measure-this-and-cascade-roots.md`.
      They are not a binding gap at all. Most are not even bare
      `unknown identifier`: they are
      `closure __closure_N references unknown identifier`, and **211 of
      `f`'s 306 live under `test/annexB`** — Annex B.3.3 block-level
      function hoisting. Those cases *deliberately* reference a binding
      that must not exist and assert it throws:
      `assert.throws(ReferenceError, function() { f; })`. tr's checker
      treats an unresolved free identifier as a **compile-time hard
      error**; the spec requires it to become a **runtime
      ReferenceError**. 44 more cases in the family are literally named
      `unresolvableReference`. So this is a semantic-classification
      decision about which unresolved names may be refused statically —
      **filed as S5.x, not an S1 follow-up** — and the `new` theory is
      independently disproved a second time
- [ ] **S1.8** The other two `this` shapes. S1.2 walks **top-level
      `Stmt::FnDecl` only** (`ast/this_param.rs:36`), which is one of
      three shapes; the `__this` residual is **644 cases across 175
      directories**, and it is not one problem:
      **(a) 186 cases — `this` inside a function *expression* or a
      nested function.** `var f = function () { return this; }` lifts to
      `__closure_N` and `__this` becomes a free variable the FnDecl walk
      never sees. Sample: `built-ins/Array/from/iter-map-fn-this-arg.js`,
      which is exactly `var mapFn = function () { thisVals.push(this); }`.
      **(b) 458 cases — top-level `this` in a *value* position**
      (`var o = this`, `verifyProperty(this, 'Array', {…})`). This half
      is blocked on S5.x below, not independently fixable

#### S2 — Generator and class-member syntax

Single syntactic points, large mass, narrow directory spread — the
cheapest ratio on the board. Mostly parser/lexer work; none of it
touches runtime hot paths.

- [x] **S2.30** objlit computed accessor `{ get [expr]() {} }` /
      `set` — shipped `307367b5` (rotation 237; the 552-case
      `got LBracket` cluster's true shape, NOT class members as
      first guessed). Rides the computed-key sentinel + the accessor
      define kernel (`DefineKey::Expr`); en route fixed a live
      silent-wrong: `accessor_param_kind` read sig param 1 assuming
      env-first but fn_sigs is the user face — every typed-param
      setter behind the define kernel received the NaN-box verbatim.
      (Commit messages label these knives S2.27-2.30; those numbers
      were already taken here — canonical ids are S2.30-33.)
- [x] **S2.31** `yield* [array-literal]` — shipped `aa77a133`
      (rotation 237; 340 of the 525-case yield* cluster). Indexed
      while+yield expansion (array default iterator IS index order,
      §23.1.5.1). General GetIterator delegation stays loud: the
      generator state machine has no ForOf arm and the runtime
      Array cell carries no `Symbol.iterator` property (L3b).
- [x] **S2.32** bare class fields `name;` / `#p;` / `static s;` —
      shipped `3fcd8201` (rotation 237; the 527-case `got
      Some(Semi)` cluster). An `any` slot with a synthesized
      `undefined` init.
- [x] **S2.33** user `return v;` in generator bodies completes with
      `{ value: v, done: true }` (§27.5.1.2) — shipped `591c5fd5`
      (rotation 237; first wall of the 457-case ClassRef cluster,
      sync + async). Top-level returns via a new GenSm arm; nested
      returns via a Stmt-tree walker at the four inline-emit points.
- [x] **S2.34** class-method VALUE-use route — shipped `722871cf`
      (rotation 238; second wall of the ClassRef cluster, 675
      Gen-ClassRef signatures). Four stacked gaps, one commit:
      cm_demote admits Call-shape receivers (`ref(42).next()` /
      `new C().m(…).next()` stopped dying at the speculative
      `__cm___Gen_*__next` rewrite); a layout-miss member read whose
      name is a class method (own/inherited/private-mangled/gen-
      hoisted) answers the method value via the any-member lane;
      struct_method gains the getter-as-callee arm (dynobj/arr had
      it since chunk 523); invoke_with_this routes reified
      class-method faces through the receiver-in-env adapter ABI.
      Residual: a bare `ref(42)` call with a non-instance receiver
      keeps the this-undefined TypeError (mono bodies read the class
      layout off `this`).
- [x] **S2.35** untypeable call-result toplevel lets promote to Any
      globals — shipped `b3c7a1c0` + fix `c3a4ad05` (rotation 238).
      Census of the 12287 unknown-identifier cases: 1335
      declared-but-unregistered (asyncIter 300 / values 227 / iter
      175 on top). Shape-typed calls (simple ret ann / `Symbol()`)
      keep their exact slot; `__new_*` factory calls keep nominal
      identity (the fix — promoting them walked the destr-param
      fixture into the S2.36 hole). The method-objlit half stays on
      L3b (dynobj-lane method this-home doesn't round-trip yet);
      asyncIter's consumer form needs for-await-over-any (L3b).
- [x] **S2.36** boxed-adapter struct-param coercion kernel — shipped
      `c56c47ca` (rotation 238). An inline objlit argument to an
      any-dispatched typed body arrived as a dynobj box while the
      body read its struct layout: garbage fields, silent exit 0
      (the worst silent-wrong shape — pre-existing, exposed by
      S2.35's promotion). `__torajs_anyv_arg_to_struct` materializes
      the struct repr per layout field at the adapter boundary;
      adapter synthesis moved after the stamp-pool build so Obj
      params resolve their class_tag.

- [x] **S2.37** static private class members — shipped `e6893e38` +
      `77fb4cff` (rotation 239, cluster #1: 1482 cases / 4 dirs).
      The two parser rejects drop away: a pre-mangled
      `__priv_<C>__<n>` flows through the `__sf_/__sm_` static
      machinery with zero new lanes. `this` in a static
      method/accessor body mints `Ident(<ClassName>)` at parse time
      (ES §15.7.14; the S2.1 `in_gen_class_method` precedent), so
      `this.#x` / `this.m` resolve via the existing static-member
      rewrite. `get #p()` / `set #p(v)` accepted as accessor names.
      Two bycatches: `x.#p` OUTSIDE any class body had rotted into a
      silent `undefined` — now the §13.1 early SyntaxError; and the
      dominant t262 consumption shape (`static get m() { return
      this.#m }`, a None-ann `__sm_*_get`) needed the L3b #8 return
      axis widened from explicit `:any` to missing annotations (a
      bare fn-ident return under an inferred-Any slot always died
      loud at box_to_any).

- [x] **S2.38 / S2.39** receiver-free method faces run bare calls +
      the boxed adapter substitutes literal defaults — shipped
      `790a21c1` + `7b193f4a` (rotation 239). Detached
      `C.prototype.m` values (`var ref = C.prototype.method;
      ref(42, undefined, 1)`, the dflt-params / forbidden-ext t262
      shape) stop dying on the S2.34 this-undefined TypeError when
      the compiler proves the body never observes its receiver AND
      the adapter argument surface is lossless (undefaulted params
      Any; Number/Bool literal defaults baked into the adapter via
      the `__torajs_anyv_or_default` kernel — which also fixed a
      PRE-EXISTING silent wrong: `c.method(42, undefined)` through
      the any lane answered 42 instead of applying the default, on
      every dispatch path through the adapter). Expression defaults
      and async/gen wrappers (they feed `__this` into the state
      machine) conservatively keep the loud TypeError.

- [x] **S2.40–S2.43 + three lane fixes** — rotation 240, seven
      knives, +362 passTotal. The through-line: THREE separate call
      lanes were handing arguments verbatim past the rotation-221
      `arg_conv` contract, each surfacing as a different symptom.
      - [x] **accessor-setter direct call** `d27cb5a5` — the P8.2
            F2-fix knew only I64→F64; an untyped `set p(v)` (Any
            param) received raw i64 bits and the bare-field store
            held a garbage NaN-box (p25g crash / the 7 exit-139
            compound-assign private-accessor rows).
      - [x] **sibling-class static dispatch** `e361d31a` — with two
            `function*` decls `next` becomes a sibling-owned name
            and `g.next(42)` routed through the one lane with zero
            coercion: generator RESUMPTION VALUES read back as
            `[unknown-any-tag]` on every multi-generator program.
      - [x] **S2.40** `ClassElement : ;` `7b56a07f` — a bare
            semicolon is an empty class element (ES §15.7); the t262
            elements/ suites end every class body with one (the
            474-case "got Semi" cluster #4, 618 rows across the two
            class dirs).
      - [x] **S2.41** bare `yield` `c1d8765d` — YieldExpression's
            operand is optional (§15.5.5); statement + `let v =
            yield;` lanes mint undefined. The for-of / for-await-of
            dstr suites drive every fixture generator with `yield;`.
      - [x] **S2.42/S2.43** `**` Any operands `451b2bd9` + BigInt
            guard `b9a8019b` — the anyv arith kernel grows op 4
            (Pow); `**=` on accessors rides the same lane. A BigInt
            beside an Any stays a loud reject (no BigInt lane in the
            kernel — the ToNumeric dispatch is a registered face).
      - [x] **pow ES lattice** `cc65a6c9` — `(-1) ** Infinity`
            answered 1: the self-ported pow had pinned the C99 cell
            where ES differs (abs(base)==1, ±∞ exponent → NaN), and
            its unit test never caught it because a host-linked test
            binary resolves `no_mangle pow` to libSystem. Impl split
            to `pow_es` so tests exercise the real code; `x ** 0.5`
            now rides correctly-rounded sqrt.

- [x] **S2.1** `*f() {}` generator methods — one grammar point, three
      positions, **3085 cases**, all three shipped in rotation 226. It
      was listed as parser-only; that held for one position and **not**
      for the other two:
      - [x] **object literal (253)** — shipped `797d4517`. Genuinely
            parser-only. The generator substrate was already whole, so
            the shorthand mints a synthetic top-level
            `function* __obj_gen_method_N` — `Stmt::FnDecl` already had
            an `is_generator` field — and hands the property an
            `Expr::Ident` naming it, which is exactly what ES §13.2.5
            says the sugar means. Mirrors the async shorthand next door
      - [x] **class member (2139) + class member-name path (693)** —
            shipped `7ef9b170`. **Not parser-only**, and three facts
            constrained it into exactly one shape:
            (1) `desugar_generators` runs *before* `desugar_classes`
            (`cmd_build.rs:143` vs `:156`), so at generator time a class
            is still a `ClassDecl` and `this` is still `Expr::This`;
            (2) the generator desugar turns a `function*` into a
            `__Gen_<name>` **class** whose fields hold the generator's
            params, which is how state survives across `next()` calls —
            verified: an object param does reach the body intact;
            (3) it has **no env channel** at all — `hoist_gen_fn_exprs`
            documents that prep rewrites params and lifted lets to
            `this.<name>` and everything else must resolve as a
            module-level global, on pain of a loud panic.
            So the receiver has to arrive **as a parameter** to be
            reachable from the state machine, and the `this` the user
            wrote must stop being `Expr::This` *before*
            `desugar_generators` sees it — otherwise it collides with
            the `this` that prep introduces, which points at the
            `__Gen_*` instance rather than the class instance.
            Shape that follows, and what shipped: each class generator
            method is hoisted to a top-level
            `function* __cm_gen_<C>__<m>(recv, …)` with the body's
            `this` minted as that parameter *while parsing*, and the
            class keeps an ordinary forwarder
            `g(a) { return __cm_gen_C__g(this, a); }` — so vtable
            construction, `method_owners`, visibility and dispatch are
            untouched.
            **The receiver parameter is deliberately not named
            `__this`.** That was tried first and fails loudly with
            `redeclaration of __this in current scope`: `__this` is the
            first-parameter name every `__cm_*` method carries, so
            turning it into a `__Gen_*` field collides with the `__this`
            that `__Gen_*`'s own `next()` receives. Same shape as the
            `__new_` collision in S1.1 — borrowing a load-bearing name
            inherits everything it means. `static` also had to be taught
            that `*` may follow it
      - **What the sweep says, measured @ `7ef9b170`.** All three parse
            walls went to **zero** — `expected class member name, got
            Star` 693 → 0, `expected \`(\` … after \`static\`` 669 → 0,
            `expected field name in object literal, got Star` 253 → 5.
            But **`incompatible` fell by only 11**, because most of those
            1610 cases walked straight into the next wall, one layer in:
            - `*#priv() {}` — **496 cases**. The generator member-name
              parse accepts `Ident` and reserved words, not
              `PrivateIdent`. This is S2.1 × S2.2 and belongs to S2.2
            - generator methods with **destructured parameters**
              (`*g({a, b}) {}`) — **~750 cases** reporting
              `no member .__param_destr_N on ClassRef("__Gen_…")` or
              `__param_destr_N requires a type annotation`. The
              destructuring helper param has no annotation, and the
              generator desugar turns params into `__Gen_*` fields, which
              needs one. New work item, filed as **S2.8**
            13 cases went `parse error → pass` outright, 7 to the bug
            bucket, 8 to `not yet supported`.
            **And 9 cases regressed, honestly: `pass-negative →
            negative-unsupported`.** They were passing *for the wrong
            reason* — a negative test expecting a syntax error was
            satisfied by tr's "I don't know `*`" parse error, standing in
            for the early error actually under test (`super` inside a
            generator method: 5; generator param redeclared by a body
            `let`/`const`: 2; duplicate default-param names: 2). Now that
            `*` parses, the refusal has to come from the real rule, and
            tr does not implement it yet. Per the no-metric-inflation
            rule this is water draining out, not a defect introduced —
            but it is a regression and is counted as one. Filed as
            **S2.9**
- [x] **S2.8** Destructured parameters on a generator method —
      **~750 cases**, exposed by S2.1 and closed in the same rotation.
      The diagnosis in the line above was wrong, which the minimal repro
      caught before any code changed: `function* g({a, b})` **already
      worked**, so nothing was missing from the desugar. What both new
      paths omitted was registering `ast.gen_param_destr_prefix` —
      the `name → count` table telling `desugar_generators` how many
      leading body statements are parameter-unpacking `let`s to peel into
      the `__Gen_*` constructor (ES §9.2 binds parameters eagerly, so a
      throwing destructure must fire at the call, not at the first
      `next()`). All three pre-existing generator forms register it; the
      two new ones did not, so the lets stayed in the body and
      `__param_destr_N` resolved against a field nobody created. The
      count is of body statements, so the prepended receiver does not
      enter into it
- [x] **S2.11** `super.m()` inside a class generator method — **done
      2026-07-27**. The parser half landed in rotation 226 (rewrite
      `__supercall__<m>` to `__cm_<Parent>__<m>(recv, …)` while the
      parent is still in hand, since by the time `desugar_classes`
      would do it `desugar_generators` has moved the body into the
      `__Gen_*` state machine). What remained was the receiver's type:
      the hoisted generator took it as `any`, and `any` is not admitted
      into a heap-typed parameter slot, so the call failed with
      `argument 0: expected ClassRef("A"), got Any`.
      **The note this entry used to carry was wrong** — it said typing
      the receiver nominally breaks the inherited direction. It does
      not, provided the annotation names the **declaring** class rather
      than the parent: the forwarder that passes `this` lives on the
      declaring class, and the receiver slot already admits a subclass
      by prefix layout, so a grandchild instance reaching a generator
      declared two levels up type-checks too (measured — the earlier
      "measured, both directions" claim did not distinguish the two
      annotations). `static *g()` keeps `any`, its receiver being the
      class object. One `type_ann`; fixture `gen-class-super-001`
      (super with arguments, twice in one expression, interleaved with
      `this`, inherited and grandchild receivers, static generator,
      per-instance state) is bun byte-equal JIT and AOT
- [~] **S2.12** `super.m()` resolved against the **direct** parent only,
      so `class C extends B extends A` reaching A's `m` emitted
      `unknown identifier __cm_B__m`. Found while probing S2.11, and
      **not a generator defect**: `desugar_classes_super` resolved
      ordinary methods the same way and the plain-method spelling failed
      identically. **The ordinary-method half is fixed 2026-07-27** —
      the rewrite walks up from the parent for the nearest class that
      declares the name, so an override partway up the chain still wins
      over the grandparent's version, and a name nothing declares keeps
      the existing diagnostic rather than resolving to silence. Fixture
      `super-method-chain-001`. **Still open:** the generator-method
      spelling, whose rewrite happens in the parser (it has to, since by
      desugar time the body has moved into the `__Gen_*` state machine)
      and so has only the direct parent in hand. Unifying the two —
      have the parser record `(ExprId, class, method)` and let the
      desugar do the single rewrite once the class index exists — is
      the right shape; arena ExprIds are stable, so the desugar can
      reach a body it can no longer find by walking
- [ ] **S2.21** **A token forgets it was spelled with an escape.**
      Rotation 228's S2.19 knife 2 decodes `from` to the name
      `from` and then throws away the fact that an escape produced it —
      but §12.7.2 makes that distinction load-bearing beyond the
      ReservedWord refusal the knife does implement. Nine negative
      tests moved `pass-negative → negative-unsupported` because of it,
      and they had been passing only because the `\` was a lex error
      before the knife: `import {} from "…"` and
      `as`-spelled specifiers (3 cases — tr matches `from` / `as`
      by string on a plain Ident, so it cannot tell), plus
      `void yield` / `await` used as an IdentifierReference
      inside a generator or async body (6 cases — those need S2.17's
      context tracking on top). The fix for the first three is a bit on
      the Ident token; the other six wait on S2.17
- [ ] **S2.17** **tr has no strict-mode tracking**, and S2.9 is the
      first place that cost something. `class` bodies are recognised
      (via `current_class`), but there is no notion of a `"use strict"`
      directive, of module code being strict, or of a `.ts` file being
      strict by construction. So the duplicate-parameter check has to
      choose: refuse `function f(a, a)` everywhere and lose the three
      sloppy test262 cases that assert it runs, or scope it to
      non-simple lists and accept it inside a `.ts` file where
      TypeScript (and bun) refuse. It currently does the latter —
      spec-faithful for test262, laxer than TS for tr's own surface.
      The real answer is a strict flag threaded through the parser,
      which several other early errors will want as well. Worth doing
      before the next early-error item rather than after
- [ ] **S2.16** `super.m()` from a **static** method emits
      `unknown identifier __cm_A__make`. Fails against the direct parent
      too, so it is not S2.12's chain walk — the static half simply has
      no rewrite. Found while writing the S2.12 fixture. Not yet
      censused
- [ ] **S2.13** A `class` **declaration** inside a function body reaches
      the checker unlowered: `function f() { class Inner {} … }` answers
      `internal: ClassDecl \`Inner\` reached check.rs (desugar didn't
      run?)`. The class-desugar walks top-level statements and does not
      descend into function bodies, so the declaration spelling is
      simply missed — the *expression* spelling
      (`const C = class {}`) works, being routed through
      `synth_classes` at parse time. Found while writing the S2.9
      fixture and confirmed pre-existing against the prior binary. Same
      family as the `super()` diagnostic S2.9 replaced: an internal note
      that blames a pass which did run. Not yet censused
- [ ] **S2.14** A `catch` binding that shadows a parameter erases it
      for the rest of the function: `function f(a) { try { … } catch
      (a) { … } return a }` answers `ssa-lower: unknown ident \`a\``.
      The catch parameter's scope is not popped, so the outer name
      never comes back. Found while writing the S2.9 fixture. Not yet
      censused
- [ ] **S2.15** `var` may repeat a parameter name — it *is* the same
      variable (ES §14.3.2.1) — but tr answers `redeclaration of \`a\`
      in current scope`. Note the asymmetry this leaves: as of S2.9 the
      `let` spelling is refused for the right reason while the `var`
      spelling is refused for the wrong one. Found while writing the
      S2.9 fixture. Not yet censused
- [ ] **S2.10** The receiver parameter is visible in argument-position
      diagnostics. Passing a bad argument to a class generator method
      reports `argument 1` for what the user wrote as the first argument,
      because the hoisted receiver occupies slot 0. Same defect S1.4
      records for `bind_this_param`'s hidden parameter — one fix should
      cover both, since both are "a synthesized leading parameter leaked
      into a user-facing message". Cosmetic, but it is the kind of
      cosmetic that sends a reader looking for a bug in the wrong place
- [x] **S2.9** Early errors that were being faked by a parse failure —
      **9 cases**, exposed by S2.1. **Closed 2026-07-27** except the
      two `grammar-static-gen-meth-super` cases, which are blocked on
      `class C extends Function` and were never S2.9's to begin with.
      Measured before starting, and the measurement changed the plan
      twice — read the sub-items rather than the sentence above:
      - `*method(x = 0, x)` and `*foo(a) { let a = 3 }` (4 cases).
        **"Fix the kind, not the detection" — written here after
        rotation 226 — was wrong, and rotation 227 measured why.** The
        duplicate name did become two same-named fields of the `__Gen_*`
        class and did trip `desugar_classes_fields.rs`, but that is a
        refusal by accident: the message named a synthesized class the
        user never wrote, and the plain spelling
        `function f(x = 0, x) {}` was **not refused at all**. Dressing a
        `panic!` up as a different verdict kind would have kept both.
        - **duplicate parameter names: done 2026-07-27.** ES §15.1.1
          makes it an early SyntaxError, and *where* decides whether
          unconditionally. A **method definition** (§15.4) and an
          **arrow** (§15.3.1) take `UniqueFormalParameters` — always
          refused. A **function declaration or expression** takes plain
          `FormalParameters`, whose early error applies only in strict
          code or when the list is not simple. **The first version
          refused everywhere and the sweep caught it**: three cases
          (`param-duplicated-non-strict.js` ×2, `S10.2.1_A2.js`) run
          `function f(a, a)` in sloppy code and expect it to work.
          Scoped to what the spec says, they came back. The arrow check
          sits **after** the `=>` rather than after the `)`, because
          until that token is seen the same text may still be a
          parenthesized sequence expression, and `(w, w)` is legal as
          one. Fixture `param-names-001` pins the legal side — same
          name in sibling functions, shadowing, nesting, rest, several
          destructuring holders side by side, TS parameter properties,
          accessors, arrows, and that sequence expression
        - **parameter vs body `let`/`const`: done 2026-07-27.** Same
          early error (ES §14.2.1), checked at the same six sites once
          the body is parsed and before the destructuring prelude is
          prepended — those synthesized `let`s bind the parameter's own
          leaves and are parameter names for this purpose, while the
          unspellable `__param_destr_N` holders are not. `var` is
          exempt (§14.3.2.1 makes it the same variable) and so is a
          nested function declaration, being var-scoped. Fixture
          `param-body-shadow-001` pins the legal side: inner blocks,
          nested functions and arrows, loop and catch bindings, and a
          body name matching a *different* function's parameter
      - `grammar-static-gen-meth-super` (2 cases) is blocked by
        `class C extends Function` being unsupported — nothing to do
        with generators
      - `super()` inside a generator method (3 cases) needed a
        genuinely new check — **done 2026-07-27**. It is an early
        SyntaxError anywhere but a derived constructor's body
        (ES §15.7.1), and the position turned out not to be a generator
        matter at all: an ordinary method, an object-literal method, a
        static block, a plain `function` nested in a derived
        constructor, and a base class's own constructor all reached the
        checker as `internal: super(...) reached check.rs (desugar
        didn't run?)` — an internal note blaming a pass that had in
        fact run, since the desugar only ever rewrote `super()` in a
        ctor body and silently left every other one standing. A parser
        flag, set in that one branch and cleared on entry to each
        function-like body, refuses all six positions as bun does. An
        arrow deliberately inherits the position rather than clearing
        it (`constructor() { (() => super())() }` is legal), and so
        does the sub-parser for template interpolation. Fixture
        `super-call-position-001` pins the legal side — the way to get
        a cleared-flag check wrong is to clear it where it should carry
- [ ] **S2.2** Private names `#x` — **1291 cases**. **The description
      this entry carried was wrong and the correction changes what to
      do first.** It said the lexer rejects byte `0x23` outright before
      the parser ever sees a private name. It does not:
      `lexer.rs:158` matches `b'#'` followed by an ident start and
      scans a `PrivateIdent`; only a *bare* `#` falls through to the
      unexpected-byte arm. Measured 2026-07-27 — `#x` fields, `#m()`
      methods, and reading `this.#x` from inside a generator method all
      already work. What is actually missing, in the shapes probed:
      - `static #s` — refused deliberately by the parser with its own
        "defer P8.x" message (the 350 half, and the only part of the
        original description that held)
      - `#x in obj` — the ergonomic brand check; `expected expression,
        got PrivateIdent`
      - **and the 941 turn out not to be private names at all.** The
        `lex error: unexpected byte 0x23` cluster is still 941 after
        this rotation, and its examples are
        `test/language/comments/hashbang/*`: the **hashbang comment**
        `#!/usr/bin/env node` (ES2023 §12.5, a `#!` permitted only at
        the very start of a source text). That is a lexer feature of
        its own, unrelated to `#x`, and it is the cheapest large
        cluster on the board. Split it out rather than carrying it
        under S2.2
      - ~~`*#g() {}` — a generator method with a private name~~ **done
        2026-07-27**: S2.1 taught the member position to accept `*` but
        its name only took `Ident` and the reserved words. It now takes
        the same mangle-and-force-Private route the ordinary member
        path does. Fixture `gen-class-private-001`
      The 941 figure therefore does not mean what it said. **Re-derive
      the signature from the next sweep before starting** rather than
      trusting the number here
- [x] **S2.18** **Async generator methods** — `async *g() {}`
      (rotation 228, `21a343ca`). It did read as parser-shaped and this
      time that held: a top-level `async function*` already ran, S2.1
      had taught both member positions the plain `*g() {}` shape, and
      what neither allowed was the modifier and the star standing next
      to each other. The class modifier prefix now steps over an
      intervening `*` without consuming it, and both generator paths
      register their hoisted `function*` in `async_generator_fns` —
      not `async_fns`, since §27.6 hands the generator object back
      directly and it must not be Promise-wrapped. Teaching the
      lookahead about `*` meant teaching it about private names, which
      incidentally admitted the `async #m() {}` it had also refused.
      Three limits stand behind it, none specific to member position
      and all recorded in plan-state L3b: `for await` routes to the
      async protocol only for a direct call to a named factory; the
      generator object does not survive a boundary typed as anything
      but its concrete class; and draining one through a typed return
      yields a NaN-box payload read as an integer — a silent wrong,
      reproduced on a plain top-level `async function*`
- [x] **S2.19** ~~Hashbang comments~~ → **Unicode identifiers**
      (rotation 228, `dc005d81` + `ca6b991a` + `e2527ebd`). **The
      attribution was wrong, and by two orders of magnitude.** Splitting
      the 941-case `unexpected byte 0x23` cluster by directory puts
      **6** in `language/comments/hashbang` and **884** in
      `class/elements` — the error names the byte the scan stopped on,
      not what stopped it. What stopped it was the character *after*
      the `#`: `is_ident_start` was `is_ascii_alphabetic() || '_' ||
      '$'`, so `#\u{6F}` / `#℘` / `#ZW_<ZWNJ>_NJ` — how test262 spells
      its private names — never got past lexing. Read together with its
      sibling clusters (`0x5c` 485 = S2.6, ~75 raw non-ASCII, 25 VT/FF)
      the family is **~1460 cases, the same order as S2.18**, not the
      cheap one-liner this entry claimed. Shipped as three: ID_START /
      ID_CONTINUE generated from `DerivedCoreProperties.txt` into
      torajs-ucd with codepoint-based scanning; `\u` escapes in
      identifiers, with an escaped ReservedWord refused per §12.7.2
      (`yield` / `await` held out, being only conditionally reserved
      and tr having no strict tracking — S2.17); and the hashbang rule
      itself plus the rest of §12.2 WhiteSpace
- [x] **S2.6** Unicode escapes in identifiers (lexer byte `0x5c`) —
      **485**, closed by S2.19's second knife (`ca6b991a`): same root
      cause, different byte
- [ ] **S2.3** Computed field names `[k] = v` in class bodies — **548**.
      Rotation 229 measured the shapes; it is not one failure mode but
      three, and one of them is silent:
      `parse_class_decl_member.rs:64` accepts only `[<Ident>(.<Ident>)*]`
      (minted as the P5.2 `__sym_A_B__` name) and `[<StringLiteral>]`.
      So `["lit"] = 2` is **already correct**; `[k + "2"]` is a **parse
      error**; and `[k] = 1` with `k` a variable **silently installs a
      field literally named `__sym_k__`**, so `a[k]` reads `undefined`
      with no diagnostic — metric water by the iron rule, and the first
      thing to fix. Doing it properly needs the §15.7.10 semantics (the
      key expression is evaluated ONCE at class-definition time, in
      source order) plus a home for a runtime-keyed property on an
      instance whose layout is a static struct — i.e. the dynobj
      degrade path. Deserves its own RFC; not startable as a side blade
- [ ] **S2.4** `yield*` against a non-call expression — the parser
      currently demands a direct call to a `function*` — **434**
- [ ] **S2.5** `for await (… of …)` — **654**, all in one directory.
      **Re-characterised rotation 229 by the post-fix sweep, and the
      name was wrong.** The entry read "iterable form" because that is
      what the refusal SAYS (`parser/loops.rs:226`); measuring the 654
      after shipping the iterable-form fix showed **all 654 still
      refused, and every one of them is a `dstr-assignment-for-await`
      generated case**: `for await ([v = 10, w] of [[2]])`. The head is
      a LeftHandSideExpression assigning to already-declared outer
      bindings, so `try_parse_for_of` — which wants
      `(let|const) IDENT … of` — never recognises it and the `for
      await` arm reports the only thing it can. A bare identifier
      target (`for (x of …)`) already works; the gap is the PATTERN.
      **Rotation 230 update — S2.24 shipped and three of this item's
      walls peeled in one rotation, still 0 of the 654 pass**: the
      parse wall fell (刀 2, pattern head accepted), then
      `assignment to undeclared v2` (the preamble is a
      multi-declarator, 刀 6), and the sweep-verified current wall is
      **`unknown identifier assert`** — the harness `assert` binding
      is not visible from inside the async fn bodies these generated
      cases use (their non-dstr siblings pass, so it is something
      this shape combination triggers; next rotation's entry point is
      `async-func-decl-dstr-array-elem-init-assignment.js` verbatim).
      The dir-level movement is real: 185 of its cases advanced
      parse error → type error
      **Rotation 231 update — the recorded wall was NOT substrate**:
      running the flagship case verbatim showed `unknown identifier
      $DONE`, and the root cause was the RUNNER — it never parsed
      `flags:`, so the async completion protocol (doneprintHandle.js
      `$DONE`) did not exist for any of the corpus's 5,581
      `flags: [async]` cases. Runner blade `7b160462` ports `$DONE`
      into the typed harness and judges async cases on the printed
      `Test262:AsyncTestComplete` marker (exit 0 alone is NOT a pass
      — no-metric-inflation). The flagship case now PASSES bun-oracle
      matched (tr's for-await + promise chain + `$DONE`
      function-value callback works end to end); family probe: 123
      pass / 13 honest bugs / walls now attributable parse kinds.
      Remaining family walls: decl-head patterns (RFC
      20260727-dstr-decl-shape blades A/B, this rotation) and the
      for-await element unwrap's missing non-Promise-Struct `.value`
      arm (hole Z, L3b) gating every object-pattern shape.
      **Rotation 231 close**: blades A/B/C shipped (recursive
      PatShape machine for declaration-position patterns — statement
      + for-of/for-await heads; heterogeneous heap-element literal
      repr unification). Family state @ d2b7e393: passTotal 166
      (was 0 at rotation 230 close), `requires the iterable form`
      447 → 75 (obj assignment heads / CoverInitializedName + odd
      shapes remain), and the biggest family wall is now the
      Promise(Struct) member cluster (1,001 cases corpus-wide, #4
      overall — `.then`/`.value` arms missing on
      Promise(Struct([value, done])), exactly hole Z's family).
      3 honest new timeouts: for-of/dstr `*-ary-ptrn-rest-id-
      iter-val-err` (rest over a poisoned iterator, ran for the
      first time and hangs — L3b)
- [x] **S2.24** **Destructuring assignment to existing bindings** —
      shipped rotation 230 (RFC 20260727-dstr-assignment, six blades:
      implicit-any uninit let / statement-form expansion / for-of
      bare-pattern head + the S2.28 box half / S2.26 scalar-Str unbox /
      S2.27 undefined-branch ternary / multi-declarator K.3
      registration). Statement form and both for-of/for-in mirrors
      work with defaults, holes, rest, nesting and member targets;
      sweep moved **+207 passTotal** with 210 dstr-adjacent cases
      turning pass. Recorded boundaries (loud): EXPRESSION-position
      patterns (`result = [a, b] = vals` — the entire
      expressions/assignment/dstr directory is this chained shape,
      needs the value-of-pattern semantics via a Sequence tail);
      object rest; `{ x = D }` CoverInitializedName
- [x] **S2.5-a** `for await` over an iterator the loop does not get as
      a direct factory call — shipped rotation 229 (`19f7fdde`). Split
      out of S2.5 because it is a real defect that carried **none** of
      that item's census mass (recording it under S2.5 would be exactly
      the metric water the iron rule forbids). Rotation 228 localised
      it correctly: `parser/try_parse_for_of.rs:267` keys on
      `Expr::Call { callee: Expr::Ident(name) }` being in
      `async_generator_fns`, so a method call and a captured iterator
      fell to the sync protocol, which panicked on the `Promise` that
      `next()` returns. Fixed with an await flag on `Stmt::ForOf` —
      the fact had no proxy in that lane, since the array lane's marker
      is a `.value` wrap on `elem_expr` and the protocol lane ignores
      `elem_expr` — plus an awaiting arm that unwraps each step
- [ ] **S2.26** **Any→typed member assignment stores raw box bits** —
      SILENT WRONG, found rotation 230 while probing S2.24's nested
      patterns; the scalar / string directions are fixed
      (rotation 230 刀 4): `o.k = (v: any) 9` printed **NaN** and
      `o.k = src.k` a NaN-box bit pattern, because the struct field
      store's width-align match had the box arm for `(Any ← scalar)`
      but nothing for the reverse — an Any value into a declared
      Number / Str field now unboxes through `coerce_any_to_number` /
      `coerce_to_str`, the kernels every other any→typed sink uses.
      Same family as RFC 20260727-promise-typed-readback (settle /
      await lanes). REMAINING: Bool- and heap-typed fields
      (`o.flag = (any)` / `o.child = (any)`) still fall through raw —
      no established unbox kernel carries their type-guard story yet
- [ ] **S2.28** **Typed OOB element read crossing into `any`** —
      found rotation 230 while probing S2.24's for-await face. The
      SILENT half is fixed: `let t = [1]; let b: any = 0; b = t[1];`
      printed **NaN** because `coerce_for_local` used the eid-blind
      `box_to_any` (the global-assign and let-init lanes were already
      eid-aware), boxing the undefined-sentinel's raw bits as a
      Number — one-line route through `box_to_any_from_expr`, and the
      plain-for-of no-default past-end slot
      (`for ([w1 = 20, w2] of [[2]])`) came right with it. REMAINING:
      the `for await` variant of the same shape still THROWS a
      catchable "array index out of bounds" RangeError
      (`__torajs_arr_oob_throw`, the loud OOB exit for element lanes
      with no undefined representation — I64 / nested-heap); loud,
      not silent, but bun answers undefined. Carries the
      no-default-past-end slice of S2.5's 654 under `for await`
- [x] **S2.27** **Ternary branches of Undefined vs T reject instead
      of unifying** — shipped rotation 230.
      `let [u = 13] = [undefined]` (and the S2.24 assignment mirror)
      answered `ternary branches differ`, because a MONOMORPHIC
      `Array<Undefined>` source makes the destructuring default's
      guard ternary type its branches Undefined vs Number (the
      `Array<Null>` mono edge already unified through the Null →
      Nullable arm). Two halves: `unify_ternary` widens
      Undefined-vs-T to Any (the S129-1 mixed-Any shape), and
      `widen_branches` boxes BOTH sides expr-aware when exactly one
      branch types Undefined — the fall-through used to load the
      undefined branch's ConstPtrNull through the other branch's
      slot type (`cond ? undefined : 42` answered 0)
- [ ] **S2.29** **The 32 KiB frame cap now costs 16
      unicode-identifier passes** — regression recorded rotation 230,
      root-caused, accepted deliberately. The
      `language/identifiers/start-unicode-*.js` files declare ~6000
      bare `var ࠃ;` bindings; pre-刀0 those stayed Uninit (zero-size,
      accidentally under the cap), post-刀0 each is a real 8-byte any
      slot → frame 49 KiB > the scaled-imm12 cap → loud
      "not yet supported" (the cap's own recorded message: "scratch-reg
      path lands later"). The old passes were accidental — the
      bindings were not real; the fix is the cap's scratch-reg path,
      not a revert
- [ ] **S2.7** Untyped class field without a literal initializer — **245**
- [x] **S2.25** **A `.then` handler may declare no parameter** — shipped
      rotation 229 (`53466c5a`). `p.then(() => { … })`, the everyday
      side-effect shape, was a type error on every typed receiver. The
      reasoning for admitting it was already written down one arm over:
      the `Promise<Undefined>` receiver has taken a 0-arg handler since
      P10.2-A1.1, documenting that the kernels call through
      `int64_t (*)(int64_t)` either way. §27.2.5.4 hands the value to
      onFulfilled as one argument; how many the handler declares is its
      own business
- [x] **S2.22** **for-of inside an arrow function** — shipped rotation
      229 (`4196ae10`); never a census item, it surfaced while writing
      S2.5's fixture with the ordinary `(async () => {})()` driver.
      Only arrows compute free variables, and the for-of desugar's own
      loop counter was answering: `elem_expr` is `src[i]`, walked before
      `i` was bound. So NO for-of of ANY source shape could appear
      inside an arrow — wider than the S2.5 item that exposed it
- [x] **S2.23** **Return annotation on a class generator method** —
      shipped rotation 229 (`2a2a2615`); also not a census item.
      `*g(): Generator<number>` was a hard type error in all four member
      positions (instance / static / private / async) while the same
      method unannotated ran. The method parses into a hoisted
      `function*` plus a forwarder; the unwrapped yield type belongs to
      the first, and the forwarder was handed it too, so it declared a
      number return over a body that answers the generator object
- [ ] **S2.20** **Method signatures in inline object types** —
      `{ m(): number }` — shipped rotation 228 (`98efa861`); listed
      here because it was never a census item, it surfaced while
      probing S2.18. TS reads a MethodSignature as the property
      holding a function, so it produces the `__fn(P|..)->R` spelling
      `{ m: () => number }` already used; only the inline annotation
      refused it, the named-alias lane already took it

#### S3 — Name resolution at ssa-lower

Checker accepts these; the name is lost between check and lowering.
One cluster family, split by what the lost name is:

- [ ] **S3.1** `callbackfn` — **486 cases in a single directory**
      (`test/built-ins/Array`), plus `callbackfn1` 36 / `callbackfn2` 4.
      The name is test262 house style for a callback parameter, so this
      is one binding shape, not 526 problems
- [ ] **S3.2** Already-implemented globals unreachable from ssa-lower —
      `Math` 98, `JSON` 34, `WeakMap` 25, `WeakSet` 20, `WeakRef` 13
      ≈ **190**. These objects exist; something about the position they
      are referenced from loses them. ~~**Design note:** likely the same
      value-position-vs-call-position split~~ — **confirmed 2026-07-27**:
      it is exactly that split, and it is the same one behind S1.8(b),
      S5.5 and the `globalThis` cluster. Folded into **S5.6**; do not
      open this separately

#### S4 — Big missing features

- [ ] **S4.1** `eval` — **1009 cases**, 25 directories. For an AOT
      runtime this is a design question before it is an implementation
      one: decide the shape (compile-time evaluation? a documented
      refusal of indirect eval?) and **write the RFC first**. Whatever
      the decision, it lands in the subset-decision register (S7.2) —
      cases a refusal leaves incompatible must be attributed, not left
      as an anonymous cluster.
      **RFC drafted 2026-07-28** (`.claude/rfcs/20260728-eval-shape/`,
      rotation 241): a full per-case call-shape census @ `2879793f`
      puts **91.2 % of the cluster's eval arguments in compile-time
      literals** (direct-literal 840 / indirect-literal 142 / dynamic
      43 / value-ref 29 / aliased 21 / indirect-dynamic 2, of 1077).
      Proposed shape: staged static expansion (E1 global object +
      attributed refusal → E2 indirect-literal → E3 direct-literal,
      completion-value machinery as the shared prerequisite), dynamic
      eval deferred to a post-v1.0 runtime tier (E4, a phase not a
      refusal); the register entry covers only the 66 dynamic cases.
      **Sign-off now delegated** (S7.2 protocol, 2026-08-11): the
      literal-face knives shipped rotations 322–327 without needing a
      register entry; the dynamic-face entry lands once its residual
      cluster is re-measured against the shipped surface
- [ ] **S4.2** `arguments` object — **426** at two stages (checker
      `unknown identifier` 389, ssa-lower 37)
- [ ] **S4.3** `with` statement — 65 seen via the `__this` cluster;
      re-measure once S1.2 lands, since `with` cases currently fail on
      `__this` before reaching the `with` itself. A candidate for a
      registered refusal (TS itself rejects `with` in strict mode) —
      but that is a decision to record, not a default

#### S5 — Type-system boundaries

Cases where tr's checker refuses a program bun runs. Each needs a
decision — widen the checker, or record a subset-boundary refusal in
the S7.2 register with its rationale (per the test262 discipline in
`.claude/rules/torajs-design-principles.md`: neither bun failing nor
our own checker objecting is, by itself, a reason to skip).

- [ ] **S5.1** `not callable: type Any` — **539**, 21 directories
- [ ] **S5.2** `parameter requires a type annotation` — **380**
- [ ] **S5.3** `no member X on type Promise(Struct(…))` — **372**,
      concentrated on async-iterator result plumbing
- [ ] **S5.4** Parent class must be declared before the subclass, and
      must be a class rather than a type alias — **368**
- [ ] **S5.5** `RegExp` unresolved in value position — **394** (the
      RegExp substrate itself is P9-closed; this is reflective/value
      use of the constructor object, not a regex feature gap)
- [x] **S5.6** **There is no global object** — measured 2026-07-27,
      **and since resolved in its main body** (this entry's original
      table went stale; corrected 2026-08-11, rotation 367 recon). The
      global object IS minted: RFC `20260807-global-object` G1/G2/G2.5
      shipped a real dynobj singleton (relanded `503a8d1e` — 18 ctors,
      Math/JSON/Reflect, eval, §19.1.1-3 value props, self-reference,
      define-not-write attributes, identity probe, miss-loud channel),
      top-level `this` answers `"object"` since rotation 235
      (`__module_this`, a `{}` exports object per bun-as-module —
      deliberately NOT the same object as `globalThis`), and the
      rotation-367 this-rewrite knife binds a dynamic function's
      sloppy `this` to the singleton (§10.2.1.2), which made the
      fnGlobalObject harness portable. **Recorded residue, tracked in
      plan-state L3b**: G3 descriptor surface
      (`verifyProperty(globalThis, 'Array', …)` needs the gOPD global
      arm); the 19 MISSING_KNOWN names (console / parseInt family /
      NativeError ctors / queueMicrotask) each need an interned cell
      before joining the fill list; `new globalThis.Array()`
      (construct through a first-class ctor value) is a recorded
      follow-up on `builtin_ctor_cell`; the G2.5 mutation gate
      diverges between static spelling (loud) and aliased spelling
      (dynobj lane) under verifyProperty's write/delete probes; and
      six known-global name lists have no single source of truth
      (`is_known_builtin_global` / `MISSING_KNOWN` /
      `free_vars_globals` / `is_known_ns` / typeof static string /
      `array_is_array`) — a drift risk, collapse candidates
- [ ] **S5.7** Which unresolved names may be refused at compile time.
      The finding behind S1.7: tr treats every unresolved free
      identifier as a compile-time hard error, but the spec makes it a
      runtime `ReferenceError`, and **entire test262 families assert
      exactly that throw** (Annex B.3.3 hoisting, the 44
      `unresolvableReference` cases). A checker that never refuses an
      unknown name is not the answer either — that is TS's whole value.
      So this is a boundary decision about *which* unresolved names are
      statically refusable, and it needs a written rationale in the
      S7.2 register either way
- [ ] **S5.8** A module-level binding holding a **class instance** is
      not visible from a function body. Four lines:

      ```ts
      class A { tag: string = "A" }
      const inner = new A();
      function show(): string { return inner.tag }   // unknown identifier `inner`
      ```

      `const` and `let` fail alike, and reading `inner` at module level
      first does not help. A scalar (`const n = 5`) in the same position
      works, so the module-globals channel exists and simply does not
      carry struct-typed bindings. Found while writing the S2.9 fixture
      (three separate attempts at an unrelated line kept hitting it) and
      confirmed against the prior binary. Related to S5.7 in symptom
      (`unknown identifier`) but not in cause: this name **is**
      declared. Not yet censused — worth doing early, since the shape is
      ordinary enough that the census signature is probably large

#### S6 — Runner-side, not substrate

`harness-includes` core mass is **1239 cases** needing no substrate
work — only a typed port of the helper into
`conformance/test262-harness.ts`. (Of that, ~110 — resizable-buffer and
proxy-trap helpers — sit on core paths but serve post-v1.0 features;
port the rest first.)

- [ ] **S6.1** `isConstructor.js` — **377** across 40 directories
- [ ] **S6.2** `asyncHelpers.js` — **341**
- [ ] **S6.3** `fnGlobalObject.js` 130, `nativeFunctionMatcher.js` 69,
      ~~`deepEqual.js` 40~~ (ported 2026-08-19, rotation 441 —
      faithful `_compare` chain, 54 cases unlocked), sm shells ~60
- [ ] **S6.4** Make `Test262Error` carry its message through. **289 of
      the 695 bug-bucket cases (41.6 %) report bare `uncaught
      Test262Error` with no detail**, which makes the largest bug
      cluster unanalysable. Highest-leverage runner change on the list:
      it fixes zero cases and makes 289 diagnosable

#### S7 — The tail, and the gate predicate

Deliberately **not** enumerated item by item. Beyond the groups above,
core holds clusters #21–#482 (each ≥ 4 cases, individually
attributable) and a residue of 911 clusters of ≤ 3 cases each (1278
cases, 4.8 %).

- [ ] **S7.1** Re-cluster after each S-group lands and re-cut the line.
      The tail is not static: S1.4 predicts entire cascade clusters
      vanish, and every unlocked case can surface a *new* signature
      (this sweep moved 139 cases from `incompatible` straight into the
      bug bucket — the normal direction of travel, not a regression)
- [ ] **S7.2** **The gate predicate and the distance-to-v1.0 number.**
      Printed by `cluster_incompat.py` as the last thing it does, quoted
      in every rotation-close report
      (`.claude/rules/torajs-autorun-pipeline.md` step 0b):

      > **every core cluster of ≥ 4 cases is either resolved, or
      > attributed to an entry in the subset-decision register below.
      > The count of unattributed ≥ 4 clusters drives to 0.**

      Latest @ `03200808`, 2026-08-11, rotation 366 — the first
      register-aware stamp (SR-1 attributes 752 noStrict cases;
      previous `98efa861` / `b58797fb` in parentheses, pre-register
      figures):

      | | |
      |---|---|
      | clusters ≥ 4 cases, unattributed | **270** (484, 487) ← drives to 0 |
      | cases in them | 4023 (24438, 24755) |
      | register | 1 entry, 752 cases attributed (—, —) |
      | clusters ≤ 3 cases | 760 (919, 919) — 983 cases residue |
      | core total | 5758 (25720, 26046) |

      Both numbers have now fallen two rotations running. Before that,
      for two rotations, the movement had the other predicted shape —
      count up, cases down. Rotation 225 shipped
      S1.1/S1.2/S1.5 (`__new_*` 1138 → 926, `__this` 783 → 636);
      rotation 226 shipped S2.1, which took three parse walls to zero —
      1610 cases — while `incompatible` fell by only 11, because most of
      them hit the next wall one layer in and surfaced the two new
      signatures now filed as S2.8 and S2.9. **This is why the two
      numbers must be read together**: a rotation can demolish 1610
      cases' worth of one obstacle and move the headline by 11.

      **Expect the count to rise before it falls.** Unlocking a gap lets
      cases that could not previously run do so, and they surface their
      own signatures. That is progress presenting as a bigger number —
      which is why the count is never read without the case count
      beside it.

      **Subset-decision register**. A cluster may be closed by decision
      instead of by implementation — S4.1's eval shape and S5's checker
      boundaries are the expected entrants — but only by an entry here:
      cluster signature (or a mechanical predicate), case count at
      decision time, rationale, sign-off. An empty register plus 0
      unattributed clusters is full closure; a fat register is visible
      scope-cutting and reads as exactly that.

      **Sign-off protocol (updated 2026-08-11)**: takagi delegated
      register decisions to the agent ("这些都应该要你自己根据项目原则
      做好决策,不应该是要等我") — entries are decided against the
      design principles, recorded here with full rationale, and remain
      takagi-vetoable; a vetoed entry reverts to unattributed and the
      clusters go back on the countdown.

      The machine-readable register lives at
      `hardev/autorun/subset_register.json`; `cluster_incompat.py`
      applies each entry's predicate and prints attributed mass beside
      the gate numbers, so attribution is computed, never eyeballed.

      **SR-1 — sloppy-only surface (noStrict-flagged cases)**. Decided
      2026-08-11 @ `c025fd3f`; predicate = test262 frontmatter `flags:`
      contains `noStrict`; **752 core cases at decision time** (of 5758
      core; cluster-level effect measured 293 → ~270 unattributed ≥ 4
      clusters, 4764 → ~4046 cases). Rationale: tr's language surface
      is TypeScript modules, and module code is always strict (§16.2);
      the reference baseline bun runs `.ts` as ESM — the same
      always-strict surface. A noStrict-flagged case *requires* sloppy
      semantics for its assertions to hold and is not expressible as a
      TS module on either runtime, so this is a language-definition
      boundary, not a missing feature — per the test262 discipline the
      "why" is structural, not "bun fails too". A sloppy/script mode
      would be a product-surface expansion foreign to TS; if ever
      wanted it is a post-v1.0 roadmap phase, and this entry then
      retires. Corollary recorded the same day: strict semantics must
      be *implemented* where tr still leaks sloppy behaviour — the
      known B.3.3 block-level function leak (`nested_fns.rs`, the
      26-case passNoOracle water family from the eval rotation) is a
      substrate debt this entry does NOT cover, tracked in plan-state
      L3b

      **SR-2 — source-phase imports proposal (ModuleSource /
      `import source`)**. Decided 2026-08-11 @ `dfca9923`; predicate =
      test262 frontmatter `features:` contains `source-phase-imports`
      (the register's second predicate kind, `test262-feature`, landed
      with this entry); **93 incompatible cases at decision time**
      (the ~91-case "ModuleSource is not defined" cluster plus
      stragglers). Rationale: source-phase imports is a stage-3
      proposal (`import source` / %AbstractModuleSource%), not a
      ratified ECMA-262 clause, and bun stable rejects the syntax at
      parse — but per the test262 discipline the recorded "why" is
      structural, not "bun fails too": the phase semantics presuppose
      a module-source reification layer that tr's AOT
      compile-and-link model has no design for yet, which makes this
      a post-v1.0 roadmap phase rather than an out-of-scope decision.
      The entry retires (cases return to the substrate queue) when
      the proposal is ratified or bun ships it, whichever is first

#### S8 — Cases tr accepts and gets wrong

The bug bucket (**695** @ `9215301c`) is closer to the gate than
anything in `incompatible`: these compile, run, and produce the wrong
answer. 210 clusters. The largest — 289 cases, 41.6 % — is the bare
`uncaught Test262Error` mass that stays unanalysable until S6.4; the
largest *analysable* cluster is 21 cases. Enumeration is therefore
blocked on S6.4, except for what must not wait:

- [ ] **S8.1** Land S6.4, re-cluster the bug bucket, then enumerate here
- [ ] **S8.2** 12 cases exit 138/139 — **silent crashes, triage first**
      regardless of cluster size; a crash is never a subset boundary
- [ ] **S8.3** Six pass regressions from the `9215301c` sweep,
      case-level identified in `plan-state.md`, not yet attributed to a
      commit in the rotations 220–224 window
- [ ] **S8.4** Twenty cases moved `type error` → `tr-timeout` in the same
      sweep, all in the `iter-val-err` / `spread-err-*-itr-value` family
      — they now compile but do not terminate. Shape suggests an error
      that fails to propagate out of an iterator step, leaving the
      iteration unbounded. One root cause is plausible for all twenty
- [~] **S8.5** An array spread of a generator answers garbage once it
      crosses a function return. **Three lines**, found 2026-07-27
      while writing an unrelated fixture; **the direct-return shape is
      fixed the same day**, two shapes remain (see the end of this
      entry):

      ```ts
      function* s(): number { yield 1; yield 2 }
      function read(): number[] { return [...s()] }
      console.log(read());     // [ -562949953421311, -562949953421310 ]
      console.log([...s()]);   // [ 1, 2 ]  — same spread, printed in place
      ```

      Those values are NaN-box payloads read as `f64`, so the produced
      array's elements are Any-tagged while the `number[]` return type
      makes the caller read them raw. `[...[1, 2]]` across the same
      boundary is fine, so it is the generator source that decides the
      element representation. Nothing to do with classes — reproduces on
      a free `function*`, and equally on a public or private generator
      method. **Silent, so it outranks the refusals around it** by the
      design principles.

      **Root cause and fix.** The let-decl lane has decoded at this
      boundary since chunk 698 — `const a: number[] = [...s()]` was
      always right — via `__torajs_arr_any_to_typed`. The return lane
      admitted the same pair through the assignability lattice and paid
      nothing for it. The fix routes the return boundary through the
      same helper. Fixture `return-arr-any-typed-001` covers generators
      annotated and not, strings, spread mixed with literals, two
      spreads in one array, Set and Map-iterator sources, an arrow, and
      a class method — reading elements back individually, not only
      printing the array.

      **Still open, both filed here rather than chased:**
      - a *borrowed* return — `const a = [...s()]; return a`. The
        shared helper refuses to copy an aliasable source, which is
        right for a let-decl (copying would detach the binding from the
        source's later mutations) and arguably wrong for a return,
        where `consume_all_idents_in_return` has already moved the
        name out. Wants the gate lifted for this lane specifically,
        which is a decision about aliasing rather than a mechanical
        change
      - a *nested* one — `return [[...s()]]`. The runtime decode walks
        one level, so the inner array stays Any-tagged

**P-SURF acceptance**: every S1–S6 item either shipped or closed by a
register entry (S7.2); the S7.2 count of unattributed ≥ 4 clusters at
0; S8.2 empty; and every sweep from here on re-derives its numbers via
`cluster_incompat.py` rather than quoting this section.

**Ordering rationale**: S1 first — ~1900 core cases behind one gap plus
a cascade that makes everything downstream noise until it lands, and
its `__this` half has the widest directory spread in the census. S2
next for ratio (3085 cases behind one grammar point). S6.4 early and
out of band — it is cheap and it makes S8 legible. S4.1 (`eval`) needs
an RFC before it needs a commit. The "parser-only blast radius" claim
that put S2 second turned out to be true for one of S2.1's three
positions and false for the other two; it did not change the ordering,
but see S2.1 for why the reasoning was wrong.

**Status @ rotation 227**: S1.1 / S1.2 / S1.5 shipped (rotation 225),
S1.7 / S1.8 measured, **S2.1 shipped whole** (all three positions),
**S2.11 and S2.9 both closed** in rotation 227 (S2.9 except the two
cases that were never its own), plus S2.2's private-generator-name
third and S2.12's ordinary-method half; that rotation also surfaced
S2.12, S2.13, S2.14, S2.15, S2.16, S2.17, S5.8
and S8.5 — the last of those found and its main shape fixed in the same
rotation. S1.3 now measurable; S1.4 open with a corrected description;
S1.8(b) blocked on S5.6.

**Two roadmap descriptions were measured and found wrong in rotation
227** (S2.11's "both directions fail", S2.2's "the lexer rejects
`0x23`"), on top of the three rotation 226 corrected. Every one was
caught by a minimal repro that took under a minute. Treat a
census-derived sentence in this document as a lead, not a fact, and
run the repro before building on it.

Four orderings here were decided by measurement rather than by the
plan, which is the pattern worth keeping: S1.2 gated S1.1 (a function
mentioning `this` failed at declaration, no `new` required); S1.5 jumped
the queue as the group's only *silent* wrong answer; S1.6's cascade
theory was **disproved** by its own sweep, which redirected the work to
S5.6/S5.7; and S2.1's class half proved not to be parser work at all.
**Next: S2.2** (private names, 1291) — but note S5.6 outranks it on
kind, being a silent wrong answer rather than a refusal.

---

### v1.0 release gate

**Superseded definition** (kept for the audit trail): "P0–P13
substrate-checklists all closed = v1.0". As of 2026-07-26 all 84 boxes
are ticked and tr still rejects 26476 core test262 cases. The checklist
measured the substrate we planned; it never measured the surface. A
gate that a runtime can satisfy while failing half its corpus is not a
gate.

**Current definition** (2026-08-30, after takagi raised 轴 B's target
and added 轴 E): **P0–P13 closed ✓ *and* P-SURF closed *and* 轴 B at
target *and* 轴 E through E4**. The axes, with what each still owes —

- **轴 A (spec)** — P13 close ✓, **P-SURF open** ← S7.2's predicate:
  unattributed ≥ 4 clusters **149** (drives to 0), holding 1163 cases,
  register 2 · 251. Sweep @ `45121d4ff`.
- **轴 B (perf)** — **target raised 2026-08-24**: no longer "0
  regression" (that lower bound was met at r470 and is now table
  stakes) but **bench-tr full-matrix median tr/bun-aot ≤ 0.33 (3×),
  chasing 0.25 (4×)**. Measured @ `9325f1914` (2026-08-29, runs=3):
  median **0.502** across 44 cells, **0 cells slower than bun**, best
  `popcount` 0.047×, worst `prime_count` 0.987×. **Open by 1.52×.**
  P-PERF's own projection for S1-A2 + S2 + S6 + S7 all landing is
  **0.442 — still short**, which is why decomposition (not polish) is
  the standing mode on this axis.
- **轴 C (implementation purity / metal)** — **closed** @ `0d5a8b0`
  (0 LLVM, 0 inkwell, self-researched AArch64 backend + Mach-O writer +
  linker). The only axis that is done.
- **轴 D (multi-thread-ready)** — framing only during v1.0; the
  acceptance form is the four shape rules in Foundation, not an
  implementation. Real biased-ARC switch is P16, post-v1.0.
- **轴 E (platform reach)** — **open, newly on the gate**: E1
  linux-aarch64 / E2 linux-x86_64 / E3 macos-x86_64 / E4 windows-x86_64.
  Today: one combination of the five. Tracked as `P-PLAT`.

— and no new external dependencies, conformance gate green, per the
standing contract.

**Two of the five axes are closed or framing-only; three are open.**
轴 A has a countdown with a defined zero. 轴 B has a number and a known
shortfall. 轴 E has four named steps and no work started. That is the
honest shape of the distance to v1.0.

**test262 pass rate remains an observation, not the gate.** This is
unchanged and deliberate: the gate is S7.2's predicate — every core
cluster of ≥ 4 cases resolved or attributed in the subset-decision
register, unattributed count at 0 — not a percentage. Percentages
invite candy-coating; a cluster census with a signed register does not:
scope cut by decision stays visible as register entries instead of
dissolving into a rate.

---

### P14 — Proxy + Reflect (post-v1.0)

**Goal**: meta-object protocol, all 13 trap types.

**Substrate checklist** (strict order):

- [ ] **P14.1** Proxy class with handler trap dispatch
- [ ] **P14.2** `Reflect.*` spec methods
- [ ] **P14.3** Trap interop with Object.keys / for-in / etc.
- [ ] **P14.4** Proxy.revocable

---

### P15 — Tail call + edge spec (post-v1.0)

**Goal**: proper tail calls in strict mode + remaining spec edges
(annexB legacy, locale-dependent behaviour, host hook tests).

**Substrate checklist** (strict order):

- [ ] **P15.1** Tail call optimisation in strict mode (spec §15.10.3)
- [ ] **P15.2** annexB legacy semantics
- [ ] **P15.3** locale-dependent behaviour (Intl subset)
- [ ] **P15.4** Host hook tests — open new sub-trunk when runner
      breakdown points here

---

### P16 — Multi-thread substrate implementation (post-v1.0, placeholder)

**Goal**: 落地 vision "比 bun 上限高得多的真多线程能力"。详见
`.claude/vision.md` 三-1 节 + `rules/torajs-design-principles.md` §6。
**这是 placeholder phase** — v1.0 期间走轴 D framing（substrate ready
shape，0 增量代价）；P16 真切换在 v1.0 ship 后立项展开，每项有自己的子
RFC（biased ARC state machine / share transition / shared heap allocator
/ concurrent cycle collector / Send/Sync semantic enforcement / Worker
API 重设计 / per-thread budget 实测）。

**Substrate checklist (placeholder, 待 v1.0 ship 后展开)**:

- [ ] **P16.1** biased ARC state machine — owner_thread_id + share
      transition + atomic 慢路径 emit；参考 CPython 3.13 PEP 703 + Lee
      PACT 2018
- [ ] **P16.2** Shared heap allocator — thread-local cache + cross-thread
      free queue（mimalloc 模型 + raw syscall mmap-backed）
- [ ] **P16.3** Concurrent Bacon-Rajan cycle collector — 多 mutator
      assumption + 跨线程 buffer
- [ ] **P16.4** `Send` / `Sync` semantic enforcement in SSA-lower（user
      JS 语法透明，substrate 层 enforce）
- [ ] **P16.5** Worker API 重设计 — 1 shared heap + 任意 object cross-
      thread + 无 postMessage + Rust native thread cost
- [ ] **P16.6** per-thread budget 实测 — thread bootstrap cost / TLS
      metadata footprint / 跟 Rust `std::thread::spawn` 对齐基准

**Bench**: 多线程 workload SOTA vs bun-aot / nodejs worker_threads / go
goroutine。single-thread typed-tier 0 regression invariant 仍生效（biased
ARC owner-thread fast path 0 atomic 增量）。

**自研**: 0 deps 不变；atomic ops 走 LLVM intrinsics 不引入 crate；
syscall 走 raw（A5 轴产物）。

---

## Execution rules

1. **Phase order is fixed.** Do not start P(N+1) until P(N)'s substrate
   checklist is closed.
2. **Item order within a phase is fixed.** Each item's commit message
   names the item id (e.g. `P4.0`).
3. **Every commit ships through the conformance gate.** `conf gate`
   must stay green.
4. **Typed-tier bench gates every commit.** No regression past 3× CI
   noise.
5. **Stop and discuss only on:**
   - design forks not in this doc (e.g. K.3 globals扩 Type::Any vs
     runtime side table choice)
   - irreversible decisions (e.g. dropping a feature from a phase)
   - ambiguous-recovery failures (e.g. a substrate item turns out to
     need its own substrate — log as a P{N}.0 pre-blocker)
6. **Do not branch out of this doc** to side cleanups, refactors, or
   nice-to-have wedges. Append them as P{N}.{x+1} if they're needed
   for the current phase, or to `## Backlog` (below) if they're not.

---

## Backlog (orthogonal items, not on the trunk)

Useful but not on the test262-100% critical path. Pick them up between
phases only when blocked, never as a primary track.

- **f64.toString(radix) trailing-digit round-half-to-even** — current
  helper truncates at 52 digits; bun rounds the 53rd. Affects
  long-fraction cases only.
- **Array<f64> literal layout** — `let xs: number[] = [1.5, 2.5]`
  currently stores f64 bits in i64 slots; need real f64 array layout.
- **SameValueZero NaN-in-Array<f64>.includes** — `includes` on
  Array<f64> for NaN should return true; FCmp(Oeq) returns false.
- **String.search with RegExp arg** — currently string-arg only.
  Symbol.search dispatch is the wider substrate.
- **`xs.length = N` array truncation** — write-side on `.length`.
- **Closure parameter type inference from outer call site** —
  filter/map/reduce on Struct array currently rejects.
- **Generic explicit type-arg `id<T>(x)`** — substrate via the
  generic-instantiation path (probably part of P0).
- **Generator multi-class alias bug** — see notes on
  parser-generator-wrapped-return-001 fixture.
- **`this` on static class method** — currently `unknown identifier
  __this`.
- **JSON.stringify with indent (2nd arg)** — currently the indent arg
  is ignored.
- **`typeof x === "type"` narrow** — narrows aren't yet aware of
  `typeof` shape; covered partially by P3 / P0.
- **T-35 test262 runner cargo metadata target_dir** — already
  symlink-fallback'd, nice-to-have.
- **T-32 ArrayBuffer / TypedArrays** — multi-week substrate, schedule
  when a phase needs it.
- **T-36 Date.prototype.setX statics**.
- **T-40 `new Function(body)` global ctor** — eval-子集；跟 AOT 哲学
  冲突，needs design.
- **T-41 `eval` global** — extremely deep, requires `tr` to embed
  itself; design-pending.
- **Sparse Array support** — `a[2^32-1] = X` semantic.

---

## Detoured (kept for audit trail, not active)

Probed and deferred — substrate not in place. Will resume when the
named pre-requisites land.

### P-PARSE — ES syntax parser completeness (2026-05-14)

Inserted to clear parser surface before P0 typecheck work. **Status:
items P-PARSE.1–5 absorbed into P0 / P3 / P4 work as substrate-correct
fixes; the standalone phase is no longer tracked.**

### P-COERCE-B — ToPrimitive in `+` for Struct / Function (2026-05-14)

Deferred — requires object-literal method dispatch (now landed via
P3.struct-method-dispatch) + class-instance method discovery (P4
substrate). **Resume when P4 closes** — pick up as a P5+ wedge.

### P-CLOSURE-C — closure monomorphization at call sites (2026-05-14)

Deferred — overlapped significantly with P0's tagged-Any work; P0's
tagged-Any path subsumes it. **Closed without standalone resumption**;
revisit only if a specific case shows P0 doesn't cover it.

---

## Principles (kept)

- Foundation: `docs/design-principles.md` — five-pillar rubric (高性能
  / 自研 / 正统 / 规范 / 上限优先).
- Refcount + universal heap header: `docs/refcount-architecture.md`.
- Coding rules: `.claude/rules/common/`, `.claude/rules/{rust,
  typescript}/`.
- Project-specific principles: `.claude/rules/torajs-design-principles.md`.

---

## BENCH — cross-runtime perf benchmark (cross-cutting track)

Runs on every commit alongside conformance. Same set of cases as
v3 (popcount, fib40, generic-pair-1m, array-sum-1m, closure-pipeline-1m,
promise-then-100k, ackermann, …). **Acceptance: typed-tier benches stay
green end-to-end through P0–P13.**

Cross-runtime SOTA push happens every N phase as a perf-focused
sprint, not at v1.0 gate. Detailed bench harness layout, oracle setup,
and per-case budget table live in `docs/bench.md` (TODO: extract from
v3 roadmap appendix).

**报数口径（2026-08-21 起强制，见 `P-PERF`）**：每轮除总 `run_ms` 外
必须同时报 **work-only（`run_ms − startup_ms`）**、`startup`、
`artifact_bytes` 三项。只报总时间会让固定成本红利掩盖 per-op 差距。

---

## P-PERF — 竞品对位与固定成本（cross-cutting track，2026-08-21 立）

> 起因：bun 1.4 把核心从 Zig 机械移植到 Rust 后发布。完整调研与全部
> 实测证据在 `.claude/tasks/2026-08-21/bun-14-competitive-study.md`；
> 本节只留**执行项与判据**。

### v1 目标（takagi 2026-08-24 立）

> takagi：「perf 还是不太够，我期待最后 v1 要 3-4× 于 bun」。

**轴 B 的 v1.0 gate 从 r470 的「零落后」（下界，已达成：总时间 43/43
全赢）抬到：bench-tr 全矩阵总时间中位 tr/bun-aot ≤ 0.33（3×），追
0.25（4×）。** 实测 @ `6abe7e8`（2026-08-24，runs=3，44 cell）：
median **0.618** → S7 三刀（rotation 484）**0.593** → S7 r1+r2+r3 +
S2 刀 1（rotation 485，`bench/results/2026-08-23-mini-d9a309e.json`）
**0.518** → S1-A2 B1+A1（rotation 486，
`bench/results/2026-08-24-mini-0f5d964.json`）：改动面 17 cell 比值
中位 −8.1%（json 0.802→0.643 / regex-dfa ×8 −7.8~−13 / csv 族 −8），
未改动面 27 cell +0.1%；跨轮名义 median 0.532 是对照组机器漂
（rust/bun 绝对值同向 +4~12%，r211 判据），同轮自归一化读数为准。S2 刀 1 = fetch 改 dlopen 懒加载 libcurl（`00c0766b0`）：
空程序 startup 2.1 → **1.4 ms**（rust-hello 1.3 / C-hello 0.94 同法
同机；≤1.0 残余通路 = 产物瘦身 dead-strip，架构件）。通路量化（分解
见 `bench/results/2026-08-23-mini-6abe7e8.json`，startup 旧基线
tr 2.09 / bun-aot 3.97 / rust 1.12 ms）：

1. **S7（新）— tr-vs-rust abstraction gap 关闭。** rust 行是 native
   ceiling 代理；work-only 口径 >1.5× 的族全部是 §perf-decomposition
   意义上的 abstraction 浪费，此前因「总口径赢 bun」从未进攻击名单：
   `rpn-eval` **3.98×** / `generic-id-1m` **2.17×** / `collatz`
   **1.89×** / `array-map-1m` **1.88×** / `stack-pop-1m` **1.77×** /
   `array-sum-1m` **1.77×**（中位 trW/rustW = 1.32）。共因假说待
   profile：数组热环 per-iter 成本 + 调用/分派固定价。
2. **S2 — startup 2.09 → ≤1.0 ms**（独立贡献：median 0.618 → 0.515）。
3. **1+2 全落地的投影 median = 0.442，尚不达标。** 残余三族：
   regex-dfa 全族（proj 0.58-0.63）与 json/str/csv（0.56-0.77）→
   S1-A2 逃逸栈分配（架构件）；纯数值 `gcd1m` / `prime_count` /
   `mandelbrot`（0.85-0.95 —— bun JIT 已贴 native，rust 自身仅
   1.05-1.26× 于 bun）→ 要超越 -O native 的 codegen：**S6 SIMD 从
   「独立轴、大载荷才兑现」升格为 v1 达标必需**（mandelbrot 是 f64
   密集，NEON 双 lane 有 ~2× 的物理空间），外加 regalloc / 调度轴。
4. **3-4× 是 S1-A2 + S2 + S6 + S7 的合取，无单点银弹。** 各族分头
   走两步舞（Phase A decomposition → Phase B attack），禁 polish。

### Phase A @ 2026-08-30 — 判据从「指令条数」改成「依赖链」

rotation 533 对 `number[]` 热路径跑了完整两步舞的 Phase A(实测半 =
反汇编 + 消融 + `*_STATS`;源码半 = read-only agent 的 18 段拆解,
预算对账 **−0.2% / +0.3%**,两个独立锚)。产出改变了这条轴的攻击方式。

**结论一:在 M4 Pro 上,链外指令接近免费。** 反汇编数出「9 条里 5 条
浪费」(循环内重建整数立即数 ×2、φ 没合并的纯 `mov` ×3)听起来是 56%
的税,**实际接近 0%** —— 它们全在循环携带依赖链之外,被 8-wide 发射槽
吃掉。2026-08-24 那一轮的两把刀(c2 / r0)已经各实测中性一次,这是第
三次确认。**判据不是条数,是哪几条在链上。**

**结论二:真正的成本是一条被强制成 f64 的累加器。** `sum = sum + xs[j]`
经 W5 growth-cycle 规则(`num_width/width.rs:243-249` →
`cycle.rs:52-66`)把 **`Global("sum")` 与 `Elem(Global("xs"))` 双双**
判成 F64(`TORAJS_NUM_WIDTH_STATS=1` 实测,元素那一半推翻了源码半的
静态推导)。于是链上是 3-cycle `FADD` + FP φ-copy,而 rust 的 i64
累加器是 1-cycle `add`。实测锚 `arrread` = 1.30 ns/iter = 5.7 cyc,
rust ~2.1 cyc。**这一项占 `array-sum-1m` 全部 gap 的 83%。**

> **rotation 534 修正**:这一条的**量级**成立(手写 i64 版实测 −47%),
> 但**成分**分错了 —— 强制关掉元素那一半的 seed 只值 **2.3%**,
> 钱全在累加器那一半。而且 rust 那 12.1 ms 是 `Vec<i64>` 的**另一个
> 程序**,不是同题竞品。详见下面 S7-c / S7-d 与「报数必须带语义列」。

**结论三:`xs.push` 不是问题,它是我们的优势。** push-loop 检测
(`ssa_lower_push_loop_detect.rs`)把 runtime 调用整个消掉,per-iter
是 4 条内联指令 + 一次 exact-fit `mmap`。消融实测 **0.30 ns/iter
vs bun 3.21 —— 领先 10.7×**。**不要动它**;任何破坏该检测的改动在这
条 workload 上是 13 ms 的悬崖。

**执行顺序(替代此前 S7 的笼统表述)**:

- ~~**S7-a `arr_mutators` 的 deque 判据(1 LOC)**~~ **SHIPPED**
  `d5d05bba9`(rotation 533)。`ssa_lower_arr_mutators.rs` 的硬编码
  `false` 换成 Index 读 lane 一直在用的 `arr_expr_is_non_deque`:
  省 1 条链上 load + 4 ALU,并解锁 `SCALED_ADDR` 折叠。
- ~~**S7-b `map`/`filter` 的每元素跨归档调用(~90 LOC)**~~ **SHIPPED**
  `95c62e083`(rotation 534),实测 **−21.6%**(投影 −15%)。
  同轮 A/B、各五趟交错:torajs 37.95(σ0.26)→ **29.76**(σ0.03);
  对照 bun-aot +0.7% / rust −0.8%(机器没漂),未触及的 `array-sum-1m`
  20.74 → 20.77。内循环现在是 **13 条、零调用**。形状抽成
  `ssa_lower_arr_prereserve` 的三个 emit,`for`/`while` 的 push 快路径
  与它共用同一份契约。**立项事实第 4 条在这里兑现完毕。**
- **S7-c 累加器的 i64 表示** —— **rotation 534 的 Phase A 换掉了这条的
  问题**(全文 `.claude/tasks/2026-08-30/perf-w5-accumulator-decomposition.md`)。
  上一轮留的入口「W5 为什么把 `Elem` 也毒化了」已答,而且答案不是攻击面:
  1. **`Elem` 被判 F64 是对的,而且不值钱。** 原因写在
     `num_width/container_walk.rs:188-211`:一次 index 读可以越界,
     ES §10.4.2.1 那时答 `undefined`,I64 槽没有位模式表达它。
     **把那条 seed 强制关掉实测只快 2.3%**(20.77 → 20.29,同轮
     bun/rust 对照 < 0.5%)。上一轮「narrow `Elem` 是 83% 的 gap」被推翻。
  2. **`float_demote` 的 rescue 不存在。** 上一轮读码结论是它把两个 def
     判成 `Fit::Guarded` 后被 `profitable()` 的 `checks=4 > savings=3`
     扔掉,只差一个比较。实测:`profitable()` 在两个 bench case 上
     **零次调用**,候选集里只有 **1** 个值且是 `Fit::Exact`。
  3. **真链条**:`main` 的六个 f64 值 dump 显示 `LoadDyn`(即 `xs[j]`)
     **没有 interval fact** → 消费它的 `FAdd` 没有 → 累加器没有 →
     连进候选集的资格都没有(候选集 = `facts.keys()` 过滤 F64)。
- ~~**S7-d 按数组聚合的元素区间**~~ **SHIPPED**(rotation 536)——
  `.claude/rfcs/20260830-array-elem-interval/`,四期全落地。
  **`array-sum-1m` 同轮 A/B:19.61 → 12.49 ms(−36.3%),
  tr/rust 1.693 → 1.084**;对照 bun −0.08% / rust −0.5%(机器没漂)。
  内循环从 `fadd` 变成 `add` + 一条溢出 guard。
  - P0 `8be8dfdb2` —— 用流不敏感的 fact 折掉恒假的 `i < 0`。
  - P1 `be49aa268` —— 界内证明搬进 width walk(一份真源两个消费者)。
  - **P2 `5d5ab0769`** —— 元素 seed 变成有条件的。**RFC 写的条件不够**:
    它只写「在证明集里就不 seed」,而那个证明集是单端的。元素宽度问的是
    「这个读会不会越界」= 两端的问题,所以 seed 的条件用了一份新的两端
    证明(`num_width/bounds_lower.rs`):起点是非负整数字面量、循环内每处
    写都是非负步长、且**这个名字在整个模块里的每处写都在这个循环里**
    (体内一次调用打到赋值同名全局的函数,或一个捕获了它的闭包,都会绕过
    归纳;这条查表达式 arena 而非走一趟 —— 走漏一个就是 silent-wrong)。
    判别是精确的:`array-sum-1m` 的 `j` 从 0 起只 `+1` → 窄化;
    `arr-index-read-negative-guarded-001` 的 `i` 从 −2 起 → 保留 seed。
  - **P3 `bea966817`** —— SSA `interval` 的 key 从「一个 SSA 值」扩到
    「一个 SSA 值 ∪ 分配点的元素」(Cousot 分配点抽象堆),元素点作为
    「defs 是 store 的多 def cell」走同一条 Kleene 上升 / widen / narrow。
    **写入面是 allowlist 不是 blocklist**:`torajs-arr` 导出 ~135 个入口,
    逐个判「写不写元素」是漏一个就 silent-wrong 的形状;反过来,只有当
    一个分配的**每一处**使用都落在那张小表里才产生事实,明天新加的 runtime
    函数会让分析退化而不是出错。
  - 顺带 `8b96db55e` —— `float_demote::merge` 的写回块按 hash 序编号。
    **自这个 pass 写下来就潜伏着**,要 versioned region + merge bridge 带
    cell + 慢侧多出口才现形,而在元素点让这条循环可 demote 之前没有任何
    bench case 走到。`build_determinism.sh` 报 `array-sum-1m,ok,2`(N=12)
    —— gate / nextest / fmt / warning 计数**全部**看不见只在字节上的差异。
- **S7-e 已并入 S7-d 的 P2**,随 P2 一同 ship。

**ceiling 是实测的,不是投影的**:`array-sum-1m` 手写成 torajs 自己的
`i64` 标注(`let xs: i64[]` / `let sum: i64`)后跑 **11.3 ms**,而
`number` 版 21.3 —— **−47%**,且**快过同机 rust 的 12.1**。
**这条 cell 上不存在抽象税。**

**下面这条守卫省略,rotation 535 发现是错的并已修(`85a93c254`)**:
`i < xs.length` 只 settle 上界,对 `i >= 0` 一个字都没说,而省掉的是
**两条**比较。`i` 从负数走上来时读到 data 指针之前的内存 ——
`0 3.0266414627e-314 10 20 30`,而答案是 `undefined undefined 10 20 30`。
一次静默的越界堆读,gate 3467/0/4 与 Guard Malloc 全量扫描**都没看见**
(套件里每个带下标的循环都从 0 向前走)。修法保留 `i < 0`、仍省掉
`>= len` 与它的长度载入。今天代价测不出来(不在循环携带依赖链上),
**demote 之后值 18%** —— 那就是 RFC 的 P0。

**报数必须带语义列(2026-08-30 起)**:`array-sum-1m` 的 `main.rs` 用
`Vec<i64>`、`main.go` 用 `[]int64`,而 `.ts` 用 `number`(f64)——
**不是同一个程序**(此处和数恰好 < 2^53 所以输出相同)。数值型 cell 上
**同语义的对手是 bun**(该 cell tr 20.8 vs bun 41.1 = 快 1.98×);
rust/go 是**另一个程序的硬件天花板**,不是同题竞品。与
methodology §1「计时比较的是不同的工作」同族。

**明确不要攻**(已在 parity,或归因错误 —— 记在此防重复):
`xs[j]` 的元素访问(已是一条 AGU-scaled `ldr`,与 rust 同形)/
边界检查(guard-dominated elision 已消除)/ push 的 RC(`I64`/`F64`
不 `is_refcounted()`,静态为零)/ `generic-id-1m` 的 2.15×
(**是 rust 在自动向量化,不是 tr 退步**)/ 整数常量提升与 move
coalescing(链外,估中性)。

完整拆解与两次翻盘记录:
`.claude/tasks/2026-08-30/perf-loop-codegen-decomposition.md`。

### 立项事实（全部 2026-08-21 实测，两轮互证）

1. **总口径领先 39/44，work-only 口径输 17/41。** 差别是我们的固定
   启动成本红利（tr 2.589 ms vs bun 5.202 ms）。
2. **输的形状高度一致**：`regex-wireback-minlit` 1.84× /
   `json-stringify` 1.6× / `regex-dfa-iflag` 1.55× / `regex-dfa-dotall`
   1.54× / regex-dfa 全族 1.16–1.37× / `split-only` 方向性 3–4×。
   **换成 ns/iter 看，全族都是同一个量级的固定加价：+20 ~ +45 ns。**
3. **赢的形状同样一致**：控制流 / 调用 / 分配 / 泛型单态化 / Promise
   状态机 —— `popcount` 0.034× / `throw-catch` 0.061× /
   `async-fn-call` 0.226× / `generic-pair-1m` 0.262×。**这一族是结构性
   收益（AOT + 静态类型 + RC + 无预热），要保护并扩大取样。**
4. **归因未定，两条初判已被 Phase A 自己推翻（2026-08-21 当日）**：
   - **"根因是逐字节扫描 / SIMD=0" 是错的。** 输得最狠的
     `regex-wireback-minlit` 是 `"x".match(/x/)` —— **1 字节干草堆，
     几乎没有扫描**；regex-dfa 全族的干草堆只有 20–26 字节，SIMD 在这个
     尺寸上最多值几 ns，而 gap 是 20–45 ns。`grep -rn
     'core::arch|std::arch' crates/` 全仓只命中 `torajs-syscall` 一处
     `asm!`（**SIMD=0 这个事实为真**，两处自称 SIMD 的快路径
     `torajs-str/src/split/ops.rs:74` 与
     `torajs-regex/src/vm/search.rs:291` 确实是标量），但它**不是这批
     case 的根因**，是一条独立的、大载荷才兑现的未来轴。
   - **10 条 regex case 的两侧源码形状不同（`main.ts` 把 RegExp 提到
     循环外，`main.tora.ts` 写在循环内），但这条不影响结论 ——
     自己的假说当场被读码推翻。** `ssa_lower_lit.rs:118` 已有 fn-scope
     regex 字面量 LICM 缓存（`regex_lit_cache`，键 = `(pattern, flags)`，
     首次出现就提到入口块 `BlockId(0)`、后续复用），所以循环内写法与
     手工提出去**等价**。10 个 fixture 仍已改齐（源码对称本身该保持，
     且重测可作 LICM 缓存真在跑的运行期验证），但**regex 族的 gap 是
     真的**。附带确认：`optimize.rs:421` 的 `is_pure` 里
     `InstKind::Call => false`，所以 e-graph 侧不会做这个提升 ——
     提升完全来自 lower 侧那个缓存。
   - **干净的输只有两条**：`split-only`（两侧程序逐字相同）与
     `json-stringify`（我们那侧还更省 —— bun 每次迭代调一个
     `makeRecord()` 辅助函数，我们是内联的）。
   - **当前领先假说（待 profile 定类别，不得先攻）**：凡"builtin 返回
     一个新分配的堆对象"我们就多付固定成本。两条待证机制：
     ① **跨边界不可内联** —— 用户代码由自研后端生成、runtime 是预编译
     staticlib、由自研链接器拼装，`__torajs_*` 每次都是不透明调用；
     `torajs-mmalloc/src/core.rs:118` 的 `#[inline(always)]` 注释明写
     "让 fat LTO + cc -flto 把热路径内联进用户二进制 IR"，**而 inkwell
     随 `eded11f` 退役后这件事已经不发生了**；
     ② **短命垃圾的确定性析构** —— `split-only` 每次迭代造 7 个 Substr
     立刻丢弃，RC 必须逐个还；JSC 在 nursery 里 bump 分配、早死对象
     几乎零回收成本。**我们永不引入 GC（§6 HARD RULE），所以这条的答案
     必须是别的** —— 例如用 AOT + 静态类型证明不逃逸后栈分配整块，
     那是 bun 结构上做不到而我们能做的。
5. **artifact 的 25.7× 领先是"相对 bun 大"不是"我们小"**：hello-world
   产物 2,490,457 B，其中 `__text` 1,978,520 B；44 条 case 的产物跨度
   只有 17 KB → **99.3% 是固定 runtime**。同机 Rust hello world 469 KB。
   2026-05-24 时 `fib40` 产物 351 KB，三个月长了 7×，**没有任何机制
   在拦它**。根因：`torajs-link/src/archives_merge.rs` 的可达闭包是
   **归档成员（`.o`）粒度**，而 `[profile.release] codegen-units = 1`
   让每 crate 只出一个 `.o` —— 引用一个符号就拉进整个 crate。
6. **启动比 native 地板慢 1.0–1.2 ms**（三轮一致：+0.99 / +1.04 /
   +1.24 vs 同机 Rust hello world 1.351 ms）。而 bun 1.4 把 Linux 启动
   从 10.9 降到 5.1 ms —— **对手正在我们的头号差异化轴上追赶**，
   我们的启动优势从 3.1× 掉到 2.0×。

### 执行项（顺序执行，不是候选清单）

- **S1 — builtin 返回堆对象的固定加价（Decomposition 进行中）。**
  走 `rules/torajs-perf-decomposition.md` 两步法。**Pre-Phase-A gate
  已跑并两次翻盘**（见上第 4 条）：fixture 不对称已修，SIMD 归因已撤。
  剩余步骤，顺序不可换：
  1. 干净机器重跑两轮，拿 fixture 修正后的诚实 gap 列表（regex 族的
     数字在此之前不可引用）。
  2. **leaf-symbol profile 定类别**（`sample`，按 §9：profile 定类别、
     消融定价格；**禁止用计时器夹小步**）—— 对 `split-only` /
     `json-stringify` / `match` 三个放大到秒级的探针跑。
  3. Pre-Phase-B gate：Top-1 攻击面在总 self-time 里 **≥ 双位数 pp**
     才允许进 Phase B；不到就回去重拆。
  4. 消融定价：把预测拆成几个能分别测量的量，**要求它们的和与实测
     总量对上**（§9 唯一存活的那次预测就是这样来的）。
  **SIMD 不在 S1 里** —— 它是独立的 S6（下），大载荷才兑现。

  **S1 已落两刀（2026-08-21，`fe240fec` + `f4225b90`）**：
  ①`#[global_allocator]` 在用户 AOT 产物里从未生效（两张表语义相反：
  符号索引 first-wins、地址表 last-wins）；②三处 i64→字符串路径全在
  走 `core::fmt`。合计 `json-stringify` work-ratio **1.758 → 1.248**，
  `i64-to-str` **0.665 → 0.351**，regex 全族各降 0.05–0.12（它们的
  共享前缀里有 `i.toString()`）；总时间领先 **36/44 → 38-40/44**，
  几何均值 0.642 → **0.628**；输的那 16 条一条没多、中位 1.352 →
  **1.271**。**S1 剩余**：A1 WeakRef 观察位（split/match 各 ~4%）、
  B match 结果 exec-shape 属性惰性化（match 17%）、A2 逃逸分析后
  栈分配整块结果数组（split 46%，架构件）。
- **S2 — 启动降到 native 地板。** 把 2.589 → 1.351 的 1.2 ms 差拆开。
  **必须用消融法（删掉某一步量整体差值），禁止用计时器夹小步**
  （`perf-decomposition` §1：优化构建里时钟读取不是屏障）。嫌疑面：
  内建原型 / class metadata 注册在启动期跑、池预热、TLV 初始化。
  **这是防守性工程，不是锦上添花。**
- **S3 — artifact 函数粒度 dead-strip。** 链接器可达闭包从成员粒度走到
  **section / symbol 粒度**，配合提高 runtime crate 的 `codegen-units`。
  目标：hello-world 产物进百 KB 区间。顺带重开 `-Cpanic=immediate-abort`
  那条被搁置的取舍（`scripts/release-build.sh` 头部记着"需要一个把 size
  收益与热路径布局解耦的后续"，函数粒度 strip 正是另一条路径）。
- **S4 — HARD RULE 机械化。** `[workspace.lints]` +
  `clippy.toml` 的 `disallowed-{methods,types,macros}`，覆盖：runtime
  crate 禁 `eprintln!` / `thread_local!`（AOT staticlib 下分别 SIGBUS /
  静默零输出退出，见 `rules/torajs-build-and-port.md` B-2b）、refcount
  必走 `emit_rc_inc/dec` helper（设计原则 §6.2，现在只是 doc 里的一条
  grep）、新 global state 形态、`warnings = "deny"`。**收益是把三类
  反复复发的坑变成编译错误。**
- **S5 — bench gate 升级。** `artifact_bytes` 与 `startup` 升为独立
  回归 gate（现在只是结果里的一行数字）；每轮报 work-only 口径。
  **work-only 有条件数问题(2026-08-21 实测立)**：它是两个大数相减，
  当对手的 work 项相对减法误差不够大时读数不可信。判据 =
  `hypot(sd_case, sd_startup) / bun_work`：**≥25% 直接不报**
  （当前 `promise-chain-1k` 200% / `promise-all-1k` 69% /
  `array-any-indexed` 42%），10–25% 标记为勉强
  （`str-concat-ascii` / `multibyte-concat` / `split-only` /
  `fifo-queue` / `prime_count` / `promise-await`）。本轮三条"变差"的
  case 全部落在这两档里，逐条查明**不是回归**（tr 要么没动、要么与
  bun 同向移动）。**判据永远是"未改动的对照有没有同向移动"**
  （rotation 211）。
  **另加一条 fixture 等价性 gate**：`main.tora.ts` 与 `main.ts` 必须是
  同一个程序（2026-08-21 抓到 10 条 regex case 两侧结构不同、白比了很久
  ——stdout 相同不能证明在做同样的工作，这正是 perf 方法论
  「计时前断言各实现输出一致…否则比较的是不同的工作」的另一面）。
- **S6 — SIMD 字节扫描 substrate（独立轴，大载荷才兑现）。**
  事实为真（全仓 SIMD=0），但**不是当前 bench losses 的根因**，
  所以排在 S1–S5 之后，并且立项前要先有一条**载荷够大**的 case 证明
  它值钱（现有 regex case 的干草堆只有 20–26 字节）。届时产物是
  `torajs-simd` 石头层（aarch64 NEON）：`index_of_byte` /
  `index_of_any` / `index_of`(memmem) / `count_byte`。
  纪律：先量再写 —— 不要重复"注释自称 SIMD 实际是标量"。

### 与轴 B 的关系

本 track 是轴 B 的当前执行面。轴 A（P-SURF）与本 track **并行**，
按 CLAUDE.md 的顺序执行计划取 L3a 顶项，不开二选一。

---

## P-PLAT — 全平台（cross-cutting track，2026-08-30 立）

> takagi 2026-08-30：「全平台也必须拉入 v1」。本 track 是轴 E 的执行面。
> 立项调查见本节「绑定面」；轴 E 的终态与顺序定义在 Foundation。

### 为什么这不是「一次移植」

tr 的差异化是「AOT 出 native 小产物、启动快」。这三条如果只在
`aarch64-apple-darwin` 上成立，它们就不是这个 runtime 的属性。更实际
的一面：**服务端几乎全是 Linux**，而 tr 今天在服务端一行都跑不了 ——
一个 TS runtime 缺席 Linux，等于缺席它最大的使用场景。

### 一个别人没有的结构优势：交叉编译几乎是白拿的

tr 是 AOT 编译器 + **自研链接器**（`torajs-link`，不调用系统 `ld`）。
这意味着：**产出目标平台的二进制不需要目标平台的 toolchain，也不需要
在目标平台上跑**。host 上的 `tr build --target x86_64-unknown-linux-gnu`
应该直接出可运行的 ELF —— 我们自己发指令字节、自己排段、自己写重定位。

需要的只有目标平台的 runtime staticlib，而那些由
`scripts/release-build.sh` 的 `-Z build-std` 路径按 target 生成，本来
就已经是 cross-target 形态（当前显式 target 是
`aarch64-apple-darwin`，见 `scripts/release-build.sh:46`）。

**判据**：E1 收口时，mac 上 `tr build --target aarch64-unknown-linux-gnu`
出的 ELF 在 Linux 上直接跑通，全程没有 Linux 机器参与构建。做不到就说明
链接器里还藏着对 host 的隐式依赖，那本身是要修的债。

### 绑定面（2026-08-30 实测，`wc -l` + grep）

| crate | 行数 | 平台绑定的部分 | 可复用的部分 |
|---|---|---|---|
| `torajs-codegen` | 10,995 | `enc/*` 1,257（AArch64 指令编码）+ `compile/*` 4,930（指令选择） | `reg`/`regalloc`/`linear_scan*`/`liveness`/`spill_weight`/`frame` **4,096 行算法层**，需参数化寄存器集与调用约定 |
| `torajs-obj` | 2,464 | `macho/*` 1,285 | `object.rs` 1,013 中间层 |
| `torajs-link` | 25,536 / 78 文件 | **56 文件命中** `LC_*` / `MH_*` / `__TEXT` / `dyld` / chained fixups | 22 文件 |
| `torajs-syscall` | 1,066 | `arch_aarch64_macos.rs` trampoline + `sysno.rs` 191 行号表 | **`safe.rs` 已格式无关** —— XNU 的 carry-flag 错误约定已被归一化成 Linux 风格 `raw < 0 → -errno` |

`torajs-link` 是唯一一处**必须先做抽象才能动**的地方。其余三处是
「并列加一个实现」，抽象成本低。

### 硬设计约束（不可议价）

1. **平台分派必须是编译期单态化**（泛型 + `cfg` + const generics），
   **不是运行期 `dyn` 分派**。理由是第一设计原则：一个为可移植而在热
   路径上多一次间接跳转的后端，把轴 E 的收益从轴 B 身上扣了出来。
   Foundation 的五轴优先级明写轴 B 不让位于轴 E。
2. **ELF64 / PE-COFF writer 自研**，与现有 Mach-O writer 同等地位。
   **不引入 `object` / `goblin` / `gimli`** —— 轴 C 是 0 外部 dep，
   且我们已经自研了更难的那一半（链接器）。
3. **libc 仍是唯一允许的 runtime 外部接口**（设计原则轴 C）。Windows
   上的对应物是 ucrt；`torajs-syscall` 在 Windows 上退化为对
   ntdll/kernel32 的 import，因为 Windows **没有稳定的 syscall 号面**
   —— 这是 E4 最大的形态差异，不是可以照抄 E1/E2 的。
4. **每一步单变量**（Foundation 的 E1→E5 顺序）。E3 的成本应该接近零；
   **它不接近零就是 E1/E2 抽象没做对的信号**，当作免费的架构自检。

### 执行项（顺序执行，不是候选清单）

- **E0 — 格式抽象 + target triple 贯通（前置，E1 的一部分）**。
  今天 `aarch64-apple-darwin` 在 5 处硬编码
  （`torajs-link/src/archive.rs:590,690`、`archives_merge.rs:548,552`、
  `scripts/release-build.sh:46`）。把 target 变成一等参数，
  `torajs-obj` 抽出 `ObjectFormat` trait（编译期单态化），
  `torajs-link` 的 56 个 Mach-O 文件按「格式相关 / 格式无关」重新划线。
  **这一步不产出新平台，产出的是后面四步的地基** —— 也是唯一一步
  值得先写 RFC 的。
- **E1 — `aarch64-unknown-linux-gnu`**。ELF64 writer + Linux syscall
  trampoline（`svc #0`，号走 `x8`）+ Linux 号表。ISA 不变。
  acceptance：mac 上 cross-build 出的 ELF 在 Linux 上跑通 conformance
  gate 全量。
- **E2 — `x86_64-unknown-linux-gnu`**。x86-64 变长指令编码器 + 指令
  选择 + SysV AMD64 调用约定 + 寄存器集参数化。**这是五步里 codegen
  工作量最大的一步**（`enc/*` + `compile/*` 的对应物）。
- **E3 — `x86_64-apple-darwin`**。格式（Mach-O）与 syscall（XNU）都
  已有，ISA 复用 E2。**成本应接近零，是 E0 抽象质量的验收。**
- **E4 — `x86_64-pc-windows-msvc`**。PE-COFF writer + Microsoft x64
  ABI（不同的参数寄存器、shadow space、不同的 unwind 形态）+ import
  table 驱动的系统调用。三者全新。
- **E5 — `aarch64-pc-windows-msvc`**（post-v1.0）。ISA 已有，ABI 与
  E4 共用。

### 与轴 A / 轴 B 的关系

三条 cross-cutting track（P-SURF / P-PERF / P-PLAT）**并行**，按
CLAUDE.md 的顺序执行计划取 L3a 顶项，不开二选一。
**bench 与 conformance 的对位在每个新平台上重新成立才算该步收口** ——
一个跑得起来但慢一倍的 Linux 产物不是 E1 收口。

---

## Historical roadmaps

`docs/roadmap-historical.md` preserves the v1 (P0–P13 foundation), v2
(33-item perf-gated), v3 (V3-XX wedge-cycle to 522/521 curated), and
v4 (test262-100% trunk) plans verbatim. Read them for the *why* of
tora's foundation. Do not read them for *what to do next* — that
lives only in this file.
