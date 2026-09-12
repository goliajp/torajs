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
- `hardev/autorun/preflight_mini.sh [-c crate]... [-F fixture]...` — **A 轨 pre-flight 一键版**(rotation 639):把 dev 上
  `git status` 的 crates/ + conformance/cases/ 改动逐文件 rsync 到 mini,在 login zsh 里跑 `cargo fmt`(触及的 crate)/
  `cargo fmt --all -- --check`(文件名列表,不截断)/ iter 与四目标 release 两次 build(各自 exit 码 + warning 计数)/ 触及 crate 的 nextest /
  `staticlib_freshness.sh iter` / 每条触及 fixture 的 EQ·DIFF(`.cts` 对 `.expected`,`.ts` 对 bun),再按内容取回 fmt 改动、跑本地
  `file_size_audit.sh`,最后一行 `PREFLIGHT PASS` 或 `PREFLIGHT FAIL: <原因>`(exit 1)。**能红**:rotation 639 用一条 expected 写错的临时
  fixture 验过 `PREFLIGHT FAIL: fixture: …`。
- `hardev/autorun/fmt_takeback.sh [file...]` — 按**内容**(dev/mini md5 逐文件比对,一次 ssh)取回 mini 上被 `cargo fmt` 改写的文件;
  无参数时取 `git status` 里 crates/ 下的改动集。mini 的 `git status` 对 rsync 树列出几百个 `M`,不能用来找 fmt 改动(rotation 639)。
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
{"rotationId":"r-1747836296-a1b2","at":"2026-05-20T12:34:56Z","ts":1747836296,"project":"torajs","trigger":"manual","prevHead":"aaaef71","handoffSha":"sha256:abc...","handoffAgeSec":12,"conformanceBefore":"3773/0/4","commitsInSession":12,"axis":"A","triggerReason":"d"}
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
- `conformanceBefore`：conformance 三元组 `N/F/S`，来源是 handoff
  `## rotate 触发` 段的 `gate:` 行（TRIG-3 契约的一部分），缺行置 `null`
  （不 fabricate）。**2026-09-07 修**：此前探的是
  `~/.claude-profile-{2,1}/.../memory/project_status_*.md` —— 两代前的 profile
  记忆形态，目录早已不存在，于是它在**每一行**都返回 null。一个指向已消失世界
  的探针，其失效形态与「这一轮没有数据」完全一致。
- `commitsInSession`：`git rev-list --count <上次 self 的 prevHead>..HEAD`。
  **2026-09-07 修**：此前是硬编码的字面 `null`，909 行一次都没填过——而
  `trig_gate.sh` 每次调用都算出了这个数然后扔掉。调 TRIG-1 阈值时只能靠
  `prevHead` 事后反推。
- `axis`：**2026-09-07 新增**。本轮服务的 v1.0 gate 轴，`A`–`E`，多值逗号分隔；
  来源是 handoff `## rotate 触发` 段的 `axis:` 行。TRIG-5 读它判断轴倾斜。
  旧行无此键 → 读作未知，**打断 streak 而不是延长它**。
- `triggerReason`：handoff 的 `reason: <a|b|c|d>` 字母，缺失置 `null`。
  2026-09-07 起 gate 不再要求这一行（它 81.6% 都填 `d`，而 `d` 的定义就是
  「commit ≥ TRIG-1」——在复读 TRIG-1），但仍记录，历史序列不断。

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

## TRIG-1..7 spec（rotation 触发不变量,`trig_gate.sh`;P2.0 SHIPPED,2026-09-07 重设计,2026-09-09 加 6/7）

INV-1..5 治 rotation _执行_(stop_hook 之前、watcherd 之前 act 前),TRIG-1..7
治 rotation _触发_(trigger.sh self 入口);两侧 gate 互相独立、机器化、
零模型自评估。

**2026-09-07 重设计的实测依据(takagi 发起)**。P2.0 的四条门**全部指向同一个
方向** —— 反「rotate 太早」。于是实际只有 TRIG-1 在咬人,而它已经从「门」退化
成「配额」。重设计前最后 60 轮实测(靠 `prevHead` 逐轮反推 —— 因为
`commitsInSession` 当时是硬编码的 null,见下方「同轮修掉的两处腐坏」):

