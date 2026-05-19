# torajs 研发环境优化 backlog（质量中性，按杠杆排序）

每项必须过「第一硬规则」（不降验证覆盖/正确性，见 README）。每项带：杠杆估计、
**质量为何中性**的论证、**可机器判定的验收**、状态。做掉一项 → 标 done + 记实测墙钟
（对照 `ENVIRONMENT.md` §5 基线）。发现新项 → 追加，别动已 done 的结论。

基线（优化前）：full conformance 628 串行 ≈ **30min**（内置隔离 target）。

---

## devperf #1 — 快速迭代 profile【最大杠杆，root-caused 2026-05-19】

**root-cause（devperf P0，task #22）**：sccache 对 torajs 内循环结构性无关
（全局共享 + bin/改源 non-cacheable，非 bug）。真杠杆 = `[profile.release]`
（`lto="fat", codegen-units=1`，最大优化 **ship** profile）被**每次迭代构建
复用**。**实测 touch torajs-core → 重建 tr = 28.5s**（vs no-op 0.05s）。
**做法**：新增独立快速迭代 profile（`[profile.<name>]` 继承 release 但
`lto=false, codegen-units=256, opt-level=1, debug=false`），把**功能迭代 +
conformance 的 tr build** 切到它；**bench + 最终 ship 仍 `--release`**（fat-LTO
真 ship 二进制）。
**质量为何中性（带证明）**：opt-level/LTO/cgu **不改程序语义**（教科书不变量）
→ 快 profile 的 tr 与 release 的 tr 对所有 conformance case **stdout 逐字节
相同**；故 conformance 用快 profile **仍是同覆盖同 byte-equal 判定**，正确性
等价。**bench 不切**（bench 测 ship 二进制 runtime 性能，必须 fat-LTO；切了
就测错东西 = 违反第一硬规则）→ 覆盖/正确性零损失。
**可机器判定验收**：① 快 profile 跑 full conformance **仍 629/0/1**（实证
correctness-equivalence）；② `touch torajs-core && build tr(fast)` 墙钟
**≤ 5s**（vs 28.5s baseline）；③ bench 仍走 `--release`（grep 确认 bench-harness
runner 描述符未改 + 一次 bench 跑 artifact_bytes 与 release baseline 一致）；
④ 0-warn / fmt-clean 不破。
**预估**：内循环 28.5s → ≤5s（~6x），叠加 conformance 并行（P1）后 dev-loop
质变。

**✅ DONE 2026-05-19**（实测全达，超目标）：
- `[profile.iter]`（inherits release；`lto=false, codegen-units=256,
  opt-level=1, strip=false`）+ conformance/runner `--release`→`--profile iter`。
- **edit→rebuild tr: 28.5s → 2.49s（~11.4×，目标 ≤5s 超额）**。
- **full conformance(iter tr) = 629/0/1**（correctness-equivalence 实证；
  opt-level/LTO 语义不变在 torajs 629 真实 case 成立）。
- bench/ship 物理隔离：`target/iter/tr` ≠ `target/release/tr`，bench runner
  描述符硬编码 `target/release/tr` 未改 → 结构上不受影响。
- **⚠️ 操作 nuance（非静默）**：以前 `cargo run -p torajs-conformance` 顺带
  产出 `target/release/tr`；现在它产 `target/iter/tr`。**bench 前必须显式
  `cargo build --release -p torajs-cli`** 否则 `target/release/tr` 可能 stale/
  缺失 → bench 测错二进制（违反第一硬规则）。这是 bench pillar 须吸收的契约
  （bench-harness/caller 应在跑 bench 前强制 release-build；记入 bench D-系列）。
**状态：DONE ✅**

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

### B0 — bench 前强制 release-build 契约【正确性前置，devperf #1 引入，必做】

**背景**：devperf #1 后 `cargo run -p torajs-conformance` 产 `target/iter/tr`，
不再顺带产 `target/release/tr`。bench runner 描述符硬编码 `target/release/tr`。
**风险**：若跑 bench 前没人 `cargo build --release -p torajs-cli`，bench 测的
是 stale/缺失的 release 二进制 = 测错东西 = 违反第一硬规则（覆盖/正确性）。
**做法（已实现）**：`bench run` 启动即 `ensure_release_tr(workspace)` —
`cargo build --release -p torajs-cli`（cwd=workspace，幂等），失败 fail-fast
拒跑，并校验 `target/release/tr` 存在。选 auto-build 而非纯 fail-fast：幂等
（fresh 0.08s no-op / stale 重建），bench 永远测当前 ship 二进制，零人工步、
零 footgun。
**✅ DONE 2026-05-19**（实测）：stale release-tr（上次建的是 iter）→ B0 触发
自动重建 30.48s 后正常 bench；fresh → guard 0.08s no-op 直接 bench。fmt clean,
0-warn。bench-harness tooling，无 substrate（不需 conformance gate）。
**状态：DONE ✅**

