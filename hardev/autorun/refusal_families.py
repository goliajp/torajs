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
        r"|not defined on BigInt operands|mixing BigInt"
        # `+` / `**` / 一元 `+` 与 bitwise 是同一句 ToNumeric,只是消息
        # 没写 arithmetic/bitwise 这些词,纯正则漏网(r676 补)。
        r"|requires matching number|on bigint is a TypeError",
        "§13.15.3 ToNumeric 类型不等则抛;§6.1.6 BigInt 与 Number 不混用;"
        "§13.6 `**` 同款;§7.1.4 ToNumber(BigInt) 抛。"
        "类型已知,编译期就能算出结果是抛。",
    ),
    (
        "F2",
        "调用非函数",
        "gap",
        r"not callable|is not a function|value is not a constructor",
        "§7.3.14 Call 要求 IsCallable,否则 TypeError —— **但 r677 逐条读下来,"
        "今天命中它的 15 条一条也不是这个**。15 条里 14 条的消息是同一句 "
        "`not callable: type Object(\"Function\")`,而它们全都是 "
        "`Function(p, \"return …\")` 这种**实参不是常量字符串**的动态函数构造:"
        "`ast/desugar_eval/function_ctor.rs` 只在实参全为常量串时做 §20.2.1.1 的"
        "文本装配,其余按模块 doc 明示的 honest-reject 留给今天的 loud reject"
        "(运行期字符串要把编译器放进产物,那是 B 层)。`Function` 于是以"
        "「一个不可调用的值」的形态落到通用调用检查上 —— 那句消息是二次症状,"
        "不是判断。**档位因此是 gap(实现缺口),不是 throw。** "
        "裸 `Function(\"a\",\"return a+1\")` 与 `new Function(…)` 今天都能跑,"
        "实测与 bun 一致;差别只在实参是不是常量。",
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
        "这是 TS 语言面自己的东西,编译期拒是对的。"
        "**r677 逐条过 tsc:51 条里 39 条 tsc 同判(默认值正确),"
        "12 条 tsc 接受 = checker 推断过窄**,不占档位,是加宽点。"
        "六条轴见 ch1 §3.3:裸 `var x;` 后赋两种形状(7)· 循环体内裸 "
        "`let`(1)· 生成器函数表达式各铸一个名义 ClassRef(1)· "
        "`var x = {}` 之后赋任意对象(1)· getter-only 改赋 setter-only(1)· "
        "只抛出的箭头函数返回 never(1)。",
    ),
    (
        "F6",
        "const 重赋值",
        "throw",
        r"cannot assign to const",
        "**r677 推翻了原来的 refuse**:§6.2.5.6 PutValue → §9.1.1.1.5 "
        "SetMutableBinding 步骤 5 对 strict immutable binding 强制 "
        "S = true,所以规范给的是运行期 TypeError,而 test262 的四条 "
        "`using-invalid-assignment-*` 是把写包在 "
        "`assert.throws(TypeError, ...)` 里的 **positive** 测试 —— 文件被拒"
        "就一条断言都跑不了。tsc 报 TS2588、bun 报「This assignment will "
        "throw」,两者都知道结果却都不给可执行物,而 ch1 §4 的表早就把这一行"
        "归了档 B。已落地(`Ast::const_writes` → `lower_immutable_write_throw`),"
        "下一次 sweep 这一族应当归零。",
    ),
    (
        "F7",
        "重复声明",
        "refuse",
        r"redeclaration|duplicate",
        "早错(SyntaxError),spec 要求在求值前拒 —— 规则本身没错,**但 r677 "
        "逐条读源码发现今天命中它的七条一条都不是真重复声明**:五条是自引用"
        "模块被处理多遍或 namespace desugar 把绑定铸进用户作用域"
        "(`instn-once` / `eval-self-once` / `export-star-as-dflt` / "
        "`own-property-keys-binding-types` / `import-defer-does-not-evaluate` —— "
        "被点名的 `dflt1` / `named` / `ns_1_2` 在源码里各只声明一次),"
        "一条是 `export function` 铸两遍(`verify-dfs`),一条是 harness 与 "
        "case 各写一个 `var $262`(同作用域 `var` 重复声明在 JS 里合法)。"
        "前六条属 L1 模块机制缺口,末一条是收窄。**族的默认值与命中它的条目"
        "可以整体不一致** —— 这是逐条落档存在的理由。",
    ),
    # ★ F16 必须排在 F8 之前 —— 两者消息同形,差别只在名字是谁铸的。
    (
        "F16",
        "编译器合成名未解析",
        "gap",
        r"unknown identifier `__",
        "**不是三分法的一档,也不是 F8。** 这些名字带 `__` 前缀,是 desugar "
        "自己铸的内部符号(`__superbase__` / `__nsdefault___dyn_ns_*` / "
        "`__cm_<C>__<m>` / `__cc0_*`),不是用户程序写下的标识符 —— §9.1.1.1 "
        "的运行期 ReferenceError 与它们无关。它们说的是某条 desugar 链没接上"
        "(super 机制 / 动态 import 命名空间 / 类方法 / 捕获车道),属 L1 底层"
        "机制,不在 S0 范围。r676 实测:F8 剩下的 23 条**全部**是这一类,"
        "一条真正的用户未声明标识符都没有 —— 那条机制早已建成(它发 warning "
        "不拒收),所以真正落进 checker 拒收的只剩内部符号。",
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
        "F11",
        "迭代源/索引接收者类型",
        "throw",
        r"can't index into",
        "§13.7.5.6 ForIn/OfHeadEvaluation → §7.4.2 GetIterator:非对象 / 无 "
        "`@@iterator` 是**运行期** TypeError。今天这一族全是 for-of 头;"
        "将来若混进裸 `false[0]`(JS 里合法,答 undefined)本族要 split。",
    ),
    (
        "F12",
        "switch case 与 scrutinee 类型不等",
        "throw",
        r"differs from scrutinee",
        "§14.12.4 CaseBlockEvaluation 用 IsStrictlyEqual,而它是**全函数** —— "
        "跨类型答 false,不是未定义。规范里没有「case 类型须等于 scrutinee」"
        "这条约束,所以拒收是把 B 当成了 A。发射点的注释自己已经写对了这个"
        "道理(为 `any` 开了口),只是没推到非 any 的跨类型对。",
    ),
    (
        "F13",
        "分支类型合一(ternary join)",
        "any",
        r"ternary branches differ",
        "§13.14 ConditionalExpression 对两个分支的类型**零要求**。判据 4 说"
        "「规范没有这个概念」→ 但它也**没有**对应的运行期错误,所以不是 A "
        "是 C:join 到 Any。同文件已为「两个无共同祖先的用户类」定了 "
        "join-to-Any 的 posture,这里只是没把 Struct/ClassRef/Number 的混合"
        "也交给它。",
    ),
    (
        "F15",
        "私有字段未声明",
        "throw",
        r"private field `#.*is not declared",
        "§15.7 把类体内每个 PrivateName 放进整个类体的作用域。引用**可解析**"
        "时不匹配是 §7.3.32 PrivateElementFind 的运行期 brand check"
        "(PrivateFieldSet step 4 TypeError);不可解析才是 §13.1 的早错。"
        "读路径已经查过作用域表,**写路径漏了那一跳** —— 不是策略分歧。",
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
    # ★ F14 必须排在 F3 **之后** —— `not yet supported` 与 F3 的
    # `primitive-coercible` 有重叠,放前面会从 F3 偷走条目(实测 3 条)。
    (
        "F14",
        "实现缺口(not-yet-supported)",
        "gap",
        r"not yet supported|only valid inside an array literal",
        "**不是三分法的一档**,是 ch1 §3.1 划出的那条边界:这一族的成员各自"
        "要跑一次判据 2 才知道自己在哪边 —— 运行期语义已正确的是档 B"
        "(checker 白名单/形状漏项,直接进 S0.4);运行期本身有分歧的归 L1,"
        "不在 S0 范围。标 gap 是为了不让它被当成一整族批量处理。",
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
    for v in ("throw", "refuse", "split", "any", "gap", "?"):
        if verdict_total[v]:
            print(f"  {v:<8}{verdict_total[v]:>5}  "
                  f"{verdict_total[v] * 100 // len(refusals)}%")
    # 账目必须闭合。一个漏掉某个 verdict 的汇总与一个"这些条目不存在"
    # 长得一样 —— 加一个 verdict 却忘了把它加进上面的遍历,正是这个量具
    # 会骗人的方式(r676 当场踩过一次,360 对 366)。
    booked = sum(verdict_total.values())
    if booked != len(refusals):
        sys.exit(
            f"refusal_families: 账目不闭合 —— 汇总 {booked} 条,实际 "
            f"{len(refusals)} 条。某个 verdict 没进上面的遍历列表。"
        )
    print("\n★ 档位标在族上是起点不是终点 —— S0.1 要求逐条落档,族给的是默认值。")


if __name__ == "__main__":
    main()
