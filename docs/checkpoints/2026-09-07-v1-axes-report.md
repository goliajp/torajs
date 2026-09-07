# checkpoint 2026-09-07 — v1.0 五轴进度汇报

> 触发：takagi explicit ask（"做整体的项目进度汇报，要有百分比和 bench"）。
> 上一份 = `2026-09-06-v1-axes-report.md`（一天前，rotation 598）。所有数字
> 当场取自 tool 输出（git / sweep json / 当轮实跑的 bench / roadmap），无一凭记忆。
> **本轮 bench 是当场在 mini 上按 HEAD 源码重跑的**，不是引用昨天的读数。

HEAD `48e7eab7a`（rotation 610 close），branch `develop`，
7371 commits since 2026-04-26（134 天），rotations.jsonl 914 行。

---

## 1. 整体进度 / 当前位置

v1.0 的 gate（roadmap `### v1.0 release gate`，2026-08-30 版）是**五条轴**：

| 轴 | 内容 | 状态 | 完成度 | Δ vs 09-06 |
|---|---|---|---:|---:|
| A | spec surface（P0–P13 + P-SURF） | **open** — 唯一有倒计时的 | **73.4%** | +1.4pp |
| B | perf（全矩阵中位 tr/bun-aot ≤ 0.33） | **open** — 有数字有缺口 | **73.2%** | +0.2pp |
| C | 自研纯度 / metal | **closed** @ `0d5a8b0` | **100%** | = |
| D | multi-thread-ready 形态 | v1.0 期只要 framing | **形态成立** | = |
| E | 全平台 E1–E4 | **open — 未起步** | **0%** | = |

**加权合成（等权三条 open 轴，C/D 不计入分母）：48.9%**（09-06：48%）。
口径与上一份一致，逐轴定义写在各节里——没有一个是"感觉"。
保守之处仍是同一条：**轴 E 一行代码没写，而它是 gate 的一整条**。

一天之内（rotation 599–610，共 81 个 commit）的形状：**+1.4pp 全在轴 A**，
而轴 A 的这 1.4pp 里含一次**去水分的诚实回撤**，见 3.3。

---

## 2. 轴 C — Metal 化 / 全自研（已收口，100%）

| 项 | 实测 |
|---|---|
| workspace crate 数 | **49** |
| **零外部依赖的 crate** | **47 / 49（95.9%）** |
| 有外部 dep 的 crate | `torajs-cli`（LSP + REPL，5 个）、`torajs-playground-api`（web API，12 个） |
| metal core（runtime + compiler + toolchain） | **0 外部 dep** |
| C 源文件 | **0** |
| LLVM / inkwell / llvm-sys | **0** |
| Rust LOC（crates/） | **654,599**（09-06：652,027，+2,572） |

自研栈：`torajs-codegen` 11,178 行（SSA → AArch64 机器码，Linear Scan + AAPCS64）
→ `torajs-obj` 2,464（Mach-O writer）→ `torajs-link` 25,542（自研链接器，不调
系统 `ld`）；`torajs-egraph` 25,637（优化）；`torajs-core` 243,719（前端 + SSA）。
toolchain 仍 pin `nightly-2026-07-10`，原因是 `-Z build-std` + 自研 panic-runtime
的三个 compiler-internal feature——**不是语言层**（零无条件 `#![feature]`，edition 2024）。

---

## 3. 轴 A — spec surface（73.4%）

### 3.1 substrate checklist：88 / 88 全打勾，但它已不是 gate

roadmap 自己写明这条定义被 supersede：*"a gate that a runtime can satisfy while
failing half its corpus is not a gate"*。

### 3.2 真 gate：未归因 ≥4 案例簇的倒计时（终点 0）

| | baseline 2026-07-26 | 09-06 | **09-07** | 自 baseline 降幅 |
|---|---:|---:|---:|---:|
| **未归因 ≥4 簇** | 482 | 135 | **128** | **−73.4%** |
| 它们持有的案例 | 25,198 | 1,002 | **931** | **−96.3%** |
| core 总数 | 26,476 | 1,827 | **1,723** | **−93.5%** |
| subset-decision register | 空 | 2 条 · 215 | 2 条 · **175** | — |
| ≤3 条残尾 | 911 簇 / 1,278 | 451 / 610 | **455 / 617** | −33% |

**轴 A 完成度取簇数口径 = 73.4%**。另两个口径：P-SURF 的 S-group checkbox
**36 / 78 = 46.2%**（更悲观，含 post-v1.0 登记项）、案例口径 **96.3%**（更乐观，
剩下的簇个个更硬）。**三口径 46% / 73% / 96%，取中位 73.4% 报告**，与上一份同法。

