# checkpoint 2026-09-06 — v1.0 五轴进度汇报

> 触发：takagi explicit ask（"做整体的项目进度汇报，要有百分比和 bench"）。
> 上一份 checkpoint = `2026-06-14-w-j-a3a.md`。所有数字当场取自
> tool 输出（git / sweep json / bench json / roadmap / plan-state），无一凭记忆。

HEAD `fd1bc2a2e`（rotation 598 close），branch `develop`，
7290 commits since 2026-04-26（134 天），rotations.jsonl 902 行。

---

## 1. 整体进度 / 当前位置

v1.0 的 gate 定义（roadmap `### v1.0 release gate`，2026-08-30 版）是
**五条轴**，不是 checklist：

| 轴 | 内容 | 状态 | 完成度 |
|---|---|---|---:|
| A | spec surface（P0–P13 + P-SURF） | **open** — 唯一的倒计时 | **72%** |
| B | perf（bench-tr 全矩阵中位 ≤ 0.33） | **open** — 有数字有缺口 | **73%** |
| C | 自研纯度 / metal | **closed** @ `0d5a8b0` | **100%** |
| D | multi-thread-ready 形态 | v1.0 期只要 framing | **形态成立** |
| E | 全平台 E1–E4 | **open — 未起步** | **0%** |

> 百分比的定义各轴不同，逐轴在下面写明——**没有一个是"感觉"**。
> 三条 cross-cutting track（P-SURF / P-PERF / P-PLAT）并行推进。

**加权合成（等权三条 open 轴，C/D 不计入分母）：约 48%**。
这个数字的诚实之处在于：轴 E 一行代码没写，而它是 gate 的一整条。
若按"工作量"而非"轴数"加权（A 已耗 3 个月、B 是持续 track、E 是
估算 4 步 codegen+linker 工作），composite 落在 **55–65%** 区间——
本报告取**保守口径 48%**，理由是未起步的轴不该按乐观估算折现。

---

## 2. 轴 C — Metal 化 / 全自研（已收口，100%）

| 项 | 实测 |
|---|---|
| workspace crate 数 | **49** |
| **零外部依赖的 crate** | **47 / 49（95.9%）** |
| 有外部 dep 的 crate | `torajs-cli`（LSP + REPL：lsp-server/lsp-types/serde/serde_json/rustyline）、`torajs-playground-api`（web API：axum/tokio/…） |
| metal core（runtime + compiler + toolchain） | **0 外部 dep** |
| C 源文件 | **0**（`find crates -name '*.c'`） |
| LLVM / inkwell / llvm-sys | **0**（Cargo.toml 里只剩描述性注释） |
| Rust LOC（crates/） | **652,027** |

自研栈全貌：`torajs-codegen` 11,178 行（SSA → AArch64 机器码，Linear Scan
寄存器分配 + AAPCS64）→ `torajs-obj` 2,464 行（Mach-O writer）→
`torajs-link` 25,542 行（自研链接器，不调系统 `ld`）；
`torajs-egraph` 25,637 行（优化）；`torajs-core` 241,454 行（前端 + SSA）；
runtime crates 合计 340,442 行。

toolchain 仍 pin nightly-2026-07-10，原因是 `-Z build-std` + 自研
panic-runtime 用的三个 compiler-internal feature——**不是语言层**
（零无条件 `#![feature]`，edition 2024）。

---

## 3. 轴 A — spec surface（72%）

### 3.1 substrate checklist：已 100%，但它不是 gate

P0–P13 的 checkbox **88 / 88 全打勾，0 未打**。roadmap 自己写明这条
定义已被 supersede：*"a gate that a runtime can satisfy while failing
half its corpus is not a gate"*。

### 3.2 真 gate：未归因 ≥4 案例簇的倒计时（终点 0）

| | baseline 2026-07-26 | 现在 2026-09-06 | 降幅 |
|---|---:|---:|---:|
| **未归因 ≥4 簇** | 482 | **135** | **−72.0%** |
| 它们持有的案例 | 25,198 | **1,002** | **−96.0%** |
| core 总数 | 26,476 | **1,827** | **−93.1%** |
| subset-decision register | 空 | 2 条 · 215 案例 | — |
| ≤3 条残尾 | 911 簇 / 1,278 | 451 簇 / 610 | −33% |

