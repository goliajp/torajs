# hardev autorun pillar — agent-session 编排治理

> **Mandate**: 让长时间 autorun（agent-driven 持续推进）成为**低 drift、
> 可观测、机器治理**的常态，而不是「Claude 自评估累了 → takagi 同意切 →
> 手动 `/handoff save` → 手动 `/clear` → 手动 `/handoff resume`」的人工循环。
>
> **Methodology**, 与所有 hardev pillar 一致：**先 spec、先 metric、再 mechanism**。
> 一上来不上 daemon、不上自动 rotation；先把 trigger 协议落到 CLAUDE.md HARD
> RULE、把 rotation log 落到 `rotations.jsonl`、把 baseline metric 落到
> `hardev/metrics.md` §6 autorun，**measure 一周，然后再决定**自动化阈值。

## Why this pillar exists — 问题是真实的且可重现

torajs 是 1M-context 体量项目，长时间 autorun 推进时会出现：

- **drift out of hard rule**——session 末段中文沟通规则、4-layer planning、
  zero-warn 等硬规则破裂的频率上升（已多次发生：中文转英文中文交替输出，
  hot 计划写到一半夹带不属于本 commit 的 polish）。
- **silent-wrong 风险上升**——疲劳期模型容易写「看起来合理但未实证」的
  代码或 prose，违反 `.claude/rules/common/anti-hallucination.md` Rule 2
  (tool-first not memory-first)。
- **handoff 流程开销**——每次切都要 takagi 同意 + 模型自跑 save + 人工
  /clear + 人工 resume，takagi 心智负担与项目推进解耦得不够。

但**直接上自动化 rotation watcher 是错误的入口**——尚未 measure 就 mechanism
违反 hardev 第一硬规则。先把信号、协议、记录三件事落实，再决定是否上 daemon。

## Architecture（v1 设计；P0 只 ship 子集）

### Layer 0 — metric SoT（先 measure）

`hardev/metrics.md` §6 autorun。所有数字 `[D]` 待测，无 untagged 数据：

- 平均 session 时长（commit→rotation 间隔）
- handoff fidelity（resume 后第一条 user msg 之前模型是否表现「丢上下文」迹象）
- drift 事件数 / session
- rotation 间 unstaged 改动丢失次数
- rotation 期 conformance 回归次数

每次 rotation 写一行 JSON 到 `hardev/autorun/rotations.jsonl`（schema 见
下方 `## JSONL schema`）。文件 .gitignored（per-developer runtime data）。

### Layer 1 — 信号源（P1 上 Stop hook；P0 用手动 trigger）

**P1 设计**：Claude Code `settings.json` `Stop` hook 在 agent turn 结束时
触发，读 `.claude/autorun-intent`（Claude 在自评估满足 rotation 条件时写出，
含 rotation_id），若存在 + handoff.md < 90s 则写出 `.claude/autorun-marker`。
watcher 监听 marker（fswatch 事件驱动，零轮询），完全不 scrape pane 文本——
sentinel 不再是字符串而是文件存在性，docs / commit message / 这份 README
本身引用 "autorun-marker" 字眼都不会误触发。

**P0 ship 范围**：仅手动 trigger。`hardev autorun trigger` 一行命令产生
rotation_id + 写 intent 文件 + emit JSON line + 打印 takagi 的下一步指令
（自己跑 `/handoff:handoff save` → `/clear` → `/handoff:handoff resume`）。
不上 Stop hook、不上 watcher、不上 launchd。

### Layer 2 — Agent 协议（CLAUDE.md HARD RULE）

CLAUDE.md 「Autorun rotation protocol」节明文（**已写入；非草案**）：

每完成一个 phase 收口，或察觉以下任一信号——
- 自己刚 drift 出硬规则（中文沟通 / 4-layer planning / disk hygiene）
- silent-wrong 风险上升（疲劳症状）
- session commit 计数 ≥ 5 且当前 hot 项接近 done

按 sequence 收尾：