### 3.3 test262 全量（53,174 case，观察值不是 gate）

@ `8ffa78b99`，10 workers，**436s**，harnessError 0：

```
pass 30,991 · passNoOracle 1,174 · passNegative 4,459
passTotal 36,624 · bug 11,488 · incompatible 5,062 · trAccepted 48,112
in-scope 通过率 68.88%   ·   tr-accepted 通过率 76.12%
tr 接受率 48,112/53,174 = 90.48%
```

**★ 这个数比 09-06 的 69.06% 低 0.18pp，必须逐段归因——它不是回归，是去水分。**
（每个点取自 `git cat-file blob <sha>:hardev/test262-latest.json`，机械可复核）

| rotation | HEAD | passTotal | Δ | 性质 |
|---|---|---:|---:|---|
| 598 | `550c3b973` | 36,723 | — | 上一份 checkpoint 的报数 |
| 602 | `3e91a45c8` | 36,729 | +6 | |
| 603 | `9043b3dfb` | 37,121 | **+392** | 其中 **+390 当轮自查为水分** |
| 604 | `cfd371d5c` | 36,494 | **−627** | **修 harness 盲区 → 诚实 re-baseline** |
| 605 | `573d37b17` | 36,496 | +2 | |
| 606 | `3bffb6abc` | 36,518 | +22 | |
| 607 | `7b29ca5b5` | 36,549 | +31 | |
| 608 | `f3ee3cdc5` | 36,605 | +56 | |
| 609 | `70bdac732` | 36,621 | +16 | |
| 610 | `8ffa78b99` | **36,624** | +3 | |

水分的形状（设计原则 §5 no-metric-inflation）：runner 的 transform 把
`assert.throws(TypeError, fn)` 改写时**丢掉期望的错误类**，于是用到 tr 未实现全局
（Temporal / Intl / SharedArrayBuffer）的 case 里回调抛 ReferenceError，harness 只问
"抛了没有"就报 pass——bun 抛的是真 TypeError，**两边 stdout 都空、exit 都 0，oracle
也分不出来**。rotation 603 抽样 40 条用 `--dump-src` 导出真源码逐条跑，**39/39 全是水**；
604-01 修好 harness（类身份比较早已能做，那句注释过时了），数字当场掉 627。

**扣掉去水分之后：604 → 610 六轮真实前进 +130，pass 回归 0。**
把 598 的 36,723 与今天的 36,624 直接相减得到的 "−99" 是**两个不同尺子的读数之差**，
不构成回归——这正是 IRON RULE 要求把 passTotal 与成分分开报的原因。

**通过率曲线**（每点一次全量 sweep；09-07 之后的点与之前**不同尺**，已标注）：

| 日期 | in-scope % | passTotal | 尺子 |
|---|---:|---:|---|
| 2026-06-13 | 12.66 | 6,730 | 旧 harness |
| 2026-07-14 | 21.74 | 11,560 | 旧 harness |
| 2026-07-28 | 34.83 | 18,523 | 旧 harness |
| 2026-08-11 | 54.63 | 29,048 | 旧 harness |
| 2026-08-23 | 64.87 | 34,493 | 旧 harness |
| 2026-09-06 | 69.06 | 36,723 | 旧 harness（含 ~627 水） |
| **2026-09-07** | **68.88** | **36,624** | **修正后 harness** |

134 天从 0 → 68.88%（其中 85 天从 12.66% → 68.88%）。

### 3.4 自研 conformance gate

**3,773 pass / 0 fail / 4 skip**（rotation 610 close，spawn-bun 0；09-06：3,722/0/4，
**+51**）。`conformance/cases/` 3,549 条目（3,400 `.ts` + 79 `.cts` + 44 目录形模块 case + 26 expected）。

### 3.5 下一个最大缺口（top 5 簇 @ `8ffa78b99`）

① **91** `needs a ModuleSource substrate`（已由 register SR-2 归因，92 条）
② **20** decorator `lex error: unexpected byte 0x40`（提案语法）
③ **20** import-attributes（提案语法）
④ **19** 类型参数先推 Number 后见 Undefined（真语义缺口）
⑤ **18** `not callable: type Object("X")`（真语义缺口）

另有 **19** 条 spread 的 `box_to_any element type FnSig`——根因与登记边界都已定位，
是可直接动手的一条。簇覆盖率：top-100 **55.0%** / top-200 **73.4%** / top-400 **88.5%**。

