# torajs 构建/缓存环境 — Ground Truth

记录**实测确认**的环境事实，尤其**反直觉**和**已纠正的错误认知**。
后续做优化前先读这里，避免基于错误前提行动。每条都标实测来源。

## 0. ⚠️ `ls -t … | head -1` 在本机不可信 + 绝不 `rm $VAR`（本会话两次踩坑）

`ls -t bench/results/*.json | head -1` **数次返回旧的已提交文件而非刚写的新文件**
（mtime 排序在本机 APFS / 多文件场景不可靠）。bench-harness 自己会打印
`results: <绝对路径>` —— **永远用那个输出路径**，绝不用 `ls -t|head` 猜。
更严重：把猜错的路径塞进 `$BJ` 然后测试 scaffolding `rm -f "$BJ"` →
**误删了 `683bd95` 提交的真·全量 baseline `2026-05-19-mini-23a6e31.json`**
（208 行/26 case/8 runtime），靠会话末强制 disk-hygiene audit 的
"D 文件 → look before delete" 才发现，`git checkout --` 复原。
**铁律**：(a) 取刚生成文件用工具自报的绝对路径，不用 `ls -t|head`；
(b) `rm -f "$VAR"` 前必确认 `$VAR` 不是 tracked/committed（`git ls-files`
查；或只 `rm` 明确 untracked 的 `??`）；(c) 这正是 CLAUDE.md disk-hygiene +
anti-hallucination "verify before delete" HARD RULE 存在的原因——它本会话
真的兜住了一次数据丢失。

## 1. cargo target dir（torajs = 项目私有内置盘）

- 全局机制（见 `~/.claude-shared/global/cargo-target-dir.md`）：`~/.local/bin/cargo`
  wrapper 把 `CARGO_TARGET_DIR` 指向外置卷 `/Volumes/INTEL2T/...`，未挂载则回退
  `~/.cargo-target-fallback`（内置）。这是**非-torajs 项目**的全局策略，省内置盘空间。
- **torajs 例外（2026-05-19 落地）**：torajs 用**项目私有、内置盘**的 target：
  - wrapper 加了 generic `.cargo-target-local` marker 机制：cwd 或祖先有该文件 →
    wrapper **不设** `CARGO_TARGET_DIR`（让项目 config 生效，无 hardcode，任何
    perf-critical 项目可 opt-in）。
  - `<torajs>/.cargo-target-local`（marker，opt-in）
  - `<torajs>/.cargo/config.toml` → `[build] target-dir =
    "/Users/doracawl/workspace/goliajp/torajs/target"`（覆盖 home
    `~/.cargo/config.toml` 的 INTEL2T；project config 优先级 > home config）
  - 验证：`cargo metadata --no-deps --format-version 1 | …["target_directory"]`
    = `/Users/doracawl/workspace/goliajp/torajs/target`（内置 disk3s5）
- **tr binary 正统路径**（脚本里一律这么取，禁相对 `target/release/tr`）：
  `TR=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')/release/tr`

## 2. ⚠️ 已纠正的错误认知（务必读，否则会重蹈）

**错误**："cache 落内置盘慢，移到外置 INTEL2T 提速"。
**真相（实测 + `cargo-target-dir.md` 原文）**：
- 外置 INTEL2T 做 cargo target **比内置慢 2-3x**（cargo 是大量小文件随机 IO，
  外置 SSD 顺序快但随机慢）。移外置只会更慢。
- **sccache 才是 build 速度的真杠杆**，且**与 target-dir 位置基本无关**——sccache
  全局缓存编译产物，命中即取。实证：空 `<torajs>/target` "从头" release build
  仅 **60.83s real**（正常从头应 5-15min），就是 sccache 命中所致。
- 结论：torajs 切内置 target 的真实收益是**项目隔离 + 无共享 `cargo clean` 互删
  风险 + 去掉共享盘并发争用**（modest），**不是** build 提速（sccache 已覆盖）。
  不要再说"换 target 位置提速 N x"。

## 3. sccache 对 torajs 内循环无关 + 真杠杆 = ship-profile 全量重编（devperf P0 ROOT-CAUSED 2026-05-19）

旧断言"sccache 3285 hits / build 真杠杆 / 动磁盘没用"——**双重错误，已 root-cause**：

- **sccache 是全机全局共享服务**（`sccache --show-stats` 是跨所有项目计数，
  实测时 pid 94050 正为**别的项目** `frz`（INTEL2T shared cache）服务）。
  那个"0% / 3285 hits"都是全局快照、非 torajs 专属信号，无参考价值。
- **结构性**：`Non-cacheable calls 1373`，`Non-cacheable reasons: crate-type`。
  sccache **设计上只缓存 lib/rlib，不缓存 `bin`/proc-macro/build-script**。
  torajs 热重建 = `tr`（torajs-cli **bin**，永不缓存）+ 改 torajs-core 源
  （改了 = genuine miss = **正确**，非 cache 故障）。**sccache 结构上无法
  加速 torajs 迭代内循环**——这不是配置 bug，是 sccache 本质。
