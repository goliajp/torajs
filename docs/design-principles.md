# torajs 设计原则（HARD RULES）

torajs 是公司 #1 核心项目，对标 bun。所有架构决策、新增 runtime / 编译器组件、性能取舍、API 设计都必须同时满足以下四条。任何违反任意一条的方案不被接受——哪怕是临时方案、MVP、绕开补丁也不行。

## 1. 高性能（performance-first）

不为了语义优雅或实施便利牺牲 hot-path 性能。

- **`run_ms` 优先**：run_ms 是首要指标，compile_ms 只在 run_ms 撞顶后才动
- **每个 hot-path 改动要做心算**：array slot copy 一条 inc 指令是 OK 的；每次 array read 一次 alloc 是不能接受的（clone-on-read 这类方案因此被否）
- **微观优化要让位给架构**：当一个架构选择和一个微优化冲突，先选对架构（refcount 一次 inc），再考虑能不能 inline / vectorize 它
- **Bench 不能回归**：任何 commit 不允许 bench 回归（噪声范围内的波动除外）

## 2. 自研（in-house）

runtime / 编译器内核必须自己造，不引入外部依赖来"省事"。

- **不引入 GC 库 / 不嵌入 V8/JSC/QuickJS**：那些都是别人的东西。tr 的 ownership / refcount / lowering 都要自己写
- **不写"绕开 X 因为太复杂"的方案**："rewriting V8 is too big" 这种 framing 不能接受
- **C runtime + LLVM IR 是允许的边界**：libc / LLVM / inkwell 是 build 工具不算外部依赖；Rust crate（serde / tokio 这类）只在 host 编译期可用，不进 runtime
- **算法可以学习别人，代码必须自己写**：refcount 算法是 Swift ARC 的，但实现是自己的

## 3. 正统（mainstream / textbook）

走 PL implementation 的主流路径，不发明边缘流派。

- **PL textbook 怎么写就怎么做**：state-machine 化的 generator、refcount on heap header、SSA + LLVM lowering、vtable for virtual dispatch——这些是教科书答案
- **跟头部语言对齐**：refcount 选 Swift ARC / CPython / Objective-C 这条路，不是某个 hobby 语言的奇思妙想
- **语义跟 TS spec 对齐**：语义按 TS 规范，不引入 Rust ownership / borrow checker / RAII 等异语言概念
- **拒绝"听起来很 cool 但没人用过"的方案**：region inference、effect handlers、自创类型系统等等——不做

## 4. 规范（disciplined / engineering-grade）

代码质量按生产 PL implementation 标准走，不写 ad-hoc patch。

- **架构改动一次到位**：layout 变了就一次性改完所有 access site，不留 backwards-compat shim / `// TODO 后续修` 注释
- **统一抽象**：所有 non-Copy heap object 共用同一份 header，不为每个 type 写一套独立 layout
- **C struct + LLVM struct type 双向定义**：runtime 和 inkwell 必须用同一份 named type，不散 magic offset 数字
- **NULL safety + debug assert**：`refcount != 0` 是 inc/dec 的前置条件，debug build 加 assert
- **命名跟主流靠齐**：`__torajs_rc_inc` / `__torajs_rc_dec` / `__torajs_heap_header_t` 这种 PL textbook 命名风格
- **拒绝"silent wrong"和"MVP limitation"**：silent leak / typecheck error / MVP 注释都视为债

## 5. 上限优先（when choosing between paths）

当多个方案都满足前 4 条原则时，**永远选上限最高、未来空间最大的那条**——不选"现在更省事"或"短期 ROI 看上去更明确"的方案。

"上限"用标准指标衡量：
- **runtime perf**：跑出来的程序速度（demo run_ms / bench scoreboard）
- **build perf**：编译时长（`tr build_ms` / `tr run` cache miss / ssa_lower 时间）
- **artifact size**：编译产物大小（binary KB）
- **未来扩展性**：能不能在这条架构之上叠加更高级的优化、新 feature、新类型，而不需要推翻重来

具体例子（substring 设计的两条候选路径）：

| 方案 | run_ms | build_ms | size | 未来扩展 |
|---|---|---|---|---|
| **A. unified Str + data_ptr indirection** | OWNED 也走 1 indirection，hot-loop 退步 | 不变 | 同 | 局促，再加 ConsString 要叠 indirection 链 |
| **B. Substring 独立 Type::Substr (Swift / Rust 模式)** | OWNED 不动，view 零 alloc | +轻微 | 同 | 顶上能叠 ConsString / SSO / interning |

A 短期容易写、surface 单一；B 是 Swift / Rust / .NET 三大头部 PL 的路径，上限和未来空间都更高。**选 B**——哪怕短期工作量大 3-5 倍。

反例（不要踩的）：
- "先做 A，未来再迁移到 B" — 这是把架构债攒着，下次重构时全部翻一遍
- "B 太难了，A 也能 work 70%" — 70% 的方案配不上对标 bun 的目标
- "现在没时间做 B，先 ship A" — 永远不会有"以后"的时间

## 应用流程

任何新 runtime helper / SSA 编译 pass / 架构层面的改动，**先按这五条原则自查**：

- 高性能：hot-path 心算过了吗？bench 会不会回归？
- 自研：依赖了什么？是否可避免？
- 正统：哪个头部语言 / textbook 是这么做的？
- 规范：layout / 命名 / 抽象层次干净吗？是不是一次到位？
- 上限优先：列出所有候选方案，按 run_ms / build_ms / size / 未来扩展 排序，选上限最高的那条

五条都过才能落地。任何一条没过，回到设计阶段——不要"先 ship 再说"。
