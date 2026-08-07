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

rotation 收尾 gate（`torajs-autorun-pipeline.md` 收尾 sequence 引用）：
- `hardev/autorun/wait_mini_gate.sh <log>` — 阻塞到 mini conformance gate 出摘要行
- `hardev/autorun/build_determinism.sh [N]` — **步骤 0c 的 gate**。全部 bench case
  各建 1+N 次，答两个问题：还编译得过吗、重复构建是不是只出一个 artifact。
  CSV 到 stdout + 带 N 的摘要行；任一 build 失败或任一 case 多于一个 sha → exit 1。
  默认 N=12（判据 N≥10，低于此的"确定"是假阳性，脚本会 warn）；44 case 全量 ~47s。
  测的是 `target/<profile>/tr`，profile 由 `HARDEV_TR_PROFILE` 定，默认 `iter`
  —— 即 conformance gate 自己跑的那个 binary。
- `hardev/autorun/kill_stray_shells.sh` — 收尾时清本 rotation 泄漏的子进程

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
| **INV-5** | 新生成 `rotation_id` 不在 `rotations.jsonl` 已有行中 | id 冲突会污染下游 audit / dashboard 的 join。绝对发生概率 ≈1/65536（同秒），guard 成本零。**注意：仅 trigger.sh pre-append + self-test 显式传 rid 触发；stop_hook / watcherd 调用必须省略 rid（trigger.sh 已 append rid → 传则 INV-5 必 FAIL → stale-intent loop）** | `grep -q "\"rotationId\":\"$ID\"" rotations.jsonl` |

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
# Stop hook (P1.2) — note: NO rid passed (trigger.sh already appended
# the rid to jsonl before this hook runs; passing it would INV-5 FAIL
# the rotation forever, see check.sh:INV-5 header).
if [ -f "$INTENT_FILE" ]; then
  rid=$(cat "$INTENT_FILE")
  if "$AUTORUN_DIR/check.sh" >&2; then
    printf '%s\n' "$rid" > "$MARKER_FILE"
    rm -f "$INTENT_FILE"
  fi
fi

# Watcher (P1.3) — defense in depth re-check before acting (also no rid):
if "$AUTORUN_DIR/check.sh" >&2; then
  tmux send-keys -t "$pane" '/clear' Enter
  ...
fi
```

## TRIG-1..4 spec（rotation 触发不变量,P2.0 SHIPPED — `trig_gate.sh`）

INV-1..5 治 rotation _执行_(stop_hook 之前、watcherd 之前 act 前),TRIG-1..4
治 rotation _触发_(trigger.sh self 入口);两侧 gate 互相独立、机器化、
零模型自评估。

**背景**:`metrics.md` §6 "Baseline @2026-06-15 (171-row drift surface)"
量化 — 近期 self rotation wall p50 从 98 min 退到 48 min(−51 %),3 个
连续 10–13 min session 用 "file-size 单文件 clear = phase 收口" 当 trigger,
本质是 `cases#rotate-as-procrastination` 的 surface 变体(file-size prep
当 phase 收口)。autorun-pipeline §rotation ① "phase 收口" / ③
"silent-wrong 风险升高" 两条 fuzzy,可被滥用绕开 ④ commit ≥ 5 硬条件。
TRIG-1..4 把"是否该 rotation 触发"机器化,删 fuzzy 自评估。

| ID | 不变量 | 失败 = 什么风险 | 实现 |
|----|--------|----------------|------|
| **TRIG-1** | `git rev-list --count <last-self-rotation-prevHead>..HEAD` ≥ **N** (= 5 候选) | session 工作量不足以构成合法 rotation — 把 substrate prep 切小段每段 rotate 一次,**等于 rotate-as-procrastination** | git rev-list |
| **TRIG-2** | 当前 `ts` − 上次 self rotation 行 `ts` ≥ **M** 秒 (= 5400 = 90 min 候选,takagi framing "1/2-3/4 ctx 至少") | wall time 太短 = context 未充分利用 = rotation 频繁不必要 | rotations.jsonl tail self filter |
| **TRIG-3** | handoff.md 末段含 `^## .*rotate 触发` heading,且段内含 closed enum 之一: `(a)` phase substrate-completeness 完整字串 `Phase [A-Z][0-9a-z]* close`(W-J/W-N 系列)/ `(b)` HARD RULE break + recovery / `(c)` measurable silent-wrong signal(fabricated tool output / 中文规则破裂 evidence) / `(d)` commit ≥ TRIG-1 且自然 chunk 边界 | reason fuzzy = 借口空间太大 = TRIG-4 绕过路径 | handoff.md grep -E |
| **TRIG-4** | handoff.md "rotate 触发" 段 grep -i 不含 blacklist 借口 phrase:`file-size .* clear` / `prep work done` / `audit complete` / `fresh session 更稳` / `subagent prep 完整` / `ROI` / `工程量大` / `complexity` / `substantial work` | 这些是已知 procrastination 借口,触发者会用这些字眼包装 "phase 收口" 假象 | handoff.md grep -iE |