### 3.6 test262 数字看不见的那一面（连续两轮）

rotation 609 + 610 真正移动的面是一张 **162 格接收者 × 键类型矩阵**：与 bun 不一致
的格子 **83 → 25 → 12**，而移动的 71 格里大多数在 test262 里根本没有对应 case
（`"ab"[undefined]` / `m["a"]` 的读写 / `({a:1})[sym]`）。剩下 12 格中 **9 格已用
`tsc --noEmit --strict` 查证为有意分歧**（TS2542 无条件），不是缺口。
**这类面只有把矩阵铺开逐格对才看得见，gate 与 sweep 都看不见。**

---

## 4. 轴 B — Benchmark（73.2%）

**数据源**：`bench/results/2026-09-07-mini-48e7eab7a.json`——**本报告当场在 mini 上
重跑**（`cargo run -p bench-harness --release -- run`，rc=0），host mini，46 cases ×
4 runtime，**0 stdout mismatch**。对比器：**bun 1.4.2** / **rustc 1.99.0-nightly
(af3d95584)** / tr 0.1.0。

> **两条 provenance 声明（不藏）**：①`bench-harness` 与 598-01 修掉的 test262 runner
> 有同一个缺陷——戳的是运行目录的 `git rev-parse HEAD`，而 mini 收代码走 rsync，
> 那个 HEAD 是 `79d324b18`。构建输入已用 `rsync -rcn` 对 `crates/` `scripts/`
> `bench/cases/` `bench/harness/` `Cargo.toml` `rust-toolchain.toml` **六处逐字节
> 验证与 dev HEAD 相同**，故入库时改戳 `48e7eab7a`；给 bench-harness 加 `--head-sha`
> 仍是登记中的 follow-up。②**本轮 mini 被另一个项目争用**（spg-server 全程 ~72% 一核），
> 且 harness 层 `runs=1`（hyperfine 仍 warmup 3 + runs 10）。**所以绝对 run_ms 跨天
> 不可比，可比的是同轮内自归一化的 tr/bun-aot 比值**——而轴 B 的 gate 恰恰就是那个比值。

### 4.1 headline

| 指标 | 09-06（runs=3，空机） | **09-07（runs=1，争用）** | 轴 B 目标 |
|---|---:|---:|---:|
| **中位 tr / bun-aot** | 0.5107 | **0.5096（≈ 1.96× 快）** | **≤ 0.33（3×）**，追 0.25（4×） |
| 几何均值 tr / bun-aot | 0.4309 | **0.4317（≈ 2.32× 快）** | — |
| **比 bun 快的 cell** | 45 / 46 | **45 / 46（97.8%）** | 46 / 46 |
| 比 bun 慢的 cell | 1（map-str 1.47×） | **1（map-str 1.41×）** | 0 |
| 中位 tr / rust | 1.072 | **1.038**（23 个可比 cell） | — |
| AOT / 解释器 | 7.5× | **7.53×** | — |

**两次独立读数——不同日、不同机器负载、不同 harness runs 参数——中位差 0.0011、
几何均值差 0.0008，全部落在噪声带内。** 这同时证实了两件事：81 个 commit 的
checker/parser 改动对 perf 侧零影响（本来就该如此，它们都在编译期）；以及自归一化
比值确实抗机器状态漂移（rotation 211 教训的正面兑现）。

**完成度算法**（沿用上一份）：从 parity（1.00）走到目标（0.33），已走
`(1.00 − 0.5096) / (1.00 − 0.33)` = **73.2%**。**剩余需再快 1.54×。**
P-PERF 自己的投影（S1-A2 + S2 + S6 + S7 全落地）是 0.442，**仍不够**——所以这条轴的
标准模式是 decomposition，不是 polish。

### 4.2 代表性 cell（当轮实测，ms）