**轴 A 完成度取簇数口径 = 72%**（案例口径 96% 偏乐观：剩下的簇个个更硬）。
P-SURF 的 S-group checkbox：**36 / 78 = 46.2%**——它比簇数口径更悲观，
因为 S 组里含 post-v1.0 的登记项。**三个口径 46% / 72% / 96%，
取中位 72% 作为报告口径。**

### 3.3 test262 全量（53,174 case，观察值不是 gate）

@ `550c3b973`，10 workers，**378.5s**，harnessError 0：

```
pass 31,126 · passNoOracle 1,140 · passNegative 4,457
passTotal 36,723 · bug 11,273 · incompatible 5,178 · trAccepted 47,996
in-scope 通过率 69.06%   ·   tr-accepted 通过率 76.51%
```

**通过率曲线**（每个点是一次全量 sweep）：

| 日期 | in-scope % | passTotal |
|---|---:|---:|
| 2026-06-13 | 12.66 | 6,730 |
| 2026-07-14 | 21.74 | 11,560 |
| 2026-07-28 | 34.83 | 18,523 |
| 2026-08-11 | 54.63 | 29,048 |
| 2026-08-23 | 64.87 | 34,493 |
| 2026-09-06 | **69.06** | **36,723** |

85 天从 12.66% → 69.06%（+56.4 个百分点）。

### 3.4 自研 conformance gate

**3,722 pass / 0 fail / 4 skip**（rotation 598 close，spawn-bun 0）。
`conformance/cases/` 3,494 条目（3,354 单文件 + 44 目录形模块 case）。
本轮 +4 fixture，pass 回归 0。

### 3.5 下一个最大缺口（top 5 簇）

① 91 `needs a ModuleSource substrate`（已由 register SR-2 归因）
② 25 `type mismatch assigning to global X`
③ 20 decorator `lex error: unexpected byte 0x40`
④ 20 import-attributes
⑤ 19 类型参数先推 Number 后见 Undefined

簇覆盖率：top-100 **55.8%** / top-200 **74.1%** / top-400 **88.9%**。

---

## 4. 轴 B — Benchmark（73%）

**数据源** `bench/results/2026-09-06-mini-fd1bc2a2e.json`（本报告当场在 mini 上按
HEAD 源码重跑：`release-build.sh` 先跑且 `RB_ERRORS=0`），host mini，`--runs 3`
（3 轮完全交错，行值 = median + MAD），46 cases × 4 runtime，0 stdout mismatch。
对比器版本：**bun 1.4.2** / **rustc 1.99.0-nightly (af3d95584)** / tr 0.1.0。

> **provenance 手工改正**：`bench-harness` 与 598-01 修掉的 test262 runner 有
> **同一个缺陷**——它戳的是「运行目录的 `git rev-parse HEAD`」，而 mini 收代码走
> rsync 不走 pull，那个 HEAD 是 `79d324b18`（rotation 594）。构建输入已用
> `rsync -rcn` 对 `crates/` `scripts/` `Cargo.toml` `rust-toolchain.toml` 四处
> 逐字节验证与 dev HEAD 相同，故入库时改戳 `fd1bc2a2e`。**登记未修**：给
> bench-harness 加 `--head-sha` + dirty 标记，与 598-01 同形。

### 4.1 headline

| 指标 | 值 | 轴 B 目标 |
|---|---:|---:|
| **中位 tr / bun-aot** | **0.5107**（≈ 1.96× 快） | **≤ 0.33（3×）**，追 0.25（4×） |
| 几何均值 tr / bun-aot | **0.4309** | — |
| **比 bun 快的 cell** | **45 / 46（97.8%）** | — |
| 比 bun 慢的 cell | 1（`map-str-100k` 1.47×） | 0 |
| 中位 tr / rust | **1.072**（23 个可比 cell） | — |
| 中位 tr / tr-run（解释器） | 0.134（AOT 快 7.5×） | — |

**完成度算法**：从 parity（1.00）走到目标（0.33），已走
`(1.00 − 0.5107) / (1.00 − 0.33)` = **73.0%**。剩余需再快 **1.55×**。

**与前一轮的一致性**：09-05 @ `66edba5`（38 个 commit 之前）中位 0.511 / 几何
均值 0.4326 / 45-46 快。今天 0.5107 / 0.4309 / 45-46 —— **两次独立全矩阵读数
逐项落在噪声带内**，坐实那 38 条窄修没有 perf 侧影响；同时 bun 自己从 1.4.1
升到 1.4.2 也没有移动对位。
P-PERF 自己的投影（S1-A2 + S2 + S6 + S7 全落地）是 0.442，**仍不够**——
所以这条轴的标准模式是 decomposition，不是 polish。