1. 跑 `/handoff:handoff save`（save 完成）
2. 跑 `hardev/autorun/trigger.sh self`（生成 rotation_id + emit log）
3. emit **最多一行** status confirmation（让 takagi UI 看到 boundary signal，
   形如 `rotation <id> recorded · /clear + /handoff:handoff resume 切到新 session`），
   **这一行之外**不再输出任何 token——不解释、不收尾、不道别。

Why 一行而非 0 行：**P0 stage（无 watcher）下 takagi 屏幕需要一个 agent-side
boundary cue** 来区分「agent 完成 rotation 等 /clear」vs「agent 卡了」。完全
STOP 在首次 dogfood (2026-05-20) 被误判为后者。**P1 stage（Stop hook + watcher
上线）后此条 fall back 到完全静默** — watcher 进入接管 boundary signaling。

takagi 也可 `hardev/autorun/trigger.sh manual` 强制 rotation；
任何角色发起后，handoff resume 的执行**当前是手动**（P0），
P1 上 watcher 后自动完成 /clear + resume。

### Layer 3 — Watcher（P1 才上）

`hardev/autorun/watcherd.sh` + `~/Library/LaunchAgents/com.hardev.autorun.plist`。
fswatch 监听 marker → 校验 INV-1..5 → `tmux send-keys '/clear' Enter` → 等
pane idle → 发 `/handoff:handoff resume` + 拼接 `autorun-inbox.md` 内容。

**P0 不上**——先 measure 1 周再决定阈值。

### Layer 4 — CLI surface

P0 ship 的：
- `hardev/autorun/trigger.sh [self|manual]` — 触发一次 rotation 并记录
- `hardev/autorun/log.sh [--tail N]` — 渲染 rotations.jsonl 为可读表格

P1 扩展（不阻塞本次 ship）：
- `hardev/autorun/check.sh` — INV-1..5 机器校验
- `hardev/autorun/status.sh` — daemon 状态 + 最近 rotation + inbox 字数
- `hardev/autorun/pause.sh` / `resume.sh` — daemon 仍跑但跳 marker
- `hardev/autorun/init.sh` — 装 Stop hook + plist + 项目 config.toml

## JSONL schema（rotations.jsonl）

每次 trigger 写一行（compact, 无 trailing newline 外的空格）：

```json
{"rotationId":"r-1747836296-a1b2","at":"2026-05-20T12:34:56Z","ts":1747836296,"project":"torajs","trigger":"manual","prevHead":"aaaef71","handoffSha":"sha256:abc...","handoffAgeSec":12,"conformanceBefore":"631/0/1","commitsInSession":null}
```

字段语义：

- `rotationId`：`r-<unix-ts>-<random4hex>` —— **唯一**（time + 16bit 熵）。
  外部 grep / pane 内容里出现这个串也无法 collide（实际 ID 是 trigger 时
  随机产生）。
- `at`：RFC-3339 UTC trigger 时间。
- `ts`：epoch seconds，便于 sort / 时间差计算。
- `project`：默认从 `git rev-parse --show-toplevel` basename 推出。
- `trigger`：`self`（agent 自发起）/ `manual`（takagi 命令行）/ 将来 `hook`
  / `daemon` 等。
- `prevHead`：`git rev-parse --short HEAD`，trigger 时刻。
- `handoffSha`：`shasum -a 256 .claude/handoff.md`，便于 audit。
- `handoffAgeSec`：trigger 时刻 `.claude/handoff.md` 的 mtime 距 now 秒数。
  P1 INV-1 要求 < 90。
- `conformanceBefore`：尝试从 status memory header grep 出 `NNN/0/1`，
  失败置 `null`（不 fabricate）。
- `commitsInSession`：P0 占位 null；P1 配合 Stop hook 才能精确。

**schema 不变性 (HARD RULE)**：现有字段不删、不改语义。新字段只追加，且默认
允许缺失。下游消费者（dashboard、metrics 报告）必须 tolerate 旧记录。

## INV-1..5 spec（P1.1 SHIPPED — `check.sh`）

5 条 pre-act 不变量。任何一个 FAIL，rotation 都**不允许**继续（Stop hook
不写 marker；watcher 不发 tmux send-keys）。机器化把 P0 baseline 暴露
出来的真实失败模式（row #6 `handoffAgeSec = 7489 s`）变成 gate。

