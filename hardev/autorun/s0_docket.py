#!/usr/bin/env python3
"""S0.1 的逐条档案 —— 把 checker 的每一条拒收定到三分法的一档。

`refusal_families.py` 按**族**给默认档位,而 ch1 §3.1 已经记下族的默认
值会错:F3 标的是「运行期语义早已完整,差别只在 checker 拦不拦」,一问
就发现其中一部分**运行期语义本身是错的**。所以 S0.1 要求逐条落档。

逐条落档的机械办法是 `crate::s0_admit` 那个闸:带 `TORAJS_S0_ADMIT=1`
跑一次全量 sweep,checker 的 Error 降级成 warning,于是**每条被拒的
case 自己就是判据 2 的探针**,它闸开后的结局直接给出档位:

  B          闸开后转为通过 —— 运行期语义早已正确,S0.4 落地即通过。
  L1         闸开后跑起来了但答案仍不对 —— 运行期语义有缺口,不在 S0
             范围(先修运行期,再谈 typed 直出)。
  S0.4       闸开后仍被编译期拒,或崩 —— typed 车道没有这条路,
             S0.4 要为它 emit 那次 throw。这是本阶段真正的工作面。

用一行手写 `any` 探针问同一个问题是**近似**:探针写错了整档就错。这里
用的是 test262 自己的语料,不是近似。

usage:
  s0_docket.py <baseline-incompat.ndjson> <admit-verdicts.txt> [--list BUCKET]
               [--family F3]

其中 admit-verdicts.txt 是带闸 sweep 的 `--dump-verdicts` 产物。
"""

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cluster_incompat as C  # noqa: E402
import refusal_families as F  # noqa: E402

# 闸开后的 verdict → 档位。判的是 verdict 的**前缀**,因为 runner 把
# oracle 的有无编进了同一个字段(`pass` / `pass-no-oracle` 都是通过)。
PASSING = ("pass",)
# 仍被编译期拒的形态。`type error` 是 checker 自己;`parse error` /
# `lex error` 不可能被这个闸影响,它们出现在这里说明**基线数据与闸开
# sweep 不是同一份语料**,要当装置故障看。
STILL_REFUSED = ("incompatible:type error", "incompatible:no-oracle:type error")


def bucket_of(verdict: str) -> str:
    """闸开后的 verdict 落哪一档。"""
    if verdict.startswith(PASSING):
        return "B"
    if verdict in STILL_REFUSED:
        return "S0.4"
    if "exit 139" in verdict or "tr-timeout" in verdict:
        # 崩 / 挂:typed 车道放行之后 lowering 没有对应的路。与「仍被
        # 拒」同属 S0.4 的工作面,但单独标出来 —— 它比一条诊断更贵。
        return "S0.4-crash"
    return "L1"


def read_verdicts(path: str) -> dict:
    out = {}
    for line in open(path):
        if "\t" not in line:
            continue
        p, v = line.rstrip("\n").split("\t", 1)
        out[p] = v
    if not out:
        # 空的 verdicts 与「全部通过」长得一样 —— 必须红。
        sys.exit(f"s0_docket: no verdicts parsed from {path} — wrong file?")
    return out


def main() -> None:
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    baseline, admit_verdicts = sys.argv[1], sys.argv[2]
    want_bucket = None
    want_family = None
    if "--list" in sys.argv:
        want_bucket = sys.argv[sys.argv.index("--list") + 1]
    if "--family" in sys.argv:
        want_family = sys.argv[sys.argv.index("--family") + 1]

    rows = [json.loads(line) for line in open(baseline) if line.strip()]
    core = [r for r in rows if not C.is_post_v1(r["path"], r["msg"])]
    refusals = [r for r in core if "type error" in r["kind"]]
    if not refusals:
        sys.exit(f"s0_docket: no checker refusals in {baseline} — wrong file?")

    verdicts = read_verdicts(admit_verdicts)

    missing = [r for r in refusals if r["path"] not in verdicts]
    if missing:
        # 基线里有、闸开 sweep 里没有 = 两份产物不同源。任何据此得出
        # 的档位都是假的,所以这里退出而不是跳过。
        sys.exit(
            f"s0_docket: {len(missing)}/{len(refusals)} refusals absent from "
            f"{admit_verdicts} (e.g. {missing[0]['path']}) — the two runs are "
            f"not the same corpus"
        )

    docket = []
    for r in refusals:
        fid, fname, fverdict, _ = F.family_of(r["msg"])
        v = verdicts[r["path"]]
        docket.append(
            {
                "path": r["path"],
                "family": fid,
                "family_name": fname,
                "family_default": fverdict,
                "msg": r["msg"],
                "admit_verdict": v,
                "bucket": bucket_of(v),
            }
        )

    if want_bucket or want_family:
        for d in docket:
            if want_bucket and d["bucket"] != want_bucket:
                continue
            if want_family and d["family"] != want_family:
                continue
            print(f"{d['bucket']:<10}{d['family']:<5}{d['path']}")
            print(f"          {d['msg'][:130]}")
            print(f"          admit → {d['admit_verdict']}")
        return

    n = len(docket)
    print(f"S0.1 docket: {n} checker refusals in core\n")
    counts = Counter(d["bucket"] for d in docket)
    print(f"{'bucket':<12}{'n':>5}  {'%':>4}  含义")
    meaning = {
        "B": "闸开即通过 —— 运行期已正确,S0.4 落地即拿下",
        "S0.4": "闸开仍被编译期拒 —— typed 车道要 emit 那次 throw",
        "S0.4-crash": "闸开后崩/挂 —— lowering 无对应路径",
        "L1": "闸开跑起来但答案仍不对 —— 运行期缺口,不在 S0 范围",
    }
    for b in ("B", "S0.4", "S0.4-crash", "L1"):
        if counts[b]:
            print(f"{b:<12}{counts[b]:>5}  {counts[b] * 100 // n:>3}%  {meaning[b]}")

    print("\n按族 × 档(行 = 族的默认值,列 = 实测):")
    per_fam = defaultdict(Counter)
    for d in docket:
        per_fam[(d["family"], d["family_default"])][d["bucket"]] += 1
    print(f"{'fam':<5}{'默认':<8}{'B':>6}{'S0.4':>6}{'crash':>6}{'L1':>6}   族名")
    for (fid, default), c in sorted(per_fam.items()):
        name = next((f[1] for f in F.FAMILIES if f[0] == fid), "未归族")
        print(
            f"{fid:<5}{default:<8}{c['B']:>6}{c['S0.4']:>6}"
            f"{c['S0.4-crash']:>6}{c['L1']:>6}   {name}"
        )

    print("\n★ 族的默认值与实测不一致的格,就是 ch1 §3.1 那条纪律兑现的位置。")


if __name__ == "__main__":
    main()