### B1 — artifact_bytes 自动 gate（`bench compare`）✅ DONE + N-run 聚合(→B1b)

**✅ DONE 2026-05-19**：`bench compare <base> <cur> [--allow-artifact-delta
case:runtime,…]`（`bench/harness/src/compare.rs` + main.rs 接线）。编码本会话
实证方法论：**artifact_bytes 硬门**（确定性唯一可信信号；任一 per-case 变 =
回归疑似 → 退出码 1，除非 `--allow-artifact-delta` justify）+ **run_ms
noise-aware**（仅当同 case artifact **也**变才分类；artifact 不变则 run 差
by-construction 是噪声，永不报）。**验收实测**：精确复现先前手跑 python 结论
（8b73988→76ace15：torajs `array-sum-1m -16` / `throw-catch-100k +416`，
逐一吻合；94 identical）；未 justify→VERDICT FAIL exit 1；justify→PASS exit 0；
同文件→0 delta PASS。fmt clean、0-warn。**灭"agent 手跑 ad-hoc python"
反模式 —— 回归裁定现可复现、在仓库、机器判定。** bench-harness tooling，
无 substrate（验收即测试，不需 conformance gate）。
**状态：DONE ✅**（compare 核心）

### B1b — N-run 原生聚合（median/MAD）✅ DONE

**✅ DONE 2026-05-19**：`bench run --runs N`（默认 1，完全向后兼容）。
N 个**完整 interleaved pass**（每 pass 跑全 case×runner 矩阵，重复 N 次 →
median 采样跨时机机器状态变异，匹配历史"3 full-suite run"本意，非 N 次
背靠背单 cell）。per-cell 聚合：`run_ms`=median、`run_stddev_ms`=**MAD**
（robust spread，单点 thermal 尖峰几乎不动它）、`compile_ms`=median、
`artifact_bytes`=全同则取该值否则 median（±N 字节 linker 漂移良性，compare
已保守处理）、`status`=worst（单 pass fail 不被聚合掩盖）。`Report.runs`
字段记聚合深度。**一次调用产一个统计稳健 json，无同名覆盖、无 log-parse
hack。** **验收实测**：`run fib40 --runtime torajs --runs 3` → json `runs:3`，
fib40 median 176.194ms / MAD 4.0612；`bench compare` 直接吃；无 flag → runs:1
单 pass 行为不变。fmt clean、0-warn。无 substrate。
**状态：DONE ✅**

### B1-orig — （原 B1 描述存档，已被上面拆分实现）

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

### B2 — `--self` per-commit 快速路径 ✅ DONE（artifact 预检拆 B2b）

**✅ DONE 2026-05-19**：`bench run --self` → 仅 torajs/torajs-run runtime（丢
bun/node/go/rust/python——那是 SOTA 横向对比 = phase-close 关注，非 per-commit
回归门）。per-commit ~3-4× 提速，**覆盖不减**（回归 target 是 torajs vs 自身
baseline；phase-close 仍全 8-runner，第一硬规则）。显式 `--runtime` 永远 override
`--self`。打印 per-commit-scope notice 防误当 phase-close。**验收实测**：
`run fib40 --self` 仅 fib40×{torajs,torajs-run}+notice；`--runtime bun-jsc`
override（无 notice，仅 bun-jsc）；help 显示。fmt clean、0-warn、无 substrate。
**状态：DONE ✅**

### B2b — artifact 预检（artifact 不变跳 timed → 秒级）【follow-on，最大速度】

**做法**：`bench run --self --vs <baseline.json>`：对每 case 走 torajs runner 的
compile 路径只 stat artifact_bytes（无 hyperfine、无 timed run），逐一对 baseline。
全 identical → "0 regression by construction（机器码未变），timed 跳过" 秒级 exit 0；
任一 differ → 该 case fallback 跑完整 timed（覆盖不减）。**质量为何中性**：artifact
不变 ⟺ 机器码不变 ⟺ run_ms 不可能真回归（教科书 + 本会话实证）；变了仍全 timed
测，第一硬规则满足。**验收**：tr 不变 `--vs` 秒级 PASS；改 torajs-core 后该 case
fallback timed 且结论与全 timed 跑一致。**状态：TODO（B2 follow-on）**

### B2-orig — （原 B2 描述存档，已拆 B2 + B2b 实现/计划）

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