- **真杠杆 = profile**。`Cargo.toml [profile.release]` = `opt-level=3,
  lto="fat", codegen-units=1, strip=true`（最大优化 **ship** profile）。
  但 torajs **每次迭代构建都 `--release`**（conformance 建 tr / bench /
  所有 `cargo build`）。**实测：touch torajs-core → 重建 tr = 28.5s**
  （fat-LTO 全程序 + cgu=1 无 codegen 并行 = 最慢）；no-op 0.05s（cargo
  正确跳过）。每次源改付 28.5s × 每会话数百次 = dev-loop 主导成本，一直
  被 sccache 误解掩盖。
- **对产物对**（fat-LTO/cgu=1 给 ship 二进制最高 runtime 性能 = 高性能
  pillar）；**对迭代灾难**。修法 = **独立快速迭代 profile**
  （lto=off / cgu=many / opt 低）用于功能迭代 + conformance（语义与
  opt-level 无关 → 628/0/1 仍证正确性，第一硬规则不破：覆盖不减）；
  **bench + 最终 ship 仍用 fat-LTO release**（bench 必须测真 ship 二进制）。
  见 `optimization-backlog.md` devperf #1 + `metrics.md` §1 重测。
- **教训**：旧 §3 是 metrics-first 之前"凭印象记 ground truth"的反例
  ——一个全局工具的全局快照被当成 torajs 专属结论写进 ground truth。
  今后 build 性能结论必须 (a) 区分全局/项目 (b) 受控实测 (c) 标 provenance。

## 4. conformance gate 的真实成本结构（优化重点）

- ~~`conformance/runner/main.rs:61` 是**纯串行**~~ → **P1 已并行化**
  （`6ab22f9`）：启动 `cargo build` 一次拿 tr 路径，8-worker pool 跑 ~628
  cases，结果原序回放。墙钟 ~30min → ~3min。下面成本结构描述保留作历史/
  并行后第二大成本（tr→SSA→LLVM）参考。
- 每 case 跑 3 子步：① bun run（或 `.expected` 覆盖）② `tr run` ③ `tr build &&
  ./out`，三者 byte-equal 才 pass。
- 实测墙钟：在共享 contended fallback + 机器并发负载下 **~50min**；切 torajs
  内置隔离 target 后 **~30min**（去掉共享盘争用，modest 改善）。仍 ~30min 的
  瓶颈 = **串行**，不是磁盘。
- 对照：`conformance/test262-runner/main.rs` **已有 `--workers N` 并行 pattern**
  （codebase 内已验证可行）——这是 conformance runner 并行化的现成参照。
- 每 case 的 `tr build` 走 torajs 自研 AOT（tr→SSA→LLVM→cc→执行），其中 cc
  部分被 sccache 缓存（C/C++ 1388 hits），但 tr→SSA→LLVM 是 torajs 自己的活，
  628× 跑，**不被 cargo/sccache 覆盖** → 这是并行化之外的第二大成本。

## 4b. ⚠️ 跨天 mac bench run_ms 有系统性环境偏移（实证 2026-05-19）

P7.4-a-b ship 后 3-run bench median 对照 baseline `8b73988`（2026-05-18 测）：
**所有 case 系统性 +15~17%**，**包括 artifact_bytes 逐字节相同的 case**
（fib40/mandelbrot/gcd1m — 机器码 provably 未变）。机器码相同 → +15% run_ms
物理上不可能是真回归 → 偏移源是**环境**：baseline 另一天空闲机测，当前 run
是机器连跑数小时 conformance+build+bench（thermal throttle + 抢占）测的。

**铁律**：
- **artifact_bytes 是唯一可信的回归判据**（确定性、mac-noise-immune；同 tr+同源
  逐字节相同 ⟺ 机器码未变 ⟺ 0 perf regression by construction）。
- **跨天 / 跨机器状态的 mac run_ms 比较不可作回归裁定**——系统性偏移可达 ±15%+，
  单点尖峰可达 +200%+（实测 fib40 单 run 566ms vs 152）。
- run_ms 仅在**同一 idle 机器状态、同次会话内**对比才有限可信；做正式 perf 裁定
  需 `.dev` B1（同机基线 + 统计带）/ B4（机器状态卫生）落地后。
- `-NN B` 量级的 artifact 漂移若在 run 间于不同 case 间跳动 = 良性 linker
  非确定性（非 per-case codegen 变化）；只有**跨 run 稳定**的 artifact delta 才
  追究（如 a-b 的 throw-catch +416B = throw-runtime relink，预期非回归）。

## 5. 关键时间基线（2026-05-19，留作优化前后对比锚点）

| 项 | 墙钟 | 备注 |
|---|---|---|
| 空 target 从头 release build | 60.83s | sccache 命中主导；非真"从头" |
| 增量 no-op build | ~0.16s | 稳态 |
| full conformance（628，串行，内置隔离 target） | ~30min | 优化前基线 |
| full conformance（628，**并行 8 workers**，内置隔离 target） | **~3.0min** | P1 后实测（174–181s ×3，627/0/1，零 flake）；commit `6ab22f9` |
| full conformance（共享 contended fallback） | ~50min | 旧环境，已不再用于 torajs |

后续每做一项优化，在 `optimization-backlog.md` 记新墙钟 + 在 `metrics.md` 更新该指标的 *now* 列 + 对照此表。
