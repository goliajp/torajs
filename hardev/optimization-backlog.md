# torajs 研发环境优化 backlog（质量中性，按杠杆排序）

每项必须过「第一硬规则」（不降验证覆盖/正确性，见 README）。每项带：杠杆估计、
**质量为何中性**的论证、**可机器判定的验收**、状态。做掉一项 → 标 done + 记实测墙钟
（对照 `ENVIRONMENT.md` §5 基线）。发现新项 → 追加，别动已 done 的结论。

基线（优化前）：full conformance 628 串行 ≈ **30min**（内置隔离 target）。

---

## P1 — 并行化 conformance runner【最大杠杆，质量绝对中性】✅ DONE

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
**预估**：~30min → ~4-5min（8 核）。

**✅ 已 ship `6ab22f9`（2026-05-19）**。比预估更好：实现里**额外**把 628× per-case
`cargo run`（带 build-lock 争用）换成**启动时 `cargo build` 一次**（cargo 经
`--message-format=json` 自报 binary 绝对路径，正统、无 target-dir 猜测、无新依赖），
之后每 case 直调 `tr` binary。per-case temp 路径加 `-s{slot}-p{pid}` 唯一化；
结果按原始 case 顺序回放打印 → 跨run + vs串行**逐字节同序**；`.dSYM` 同边界清理
（Disk Hygiene）。**实测**：3 次连跑均 `627 pass / 0 fail / 1 skip`，墙钟
**174.86 / 180.52 / 180.76 s ≈ 3.0min**（vs 串行 ~30min → **~10x**，优于
≤5min 目标）；跨run ok/skip/FAIL 行集逐字节相同 → **零并发 flake**。
（1 skip = `perf-005-dwarf-panic-fs`，bun 自身 exit 1 的既有预期 skip，未变。）
**状态：DONE ✅**

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

## bench-harness 专项（2026-05-19 提出；置信度/速度/覆盖/专业性 4 轴）

起源：P7.4-a-b ship 后跑 bench 实证单次 run_ms 摆 [−65%,+171%]（远超文档 ±20-40%），
且 pipeline 规定的 "3-run median" 在 json 层做不到（同名覆盖）。grounded 自
`bench/harness/src/{bench,report}.rs`。**第一硬规则在此尤其关键：bench 的提速
绝不能以降低计时置信度换取**（与 conformance 并行化的免费提速本质不同——
conformance 并行不改正确性，bench 并行摧毁计时可信度，故 bench 提速只能"少做
冗余功"不能"并行"）。按 confidence-preserving 优先排序：

### B1 — artifact_bytes 自动 gate + N-run 原生聚合【最大置信度收益，~零速度成本】

**现状**：`report.rs` 仅 `fs::write` 同名 `{date}-{host}-{sha}.json`（同 sha 多跑
互相覆盖）；无 `compare` 子命令、无 baseline-diff、无回归判定。`artifact_bytes`
（确定性、mac-noise-immune、最可靠回归信号）只能人肉 eyeball。
**做法**：① `bench compare <baseline.json>` 子命令：per-case artifact_bytes 必须
逐字节一致否则 FAIL（或显式 `--allow-artifact-delta <case> <reason>` justify）；
② 多 run 不覆盖（文件名加 run-nonce 或原生 `--runs-aggregate N` 收集 N 次取
median-of-medians + MAD + 置信区间写一个聚合 json）。
**为何不降置信/覆盖**：纯增判定能力，不删任何 case、不减子步。把人肉对比变成
机器硬门是**提升**置信度。
**验收**：`bench compare` 对 76ace15 vs 8b73988 自动输出 24/26 identical / 2 shift；
3-run 聚合 json 含 median+MAD；artifact-delta 未 justify 时退出码非 0。
**状态：TODO（最高优先）**

### B2 — `--self` 快速回归模式 + artifact 预检【最大速度收益，per-commit 10min→秒级】