| 量 | min | p10 | p50 | p90 | max |
|---|---:|---:|---:|---:|---:|
| commit / rotation | **5** | 5 | 6 | 8 | **9** |
| wall (min) | — | 95 | 125 | 221 | — |

`triggerReason` 分布:**d 606 · a 133 · c 2 · b 2**。

三个结论,每个都直接对应一条改动:

1. **commit 分布左端紧贴阈值,0/60 低于 5,几乎没有超过 9** —— 门变成了目标
   (Goodhart)。**→ TRIG-1 抬到 12。**
2. **TRIG-2 的下限 90min 低于实测 p10 95min** —— 它没有约束任何东西,是
   TRIG-1 的影子。**→ TRIG-2 语义反转,管两端。**
3. **`reason: d` 占 81.6%,而 d 的定义就是「commit ≥ TRIG-1」** —— TRIG-3 在
   复读 TRIG-1,不携带信息。**→ TRIG-3 改成 axis / closed / gate 三行契约。**

外加一条只有做整体汇报时才浮出来的事实:**整个 9 月每一轮都在轴 A**,轴 B 自
08-30 起零投入、轴 E 从未起步,而五条门没有一条问过「这一轮属于哪条轴」。
**→ 新增 TRIG-5。**

| ID | 不变量 | 它独有的职责 | 实现 |
|----|--------|------------|------|
| **TRIG-1** | commit ≥ **12** **且** `closed:` 上属于本 session 的 sha ≥ **3** | **下限** — 工作量够不够摊掉固定成本;两个量,因为其中一个可被稀释 | git rev-list + `closed_session_shas` |
| **TRIG-2** | wall ∈ [**1800s**, **18000s**];**≥ 18000s 时豁免 TRIG-1** | **两端 + 唯一的机器化出口** — 太快不真实,太长该收 | rotations.jsonl tail self filter |
| **TRIG-3** | handoff `## rotate 触发` 段含三行:`axis:` / `closed:`(含本 session 区间内真实 sha)/ `gate: N/F/S`(F 必须 0) | **内容** — 哪条轴、闭合了什么、gate 读数多少 | grep + `git rev-list <range>` 校验 sha |
| **TRIG-4** | 同段 grep -i 不含 13 条 blacklist 借口 phrase | **措辞** — 已知的 procrastination 话术 | grep -iE |
| **TRIG-5** | 若前 **7** 轮 axis 与本轮相同,则同段必须含 `axes-review:` 行 **且带 `bench:` 读数**(≤ 14 天,**且 stamp 的 `bunStamped` == GitHub 最新 release**) | **分配** — 一条轴是不是在饿死其它轴;读数是不是对着今天的 bun | rotations.jsonl `axis` + `hardev/bench-latest.json` + `bun_latest_check.sh` |
| **TRIG-6** | 四个 stamp(`test262` / `determinism` / `gmalloc` / `file-size`)的 `headSha` ∈ 本 session 区间、非 `-dirty`、`verdict=ok` | **证据** — 那些机械检查真跑了吗,跑的是这份源码吗 | `hardev/*-latest.json` |
| **TRIG-7** | handoff 的 `sweep:` 行逐 token 等于 `sweep_line.sh` 从 stamp 重新推导的结果 | **誊写** — handoff 里的数字还是 sweep 说的那些吗 | `sweep_line.sh` + git 取基线 |

### 为什么 2026-09-09 又加了两条门(TRIG-6 / TRIG-7)

**前五条读的都是 handoff 写了什么,没有一条读仓库里发生了什么。** TRIG-3 检查
`gate: N/F/S` 的 F=0,却从不问 N 是不是真数;sweep / determinism / Guard-Malloc
三段汇报是自由文本。于是这三种失败对所有门都是隐形的:

- 没跑
- 跑在上一个 HEAD 上
- 上一轮的结果被带到这一轮

