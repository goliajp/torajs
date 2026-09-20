#!/usr/bin/env bash
#
# typed_census.sh 的自测 —— 验的是**它会不会红**,不是它算得对不对。
#
# 为什么需要这个:采集脚本的失效与"数据很好"长得一模一样。一个只会
# 绿的量具比没有量具更糟,因为它会让"typed 面在缩小"这件事看起来像
# 没发生(`test/instrument-must-be-able-to-fail`)。
#
# 手法:注入一个假 tr(`HARDEV_TC_TR`)与一个临时 stamp 路径
# (`HARDEV_TC_STAMP`),所以整个自测不碰真二进制、不碰生产 stamp。
#
# 阳性对照:把 AUDIT 指向一个恒 exit 0 的假脚本,下面 8 条里的 6 条
# 红色用例必须全部失败。验过这一条,这个自测本身才算能红。
#
# exit: 0 全过 / 1 有用例不符

set -u
REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." 2>/dev/null && pwd)"
[ -f "$REPO/Cargo.toml" ] || { echo "self-test: bad repo root" >&2; exit 9; }
AUDIT="${TC_AUDIT_UNDER_TEST:-$REPO/hardev/autorun/typed_census.sh}"

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
pass=0; fail=0

mk_tr() { # $1=name  $2=stdout-line ("" = print nothing)  $3=every-nth (default 1)
  local p="$TMP/$1"
  {
    echo '#!/usr/bin/env bash'
    echo "N_FILE=$TMP/$1.n"
    echo 'n=$(cat "$N_FILE" 2>/dev/null || echo 0); n=$((n+1)); echo "$n" > "$N_FILE"'
    if [ -n "$2" ]; then
      echo "if [ \$((n % ${3:-1})) -eq 0 ]; then echo '$2' >&2; fi"
    fi
    echo 'exit 0'
  } > "$p"
  chmod +x "$p"
  echo "$p"
}

run() { # $1=label  $2=expected-exit  rest=env assignments
  local label="$1" want="$2"; shift 2
  local out rc
  out="$(env "$@" HARDEV_TC_STAMP="$TMP/stamp.json" bash "$AUDIT" 2>&1)"; rc=$?
  if [ "$rc" -eq "$want" ]; then
    pass=$((pass+1)); echo "  ok   $label (exit $rc)"
  else
    fail=$((fail+1)); echo "  FAIL $label: expected exit $want, got $rc"
    echo "$out" | sed 's/^/       /' | tail -3
  fi
}

GOOD='torajs-typed-census: typed=3 any=1 refused=0 scored=4 typed_ratio=0.7500'
ZERO='torajs-typed-census: typed=0 any=0 refused=9 scored=0 typed_ratio=none'

echo "typed_census self-test ($AUDIT)"

# 1. 探针不出声 = 二进制比源码旧的那种失败。这是最值钱的一条。
run "silent tr is caught" 1 \
  HARDEV_TC_TR="$(mk_tr silent '')" HARDEV_TC_STRIDE=200 HARDEV_TC_MIN_SAMPLES=10

# 2. 探针出声但什么都没数到。
run "scored=0 is caught" 1 \
  HARDEV_TC_TR="$(mk_tr zero "$ZERO")" HARDEV_TC_STRIDE=200 HARDEV_TC_MIN_SAMPLES=10

# 3. 只有一半样本有读数(50% < 60% 下限)。
run "partial coverage is caught" 1 \
  HARDEV_TC_TR="$(mk_tr half "$GOOD" 2)" HARDEV_TC_STRIDE=200 HARDEV_TC_MIN_SAMPLES=10

# 4. 语料太小 = glob 写错或 stride 离谱。
run "tiny corpus is caught" 1 \
  HARDEV_TC_TR="$(mk_tr ok1 "$GOOD")" HARDEV_TC_STRIDE=9999 HARDEV_TC_MIN_SAMPLES=10

# 5. tr 根本不在。
run "missing tr is caught" 9 \
  HARDEV_TC_TR="$TMP/nope" HARDEV_TC_STRIDE=200 HARDEV_TC_MIN_SAMPLES=10

# 6. 未知参数不静默吞掉。单独写:`run` 把剩余参数交给 env,而 env 会
#    把 `--bogus` 当成要执行的命令名(127),量不到 AUDIT 自己的退出码。
_tr6="$(mk_tr ok2 "$GOOD")"
_out6="$(HARDEV_TC_TR="$_tr6" HARDEV_TC_STAMP="$TMP/stamp6.json" bash "$AUDIT" --bogus 2>&1)"
_rc6=$?
if [ "$_rc6" -eq 9 ]; then
  pass=$((pass+1)); echo "  ok   unknown arg is caught (exit 9)"
else
  fail=$((fail+1)); echo "  FAIL unknown arg is caught: expected exit 9, got $_rc6"
  echo "$_out6" | sed 's/^/       /' | tail -2
fi

# 7. 正常一轮必须绿。
run "healthy run is green" 0 \
  HARDEV_TC_TR="$(mk_tr ok3 "$GOOD")" HARDEV_TC_STRIDE=200 HARDEV_TC_MIN_SAMPLES=10

# 8. 绿的那一轮要写出正确的 stamp(累加 + 比值 + verdict)。
if [ -f "$TMP/stamp.json" ] && python3 - "$TMP/stamp.json" <<'PY'
import json,sys
d=json.load(open(sys.argv[1]))
assert d["verdict"]=="ok", d["verdict"]
assert d["typed"]==3*d["hits"], (d["typed"], d["hits"])
assert d["any"]==1*d["hits"], (d["any"], d["hits"])
assert d["typedRatio"]=="0.7500", d["typedRatio"]
assert d["tool"]=="typed-census"
PY
then pass=$((pass+1)); echo "  ok   stamp carries summed counts and ratio"
else fail=$((fail+1)); echo "  FAIL stamp contents wrong or missing"; fi

echo "typed_census self-test: $pass passed, $fail failed"
[ "$fail" -eq 0 ] || exit 1
exit 0