调用约定:

```
hardev/autorun/trig_gate.sh
```

- 退出 0:全部 PASS;适合作为 `&&` 链 gate(trigger.sh self 前置)
- 退出 1:至少一个 FAIL;stderr 末尾一行 `TRIG-FAILED: TRIG-N [TRIG-M ...]`
- 退出 2:内部错误(lib.sh 缺、git 不可用、rotations.jsonl 缺、handoff.md 缺)

stdout 每条 TRIG 一行 `TRIG-N STATE detail`,行格式稳定供下游解析。

**Trigger.sh 接线**:

```
trigger.sh self   → 调 trig_gate.sh
                    · PASS → 走原有 autorun_new_id + intent + record_rotation
                    · FAIL → exit 1,不 append jsonl 不写 intent
trigger.sh manual → 不调 trig_gate(takagi override 一切;cases#rotate-as-procrastination
                    决断权属用户)
```

**reason 表达约定**(handoff.md 写作纪律):agent 写 handoff 末尾必须有
精确格式段:

```markdown
## rotate 触发

reason: <a|b|c|d>

<具体 evidence,引用 commit hash / line:col / cases#xxx>
```

TRIG-3 解析 `reason: a` 等抓 closed enum;TRIG-4 grep -i 全段 blacklist。

**例子**(GREEN):

```markdown
## rotate 触发

reason: a

Phase A3c close — chunk 2(`5a6740ae`)+ chunk 3 D walker(`ab12cd34`)
ship + conformance 829 → 832/0/4(+3 named-class 新 fixture)。8 commit /
2h35min wall,跨 torajs-link + torajs-structmeta + torajs-meta + ssa
+ cmd_build 5 crate substrate 改动。
```

→ TRIG-1 8≥5 PASS · TRIG-2 9300s≥5400s PASS · TRIG-3 "Phase A3c close" match (a) PASS · TRIG-4 0 blacklist hit PASS。

**例子**(RED — 本次根治的 ground-truth):

```markdown
## rotate 触发

archive_link.rs file-size HARD limit clear sub-milestone,chunk 2 是
substantial work fresh session 起手更稳,subagent prep 完整。
```

→ TRIG-1 2<5 FAIL · TRIG-2 1299s<5400s FAIL · TRIG-3 无 `reason: <a|b|c|d>` 段 FAIL · TRIG-4 hit `file-size .* clear` + `substantial work` + `fresh session 更稳` + `subagent prep 完整` 4 个 phrase FAIL。**4/4 FAIL = rotation 非法**。

**第 1 周 measure 节奏**(`metrics.md` §6 TRIG slot 表):

- 上线后 trig_gate 默认 ENABLED(不 dry-run — gate 设计意图就是要拒 fuzzy rotation)
- `stats.sh` 跑 1 周,看 (a) self rotation 总数下降到合理范围 (b) drift 信号消失
- 1 周后 takagi review,根据数据调 N / M(可能 N 偏严 ↓ 4 或 M 偏严 ↑ 7200)

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

## Watcher（P1.3 SHIPPED — `watcherd.sh`）

P1.3 把 marker 接到 actual session 操作（`tmux send-keys`）。设计原则：
**watcherd 不是 long-running daemon**，是 single-shot script。P1.4
launchd `WatchPaths` 在 marker 出现/变化时 spawn 一次 watcherd，跑完
退出。这跟 [[feedback-only-devserver-persistent]] 完全 align — 不再
留任何长驻进程。

### 双 gate

INV-1..5 在 P1 pipeline 中跑**两次**:

1. **stop_hook 之前** —— 决定写不写 marker
2. **watcherd 实际 send-keys 之前** —— 决定发不发 keys

第二次是 **defense-in-depth**：state 可能在 stop_hook 和 watcher act
之间漂移（典型场景：用户在两者之间手动改了 tree → 引入 dirty）。如果
第二次 check FAIL，watcher rm marker 阻止再触发（避免 launchd respawn
loop），exit 1 + stderr 记 "blocked at watcher gate"。

### 模式与目标

```
hardev/autorun/watcherd.sh                # --dry-run by default (safe)
hardev/autorun/watcherd.sh --apply        # actually send-keys
```

目标 pane 选择优先级：

1. `HARDEV_AUTORUN_TMUX_TARGET` env var（推荐设；e.g. `%0`、
   `session:window.pane`、`=Claude` —— 见 `man tmux` TARGET-PANE）
2. discover：`tmux list-panes -a` 找 current command 或 title 含
   `claude` 的 pane；找不到 fall back 到 `node`（Claude Code TUI
   跑在 node 下）
