# torajs `.dev/` — 研发环境优化专题

torajs 是公司 #1 核心项目，对标 bun，对**构建/验证速度**和**代码质量**双高要求。
本目录是「研发环境优化」的常驻专题：让 dev loop 更快、更省盘、更省心，
**且绝不以牺牲质量为代价**。这是一个**持续演进**的主题——后续会不断补充工具与
优化项，不是一次性脚本堆。

## 第一硬规则（不可妥协）

> **任何优化都不得降低验证覆盖或正确性。**

具体：不减 conformance case、不去 bun-oracle 对照、不去 `tr build` AOT 路径、
不放宽 zero-warn / zero-fail / fmt-clean 的 ship gate。优化只允许改"墙钟/磁盘/
人力成本"，不允许改"验证了什么"。任何"看起来快但少验了点东西"的方案直接否决
（与 `.claude/rules/torajs-design-principles.md` 规范 pillar、`feedback_no_tech_debt`
一致）。

## 内容索引

| 文件 | 作用 | 演进方式 |
|---|---|---|
| `ENVIRONMENT.md` | 构建/缓存环境的 **ground truth** + 已纠正的认知（防后人重蹈覆辙） | 每次发现环境事实变化就更新 |
| `OPTIMIZATION.md` | 按杠杆大小排序的**质量中性**优化 backlog（有数据支撑） | 做掉一项就标 done + 记实测；发现新项就追加 |
| `clean.sh` | 安全的废弃文件自动清理器（dry-run 默认） | 发现新的可枚举废弃产物来源就加一条规则 |

## 如何扩展这个专题（给后续会话/开发者）

1. **新优化想法** → 先过"第一硬规则"自检（它改的是墙钟还是验证内容？改验证内容=否决）。
   通过则按预估杠杆插入 `OPTIMIZATION.md` 的优先级表，附**可机器判定的验收**
   （如 "conformance 仍 627/0/1 且墙钟 < X"）。
2. **新环境事实/踩坑** → 记进 `ENVIRONMENT.md`，尤其是**反直觉**的（本专题起源就是
   一个反直觉发现：外置盘做 cargo target 比内置慢，sccache 才是 build 速度真杠杆）。
3. **新废弃产物来源** → 在 `clean.sh` 加一条**可 grep 的 glob 模式 + verify-before-delete**，
   保持 dry-run 默认 + 永不碰 source/committed/非-torajs/非己产物。
4. **自动化触发**（"自动删除"的 automation 部分）→ 属 Claude Code hook（settings.json）
   范畴，由 `update-config` skill 配；脚本本体留这里，hook 只负责按时机调它。
   先把脚本做正确做安全，再谈自动触发。

## 与既有规则的关系

- 本专题**不替代** `.claude/rules/`（那是 HARD RULES + pipeline 纪律）；这里是
  "怎么把那些纪律执行得更高效"的工具与计划层。
- `clean.sh` 是 CLAUDE.md「Disk Hygiene HARD RULE」的工具化落地，不是另起炉灶。
- 缓存/构建相关结论与 `~/.claude-shared/global/cargo-target-dir.md` 对齐；
  torajs 的特例（项目私有内置 target）记在 `ENVIRONMENT.md`。