而 `rules/torajs-autorun-pipeline.md` 记着这一类至少五次 —— sweep 断线五轮
(r220-224)期间六条 pass 回归无人知晓;r613 拿 mini 上陈旧的 gz 当基线,产出
一份**形状完全正常**的假 diff(315 条变化 / 1 条 pass 回归 / 14 条后退),用一
整轮 A/B 才证伪(真值:119 条、全部前进、零回归);r571 的 neq 计数跨轮比较,
而样本集合在底下平移过。

**→ 四个机械检查各留一份入库 JSON stamp,带上它测的那份源码的 sha**(与
`hardev/test262-latest.json` 同形 —— sweep 的输出本身就是它的 stamp,不需要新
管道)。TRIG-6 读这四个事实,TRIG-7 读 handoff 的 sweep 行是否仍等于 stamp。

**stamp 治什么、不治什么**:治「看起来跟成功一模一样」的意外失败,**不治**手写
一份假 stamp —— 那仍归 `honesty/no-fabricated-data`。

**mini 上的 sha**:工作树靠 rsync 到达而 git HEAD 落在后面,所以远端本地推导的
sha 命名的是错的源码。远端调用方必须用 `HARDEV_HEAD_SHA` 传真值(与
`torajs-test262 --head-sha` 同一约定),stamp 记录它拿到的是哪一种
(`headShaSource`)。

### 为什么 TRIG-1 在 2026-09-09 长出第二个量

N=12 之后的八轮 commit 数:**13,13,14,13,13,13,14,13** —— 8/8 紧贴阈值,正是
让 N=5 变成配额的那个形状。与改动前 51 轮对比:

| 量 | 改前 | 改后 |
|---|---:|---:|
| 阈值 | 5 | 12(+140%) |
| commit/rotation | 6 | 13 |
| commit 中位规模 | 127 行 | **94 行(−26%)** |
| 每轮总产出 | ~762 行 | ~1128 行(+48%) |

**总产出真涨了 48%,改动有效;但 commit 粒度在门下面变细了 26%** —— 这就是抬高
一个计数买到的东西:配额行为从「早收」变成「切细」。**commit 计数天生可稀释。**

`closed:` 条目不是:每条要指名本 session 的一个 sha,并用一句经得起读的话说清
关了什么。凑一条比凑一个 commit 贵一个数量级。所以 TRIG-1 现在两个量都要:
commit ≥ 12(兜底)+ closed 条目 ≥ 3(判据)。wall-cap 豁免同时覆盖两者不变 ——
五小时啃一个硬问题的一轮照样能 close。

### 为什么 N 是 12 而不是 10 或 20

每轮都有**不随 commit 数变化的固定成本**:sweep 507s + `build_determinism`
197s + `gmalloc_scan` 401s = **1105s = 18.4 min** 机器时间,外加写 handoff 与
下一轮重建上下文的模型时间。按实测 p50(6 commit / 125 min)解
`wall(N) = 18.4 + N × 17.8`:

| N | wall 外推 | 机器固定成本占比 | 含 handoff 写作/起手(保守 +20min) |
|---:|---:|---:|---:|
| 6(改前) | 125 min | 14.7% | 30.7% |
| 10 | 196 min | 9.4% | 19.6% |
| **12** | **232 min** | **7.9%** | **16.6%** |
| 20 | 374 min | 4.9% | 10.3% |

6→12 拿走 14.1pp,12→20 只再拿 6.3pp —— 摊薄收益按 1/N 递减,翻倍到 12 就吃掉
大半。而另一侧有硬边界:**历史 wall p90 = 221 min**。N=12 外推的 232 min 落在
有大量先例的区间;**N=20 的 374 min 超出全部 909 轮的观测范围**,而 rotation
存在的唯一理由就是防那个区域的 drift。**12 是证据的最远边,不是保守选择。**

### 为什么 TRIG-2 必须反转(它是 N=12 的必需配套,不是附赠)

12-commit 的下限意味着:一轮花五小时啃一个硬问题(decomposition、E0 形态的
重构)可能**永远凑不满 12 个 commit,于是永远不能 close**。`trigger.sh manual`
是 takagi 的 override 通道,**模型严禁主动走**(见下方 anti-pattern),所以出口
必须机器化 —— 过 `TRIG2_MAX_WALL_SEC` 自动豁免 commit 下限。同一个数字同时
是「这一轮已经长到 drift 风险大过摊薄收益」的判据。新增的 30min 下限管相反的
失败:十二个琐碎 commit 挤在半小时里同样不是一轮。