3. 都没有 → 不 act，rm marker，exit 1

### Sentinel 行为

marker 在 GREEN 路径上消费一次（rm）。**RED 路径下 marker 也 rm**
—— 这跟 stop_hook 在 RED 下保留 intent 的设计不一样，因为 marker
是 launchd-triggered，保留 marker 会让 launchd 反复 respawn watcher
（无限 loop）。设计权衡：

- stop_hook RED：保留 intent → 下次 turn-end retry，agent 修了
  state 自然恢复，不靠 trigger.sh 重跑
- watcher RED：rm marker → 这次 rotation 丢弃，要重跑 trigger.sh
  （rare path：state 在 stop_hook 和 watcher 之间漂移）

### P1.3 验收

机器可判（详见 P1 ship 后的 acceptance 节）：

1. 无 marker → exit 0 静默
2. marker + GREEN + target set + --dry-run → log "[DRY-RUN] would
   send-keys ..." + marker rm + exit 0
3. marker + RED (artificial stale handoff) → "blocked at watcher
   gate" + marker rm + exit 1
4. marker 空内容 → "is empty · dropping" + marker rm + exit 1
5. marker + GREEN + no target → "no tmux target" + marker rm +
   exit 1

`--apply` 模式由 takagi 在 P1.5 dogfood 时显式开启；P1.3/P1.4 默认
`--dry-run` 让 takagi 可以观察 5 次完整路径才决定 go-live。

## launchd LaunchAgent（P1.4 SHIPPED — plist + install/uninstall）

P1.4 把 watcherd 接到 launchd `WatchPaths`：每次 `.claude/autorun-marker`
被 create / modify / delete, launchd 自动 spawn 一次 watcherd。**没有
长驻 daemon**——launchd 本身 already 长驻，watcherd 是按事件 fork 的
single-shot child。

### 文件结构

```
hardev/autorun/com.hardev.autorun.plist.template   ← in tree, has placeholders
hardev/autorun/install_launchd.sh                  ← sed-fills template → ~/Library/LaunchAgents/
hardev/autorun/uninstall_launchd.sh                ← bootout + rm
~/Library/LaunchAgents/com.hardev.autorun.plist    ← per-developer, NOT in tree
~/Library/Logs/hardev/autorun.{out,err}.log         ← stdout/stderr of watcherd
```

### plist 关键字段

| 字段 | 值 | 为什么 |
|---|---|---|
| `Label` | `com.hardev.autorun` | 唯一 launchctl id |
| `ProgramArguments` | `[bash, watcherd.sh, --dry-run]` | safety default —— **不会真 send-keys** until takagi 显式改成 `--apply` |
| `WatchPaths` | `.claude/autorun-marker` | launchd 监听文件 create/modify/delete |
| `StandardOut/ErrPath` | `~/Library/Logs/hardev/autorun.{out,err}.log` | 持久 log；rm 不影响 service |
| `RunAtLoad` | `false` | load 时不立即跑（双保险） |
| `KeepAlive` | `false` | one-shot per event（watcherd 跑完即退） |
| `EnvironmentVariables.PATH` | `/opt/homebrew/bin:...` | launchd 默认 PATH 太简，tmux/fswatch 找不到 |

### Install / Uninstall

```bash
# Install (idempotent — replaces any existing load):
hardev/autorun/install_launchd.sh

# Disable / remove:
hardev/autorun/uninstall_launchd.sh
```

`install_launchd.sh` 跑 `plutil -lint` 验 plist 合法，`launchctl bootstrap`
load 进 GUI launchd domain。`uninstall_launchd.sh` `launchctl bootout`
+ `rm` plist。logs 保留供 audit。

### 切换 mode

`install_launchd.sh` 接受 `--mode <--dry-run|--apply>`（默认 `--dry-run`）。
切换不靠手 sed —— 直接：

```bash
# Go live (real send-keys; only after P1.5 dogfood GREEN):
hardev/autorun/install_launchd.sh --mode --apply

# Back to dry-run safety:
hardev/autorun/install_launchd.sh --mode --dry-run
```

Mode 烧进 plist 的 `ProgramArguments[2]`（template 占位符 `@@MODE@@`）。
重跑 install 自动 bootout + 写新 plist + bootstrap，所以 launchctl cached
args 也更新到位（手 sed plist 后再调 install 会被 template 覆盖回，**不要
那么做**）。

### P1.4 验收

1. `install_launchd.sh` exit 0；`launchctl list | grep hardev` 显示一行
2. 写 `.claude/autorun-marker` → 5 s 内 launchd spawn watcherd →
   marker 消失 + log 出现在 `~/Library/Logs/hardev/autorun.err.log`
3. `uninstall_launchd.sh` exit 0；`launchctl list | grep hardev` 0 行

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
