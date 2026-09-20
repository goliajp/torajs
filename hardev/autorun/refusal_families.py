#!/usr/bin/env python3
"""把 checker 的拒收按**消息族**聚类,并给出 S0 三分法的档位。

P-SURF S0.1 / S0.2 的工具。`cluster_incompat.py` 按消息**签名**聚类,
对 checker 拒收这一片答出 366 条 / **230 个签名**(top20 只覆盖 28%)——
那个粒度上没有人能逐条判。但同一批按**族**聚只有 10 个,覆盖 91%,
而族才是「spec 有没有为这个操作定义运行期语义」这个问题问得动的粒度。

三个档位(roadmap P-SURF S0,design-principles §7.2):

  throw   spec 定义了运行期语义,而类型已知 → typed 路径直接 emit
          那次 throw,运行期零检查。这是定位的具体形态:bun 必须在
          运行期做两次 ToNumeric 才知道要抛,tsc 只报错不给可执行物。
  refuse  静态可证必错,且 spec 无对应运行期语义 —— 多半是 TS 语言面
          自己的东西(变量的槽类型、泛型参数)。编译期拒是对的。
  split   同一族里 dot 与 bracket 两种拼写该走不同的路。TS 自己就把
          它们分成两类(`m.a` 是无条件的 TS2339;`m["a"]` 是 TS7052,
          只在 noImplicitAny 下),所以这不是我们选的立场,是与 TS 对齐。

档位标在族上是**起点不是终点**:S0.1 要求逐条落档,族给的是默认值,
逐条复核时可以推翻(r609/r610 的教训:凡是写着「理由」的登记,第一件
事是拿判据问它一遍)。

usage:  refusal_families.py <incompat.ndjson> [--list FAMILY]
"""

import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cluster_incompat as C  # noqa: E402

# 顺序即优先级:一条消息归第一个命中的族。F3 放在靠后,因为它的模式
# 最宽,先让更specific的族挑走自己的。
FAMILIES = [
    (
        "F1",
        "运算符操作数类型",
        "throw",
        r"(arithmetic|bitwise|comparison|shift|relational).*operands"
        r"|not defined on BigInt operands|mixing BigInt",
        "§13.15.3 ToNumeric 类型不等则抛;§6.1.6 BigInt 与 Number 不混用。"
        "类型已知,编译期就能算出结果是抛。",
    ),
    (
        "F2",
        "调用非函数",
        "throw",
        r"not callable|is not a function|value is not a constructor",
        "§7.3.14 Call 要求 IsCallable,否则 TypeError。",
    ),
    (
        "F4",
        "成员/字段不存在",
        "split",
        r"no member |no field |has no member",
        "JS 读不存在的成员答 undefined 而不抛,所以 bracket 拼写该走 any 车道;"
        "而 TS 对 dot 拼写是无条件的 TS2339 —— 两种拼写分开走,与 TS 对齐。",
    ),
    (
        "F5",
        "赋值/槽类型不匹配",
        "refuse",
        r"type mismatch|mismatch assigning|element assignment"
        r"|assignment to .* mismatch|slot is",
        "JS 没有变量类型这个概念,spec 因此没有对应的运行期语义。"
        "这是 TS 语言面自己的东西,编译期拒是对的。",
    ),
    (
        "F6",
        "const 重赋值",
        "refuse",
        r"cannot assign to const",
        "spec 侧是运行期 TypeError,但 tsc 是编译期 TS2588 —— 与 tsc 对齐。",
    ),
    (
        "F7",
        "重复声明",
        "refuse",
        r"redeclaration|duplicate",
        "早错(SyntaxError),spec 要求在求值前拒。",
    ),
    (
        "F8",
        "未知标识符",
        "throw",
        r"unknown ident",
        "§9.1.1.1 未解析的引用是运行期 ReferenceError。tr 已有先例:"
        "TDZ 读在解析期标记、运行期抛。",
    ),
    (
        "F9",
        "所有权/别名",
        "refuse",
        r"cannot transfer|already aliased or moved",
        "tr 自己的所有权概念,spec 无对应。**但它也不是 TS 的** —— "
        "逐条复核时要单独审视这一族是否该存在(design-principles 禁止"
        "把 Rust 习语带进语言面)。",
    ),
    (
        "F10",
        "泛型/类型参数",
        "refuse",
        r"type parameter|inferred as",
        "TS 类型系统自己的判断,spec 无对应。",
    ),
    (
        "F3",
        "内建实参/接收者类型",
        "throw",
        r"argument \d+:|arg \d*\s*\(?\w*\)?\s*must be|arg must be|N-arg form"
        r"|must be number|expected \d+ argument|requires .*(arg|struct)"
        r"|receiver must be|target must be|value must be|primitive-coercible"
        r"|must be a string, number or symbol|must match elem type"
        r"|expected element type|source\[\d*\]|cb must be",
        "**最大的一族,而且它的档位只对了一半。** 内建的参数处理在 spec 里"
        "几乎总是「ToXxx 强制转换,失败则抛 TypeError」,两种结果都是运行期"
        "语义 —— 但 2026-09-20 的 5 条抽样里有 2 条 **tr 的运行期语义本身"
        "就是错的**(`Object.assign(\"x\", {...})` 不该抛却抛了;"
        "`BigInt({})` 该抛 SyntaxError 却抛 TypeError)。所以本族要再切:"
        "运行期已正确的才是档 B;运行期也错的属于底层机制缺口,不在 S0 范围。"
        "**逐条落档必须对每条跑一次 any-拼写探针,不能按族批量判。**",
    ),
]