### handoff `## rotate 触发` 段的精确格式(TRIG-3 / TRIG-5 契约)

```markdown
## rotate 触发

axis: A
closed: 8ffa78b99 键域矩阵 25→12;93df7b61b parser 折叠记下被抹掉的拼写;be4efafee 位移 510→290+199
gate: 3773/0/4

<具体 evidence,引用 commit hash / file:line / cases#xxx>
```

handoff 正文(不限于本段)还必须含两行,**都由脚本生成、直接粘贴,不要手抄**:

```
sweep: head=… pass=…(+27) … conservation=ok base=…    ← hardev/autorun/sweep_line.sh
bench: head=… median=0.5067 target=0.33 shortfall=1.535x …  ← hardev/autorun/bench_latest_line.sh
```

`sweep:` 每轮必需(TRIG-7);`bench:` 仅在 TRIG-5 触发时必需,但写了不亏。

- **`axis:`** — `A|B|C|D|E` 之一或多个(逗号分隔),对应 roadmap `v1.0 release
  gate` 的五轴。写进 rotations.jsonl 的 `axis` 字段,TRIG-5 再读回来。
- **`closed:`** — **至少 3 个** sha 必须落在 `<last-self-prevHead>..HEAD` 区间
  内(2026-09-09 从 1 抬到 3,见上)。**这是唯一无法靠措辞满足的一条**:一轮如果
  没有可指认的产出,就拿不出自己的 commit。整块可以跨多行 —— 解析器读的是从
  `closed:` 到下一个带键行之间的全部内容,不是 `head -1`。
- **`gate:`** — conformance 三元组 `N/F/S`,**F 必须为 0** —— 一轮不带红收尾。
  这一行同时是 `conformanceBefore` 遥测字段的来源(见下)。
- **`axes-review:`** — 仅当 TRIG-5 触发(连续 8 轮同轴)时必须写:其余四轴各自
  的当前状态 + 为什么这一轮仍不动它们,**并带上 `bench:` 那一行的读数**。
  「bench 本轮未跑」是一句真话,但它什么也没说 —— 整个 9 月每一轮都这么写,而
  轴 B 的中位数在 ±3% 里停了五十天,这件事直到做整体汇报时才浮出来。

`reason: <a|b|c|d>` 旧行**不再被 gate 要求**,但 `autorun_trigger_reason` 仍会
读它、仍写进 jsonl(历史序列不断)。要写就写,不写不拒。

### TRIG-5 为什么是「结算」而不是「强制换轴」

强制换轴会与 CLAUDE.md 的「顺序执行计划」HARD RULE 正面冲突,而且**做哪条轴的
决定权属于 takagi**,这条 gate 没有资格越权。它拿掉的是**沉默**:9 月每一轮都
在轴 A、轴 B 停了一个月、轴 E 一行未写,这件事直到做整体汇报时才浮出来 —— 那
太晚了。装上之后它每 8 轮就必须被写出来一次,不管有没有人问。

**向后兼容**:2026-09-07 之前的行没有 `axis` 键,读出来是 `?`,**打断 streak
而不是延长它** —— 所以 gate 需要积累 8 轮新数据后才可能首次触发。

### 同轮修掉的两处腐坏(`lib.sh`)

两处都不是设计问题,是**测量装置自己坏了而没人发现**,且症状与「这一轮没有
数据」完全一致(`perf-decomposition` §1 点名的那个失败模式):

1. **`commitsInSession` 是硬编码的字面 `null`** —— 909 行一次都没填过,而
   `trig_gate.sh` **每次调用都算出了这个数然后扔掉**。调 TRIG-1 时只能靠
   `prevHead` 事后反推整个历史分布。现在走 `autorun_commits_since_last_self`。
2. **`conformanceBefore` 的探针指向 `~/.claude-profile-{2,1}/.../memory/
   project_status_*.md`** —— 两代之前的 profile 记忆形态,目录早已不存在,所以
   它**永远返回空字符串**。现在读 handoff `gate:` 行,即一个**有主的实测值**,
   而不是对别人文件格式的猜测。

