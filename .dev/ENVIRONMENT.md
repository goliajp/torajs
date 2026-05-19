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

## 3. sccache（build 速度真正的杠杆，已配好）

- `RUSTC_WRAPPER=/opt/homebrew/bin/sccache`（环境变量，shell profile 设；
  **不在** `~/.cargo/config.toml` 里）。
- 实测 stats（2026-05-19）：12133 compile requests，cache hits 3285
  （Rust 1364 / C-C++ 1388 / Asm 533）。工作正常。
- 含义：cargo/rustc/cc 编译重活已被 sccache 缓存；动 target-dir 对 build 墙钟
  影响小。**真要再提 build 速度，调 sccache（命中率/本地缓存大小）比动磁盘有用**。

## 4. conformance gate 的真实成本结构（优化重点）

- `conformance/runner/main.rs:61` 是**纯串行** `for c in &cases`（无 rayon/
  thread/workers）。~628 cases。
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

## 5. 关键时间基线（2026-05-19，留作优化前后对比锚点）

| 项 | 墙钟 | 备注 |
|---|---|---|
| 空 target 从头 release build | 60.83s | sccache 命中主导；非真"从头" |
| 增量 no-op build | ~0.16s | 稳态 |
| full conformance（628，串行，内置隔离 target） | ~30min | 优化前基线；目标见 OPTIMIZATION.md |
| full conformance（共享 contended fallback） | ~50min | 旧环境，已不再用于 torajs |

后续每做一项优化，在 OPTIMIZATION.md 记新墙钟 + 对照此表。