| ID | 不变量 | 失败 = 什么风险 | 实现 |
|----|--------|----------------|------|
| **INV-1** | `.claude/handoff.md` 的 mtime age < 90 s | handoff 描述的状态早于 trigger 时 HEAD —— 新 session 接到一份过时的 handoff（**这就是 P0 row #6 的真实失败**） | `autorun_file_age_sec` |
| **INV-2** | `git -C <project> status --porcelain` 输出为空 | rotation 即将 /clear；未 commit 的改动（staged 或 unstaged）会对新 session 不可见 ⇒ 静默丢失 work | `git status --porcelain` |
| **INV-3** | 当前 `conformanceBefore` ≥ `rotations.jsonl` 末尾一行的 `conformanceBefore`（按 first /-separated 数字比较） | rotation 之前已经引入了 conformance 回归而未察觉。P0 baseline 10 行天然 monotonic non-decreasing，P1 把它变成 gate 而非观察 | `autorun_conformance_now` + tail jsonl |
| **INV-4** | `handoff.md` non-empty 且含 `> saved:` blockquote 行 | 文件存在但内容是 phantom（0 字节、半写、误 touch）—— mtime 满足 INV-1 也救不了，这是结构性 fallback | `grep -q '^> saved:' handoff.md` |
| **INV-5** | 新生成 `rotation_id` 不在 `rotations.jsonl` 已有行中 | id 冲突会污染下游 audit / dashboard 的 join。绝对发生概率 ≈1/65536（同秒），guard 成本零 | `grep -q "\"rotationId\":\"$ID\"" rotations.jsonl` |

调用约定：

```
hardev/autorun/check.sh [rotation_id]
```

- 缺 `rotation_id`：INV-5 SKIP，其余照跑
- 退出 0：全部 PASS（或 SKIP）；适合作为 `&&` 链 gate
- 退出 1：至少一个 FAIL；stderr 末尾一行 `FAILED: INV-N [INV-M ...]`
- 退出 2：内部错误（lib.sh 缺、git 不可用、project dir 不存在）

stdout 每条 INV 一行 `INV-N STATE detail`，行格式稳定供 P1.2 Stop hook
+ P1.3 watcher + `check_self_test.sh` parse。

**Self-test**：`hardev/autorun/check_self_test.sh` — 4 case 端到端：
GREEN happy / INV-1 stale / INV-2 dirty / INV-5 dup-id。Trap 恢复所有
副作用（handoff.md mtime + fake-dirty marker）。在 GREEN tree 上手动跑
应当 `4 pass · 0 fail` 退出 0。

**调用现场**（P1.2 / P1.3 落地后）：

```
# Stop hook (P1.2):
if [ -f "$INTENT_FILE" ]; then
  rid=$(cat "$INTENT_FILE")
  if "$AUTORUN_DIR/check.sh" "$rid" >&2; then
    printf '%s\n' "$rid" > "$MARKER_FILE"
  fi
fi

# Watcher (P1.3) — defense in depth re-check before acting:
if "$AUTORUN_DIR/check.sh" >&2; then
  tmux send-keys -t "$pane" '/clear' Enter
  ...
fi
```

## Stop hook（P1.2 SHIPPED — `stop_hook.sh`）

P1.2 把 trigger.sh 写的 `autorun-intent` 接到 Claude Code 的 `Stop`
事件上，把 INV-1..5 检查从"agent 自评估自己跑"升级为"每个 turn-end
机器执行"。stop_hook 是**唯一**有权把 intent 升级为 marker 的脚本。

### 接线（per-developer，因 `.claude/settings.local.json` gitignored）

```json
"hooks": {
  "Stop": [
    {
      "hooks": [
        { "type": "command", "command": "hardev/autorun/stop_hook.sh" }
      ]
    }
  ]
}
```

Claude Code 调 hook 时 CWD = 项目根目录，所以相对路径 work；不需要写
绝对路径，跨开发者 portable。