### 4.2 产物尺寸与启动（tr 的差异化面，两项都已远超目标）

| | tr | bun-aot | rust |
|---|---:|---:|---:|
| 产物中位字节 | **51,677** | 62,226,930 | — |
| 倍数 | — | **1,204× 大于 tr** | — |
| `startup` 产物 | **18,473 B** | 62,226,930 B（3,369×） | 469,216 B |
| `startup` run_ms | **0.921** | 3.733 | 1.125 |
| 编译中位 ms | **52.9** | 70.0 | — |

**tr 的冷启动比 Rust 原生二进制还快（0.921 vs 1.125 ms）**，产物比 bun
小三个数量级——这两条是 torajs 的立项差异化，现在是全矩阵实测值。

### 4.3 最快 / 最慢

**最快 8**：popcount 0.044 · throw-catch 0.113 · async-fn-call 0.229 ·
generic-pair-1m 0.236 · startup 0.247 · array-any-indexed 0.252 ·
fifo-queue 0.254 · promise-await 0.256

**最慢 8**：csv-rebuild 0.627 · csv-trim 0.634 · json-stringify 0.637 ·
str-replace 0.757 · gcd1m 0.894 · mandelbrot 0.934 · prime_count 0.963 ·
**map-str-100k 1.470（唯一输的 cell）**

形状读法：**输的全是纯数值紧循环**（prime_count / mandelbrot / gcd —— 这里
bun 的 JIT 已经把它们编成机器码，我们和它比的是同一件事）与 **Map 字符串
键**（唯一真输，是 hashmap 面的债）。**赢得最多的是 bun 的 JIT 够不到的
地方**：启动、异常、Promise/async、泛型单态化、闭包。

---

## 5. 轴 D / 轴 E

**轴 D（multi-thread-ready）**：v1.0 期只要求形态而非实现——refcount emit
一律走 helper、新 global state 走 `thread_local!` / `Atomic*`、heap header
预留 owner-thread 位、helper 签名不假设单 mutator。真 biased-ARC 切换是
P16（post-v1.0）。**形态成立，无未完成项。**

**轴 E（全平台）= 0%**，五步一步未起：

- **E0** 格式抽象 + target triple 贯通（今天 `aarch64-apple-darwin` 在 5 处硬编码）
- **E1** `aarch64-unknown-linux-gnu`（ELF64 writer + Linux syscall trampoline）
- **E2** `x86_64-unknown-linux-gnu`（**codegen 工作量最大的一步**）
- **E3** `x86_64-apple-darwin`（成本应接近零，是 E0 抽象质量的验收）
- **E4** `x86_64-pc-windows-msvc`（PE-COFF + MS x64 ABI + import table）

绑定面已实测：`torajs-link` 78 文件中 **56 个命中** Mach-O 特征，是唯一
必须先做抽象才能动的地方；`torajs-codegen` 的 4,096 行寄存器分配 / liveness /
frame **算法层可复用**，只需参数化寄存器集与调用约定；`torajs-syscall/safe.rs`
已格式无关。结构优势：**自研链接器意味着交叉编译不需要目标平台 toolchain**。

---

## 6. 一句话结论

**已经做完的是"难而深"的部分**（自研全栈 100%、spec 面 72%、perf 领先
bun 1.96× 且产物小 1206×）；**没做的是"宽"的部分**（四个平台、一行未写）。
v1.0 的真实距离不是 test262 的 30 个百分点，是 **135 个簇 + 1.55× perf +
4 个平台**。

---

## 附：trend 表新行（同步到 `README.md`）

| date | HEAD | roadmap phase | conformance gate | test262 in-scope % | bench median tr/bun-aot | tr artifact (MB) | workspace crates | metal-core ext dep | total LOC |
|------|------|---------------|------------------|--------------------|---|---:|---:|---:|---:|
| 2026-09-06 | `fd1bc2a2e` | P0–P13 closed · P-SURF / P-PERF / P-PLAT 并行 | 3722/0/4 | 69.06% (36723/53174) | median **0.5107** · geomean 0.4309 · 45/46 快于 bun | 0.049（中位 51,677 B） | 49 | 0（cli + playground-api 隔离 scope） | 652,027 |
