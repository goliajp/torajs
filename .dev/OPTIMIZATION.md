# torajs 研发环境优化 backlog（质量中性，按杠杆排序）

每项必须过「第一硬规则」（不降验证覆盖/正确性，见 README）。每项带：杠杆估计、
**质量为何中性**的论证、**可机器判定的验收**、状态。做掉一项 → 标 done + 记实测墙钟
（对照 `ENVIRONMENT.md` §5 基线）。发现新项 → 追加，别动已 done 的结论。

基线（优化前）：full conformance 628 串行 ≈ **30min**（内置隔离 target）。

---

## P1 — 并行化 conformance runner【最大杠杆，质量绝对中性】

**现状**：`conformance/runner/main.rs:61` 纯串行 `for c in &cases`，~628 cases。
**做法**：worker-pool 并行（参照 `conformance/test262-runner/main.rs` 已有的
`--workers N` pattern，codebase 内已验证）。每 case 独立（bun/expected + tr run +
tr build + byte-equal，无共享可变状态）。
**质量为何中性**：并行只改调度，不改"跑哪些 case、跑几步、怎么比对"。同样 628
cases、同样 3 子步、同样 byte-equal 判定。并行不改变正确性是教科书结论。
**唯一风险点**：per-case 临时文件/输出路径必须每 case 唯一（避免并发互踩）——
实现前先确认 runner 的 .ts/out/临时路径已隔离（test262-runner 那套是参照）。
**验收**：`conformance` 仍输出 **627/0/1**（与串行逐字节同集）且墙钟 ≤ 基线/核数×1.5
（8 核 → 目标 ≤ ~5min）；连跑 3 次结果稳定（无并发 flake，参照 task #11 的
torajs-embed 隔离教训）。
**预估**：~30min → ~4-5min（8 核）。**状态：TODO（最高优先）**

## P2 — 内容寻址 `tr build` 缓存（sccache-for-tr）

**现状**：每 case `tr build` 全量跑 tr→SSA→LLVM→cc，628×。tr binary/case 不变
时是纯重复劳动。
**做法**：缓存 key = `hash(case 源 + tr binary 内容 hash + build flags)` →
缓存产出的 native binary（或中间 .o）。命中即取，未命中才真编。
**质量为何中性**：key **含 tr binary hash** → tr 一改 hash 必变 → 全部 case
重编（验的就是"这版 tr 正确"，故必须重验，自动满足）。tr+case 都不变时缓存
产物**逐字节等同**重编结果 → 零正确性差异。
**验收**：tr 改动后首次 conformance 全 miss（耗时≈无缓存）；tr 不变重跑全 hit
（墙钟骤降）；两种情况 pass 集均 627/0/1。
**预估**：tr 不变的重复 gate（如 fixture-only commit、bench 期间）近乎 0 编译。
**状态：TODO**（工程量大，P1 之后）

## P3 — 分层 dev-loop / ship 全量【流程，零成本，零质量损失】

**现状**：每次迭代都倾向跑全 628。
**做法**：① 中间迭代只跑 affected fixtures + 固定 smoke 子集；② **每次真 commit
前仍强制全 628**（ship gate 完全不变）。
**质量为何中性**：ship 边界的验证**一字不变**——全 628 仍是 commit 前 mandatory。
去掉的只是 commit 之间冗余的全跑（那里本就还没 ship）。affected-set 判错也由
mandatory 全 gate 在 commit 时兜住。
**验收**：commit 前必有一次 full 627/0/1 记录；中间迭代用子集（人工纪律 +
后续可脚本化）。
**状态：TODO**（流程约定，可先写进 autorun-pipeline pre-flight 备注）

## P4 — 稳定 case 的 `.expected` blessing（次要）

**现状**：每 case 都跑 `bun run` 当 oracle（3 子步之一）。
**做法**：稳定 case 提交 `.expected`（runner 已支持 `.expected` 覆盖 bun）→
跳过该 case 的 `bun run` 子步。
**质量为何中性（带纪律）**：`.expected` 是 bun 输出的 blessed 快照；**必须**在
bun 升级 / case 编辑时 re-bless（脚本化 + 记 bun 版本）。re-bless 纪律在 →
等价 live oracle；纪律断 → 退化为 stale 快照（故纪律是验收的一部分）。
**验收**：有 re-bless 脚本 + 记录 bun 版本；`.expected` 与当前 bun 输出一致性
有 CI/periodic 校验。
**状态：TODO（边际，最后）**

---

## 已应用（本专题起源会话，2026-05-19）

- **torajs 项目私有内置 target**（去共享盘并发争用 + cargo-clean 隔离）：~50min →
  ~30min conformance。**注意**：这不是 build 提速（sccache 已覆盖），是去争用 +
  隔离；详见 `ENVIRONMENT.md` §2 的认知纠正。

## 待追加（占位，后续会话补）

- conformance per-case `tr build` 中间产物落点审计（是否在快/隔离的 tmpfs）
- sccache 命中率调优 / 本地缓存上限（build 速度真杠杆，见 ENVIRONMENT §3）
- `.dev/clean.sh` 接 Claude Code hook 自动触发（automation 部分，update-config skill）
