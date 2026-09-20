# torajs 语言规范 · 第一章 —— 类型系统对 ES 运行期语义的责任

> 状态:草案。P-SURF **S0.3**。本章定的是一条**语言定义**,不是 checker
> 的一个参数;实施见 S0.4,验收读 typed 覆盖率而不是 test262 簇数。

## 0. 这一章回答什么

torajs 是 ts 语法体系下的一门语言,占的位置是
**完整 JS/TS 语义 + 类型参与代码生成到原生机器码**(定位见
`docs/roadmap.md` 的 Positioning 段)。这一格空着不是因为没人想到,是因为
它要求同时回答两个别人都绕开的问题,而**第一个就是本章**:

> TypeScript 的类型注解是 unsound 的,而 TypeScript 官方明确拒绝让类型
> 参与代码生成(Design Goals Non-goal #5:*"emit different code based on
> the results of the type system"*)。那么当类型说一件事、而 ES 规范为
> 同一个操作定义了运行期行为时,**谁说了算**?

已有的两条路各自放弃了一样东西:静态子集路线(AssemblyScript / ArkTS /
Static TypeScript)把语言收窄到注解必然成立;类型擦除路线(bun / Deno /
Node / Perry / Porffor)把类型丢掉。**torajs 两样都不放弃**,所以必须自己
画这条线。

## 1. 三分法

一个表达式的类型与它的操作对不上时,有且只有三种处置:

| # | 情形 | 处置 |
|---|---|---|
| **A** | 静态可证必错,且**规范没有**为该操作定义运行期语义 | **编译期拒** |
| **B** | **规范定义了**运行期语义,而**类型已知** | **typed 路径直接 emit 那次行为**(抛出或强制转换),运行期零检查 |
| **C** | 类型真未知 | **any 车道**,运行期按规范判 |

**B 是这门语言的位置所在。** 以 `1n & 1` 为例——§13.15.3 说 ToNumeric
后类型不等则抛 TypeError,而两个操作数的类型编译期完全已知:

- **bun** 必须在运行期做两次 ToNumeric、一次类型比较才知道要抛。类型被
  丢掉了,它没有别的选择。
- **tsc** 编译期就知道,但它只报错,不给可执行物。
- **torajs** 编译期就知道,**emit 一条无条件 throw**,运行期零检查,而
  程序照跑 —— `assert.throws(TypeError, () => 1n & 1)` 照样捕获。

三者里只有 torajs 同时拿到「编译期知道」与「运行期照规范」。这不是额外
功能,是这条线画对之后的直接结果。

### 1.1 两个方向都是离开那一格

- **把 B 当成 A**(拒收一个规范说合法的程序)→ 往静态子集那一格滑。
- **把 B 当成 C**(降级到 any 了事)→ 往类型擦除那一格滑。

第二种尤其要警惕,因为它是**最省事的修法**:遇到 checker 拦不住的形状,
放宽成 any 就过了。design-principles §7 把这条写成 HARD RULE,而
typed 覆盖率量具(`hardev/autorun/typed_census.sh`)是它唯一的机械证据。

## 2. 判据(机械)

「规范有没有为该操作定义运行期语义」要能机械回答,否则这一章就是 366 条
逐条人判。可用的判据按可靠性排序:

1. **该操作在 ECMA-262 里有编号条款,且条款里出现 `throw a TypeError` 或
   `ToXxx`** → B。内建函数的参数处理几乎总是这个形状("coerce with
   ToXxx, else TypeError"),两种结果都是运行期语义,而两种在类型已知时
   都能编译期算出来。
2. **同一语义的 `any` 拼写今天已经跑得对** → B。这是最便宜的判据,一行
   探针:把接收者或操作数写成 `any`,若 tr 与 bun 逐字相同,说明运行期
   语义早已完整存在,**差别只在 checker 拦不拦**。

   **它有一个机械的全量版,而且必须用它**:手写的 `any` 拼写只是被拒
   程序的**近似**,近似错了整档就错,而 366 条要问 366 次。
   `TORAJS_S0_ADMIT=1`(`crates/torajs-core/src/s0_admit.rs`)让 checker
   的 Error 降级成 warning,于是**每条被拒的 test262 case 自己就是探针**:
   带闸跑一次全量 sweep,`hardev/autorun/s0_docket.py` 直接读回档位 ——
   闸开即通过的是 B,仍被拒的是 S0.4 的工作面,跑起来但答案不对的是
   L1。用的是 test262 自己的语料,不是近似。
3. **TypeScript 自己怎么判**(`node web/node_modules/typescript/bin/tsc
   --noEmit --strict`)→ 区分 A 与 C。tsc 无条件拒的(如 `m.a` 的
   TS2339、`const` 重赋值的 TS2588)归 A;只在 `noImplicitAny` 下报的
   (如 `m["a"]` 的 TS7052)归 C。**这不是我们选的立场,是与 TS 对齐。**
4. **规范里根本没有这个概念** → A。变量的槽类型、泛型参数、所有权都属
   于此:JS 没有变量类型,所以不可能有对应的运行期语义。

判据 2 有一条纪律:**凡是写着「理由」的既有登记,第一件事是拿它问一遍**。
项目里有过多次「理由写得很有说服力、一行探针就推翻」的记录——证伪只要
一行,而信了它要赔一整轮设计。

## 3. 当前账目

`hardev/autorun/refusal_families.py` 按**消息族**聚类并给出默认档位。
按签名聚是 366 条 / 230 个签名(top-20 只覆盖 28%),那个粒度上无法判;
按族聚只有十六个,**覆盖 100%**。

| 族 | 条数 | 默认档 | 规范依据 |
|---|---:|---|---|
| F3 内建实参/接收者类型 | 175 | **B** | 内建的参数处理几乎总是 ToXxx / TypeError |
| F5 赋值/槽类型不匹配 | 51 | A | JS 无变量类型,规范无对应 |
| F1 运算符操作数类型 | 35 | **B** | §13.15.3 / §6.1.6 / §13.6 / §7.1.4 |
| F4 成员/字段不存在 | 31 | **分两路** | JS 读缺失成员答 undefined;TS 对 dot 无条件拒 |
| F16 编译器合成名 | 23 | **gap** | 不是标识符,是 desugar 的内部符号 —— 见 §3.3 |
| F8 未知标识符 | 0 | **B** | §9.1.1.1;这批语料上为空 |
| F2 调用非函数 | 15 | **B** | §7.3.14 IsCallable |
| F10 泛型/类型参数 | 8 | A | TS 类型系统自身 |
| F6 const 重赋值 | 7 | A | 与 tsc 的 TS2588 对齐 |
| F7 重复声明 | 7 | A | 早错,求值前拒 |
| F14 实现缺口 | 4 | **gap** | 不是一档 —— 见下 |
| F9 所有权/别名 | 3 | **删**(§3.2) | 既不是规范的也不是 TS 的 |
| F11 迭代源/索引接收者 | 2 | **B** | §13.7.5.6 → §7.4.2 GetIterator |
| F12 switch case 类型 | 2 | **B** | §14.12.4 IsStrictlyEqual 是全函数 |
| F13 ternary 分支合一 | 2 | **C** | §13.14 对分支类型零要求 |
| F15 私有字段未声明 | 1 | **B** | §15.7 词法可解析 → §7.3.32 运行期 brand check |

**合计:B 230 条(62%)· A 76 条(20%)· 分两路 31 条(8%)·
C 2 条 · gap 27 条。**

**F1 那 +6 条是纯正则漏网**:`+` / `**` / 一元 `+` 与 bitwise 是同一句
ToNumeric,只是消息没写 `arithmetic` / `bitwise` 这些词。其中一条写的是
`` `+` on bigint is a TypeError per spec `` —— **一条把自己描述成档 B 的
拒收**。

**F11 / F12 / F13 / F15 四族的发射点,注释里的道理本来就是对的,只是
停在了半路**:F12 的注释写着「§14.12.4 用 IsStrictlyEqual,而它是全
函数」,然后只为 `any` 开了口,没推到非 any 的跨类型对;F13 的注释已经
为「两个无共同祖先的用户类」定了 join-to-Any,只是没把
Struct / ClassRef / Number 的混合也交给它;F15 的读路径已经查过
`declared_private_names`,写路径漏了那一跳。**四处都不是立场分歧,是
没走完。**

**F14 故意不是三分法的一档。** 它标的是「这一族必须逐条问,不能批量
判」—— 理由见 §3.1:同一族里既有 checker 白名单漏项(运行期是对的,
档 B),也有运行期本身的分歧(档 L1,不在 S0 范围)。给它一个档位就是
在假装那条边界不存在。

**族上的档位是起点不是答案。** 下面四节是逐条复核的结果,每一节推翻
的都是上面这张表的一格:§3.0 推翻 F4 的 `split`,§3.1 推翻 F3 的
「运行期都已正确」,§3.2 判掉 F9 整族,§3.3 是 366 条的实测账目
(并在那里推翻了量具自己的一次判定,以及 F8 的整族归属)。

### 3.0 判据 3 在 F4 上的实测 —— `split` 这一档在这批语料上无对象

F4 的默认档是**分两路**,依据是判据 3:TS 自己把 `m.a`(无条件 TS2339)
与 `m["a"]`(TS7052,只在 `noImplicitAny` 下)分成两类。判据没错,**但
这批语料里没有第二路**:31 条逐条读 test262 源码找触发行,**31 条全是
dot 拼写,bracket 零条**(其中 2 条所在文件混有 bracket 写法,而失败的
键仍是 dot)。

逐条问 tsc 之后,31 条实际分成:

- **20 条落档 A** —— tsc 同样无条件 TS2339,与 §4 差异表那一行一致。
- **11 条出族** —— tsc **不报错**,所以它们不是档位问题,是 checker 的
  缺口(`Symbol` 缺 `valueOf`、`RegExp` 缺 `constructor`、生成器对象被
  建成裸 Struct 所以缺 `return`、`X.prototype.m.call(recv, …)` 的 thisArg
  回代把检查打到错的接收者上、eval 字符串被当程序文本检查)。与 §3.1 的
  F3-b 同性质:**先修 checker,不占档位**。

**切分轴也不是 dot 与 bracket。** 发出这一族诊断的是四处、三条路:读
路径(`check_type_of_member_terminal_miss.rs`)、写路径
(`check_assign_target.rs`)、**可选链**(`check/type_of.rs` 的
`member_type`)。而可选链那一路是**第二套 Struct 成员查找**,不共享读
路径积累起来的宽松姿态 —— 一条 getter 在读路径认得(存成
`__getter_a`)、在可选链路径不认。这是
**同一个问题有两份实现**,而它们已经漂开了 —— 归 L3。

### 3.1 判据 2 在 F3 上的实测 —— 默认档位被部分推翻

上一节写着「F3 的运行期语义早已完整存在,差别只在 checker 拦不拦」。
**拿判据 2 问了一遍,这句话不全对。** 5 条抽样(`target/iter/tr` 对
bun 1.4.2,全部用 `any` 拼写绕开 checker):

| 探针 | tr | bun | 谁对 |
|---|---|---|---|
| `Object.assign(t, {a:1})`,t 是 `"x"` | `threw TypeError` | `no-throw` | **bun** —— §20.1.2.1 ToObject 后 String 包装对象可加属性 |
| `BigInt(v)`,v 是 `{}` | `threw TypeError` | `threw SyntaxError` | **bun** —— §21.2.1.1 StringToBigInt 失败抛 SyntaxError |
| `BigInt(v)`,v 是 `[]` | `threw TypeError` | `0n` | **bun** —— ToPrimitive([]) = `""`,StringToBigInt("") = 0n |
| `BigInt(v)`,v 有 `valueOf(){return "12"}` | `threw TypeError` | `12n` | **bun** |
| `[1,2,3].copyWithin(n)`,n 是 `{}` | `[ 1, 2, 3 ]` | 同 | 一致 |
| `Object.values(o)`,o 是 `42` | `[]` | 同 | 一致 |
| `a.push(true)`,a 是 `[1]` | `2` | 同 | 一致 |

**所以 F3 要再切一刀**:

- **F3-a 运行期语义已正确** → 档 B,typed 路径直接 emit 即可(S0.4)。
- **F3-b 运行期语义本身还不对** → **不在 S0 的范围内**。这些是底层
  机制缺口(错误的类型、不该抛而抛),要先把运行期修对,然后才谈 typed
  直出。它们属于 P-SURF 的 L1 底层机制,不属于本章。

**根因已定位(2026-09-20)**:`String({})` 与 `{} + ""` 都与 bun 一致,
`BigInt("[object Object]")` 也正确抛 SyntaxError —— 所以 ToPrimitive 整体
是好的、`any_to_bigint` 的 String 分支也是好的。坏的是
**`heap_to_primitive` 的 hint number 路径**:`__torajs_bigint_ctor_any`
(`crates/torajs-anyvalue/src/to_bigint.rs:101`)对 struct / array 传
`false`(hint number),该路径返回 None 并抛 TypeError,于是三条 BigInt
探针全在进 String 分支之前就死了。

**这条修正改变了账目的读法**:那 62% 不是"都能靠 typed 直出拿下",其中
有一部分要先补运行期语义。逐条落档(S0.1)因此必须**对每条跑一次判据
2**,不能按族批量判 —— 族的默认值只决定"先问哪个问题"。

这也是判据 2 那条纪律的又一次兑现:被推翻的正是**本章上一节自己写下
的理由**。理由写得越顺,越要拿一行探针问它。

### 3.2 F9 的复核结果 —— 这一族不该存在

§3 说 F9「最该被推翻」,因为该问的不是它归哪一档,而是**它是否该存在**。
复核的结论是不该,依据不是立场而是三条代码事实:

1. **它自述是替代品。** 引入它的 commit 写着「catches multi-rooted
   ownership at compile time **without needing runtime support**」——
   它站在一个当时缺席的 retain 契约的位置上。
2. **那个契约早就到了,而且这一族已经被删过四次。** RFC
   `20260705-operand-ownership` 随契约落地逐 lane 撤掉同一条拒收
   (assign-ident / assign-member / index-assign / consuming params,
   净 −440 行)。throw 与 return 是它没走到的两站。
3. **throw 根本不 move。** `ssa_lower_stmt_throw` 的注释逐字反驳这条
   检查:throw 向 throw slot 自取一份 stake,每个绑定保住自己的 drop。
   checker 的 move 账本不流向 codegen(lowering 自己重新推导),所以
   整条账本**唯一可观察的效果就是那条错误消息**。

三条 test262 case 的形状也一致:同一个绑定被 `throw` 两次。

**所以这不是「实现细节泄漏到诊断面」,它从头到尾只存在于诊断面。**
所有权既不是 ECMA-262 的概念,也不是 TypeScript 的概念 —— 而本项目的
设计原则禁止把 Rust 习语带进语言面。删。

### 3.3 逐条实测 —— 366 条带闸跑一次的结果

判据 2 的机械全量版(§2)跑完了:`TORAJS_S0_ADMIT=1` 下的一次全量 sweep,
366 条各自的结局。

| 桶 | 条数 | 含义 |
|---|---:|---|
| **B** | 79(21%) | 闸开即通过 —— 运行期语义早已正确,S0.4 落地即拿下 |
| **S0.4-lower** | 213(58%) | 闸开后 `ssa_lower` panic —— 运行期路径**不存在** |
| **S0.4-crash** | 8(2%) | 段错误 / 挂死 —— 同上,更贵 |
| **S0.4-refused** | 2(0%) | 另一条拒收路(monomorphization) |
| **L1** | 64(17%) | 跑起来但答案仍不对 —— 运行期缺口,不在 S0 范围 |

**S0 的工作面因此是 223 条(61%),而不是 §3.1 读出来的那个数。**

#### 这张表要两维读

桶只回答「运行期在不在」,回答不了「该不该拒」—— 后者来自族的规范依据。
两维交叉才是档位:

|  | 族档 = throw(B) | 族档 = refuse(A) |
|---|---|---|
| 闸开 **pass** | S0.4 emit 那次 throw 即拿下 | **⚠ 复核** —— 拒收可能是错的 |
| 闸开 **lower panic** | S0.4 要**建**这条路,不只是放行 | 符合预期:放行就崩,拒是对的 |
| 闸开 **答错** | L1:先修运行期 | L1 |

所以 F10(泛型,refuse)的 8/8 lower panic **不是待办**。

而 F8(未知标识符,throw)的 23/23 lower panic 看起来是最大的待办 ——
**逐条看之后它一条都不是**。那 23 个名字全部带 `__` 前缀:
`__superbase__`、`__nsdefault___dyn_ns_c0`、`__cm_Base__method`、
`__cc0_a` —— **desugar 自己铸的内部符号**,不是用户程序写下的标识符,
§9.1.1.1 的运行期 ReferenceError 与它们无关。它们说的是某条 desugar 链
没接上(super 机制 / 动态 import 命名空间 / 类方法 / 捕获车道),属 L1。

**F8 这一族因此在这批语料上是空的**,而这是个好消息:真正的未声明读
早就不走拒收了(它发 warning、程序照跑),所以落进 checker 拒收的只剩
内部符号。族表补了 F16 承接它们(排在 F8 之前,两者消息同形,差别只在
名字是谁铸的)。

#### 第三类东西:checker 推断过窄

refuse 族里「闸开即通过」的格是这张表最值钱的地方。逐条看 F5 那九条,
它们既不是档位问题,也不是运行期缺口:

- `type mismatch on element assignment: array of Undefined, value is
  Function` —— `let a = []` 被推成「元素类型 Undefined 的数组」,
  而 tsc 推成 `any[]` 并接受。
- `type mismatch assigning to __priv_C__field: field is Boolean, value
  is Undefined` —— 逻辑赋值**短路时根本不赋值**,tsc 因此不报。

**这是推断的缺陷,不是边界的位置。** 它与 §3.0 F4 那 11 条、§3.1 的
F3-b 同属一类:族的规则没错,是这些条目本不该被它拦住。S0.4 逐族落地时
自然会撞上,不需要在本章先行分类。

#### 施工纪律:放宽之前先问这张表,而且要问那一格里的每一条

F13(ternary join)在族表上是干净的档 C:§13.14 对分支类型零要求,
`tsc --strict` 对 `c ? 1 : {}` 与 `c ? new A() : []` 都不报,同一个文件
里已经有「两个无共同祖先的用户类 join 到 Any」的先例。照着这个把
`unify_ternary` 的 `None` 改成 Any,**结果是 silent-wrong**:
`typeof (c ? 1 : {})` 在 else 分支答 `number`,而 bun 答 `object` ——
checker 说 Any 了,而 lowering 对 Number/Struct 这一对**不 box**,
消费端就按 number 读了那个指针。

**这比拒收更糟**,而 docket 早就说了:F13 的两条实测是 **1 B + 1 L1**,
不是 2 B。族的默认档位回答「该不该拒」,那一格里的实测回答「放行之后
跑不跑得对」—— **两个都要问,而且要逐条问**,因为同一族里两条可以分属
两边。

**而且 B 桶本身也不是充分条件。** F13 那条落 B 的 case 是
`(1 ? 2 : delete(0 ? 0 : {}))` —— 它取的是 then 分支,**从不观察 else
分支的值**,所以它通过只说明「这条 case 的全部依赖都就位」,说不了
「Number 与 Struct 的 join 在所有形状上都对」。上面那次 silent-wrong 正是
一条**比 test262 那条更严**的自写探针抓到的。

所以档 B / C 的落地有一条前置:**放宽 checker 之前,确认 lowering 有那
条路**。docket 给方向,**自写探针给判决** —— 探针要覆盖那条语义的可观察
面(这里是 `typeof`),不能只重复 test262 碰巧覆盖到的那一格。闸开后答错
(L1)说明要先修运行期;按族批量放宽会把一批拒收换成一批 silent-wrong。

#### 量具自己被推翻了一次

第一版的 docket 把 `exit 3` 读成「跑起来但答错」,于是 **203 条一次性搬
错了家**,S0 的工作面从 61% 缩成 0%。`exit 3` 是
`cmd_build.rs` 的 panic hook —— `ssa_lower` panic 被捕获、印成
`not yet supported:`、然后 `exit(3)`。**它是 S0.4 最大的一块,不是 L1。**

这是 §2 那条纪律的第三次兑现,而这次被推翻的是量具本身:一张形状完全
正常、百分比加起来也对的表,可以整体指向错误的结论。

## 4. 与 TypeScript 的差异

AssemblyScript、ArkTS、Static TypeScript 都有一份成文的「我与 TypeScript
的差异」。这是 torajs 的第一版,只覆盖本章范围(类型与运行期语义的边界),
其余章节补齐后扩充。

| 面 | TypeScript(tsc) | torajs |
|---|---|---|
| 类型的作用 | 只做检查,**运行前擦除**(Design Goals Non-goal #5) | **参与代码生成**:决定布局、单态化、是否需要 tag 检查 |
| `1n & 1` | 报错,不产出可执行物 | 报诊断,**并 emit 那次 TypeError**,程序照跑 |
| 规范定义了运行期错误的操作 | 一律编译期报错 | **档 B**:编译期算出结果,运行期零检查 |
| `m.a`(m 无该成员) | TS2339,无条件 | 同左(**档 A**,与 tsc 对齐) |
| `m["a"]`(m 无该成员) | TS7052,仅 `noImplicitAny` | **档 C**:any 车道,运行期答 undefined |
| unsound 的注解 | 接受(设计取舍) | 同样接受,但**类型参与 codegen** —— 所以注解不可信的位置必须落到档 C,不能假设它成立 |
| 类型驱动的行为差异 | 明确拒绝 | 这正是本语言的位置 |

**本章不改变的**:语法面是 TypeScript 的;语义面以 ECMA-262 为准
(`bun` 一致是佐证,不是判据;两者冲突时规范胜)。

## 5. 档 B 怎么落地

机制**已经有三代先例**,不需要发明:

1. **TDZ 读** —— AST pass 打标(`ast/tdz_reads.rs`),lowering 无条件
   emit(`ssa_lower_ident.rs` 的 `emit_tdz_raise`)。checker 只是知情:
   把它 typed 成 `Any` 并让它别落进 undeclared 集。
2. **未声明读/写** —— checker 侧打标(`undeclared_reads`),经
   monomorphization 交棒进 AST,lowering emit ReferenceError。
   **这是「拒收 → typed throw」今天唯一走通的整条线**,而
   `check_entry.rs` 里那段把它降级成 warning 的代码,正是本章闸的前身。
3. **AST 期把表达式整个换成 throw-IIFE** —— `ast/readonly_globals.rs`
   的 minter,已有三个调用方(`NaN = 1`、`Map()` 这类不可调用内建、
   嵌套函数写顶层 `const`)。

**第三条是最省的通道**:它绕开 checker(不必碰 `Diagnostic`),也绕开
前两条各自付过的死值代价(替身指针、接收者链上的 `read_raises` 走查);
求值顺序也已经有先例处理(逗号运算符 / `Expr::Sequence` 前置实参,
§13.3.6.2)。它缺的只有一样:**错误类名今天写死是 `TypeError`**,
RangeError / SyntaxError 走不了。

### 5.1 错误类别不可能从消息文本推出

档 B 要 emit「那次 throw」,就必须知道抛**哪一类**。366 条消息里:

| 信号 | 命中 |
|---|---:|
| 字面出现 `TypeError` | **1** |
| `RangeError` / `SyntaxError` / `ReferenceError` | **0** |
| `per spec` | 7 |

而 §3.1 已经记下一个反例:`BigInt({})` 今天抛 TypeError,规范
(§21.2.1.1 StringToBigInt)要的是 **SyntaxError** —— 消息文本读不出这个
差别,因为它描述的是「checker 不认这个类型」,不是「规范说抛什么」。
更远的一头是:相当一部分消息的正确答案是**不抛**(F13 的 ternary、
F12 的 switch case、F14 的实现缺口)。

**所以「看到 `type error:` 就 emit 一次 TypeError」是一条会造假的机械
规则**,不能用。类别只有两个合法来源:

1. **在 push 点写死** —— 写下那条诊断的人知道它是 §13.15.3 的 TypeError。
   给诊断加一个结构化的 `(ErrorClass, 条款号)` 字段,由每个 push 点填。
   落 B 档的条数虽多,push 点很少(F1 全族只有三行)。
2. **带闸 sweep 反推** —— 闸开后 any 车道实际抛出的类就是答案。这是
   判据 2 的机械版,用的是 test262 自己的语料。

两条可以互验:push 点填的类别与闸开后实际抛的类别不一致时,**不一致
本身就是一条缺陷**(要么诊断写错了依据,要么运行期抛错了类)。

## 6. 验收

- **S0.1** 366 条逐条有档位,族默认值被复核过。
- **S0.4** 实施后 **typed 覆盖率必须上升**(基线 0.7317 @ `f7b74aa1f`)。
  簇数不是验收判据:366 条里 76% 躺在 ≤3 条的残尾里,gate 只露出 24%,
  一次「放宽成 any」的修复在簇数上读作进步、只在覆盖率上现形。