### Sentinel lifecycle

```
trigger.sh      → writes .claude/autorun-intent (rotation_id, 1 line)
                  (P0 + P1 共用 — trigger 自身行为不变)
stop_hook       → reads intent → runs check.sh <rid>
                  · GREEN → writes .claude/autorun-marker (same rid) + rm intent
                  · RED   → keeps intent (next turn-end retries)
watcherd (P1.3) → fswatch marker → re-runs check.sh → tmux send-keys
                  /clear + /handoff:handoff resume → rm marker
```

每个 sentinel 在 GREEN 路径上**精确消费一次**。RED 路径保留 intent，
让 agent 修复失败的 INV（最常见：tree dirty 就 commit；handoff 旧就
`/handoff:handoff save`）后下次 turn-end 自动 retry，**无需重跑
trigger.sh**。

### 不变量

- stop_hook 始终 `exit 0` —— hook 故障必须不能 break 用户的 turn
- 任何状态变化（写 marker / rm intent）只发生在 GREEN 路径
- stderr 用于状态 line（`stop_hook: rotation <rid> green-lit · …` 或
  `… blocked by INV check · …`）+ check.sh 自己的 5 行 INV report
- 不 spawn 任何长 running 子进程（hook latency = check.sh latency）

### P1.2 验收

机器可判：

1. 无 intent → 无 marker、exit 0
2. intent + GREEN tree + fresh handoff → marker 文件出现且内容 = rid；
   intent 被 rm
3. intent + 任一 INV FAIL → 无 marker；intent 内容保持不变

详见 `hardev/autorun/stop_hook.sh` 文件头注释。

## P0 acceptance（本次 ship 验收口径）

机器可判项：
1. `hardev/autorun/trigger.sh manual` exit 0，写出 `.claude/autorun-intent`
   + 追加一行 schema-valid JSON 到 `hardev/autorun/rotations.jsonl`。
2. `hardev/autorun/log.sh` 渲染至少一行表格，含 trigger 后的 rotation_id。
3. 重复 `trigger.sh` 三次，`rotations.jsonl` 累积三行，rotation_id 全不同。

人工判项：
4. takagi 跟着 trigger.sh 打印的指引手动跑一次 `/handoff:handoff save` →
   `/clear` → `/handoff:handoff resume`，新 session 从 handoff.md 接得上。

不上的 / 故意排除的：
- 自动 /clear、自动 resume（P1）
- INV-1..5 强制 check（P1）
- 后台 daemon / launchd（P1）
- inbox.md 异步收件箱（P1）
- dashboard rotation 面板（P2）

## 后续路径

第 1 周（measure）：takagi 跑日常 autorun，每次切 session 走 trigger.sh，
积累 N 行 rotations.jsonl。当条数 ≥ 10 时 takagi review 一次 baseline——
session 时长分布、drift 在哪个时间段集中、handoff 失败率，**有数据后**
再决定 P1 是否上 daemon、rotation 触发阈值怎么设。

第 2 周起（mechanism）：依据 metric 决定 P1 范围；若 daemon 必要则按
本文 Layer 3 落地，先 single-project（torajs）跑通 5 次完整 rotation
0 incident 才考虑 graduate 到 multi-project。

## Relationship to existing hardev pillars

- **taskq**：autorun 触发时 `taskq/check.sh` INV-1a 应自动跑（保证 plan
  source 与 HEAD 一致）。P1 INV-1 会调用 taskq check 作前置；P0 不强制。
- **cleanup**：rotation 是合适的「session 边界 cleanup」hook 时机——P1
  watcher 可以在 /clear 前调用 `hardev/cleanup/clean.sh`（dry-run-default
  保持）。
- **bench**：rotation 前后的 conformance / bench 数据写入 rotations.jsonl
  `conformanceBefore` / `conformanceAfter` 字段，未来 dashboard 可以
  追踪「rotation 是否引入回归」（INV-3 的 metric 化）。
- **metric SoT**：本 pillar 自身的 measurements 全部写进
  `hardev/metrics.md` §6 autorun，遵循「无 untagged 数字」规则。
