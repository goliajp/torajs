#!/usr/bin/env bash
#
# typed 覆盖率量具的采集端 —— design-principles §7 的机械证据,
# roadmap P-SURF S0.2。编译端在 crates/torajs-core/src/typed_census.rs。
#
# 它回答一个 test262 簇数回答不了的问题:**typed 面在扩大还是缩小**。
# S0 那 366 条里 76% 藏在 ≤3 条的残尾里,gate 指标只露出 24% —— 一次
# 把 checker 放宽成 any 的"修复"在那个数上看起来是进步,在这个数上
# 才现形。
#
# ## 这个脚本最重要的部分是它能红
#
# 一个采集脚本的失效长得跟数据一模一样:语料没匹配到、二进制是旧的、
# 探针改了名、grep 拼错 —— 每一种都会安静地产出"0 个样本,覆盖率
# 100%"。所以下面四条判据任一不满足就 exit 非 0,而不是写一个好看的
# stamp:
#
#   1. 样本数 < TC_MIN_SAMPLES(默认 40)
#   2. 有 census 读数的样本 < 样本数的 TC_MIN_HIT_PCT%(默认 60)
#   3. scored(typed+any)总数为 0
#   4. tr 不存在 / 不可执行
#
# 判据 2 是其中最值钱的一条:它抓的是"tr 跑了但没打读数",也就是
# 二进制比源码旧的那种失败 —— 与 staticlib_freshness.sh 抓的是同一族。
#
# usage:  typed_census.sh [--stride N] [--list]
# exit:   0 采集成功并写了 stamp / 1 判据不满足 / 9 环境不对

set -u

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." 2>/dev/null && pwd)"
[ -n "$REPO" ] && [ -f "$REPO/Cargo.toml" ] || {
  echo "typed_census: cannot locate repo root (got '${REPO:-}')" >&2; exit 9; }
cd "$REPO" || exit 9

STRIDE="${HARDEV_TC_STRIDE:-17}"
LIST_ONLY=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --stride) STRIDE="$2"; shift 2 ;;
    --list)   LIST_ONLY=1; shift ;;
    *) echo "typed_census: unknown arg '$1'" >&2; exit 9 ;;
  esac
done

MIN_SAMPLES="${HARDEV_TC_MIN_SAMPLES:-40}"
MIN_HIT_PCT="${HARDEV_TC_MIN_HIT_PCT:-60}"
# stamp 路径可注入,自测才能验"它会不会红"而不碰生产 stamp。
STAMP="${HARDEV_TC_STAMP:-$REPO/hardev/typed-census-latest.json}"

# tr 路径走正统解析,不写相对 target/(per-project symlink 是反模式,
# 且 CARGO_TARGET_DIR 可能指到别处)。
TARGET_DIR="$(cargo metadata --no-deps --format-version 1 2>/dev/null \
  | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])' 2>/dev/null)"
[ -n "$TARGET_DIR" ] || { echo "typed_census: cargo metadata failed" >&2; exit 9; }
TR="${HARDEV_TC_TR:-$TARGET_DIR/iter/tr}"
[ -x "$TR" ] || { echo "typed_census: tr not executable at $TR" >&2; exit 9; }

# 语料:conformance 的 .ts / .cts,按 stride 取样。它们是本仓自己的
# fixture,覆盖面跟着语言面长 —— 比 bench 的 46 格宽得多。
# `mapfile` 是 bash 4+;macOS 自带 3.2,dev 机上它不存在 —— 用可移植
# 的 while-read。这条被自测抓到过:mapfile 缺失时脚本在取样前就死,
# 退出码恰好也是 1,于是四条"能红"的用例全部因为错误的原因而通过。
ALL=()
while IFS= read -r _line; do ALL+=("$_line"); done \
  < <(ls conformance/cases/*.ts conformance/cases/*.cts 2>/dev/null | sort)
SAMPLE=()
i=0
for f in ${ALL[@]+"${ALL[@]}"}; do
  if [ $((i % STRIDE)) -eq 0 ]; then SAMPLE+=("$f"); fi
  i=$((i + 1))
done

if [ "$LIST_ONLY" -eq 1 ]; then
  printf '%s\n' ${SAMPLE[@]+"${SAMPLE[@]}"}
  exit 0
fi

n_sample=${#SAMPLE[@]}
if [ "$n_sample" -lt "$MIN_SAMPLES" ]; then
  echo "typed_census: FAIL only $n_sample samples (need >= $MIN_SAMPLES) — corpus glob or stride is wrong" >&2
  exit 1
fi

typed=0; any=0; refused=0; hits=0
for f in ${SAMPLE[@]+"${SAMPLE[@]}"}; do
  line="$(TORAJS_TYPED_CENSUS=1 "$TR" run "$f" 2>&1 </dev/null \
          | grep -m1 '^torajs-typed-census:' || true)"
  [ -n "$line" ] || continue
  hits=$((hits + 1))
  t=$(echo "$line" | sed -n 's/.*typed=\([0-9]*\).*/\1/p')
  a=$(echo "$line" | sed -n 's/.*any=\([0-9]*\).*/\1/p')
  r=$(echo "$line" | sed -n 's/.*refused=\([0-9]*\).*/\1/p')
  typed=$((typed + ${t:-0}))
  any=$((any + ${a:-0}))
  refused=$((refused + ${r:-0}))
done

scored=$((typed + any))
hit_pct=$((hits * 100 / n_sample))

echo "typed_census: samples=$n_sample hits=$hits (${hit_pct}%) typed=$typed any=$any refused=$refused scored=$scored"

fail=0
if [ "$hit_pct" -lt "$MIN_HIT_PCT" ]; then
  echo "typed_census: FAIL only ${hit_pct}% of samples produced a reading (need >= ${MIN_HIT_PCT}%)" >&2
  echo "typed_census:   most likely the tr binary predates the probe — check staticlib_freshness.sh / rebuild" >&2
  fail=1
fi
if [ "$scored" -eq 0 ]; then
  echo "typed_census: FAIL scored=0 — the probe fired but counted nothing" >&2
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  bash "$REPO/hardev/autorun/write_stamp.sh" "$STAMP" \
    typed-census fail "samples=int:$n_sample" "hits=int:$hits" >/dev/null 2>&1
  exit 1
fi

ratio="$(python3 -c "print(f'{$typed/$scored:.4f}')")"
echo "typed_census: typed_ratio=$ratio"

bash "$REPO/hardev/autorun/write_stamp.sh" "$STAMP" \
  typed-census ok \
  "samples=int:$n_sample" "hits=int:$hits" \
  "typed=int:$typed" "any=int:$any" "refused=int:$refused" \
  "scored=int:$scored" "typedRatio=$ratio" "stride=int:$STRIDE" || exit 1

exit 0