| case | tr | bun-aot | tr/bun | rust |
|---|---:|---:|---:|---:|
| startup | **1.153** | 4.395 | **0.262** | 1.316 |
| popcount | **2.533** | 53.259 | **0.048** | 2.664 |
| throw-catch-100k | **1.549** | 14.518 | **0.107** | 440.375 |
| async-fn-call-100k | **2.002** | 9.827 | **0.204** | — |
| generic-pair-1m | **2.014** | 8.633 | **0.233** | 2.347 |
| array-sum-1m | **13.586** | 42.861 | **0.317** | 13.194 |
| class-method-100k | **3.013** | 7.099 | **0.424** | — |
| closure-pipeline-1m | **18.638** | 43.797 | **0.426** | 17.525 |
| ackermann | **7.924** | 14.330 | **0.553** | 8.962 |
| regex-dfa-anchored-100k | **5.642** | 10.102 | **0.559** | — |
| promise-then-100k | **5.652** | 10.003 | **0.565** | — |
| array-sort-100k | **7.000** | 12.240 | **0.572** | — |
| fib40 | **244.968** | 400.580 | **0.612** | 193.255 |
| json-stringify-100k | **6.179** | 9.848 | **0.627** | — |
| str-replace-100k | **8.324** | 11.190 | **0.744** | — |
| mandelbrot | **44.475** | 47.442 | **0.937** | 35.763 |
| prime_count | **49.713** | 50.849 | **0.978** | 49.429 |
| **map-str-100k** | 15.527 | **11.020** | **1.409** ← 唯一输 | 5.835 |

形状读法未变：**输的全是纯数值紧循环**（prime_count / mandelbrot / gcd——bun 的 JIT
已把它们编成机器码，比的是同一件事）与 **Map 字符串键**（唯一真输，hashmap 面的债，
对 rust 也输 2.66×，是该面自身的问题不是 JIT 的问题）。**赢得最多的是 JIT 够不到的
地方**：启动、异常、Promise/async、泛型单态化、闭包。

### 4.3 产物尺寸与启动（tr 的差异化面，两项都已远超目标）

| | tr | bun-aot | rust |
|---|---:|---:|---:|
| 产物中位字节 | **51,677** | 62,226,930（**1,204× 大**） | — |
| `startup` 产物 | **18,473 B** | 62,226,930 B（**3,369×**） | 469,216 B |
| `startup` run_ms | **1.153** | 4.395 | 1.316 |
| 编译中位 ms | **60.1** | 69.6 | — |

**tr 的冷启动仍比 Rust 原生二进制快**（1.153 vs 1.316 ms），产物比 bun 小三个数量级。
这两条是立项时的差异化主张，现在是全矩阵实测值。

---

## 5. 轴 D / 轴 E

**轴 D（multi-thread-ready）**：v1.0 期只要形态——refcount emit 一律走 helper、
新 global state 走 `thread_local!` / `Atomic*`、heap header 预留 owner-thread 位、
helper 签名不假设单 mutator。真 biased-ARC 切换是 P16（post-v1.0）。**形态成立，
无未完成项。**

**轴 E（全平台）= 0%**，五步一步未起，与 09-06 逐字相同：

- **E0** 格式抽象 + target triple 贯通（今天 `aarch64-apple-darwin` 在 5 处硬编码）
- **E1** `aarch64-unknown-linux-gnu`（ELF64 writer + Linux syscall trampoline）
- **E2** `x86_64-unknown-linux-gnu`（**codegen 工作量最大的一步**）
- **E3** `x86_64-apple-darwin`（成本应接近零，是 E0 抽象质量的验收）
- **E4** `x86_64-pc-windows-msvc`（PE-COFF + MS x64 ABI + import table）

绑定面实测：`torajs-link` 78 文件中 **56 个命中** Mach-O 特征，是唯一必须先做抽象
才能动的地方；`torajs-codegen` 的 4,096 行寄存器分配 / liveness / frame **算法层可复用**。
结构优势：**自研链接器意味着交叉编译不需要目标平台 toolchain**。

---

## 6. 一句话结论

一天 81 个 commit 的净效果：**轴 A +1.4pp，轴 B 持平（两次独立读数互证），
轴 E 未动**。这一天真正值钱的不是 +1.4pp，是**把一把有 627 条水分的尺子换掉了**——
在它上面量出来的每一个未来读数才算数。

v1.0 的真实距离不是 test262 的 31 个百分点，是
**128 个簇 + 1.54× perf + 4 个平台**。

---

## 附：trend 表新行（同步到 `README.md`）

| date | HEAD | roadmap phase | conformance gate | test262 in-scope % | bench median tr/bun-aot | tr artifact (MB) | workspace crates | metal-core ext dep | total LOC |
|------|------|---------------|------------------|--------------------|---|---:|---:|---:|---:|
| 2026-09-07 | `48e7eab7a` | P0–P13 closed · P-SURF / P-PERF / P-PLAT 并行 | 3773/0/4 | 68.88% (36624/53174) · **harness 去水分后的新尺** | median **0.5096** · geomean 0.4317 · 45/46 快于 bun | 0.049（中位 51,677 B） | 49 | 0 | 654,599 |
