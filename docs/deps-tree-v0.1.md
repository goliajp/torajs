# torajs deps tree — v0.1

> 架构文档**只写真实**：已 ship 写细 · 已规划写到 L2 · 模糊只 sketch。
> 任何热化要回 [§L4](#l4--coldhot-trigger) 判定式，不靠感觉。
> [`architecture-rewrite.md`](architecture-rewrite.md) 是 **A 族 substrate** 的权威定义；
> 本文档是上层全景。两份冲突时以那份为准。

---

## L1 — 终态愿景

torajs 终态约 **40-50 个 crate**，分 **5 族**，按**石头 / 水泥** 二分对应**开源 / 闭源**；当前 A 族 ship 5/15 (+1 in progress)，其他族仍在 monolith 状态。

---

## Glossary

| 术语 | 含义 |
|---|---|
| **5 族** | torajs 全部 crate 按用途分的 5 个大类 (A/B/C/D/E)。族内严格 ↑ DAG；族间通过 runtime-glue 桥接 |
| **6 layer** | **仅 A 族内部**的层次划分 (L0–L5)。Layer-N 只能依赖 Layer-(N-1) 或更低；不允许同层循环 |
| **石头** | 算法通用 · 别的 PL 可复用 · ABI 不锁 tr 内部约定 → 走 crates.io · Apache-2.0 OR MIT 双许可 |
| **水泥** | tr 特化 · 依赖 SSA naming `__torajs_*` 或 type tag layout → workspace 内部 · 不 publish |
| **emit-call** | B 族 codegen 在 LLVM IR 里直接生成的 `extern "C"` 函数调用，调到 A / C 族的 `__torajs_*` symbol。不走 Cargo dep |
| **status** | ✅ ship · 🟡 in progress · ⏳ queued (已规划, L3b) · ⬜ sketch only (未规划) |
| **Tier (deps)** | 见 [§Deps 审计](#deps-审计)。Tier 0 = 已 carved out / Tier 1 = host-only / Tier 2 = cloud-only / **Tier 3 = runtime, 硬约束 0 deps** |

---

## L2 — 当前真实拓扑

### 总 DAG (族间)

```
                  ┌─── E. Embed + Cloud + 内部工具 ───┐
                  │  torajs-embed                       │
                  │  torajs-playground-api              │
                  │  torajs-test262-runner              │
                  └────────────────┬────────────────────┘
                                   │ Cargo dep ↑
                  ┌────────────────┴────────────────────┐
                  │  D. Toolchain (闭源水泥)             │
                  │  torajs-cli   torajs-lsp             │
                  │  torajs-repl  torajs-formatter       │
                  │  torajs-linter   torajs-bundle       │
                  │  torajs-pkg   torajs-test            │
                  └────────────────┬────────────────────┘
                                   │ Cargo dep ↑
                  ┌────────────────┴────────────────────┐
                  │  B. Compiler pipeline (闭源水泥)     │
                  │  torajs-link ↑ codegen-{llvm,cranelift} │
                  │  ↑ torajs-lower ↑ torajs-ssa         │
                  │  ↑ torajs-check ↑ torajs-types       │
                  │      (横挂 torajs-modules)            │
                  │  ↑ torajs-parser ↑ torajs-ast        │
                  │  ↑ torajs-lexer ↑ torajs-tokens      │
                  └────────────┬─────────┬──────────────┘
                               │  ⇢ emit-call __torajs_*
                  ┌────────────┘         └─────────────┐
                  ▼                                     ▼
       ┌─────────────────────┐         ┌──────────────────────────────────┐
       │  C. Stdlib / Web    │         │  A. Runtime substrate            │
       │     surface (开源)  │         │  (architecture-rewrite.md 权威)  │
       │                     │         │                                   │
       │  ⬜ json · text ·   │         │  L5: promise · regex · fetch     │
       │     url · blob ·    │         │  L4: microtask                   │
       │     stream · crypto │         │  L3: arr · dynobj · collections  │
       │     net · http · ws │         │       · cycle                    │
       │     fs · process ·  │         │  L2: str · num · bigint          │
       │     time            │         │  L1: rc · anyvalue · throw · ucd │
       │  (12 crate sketch)  │         │  L0: pool                        │
       └─────────────────────┘         └──────────────────────────────────┘

                      ──── 旁系 ────
            torajs-runtime (闭源水泥, 不发 crates.io)
            build-time staticlib glue —— 把 A + C 族 .a 串成
            单 link unit 给 tr build 用; 当前还是 12 C-file
            include_str! host, P7 收尾才缩为 thin shim

                      ──── 图例 ────
            ↑   Cargo dep 方向 (build-time)
            ⇢   emit-call (codegen 在 IR 里生成 extern "C" 调用)
```

### A 族 — Runtime substrate (15 crate · ship 5/15 + 1 in progress)

**权威定义在 [`architecture-rewrite.md`](architecture-rewrite.md)**。本节只列 status + 当前度量 + commit hash + deps，不重复 scope 描述。

**石头 / 水泥处理（实现方式待定）**：每个 substrate crate 内部 `core` (石头) + `ffi` (水泥薄壳) 拆 module。
- `core` mod = pure Rust pub fn over `&[u8]` / `&mut HeapHeader` 等通用 type
- `ffi` mod = `extern "C" __torajs_*` wrapper，build staticlib 给 tr 用
- **publish 实现方式** 还未决断 — 候选：(a) feature flag `tora-ffi` 控制 extern 暴露；(b) `torajs-<name>-ffi` 独立 sub-crate 依赖 `torajs-<name>`。第一个 crate 真正 publish 时（P7 前后）才决定，不预先规定。

#### Layer 0 — allocator

| crate | status | commit | LOC | deps |
|---|---|---|---|---|
| `torajs-pool` | ✅ | `de39cc1` (P1) | — | — (no_std) |

#### Layer 1 — foundation

| crate | status | commit | LOC | deps |
|---|---|---|---|---|
| `torajs-rc` | ✅ | `a446c1a` (P2.2) | 734 | — (no_std + alloc) |
| `torajs-anyvalue` | ✅ | `90d4757` (P2.3-d.4) | 1981 | `torajs-rc` |
| `torajs-throw` | ✅ | `6bec2b8` (P2.4-b) | 457 | `torajs-anyvalue` (逻辑依赖; Cargo 实际无 dep — throw 用 raw u8 tag, 不 import anyvalue type) |
| `torajs-ucd` | ✅ | `cba6a55` (P2.1) | — | — (pure data) |

#### Layer 2 — primitives

| crate | status | commit | LOC | deps |
|---|---|---|---|---|
| `torajs-str` | 🟡 P3.1-{a..d} done (4/7) | `587407e` | ~2000 (8 mod) | `torajs-pool` · `torajs-rc` |
| `torajs-num` | ⏳ P3.2 queued | — | — | `torajs-anyvalue` · `torajs-throw` |
| `torajs-bigint` | ⏳ P3.3 queued | — | — | `torajs-rc` · `torajs-throw` |

torajs-str 现有子模块 (in P3.1-{a..d}): layout / pool / alloc / substr / eq / to_number / lookup。
待 sub-step (P3.1-{e,f,g}): transform / split / print + IR-side 收口。
runtime_str.c LOC: baseline 5165 → 当前 4891 (−274) → 目标 0。

#### Layer 3 — containers

| crate | status | source | deps |
|---|---|---|---|
| `torajs-arr` | ⏳ P4.x | `runtime_str.c` arr* | `torajs-pool` · `torajs-rc` |
| `torajs-dynobj` | ⏳ P4.x | `runtime_str.c` dynobj* | `torajs-pool` · `torajs-rc` · `torajs-anyvalue` · `torajs-str` |
| `torajs-collections` | ⏳ P4.x | `runtime_map.c` (971) + Weak* | `torajs-dynobj` · `torajs-rc` |
| `torajs-cycle` | ⏳ P4.x | `runtime_cycle.c` (510) | `torajs-rc` |

#### Layer 4 — dispatch

| crate | status | source | deps |
|---|---|---|---|
| `torajs-microtask` | ⏳ P5.x | `runtime_promise.c` microtask 部分 | `torajs-pool` |

#### Layer 5 — surface

| crate | status | source | deps |
|---|---|---|---|
| `torajs-promise` | ⏳ P6.x | `runtime_promise.c` 上层 (~1000) | `torajs-pool` · `torajs-rc` · `torajs-microtask` · `torajs-throw` |
| `torajs-regex` | ⏳ P6.x | `runtime_regex.c` (3059) | `torajs-rc` · `torajs-anyvalue` · `torajs-ucd` · `torajs-str` |
| `torajs-fetch` | ⏳ P6.x (feature) | `runtime_fetch.c` | `torajs-rc` · `torajs-str` |

### 旁系 — `torajs-runtime` (闭源水泥, 不发 crates.io)

**当前形态**：12 个 C 文件 + lib.rs (95 LOC, 全是 `include_str!`)。`torajs-core::ssa_inkwell` 在 `tr build` 时调 `SOURCES` const 把 C 写进 temp dir, `cc -c` 每个再 link。

**目标形态 (P7 收尾)**：thin shim (~200 LOC)，把 A + C 族 staticlib 串成单 link unit 暴露给 B 族 codegen；删掉所有 `runtime_*.c`；SOURCES const 清空。

### B 族 — Compiler pipeline (~12 crate · ⬜ 当前 monolith)

**当前真实状态**：1 个 `torajs-core` 单体，10 个 .rs 文件总 65 k LOC。拆分待 [§L4 trigger 6](#l4--coldhot-trigger) (A 族全 closed) 命中。

| 计划 crate | 当前来源 | LOC | deps | 备注 |
|---|---|---|---|---|
| `torajs-tokens` | (从 lexer.rs 抽) Token enum + Span | ~100 | — | parser + lexer 共用 |
| `torajs-lexer` | `lexer.rs` | 1198 | `torajs-tokens` | |
| `torajs-ast` | `ast.rs` | **11546** ⚠️ god | `torajs-tokens` | per AST node 拆 mod |
| `torajs-parser` | `parser.rs` | **7134** ⚠️ god | `torajs-lexer` · `torajs-ast` | per grammar section 拆 mod |
| `torajs-types` | (从 check.rs 抽) Type 代数 | ~500 | `torajs-ast` | 子类型 / 联合 / 交集 |
| `torajs-modules` | `modules.rs` | 250 | `torajs-ast` | multi-file resolve |
| `torajs-check` | `check.rs` | **8199** ⚠️ god | `torajs-types` · `torajs-modules` | per check-phase 拆 mod |
| `torajs-ssa` | `ssa.rs` | 1175 | `torajs-types` | SSA IR + builder |
| `torajs-lower` | `ssa_lower.rs` | **28790** ⚠️⚠️ 最大 god | `torajs-ast` · `torajs-ssa` | per stmt-kind 拆 mod |
| `torajs-codegen-llvm` | `ssa_inkwell.rs` | 4722 | `torajs-ssa` | SSA → LLVM IR via inkwell |
| `torajs-codegen-cranelift` | (未存在) | — | `torajs-ssa` | 后续 JIT backend (stdlib.md L33) |
| `torajs-link` | (从 ssa_inkwell + main.rs 抽 cc-link 驱动) | ~200 | `torajs-codegen-*` | object → executable |

**石头 / 水泥**：B 族**默认全水泥**（依赖 tr SSA naming + tag layout）。理论上 lexer/parser/ast 可做成通用 TS frontend (类比 swc/tree-sitter)，但留闭源作护城河。

### C 族 — Stdlib / Web surface (~12 crate · ⬜ 仅 sketch)

**当前真实状态**：[`stdlib.md`](stdlib.md) 描述 hardcoded-in-check.rs 模型，已 ship: `console.log` + `Math.{sqrt,abs,floor,ceil,log,exp,pow,min,max,PI,E}` + `String.length`。Web/Bun-equivalent surface 整族**未规划**——只知道目标对标 bun。

**仅 sketch**（此表是 strawman 不是 plan；真 surface 切分要等到第一批用户 case 才能确定）：

| crate | scope | 石头/水泥 | deps |
|---|---|---|---|
| `torajs-json` | JSON.parse / stringify | 石头 | `torajs-str` · `torajs-arr` · `torajs-dynobj` |
| `torajs-text` | TextEncoder / Decoder / atob / btoa | 石头 | `torajs-str` · `torajs-ucd` |
| `torajs-url` | URL / URLSearchParams | 石头 | `torajs-str` |
| `torajs-blob` | Blob / File / FormData | 石头 | `torajs-str` · `torajs-arr` |
| `torajs-stream` | ReadableStream / Writable / Transform | 石头 | `torajs-promise` · `torajs-blob` |
| `torajs-crypto` | WebCrypto.subtle + node:crypto 子集 | 石头 | `torajs-rc` · `torajs-promise` |
| `torajs-net` | TCP / UDP socket (node:net) | 石头 | `torajs-promise` |
| `torajs-http` | HTTP/1.1+2 client + server | 石头 | `torajs-net` · `torajs-stream` |
| `torajs-ws` | WebSocket | 石头 | `torajs-http` |
| `torajs-fs` | 文件 I/O (node:fs) | 石头 | `torajs-str` · `torajs-promise` |
| `torajs-process` | spawn / exec (node:child_process) | 石头 | `torajs-str` · `torajs-promise` |
| `torajs-time` | Date / Temporal subset | 石头 | — |

### D 族 — Toolchain (~8 crate · 当前 cli 单 bin)

**当前真实状态**：`torajs-cli` 1 个 binary crate，4 个 source: `main.rs` (971) / `lsp.rs` (607) / `lsp_bench.rs` (375) / `repl.rs` (354)。

| crate | source | deps | 抽出 trigger |
|---|---|---|---|
| `torajs-cli` | `cli/src/main.rs` | 全 B + 全 D 其他 | — (永远是顶 bin) |
| `torajs-lsp` | `cli/src/lsp.rs` + `lsp_bench.rs` | (拆后) `torajs-check` · `torajs-parser` | LSP feature > 1k LOC |
| `torajs-repl` | `cli/src/repl.rs` | `torajs-codegen-cranelift` | trigger 9 (cranelift ship) |
| `torajs-formatter` | `core/src/formatter.rs` (1194) | `torajs-ast` | B 族开拆后顺手 |
| `torajs-linter` | `core/src/linter.rs` (748) | `torajs-ast` · `torajs-check` | 同上 |
| `torajs-bundle` | (未存在) | `torajs-modules` · `torajs-codegen-*` | 用户主动要求 |
| `torajs-pkg` | (未存在) | `torajs-modules` | 用户主动要求 |
| `torajs-test` | (未存在) | `torajs-cli` | 用户主动要求 |

### E 族 — Embed + Cloud + 内部工具 (3 crate · 当前全 scaffold)

| crate | status | 当前实现 | deps | 后续 |
|---|---|---|---|---|
| `torajs-embed` | 🟡 V3-14 MVP | host Rust API + C ABI (`tora_eval`)，subprocess 跑产物 | `torajs-core` | V3-16 in-process JIT (trigger 9) |
| `torajs-playground-api` | 🟡 T-22.b | axum `/api/run` → `tr build --target wasm32-wasi` + wasmtime | `torajs-core` | v1 待产品节奏 |
| `torajs-test262-runner` | 🟡 scaffold | 内部 conformance harness (`conformance/test262-runner/`) | `torajs-core` | 视 substrate 完成度提升覆盖 (当前 12.20% = 3455/28314) |

---

## L3a — Current hot work

**当前热化范围**：A 族 Layer 2 收尾 (P3.1 → P3.2 → P3.3)。其他族都不在 L3a。

热化依据：[`handoff.md`](../.claude/handoff.md) §Next + 顶位 status memory。详细 step 在 status memory + handoff，本文档**不写 step**，只列 hot 项 + L4-confirmable 收口判定。

### L3a #1 — P3.1-e Str transformation ops
- **Scope**: 10 个 transform fn (case / trim / pad / build / replace 五类) port 到 `crates/torajs-str/src/transform/`。
- **Acceptance gate** (per [`torajs-autorun-pipeline.md`](../.claude/rules/torajs-autorun-pipeline.md)): workspace `cargo build --release` 0 err 0 warn / `cargo fmt --check` clean / `cargo test --release` 不回归 ≥ 260/0 / ≥ 1 affected fixture bun-parity / `conformance` 666/0/1 / [`file-size.md`](../.claude/rules/common/file-size.md) audit clean.

### L3a #2 — P3.1-f Str split + SplitIter
- **Scope**: ~750-line C block 含 `split_pool_blocks_` (复用 Substr-tail layout) + `__torajs_arr_free` 交互。P3.1 最大单 port。
- **依赖**: L3a #1 ship。

### L3a #3 — P3.1-g Str print + IR-side ports + 收口
- **Scope**: runtime_str.c Str surface 收尾。把 IR-defined fns (`define_str_concat` / `define_str_slice` / `__torajs_str_drop` 等) port 到 Rust。
- **收口判定** = [L4 trigger 1](#l4--coldhot-trigger) 命中 (P3.1 closed)。

### L3a #4 — P3.2 torajs-num
- **Scope**: 新 sub-crate。Math namespace + Number ToNumber (ES §7.1.4)。
- **依赖**: L3a #3 完成 (= [L4 trigger 1](#l4--coldhot-trigger) 命中)。

### L3a #5 — P3.3 torajs-bigint
- **Scope**: 新 sub-crate。`runtime_bigint.c` (1306 LOC) 整片 → Rust。
- **依赖**: L3a #4 完成 (= [L4 trigger 2](#l4--coldhot-trigger) 命中)。

---

## L3b — Clear backlog (per 族)

清晰但未热化的工作。每条标 hotify trigger。

### A 族 — Layer 3-5 + 收尾

| sub-phase | scope | hotify trigger |
|---|---|---|
| **P4.x** torajs-arr | `runtime_str.c` arr* → Rust ring buffer | trigger 3 (P3 全 closed) |
| **P4.x** torajs-dynobj | `runtime_str.c` dynobj* → Rust hashtable | torajs-arr ship 后 |
| **P4.x** torajs-collections | `runtime_map.c` + Weak* → Rust Map/Set/Weak* | torajs-dynobj ship 后 |
| **P4.x** torajs-cycle | `runtime_cycle.c` → Rust Bacon-Rajan | torajs-collections ship 后 |
| **P5** torajs-microtask | `runtime_promise.c` microtask 部分 → Rust queue | trigger 4 (P4 全 closed) |
| **P6** torajs-promise | `runtime_promise.c` 上层 → Rust Promise<T> + then/catch/all/race/any/allSettled | torajs-microtask ship 后 |
| **P6** torajs-regex | `runtime_regex.c` 整片 → Rust regex VM + surface | torajs-promise ship 后 (或并行) |
| **P6** torajs-fetch | `runtime_fetch.c` → Rust HTTP client (feature) | torajs-regex ship 后 |
| **P7** glue cleanup | `torajs-runtime` 缩 thin shim + 删 runtime_*.c | trigger 5 (P6 全 closed) |

### B 族 — Compiler 拆分 (整族未热化)

12 个 planned crate 全在 `torajs-core` monolith 里。拆分严格 ↑ DAG 推进。

| sub-phase | scope (L2 一行) | hotify trigger |
|---|---|---|
| **B.1** torajs-tokens 抽出 | Token enum + Span | trigger 6 (A 族全 closed) |
| **B.2** torajs-lexer 抽出 | `lexer.rs` 1198 LOC | B.1 ship |
| **B.3** torajs-ast 抽出 | `ast.rs` god 11.5k → 独立 + node 拆 mod | B.2 ship |
| **B.4** torajs-parser 抽出 | `parser.rs` god 7.1k → 独立 + grammar 拆 mod | B.3 ship |
| **B.5** torajs-types 抽出 | Type 代数从 check.rs 抽 | B.4 ship |
| **B.6** torajs-modules 抽出 | multi-file resolve | B.5 ship |
| **B.7** torajs-check 抽出 | `check.rs` god 8.2k → 独立 + per-phase 拆 mod | B.6 ship |
| **B.8** torajs-ssa 抽出 | SSA IR + builder | B.7 ship (或与 B.6 并行) |
| **B.9** torajs-lower 抽出 | `ssa_lower.rs` ⚠️⚠️ 28k god → 独立 + stmt-kind 拆 mod | B.8 ship |
| **B.10** torajs-codegen-llvm 抽出 | `ssa_inkwell.rs` → 独立 | B.9 ship |
| **B.11** torajs-link 抽出 | cc-link 驱动从 main.rs 抽 | B.10 ship |
| **B.12** torajs-codegen-cranelift 新建 | 第二 backend (JIT) | trigger 7 (B 主拆完成) |

### C 族 — Stdlib / Web surface (整族未规划)

⬜ sketch only。trigger 8 命中前不细化 — 避免发明 12 个永远不用的 crate。

### D 族 — Toolchain

| sub-phase | hotify trigger |
|---|---|
| `torajs-formatter` / `torajs-linter` 抽出 | B 族开拆顺手 (B.3 / B.7 ship 后) |
| `torajs-lsp` 抽出 | LSP feature > 1k LOC |
| `torajs-repl` 抽出 | trigger 9 (cranelift ship) |
| `torajs-bundle` / `torajs-pkg` / `torajs-test` | 用户主动要求 (off-roadmap) |

### E 族

| sub-phase | hotify trigger |
|---|---|
| `torajs-embed` V3-16 in-process JIT | trigger 9 (cranelift ship) |
| `torajs-playground-api` v1 | 产品节奏 (off-roadmap) |
| `torajs-test262-runner` 覆盖提升 | 视 substrate 完成度 |

---

## L4 — Cold→Hot trigger

每条 trigger = **可机器判定的状态判定式**。命中 = L3b 顶位 hotify 成 L3a 新 entry，开始细化 step + acceptance。

| # | 状态判定 (可机器判) | 触发什么 hotify |
|---|---|---|
| 1 | runtime_str.c Str surface 全 extern decl 化 + 文件 LOC < 2000 | P3.1 closed → P3.2 torajs-num |
| 2 | torajs-num crate ship + 全 acceptance gate 绿 | P3.2 closed → P3.3 torajs-bigint |
| 3 | runtime_bigint.c 删除 + torajs-bigint ship | **P3 全 closed** → P4.1 torajs-arr |
| 4 | runtime_cycle.c 删除 + torajs-cycle ship | **P4 全 closed** → P5 torajs-microtask |
| 5 | runtime_{promise,regex,fetch}.c 全删除 + 对应 crate ship | **P6 全 closed** → P7 glue cleanup |
| 6 | torajs-runtime 缩 thin shim (lib.rs < 200 LOC + 无 include_str!) | **A 族全 closed** → **B 族拆启动** (B.1 hotify) |
| 7 | `crates/torajs-core/src/ssa_lower.rs` 不存在 + torajs-lower crate 独立 | **B 族基础链 (B.1–B.11) 完成** → B.12 cranelift + D.formatter / D.linter 抽离 可并行 |
| 8 | B.6 (torajs-modules) ship + 用户主动要求 C 族某 crate | C 族对应 crate hotify (从第一项开始切分 strawman) |
| 9 | torajs-codegen-cranelift ship | torajs-embed V3-16 in-process JIT + torajs-repl 抽出 |

### off-roadmap trigger 出口

L4 表只覆盖**自动顺位推进**。任何 takagi 主动指令 ("现在插入 X" / "先做 Y" / "插一个 stdlib crate") = **当场把它写到 L4 末尾作新 trigger**，然后开干。这是合法 L4 扩展不是绕开。

**不在 trigger 表里 + takagi 没主动指令 = 不该自己开干**。任何"感觉差不多了就开 X"违反 L4 → 回 [`CLAUDE.md`](../CLAUDE.md) Planning Architecture 反模式。

---

## Deps 审计

torajs vision priority **#4 = 0 deps**。但 "0 deps" 不等于 "0 行外部代码"，而是**"进 tr 产物的代码 0 行外部"**。实际操作分 **4 个 tier**：

| Tier | 范围 | 0-deps 约束 | 当前状态 |
|---|---|---|---|
| **Tier 0** — carved out | LLVM (via inkwell) · libc | 已 grant ([`CLAUDE.md`](../CLAUDE.md) "C runtime + LLVM IR 是允许的边界") | 1 dep |
| **Tier 1** — host tooling | dev/build-time · CLI 端 · 不进 tr 产物 | 软约束 — 可有 dep, 每个要 audit | 12 dep |
| **Tier 2** — cloud only | `torajs-playground-api` 专用 · 不进 tr / user 产物 | 软约束 — 可有 dep | 4 stack (9 dep) |
| **Tier 3** — runtime | A 族 substrate + C 族 stdlib (进 user 产物) | **硬约束 = 真 0 deps** | **✓ 0 dep (当前 A 族已达成)** |

### Tier 0 — carved out (1)

| dep | 用于 | 自研可行 |
|---|---|---|
| `inkwell 0.9` | B.10 `torajs-codegen-llvm` LLVM IR builder | ✗ 自研 = 重写 LLVM, 不现实 |

### Tier 1 — host tooling audit (12 dep, 待 takagi 决断)

按 "自研难度" 升序排; ✗ = 不建议自研 · △ = 中等 · ✓ = 容易。

| dep | 用于 | 自研难度 | LOC 量 | agent 建议 |
|---|---|---|---|---|
| `hex 0.4` | playground-api hex encoding | ✓ 极易 | ~100 | **自研** (~5 LOC inlined) |
| `libloading 0.8` | torajs-embed dlopen/dlsym (V3-16 in-process JIT) | ✓ 易 | ~200 | **自研** (薄 unsafe FFI 壳 ~100 LOC) |
| `sha2 0.10` | playground-api 源码 hash 缓存 | ✓ 易 | ~600 | **自研** (SHA-256 ~200 LOC) |
| `toml 1` | bench-harness config 解析 | ✓ 中 | ~1.5k | **自研** (TOML 语法可控) |
| `clap 4` | playground-api CLI args | ✓ 中 | ~5k | **自研** (torajs-cli 自家 arg parser 复用) |
| `anyhow 1` | bench-harness error wrap | △ 小 | ~500 | 替换 `Box<dyn Error>` 或保留 |
| `thiserror 2` | playground-api error derive | △ 小 | ~800 | 保留 或 manual impl |
| `tracing 0.1` + `tracing-subscriber 0.3` | playground-api logging | △ 中 | ~3k | 保留 或 自研轻量 |
| `rustyline 16` | torajs-cli REPL line editing | △ 中 | ~3k | 保留 或 自研轻量 |
| `lsp-server 0.7` + `lsp-types 0.95` | torajs-cli LSP JSON-RPC + protocol consts | △ 中-大 | ~5k 协议常量 | 保留 (LSP 协议复杂) 或自研子集 |
| `serde 1` + `serde_json 1` | torajs-cli (LSP) · playground-api · bench-harness | ✗ 大 | 10k+ | 保留 (生态标准 derive) |
| `criterion` (dev-dep) | A 族 sub-crate bench harness | ✗ 大 | 5k+ 含统计 | 保留 (bench 工业标准) |

### Tier 2 — cloud only audit (4 stack · 9 dep)

只在 `torajs-playground-api` 用; 此 crate 是 cloud server, **不进 tr / user 产物**, 不污染 vision #4 硬约束。整族 ROI 低 = 重写 Web server stack。

| stack | 用于 | 我的建议 |
|---|---|---|
| `axum 0.8` (含 macros) + `tower 0.5` + `tower_governor 0.8` + `tower-http 0.6` (cors/trace) | HTTP framework + middleware + rate limit | 保留 (cloud-only) |
| `tokio 1` (full features) | async runtime | 保留 (cloud-only) |
| `serde 1` + `serde_json 1` (同 Tier 1) | JSON 请求/响应 | 同 Tier 1 决断 |
| (其他: sha2/hex/tracing/thiserror/clap) | 列在 Tier 1 | 同 Tier 1 决断 |

### Tier 3 — runtime (硬约束 0 deps)

**A 族 substrate 当前 0 dep ✓**。具体检查命令：

```bash
# 任何 substrate crate 必须输出仅 path-dep + workspace-internal:
cargo tree -p torajs-rc -e normal
cargo tree -p torajs-anyvalue -e normal
cargo tree -p torajs-str -e normal
# ... 等等
```

C 族 stdlib 未来上线时 **同样 0 dep 硬约束**——任何 stdlib crate ship 前 acceptance gate 加一项: `cargo tree -p <crate> -e normal` 输出 0 个外部 crate。

### 新增 dep 协议 (`[[feedback_dep_approval_required]]` 强制)

**任何 PR 加新 dep (除 Tier 0 carved-out) 必须先 takagi 批准**：

1. PR 描述里写: 进哪个 Tier · 必要性 · 是否能自研 · ROI 评估
2. takagi 决断: 同意 / 拒绝 / "自研替代品"
3. 同意则更新本文档 §Deps 审计 对应 Tier 表加一行 + 注明决断 commit hash
4. 拒绝则要么自研 (开新 sub-step) 要么放弃该 feature

### audit 触发条件

每次以下事件后必须重跑本节 audit:

- 新 sub-crate ship (检查不引入新 dep)
- 现有 crate Cargo.toml 改 `[dependencies]` / `[dev-dependencies]` / `[build-dependencies]`
- workspace `Cargo.toml` 改 `[workspace.dependencies]`
- 升级 Rust edition / cargo resolver (可能引入 transitive)

audit 命令一行: `for f in crates/*/Cargo.toml bench/*/Cargo.toml conformance/*/Cargo.toml; do echo "== $f =="; cat "$f"; done | grep -A 1 '^\[\(dev-\|build-\)\?dependencies\]'`

---

## 维护协议

### 每次 substrate sub-step ship 后

1. 来本文档 §A 表更新该 crate 的 status (✅/🟡/⏳) + commit hash + 当前 LOC
2. 检查 §L4 是否命中 — 命中则在 §L3a 加新 entry，§L3b 划掉对应条目
3. **重跑 [§Deps 审计](#deps-审计)** (该 sub-step 不应引入新 dep; 引入了 = 走 §新增 dep 协议)
4. 若命中 trigger 6 (A 族全 closed) → 本文档进入 [v0.2](#文档版本) (B 族拆分启动，拓扑大改)

### 每次 cold→hot 升级

1. 把 L3b 该条目从所在族表移到 §L3a
2. §L3a 给 entry 写 Scope + Acceptance gate + 依赖
3. status memory 同步开 step 级 plan (本文档不写 step)

### 新增 sub-crate (新 sub-step ship)

1. 流程见 [`architecture-rewrite.md`](architecture-rewrite.md) §Per-crate file template + §Per-crate Cargo.toml template + §Acceptance gate
2. ship 后回 §A/B/C/D/E 对应表加一行 / 更新一行

### sketch only 段升级 (⬜ → ⏳)

某 ⬜ sketch-only 段 (C 族大部分) 被 L4 触发时，把 placeholder 替换为完整 L2 inventory + deps + 抽出 trigger。

---

## 文档版本

| version | 状态 | 升级触发 |
|---|---|---|
| **v0.1** (当前) | strawman | — |
| v0.2 | trigger 6 命中 | B 族拆分启动 (从 monolith 转 11 crate inventory) |
| v0.3 | trigger 7 命中 | D 族开始拆分 |
| v0.4 | trigger 8 命中 | C 族第一 crate hotify (sketch 段升级) |
| v1.0 | 全族 closed + API stable | 进入维护态 |

**v0.x 期间允许大幅 strawman 改动**，不需要 changelog。v1.0 freeze 后任何改动需 ADR (Architecture Decision Record)。

---

## 如何使用本文档

| 角色 / 场景 | 读什么 |
|---|---|
| 新人入项目 | §L1 + §Glossary + §L2 总 DAG (30 秒拿到全图) |
| 想知道 "下一步做什么" | §L3a 顶位 (= take next) |
| 想知道 "X 属于哪个 crate" | §L2 该族子段，按命名 + scope 检索 |
| 想 ship 新事 | 先 check §L4 是否有命中的 trigger; 没命中 + takagi 没主动指令 → **不该开** |
| 想加新 sub-crate | §维护协议 → 新增 sub-crate 段 |
| substrate ship 一个 sub-step | §维护协议 → 每次 sub-step ship 后 段 |
| 想加新外部 dep | §Deps 审计 → 新增 dep 协议 (必须 takagi 批准) |
| 想审计当前 deps 状态 | §Deps 审计 Tier 1 / Tier 2 表 |
| 想推翻一个 framing 决定 | 文档 v0.x 期间直接 PR 改; v1.0 后写 ADR |