def family_of(msg: str):
    for fid, name, verdict, pat, why in FAMILIES:
        if re.search(pat, msg, re.I):
            return fid, name, verdict, why
    return "F0", "未归族", "?", "需要新族或逐条判"


def main() -> None:
    src = sys.argv[1] if len(sys.argv) > 1 else "incompat.ndjson"
    want = None
    if "--list" in sys.argv:
        want = sys.argv[sys.argv.index("--list") + 1]

    rows = [json.loads(line) for line in open(src) if line.strip()]
    core = [r for r in rows if not C.is_post_v1(r["path"], r["msg"])]
    refusals = [r for r in core if "type error" in r["kind"]]
    if not refusals:
        # 空输入必须红:一个 0 条的"分析"与"全部归档完毕"长得一样。
        print("refusal_families: no checker refusals in input — wrong file?", file=sys.stderr)
        sys.exit(1)

    by_fam = defaultdict(list)
    for r in refusals:
        fid, _, _, _ = family_of(r["msg"])
        by_fam[fid].append(r)

    if want:
        for r in by_fam.get(want, []):
            print(f"{r['path']}\n    {r['msg'][:150]}")
        return

    print(f"checker refusals in core: {len(refusals)}   signatures: "
          f"{len(set(C.signature(r['msg']) for r in refusals))}")
    print(f"{'fam':<5}{'n':>5}  {'verdict':<8}name")
    verdict_total = Counter()
    for fid, name, verdict, _, _ in FAMILIES:
        n = len(by_fam.get(fid, []))
        if n:
            print(f"{fid:<5}{n:>5}  {verdict:<8}{name}")
            verdict_total[verdict] += n
    n0 = len(by_fam.get("F0", []))
    if n0:
        print(f"{'F0':<5}{n0:>5}  {'?':<8}未归族")
        verdict_total["?"] += n0

    covered = len(refusals) - n0
    print(f"\n归族率 {covered * 100 // len(refusals)}% ({covered}/{len(refusals)})")
    print("\n三分法的账:")
    for v in ("throw", "refuse", "split", "?"):
        if verdict_total[v]:
            print(f"  {v:<8}{verdict_total[v]:>5}  "
                  f"{verdict_total[v] * 100 // len(refusals)}%")
    print("\n★ 档位标在族上是起点不是终点 —— S0.1 要求逐条落档,族给的是默认值。")


if __name__ == "__main__":
    main()
