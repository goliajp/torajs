# torajs 构建/缓存环境 — Ground Truth

记录**实测确认**的环境事实，尤其**反直觉**和**已纠正的错误认知**。
后续做优化前先读这里，避免基于错误前提行动。每条都标实测来源。

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

## 3. sccache ⚠️ 假设被实测推翻（hardev v0.1.0 metrics-first 发现，待 root-cause）

- `RUSTC_WRAPPER=/opt/homebrew/bin/sccache`（环境变量，shell profile 设；
  **不在** `~/.cargo/config.toml` 里）。配置存在。
- ~~实测 stats（2026-05-19 早）：12133 requests，3285 hits，工作正常~~
  **此结论被推翻**：hardev v0.1.0 建基线时 `sccache --show-stats` 实测
  **0.00 % 命中率（1284 requests / 85 executed / 0 hits / 85 misses 全 Rust）**。
  早先 3285 hits 的数字与当前 0% 矛盾——可能 stats 被 reset、cache 错 key、
  或早数字来自不同机器状态。**未 root-cause 前，"sccache 是 build 真杠杆"
  这条 `[A]` 不可信**（见 `metrics.md` §0/§1 headline finding）。
- **devperf P0**：build-speed 真杠杆当前**未知**。在 root-cause sccache（为何
  0 命中）之前，不得再声称"动磁盘没用、sccache 已覆盖"——那建立在已被推翻
  的假设上。任何 build 提速决策必须先重测，不沿用本节旧断言。

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