顺带给 `ROTATIONS_LOG` / `HANDOFF_FILE` 加了环境覆盖点
(`HARDEV_ROTATIONS_LOG` / `HARDEV_HANDOFF_FILE`),生产不设,只供自测重定向。

调用约定:

```
hardev/autorun/trig_gate.sh
```

- 退出 0:全部 PASS / SKIP / WAIVED;适合作为 `&&` 链 gate(trigger.sh self 前置)
- 退出 1:至少一个 FAIL;stderr 末尾一行 `TRIG-FAILED: TRIG-N [TRIG-M ...]`
- 退出 2:内部错误(lib.sh 缺、git 不可用、rotations.jsonl 缺、handoff.md 缺)

stdout 每条 TRIG 一行 `TRIG-N STATE detail`,STATE ∈ PASS / FAIL / SKIP /
WAIVED,行格式稳定供下游解析。

**自测**:`hardev/autorun/trig_gate_self_test.sh`,10 个 case 覆盖每一条能拦住
或放行 rotation 的分支(happy / commit 不足 / 过 cap 豁免 / 低于 floor / 缺
axis / sha 不在区间 / gate 带红 / blacklist / 轴倾斜未结算 / 轴倾斜已结算),
用 fixture 重定向,不碰生产状态。**2026-09-07 之前 TRIG gate 一直没有自测** ——
这正是上面两处腐坏能长期不被发现的原因之一。

**Trigger.sh 接线**:

```
trigger.sh self   → 调 trig_gate.sh
                    · PASS → 走原有 autorun_new_id + intent + record_rotation
                    · FAIL → exit 1,不 append jsonl 不写 intent
trigger.sh manual → 不调 trig_gate(takagi override 一切;cases#rotate-as-procrastination
                    决断权属用户)
```

**例子**(GREEN):

```markdown
## rotate 触发

axis: A
closed: 8ffa78b99 键域矩阵 25→12;be4efafee 位移 check_assign_target 510→290+199
gate: 3773/0/4

609 handoff 的下一项 #1 被本轮判据证明是 609 自己写错的登记,顺着把 25 格推到
12;剩下 12 格里 9 格用 tsc 查证为有意分歧。5 commit / 跨 parser + checker。
```

→ TRIG-1 12≥12 PASS · TRIG-2 232min ∈ [30,300] PASS · TRIG-3 axis=A + sha 在区间 + gate F=0 PASS · TRIG-4 0 hit PASS · TRIG-5 前 7 轮非全 A PASS。

**例子**(RED — 原始的 ground-truth,四条门当年就是为它写的):

```markdown
## rotate 触发

archive_link.rs file-size HARD limit clear sub-milestone,chunk 2 是
substantial work fresh session 起手更稳,subagent prep 完整。
```

→ TRIG-1 2<12 FAIL · TRIG-2 1299s<1800s FAIL · TRIG-3 无 axis/closed/gate 三行 FAIL · TRIG-4 hit `file-size .* clear` + `substantial work` + `fresh session 更稳` + `subagent prep 完整` 4 个 phrase FAIL · TRIG-5 SKIP(无 axis)。**4 FAIL = rotation 非法**。

**P5 lock 与下一次 measure 节奏**(`metrics.md` §6):

`metrics.md` 从 2026-06-15 起就写着 5 / 5400 是 **candidate,待 1 周实测后
lock(P5)** —— 那个 lock 从未发生,阈值带着 `[D]` 标记跑了近三个月。本次
(2026-09-07)就是它,并且**同一条规矩再来一遍**:

- 上线后 trig_gate 默认 ENABLED(不 dry-run)
- 跑 1 周,`stats.sh` 看 (a) rotation 数是否落到 ~3.5/天 (b) wall p50 是否落在
  ~230min (c) drift 信号(中文破裂 / 4-layer 越层 / silent-wrong)有没有升高
- 1 周后 takagi review:drift 没升 → 可以考虑 16–20;drift 升了 → 退回 10
- **这次有遥测了** —— `commitsInSession` / `axis` 都是真值,不必再事后反推

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