**现状**：全跑 8 runner × 26 case ≈ 585s/run；per-commit 回归门其实只需 torajs
vs 自身 baseline，bun/node/go/rust/python 是 SOTA 横向对比（phase-close 关注）。
**做法**：① `bench run --self`：只跑 torajs / torajs-run，跳过其它 runtime；
② artifact 预检：先比 artifact_bytes（确定性、亚秒），全 identical 即秒级判
0-regression，仅变化的 case 才回退跑 timed。
**为何不降置信/覆盖**：`--self` 仅用于**per-commit 回归门**；**phase-close 仍
强制全 8-runner 全 case + B1 artifact gate**（SOTA 验证一字不变）。预检逻辑：
artifact 不变 ⟺ 机器码不变 ⟺ run_ms 不可能真回归（教科书结论），故跳 timed
是无损的。
**验收**：`--self` 输出与全跑的 torajs 行逐字段一致且耗时 ≤ 1/3；预检命中时
总耗时 < 10s 且结论与全 timed 跑一致。
**状态：TODO（高优先，B1 之后——B1 提供 compare 基建）**

### B3 — 覆盖跟着 phase 走（P7：throw/bigint hot case）【覆盖缺口】

**现状**：26 case **无 bigint**、仅 1 个 exception-ish（throw-catch-100k）。
a-b/#15 的 perf 影响 bench 没直接测（只能靠 artifact_bytes 推理）。P7=Error
phase，throw/catch/finally + bigint hot path 在被改时恰恰覆盖不足。
**做法**：每个加 substrate 的 phase 同步加打其 hot path 的 bench case。P7 →
`bigint-arith-1m`（Div/Mod/Pow 循环）+ `try-throw-catch-1m`（深 try 嵌套 +
跨 fn throw 传播，正好测 #15 may-throw 路径）。
**为何不降置信/覆盖**：纯**增**覆盖。新 case 与既有同标准（expected.txt +
cross-runtime + artifact gate）。
**验收**：P7 close 时 bench 含 ≥1 bigint + ≥1 throw-propagation case，且
artifact gate 对它们生效。
**状态：TODO（与 P7.5 / phase-close 同期）**

### B4 — 机器状态卫生 + 统计裁定 + 对比方法学文档【专业性】

**现状**：无 nice/CPU-pin/QoS/空闲检测（本会话噪声爆炸即并发抢占）；无统计
回归裁定（需 delta 超噪声带→pass/fail）；runs/warmup 固定不按 case 时长缩放
（sub-2ms case 需更多 runs）；横向对比公平性（go/rust 是否 release/-O3）未
文档化，削弱"SOTA vs bun/node/go"可信度。
**做法**：① runner 前置 `nice -n -5` / QoS + 跑前检测 load，超阈值 warn/defer；
② B1 的 compare 加 MAD-based 或 Mann-Whitney 噪声带裁定；③ run_runs 按上次
median 自适应（快 case 多跑）；④ 写 `bench/METHODOLOGY.md` 记录各 runtime
编译/优化档与公平性论证。
**为何不降置信/覆盖**：全部**提升**置信度与可信度，不动 case 集。
**状态：TODO（专业性收尾，B1-B3 之后）**

---

## 已应用（本专题起源会话，2026-05-19）

- **torajs 项目私有内置 target**（去共享盘并发争用 + cargo-clean 隔离）：~50min →
  ~30min conformance。**注意**：这不是 build 提速（sccache 已覆盖），是去争用 +
  隔离；详见 `ENVIRONMENT.md` §2 的认知纠正。

## 待追加（占位，后续会话补）

- conformance per-case `tr build` 中间产物落点审计（是否在快/隔离的 tmpfs）
- sccache 命中率调优 / 本地缓存上限（build 速度真杠杆，见 ENVIRONMENT §3）
- `.dev/clean.sh` 接 Claude Code hook 自动触发（automation 部分，update-config skill）
