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

### 3.3 逐条实测 —— 带闸跑一次的结果

判据 2 的机械全量版(§2)跑完了:`TORAJS_S0_ADMIT=1` 下的一次全量 sweep,
每条被拒的 case 各自的结局。下表是 `3924a0940` 上的 332 条(r676 落地
34 条之后的基数;第一次跑是 366 条,桶的比例几乎没动)。

| 桶 | 条数 | 含义 |
|---|---:|---|
| **B** | 49(14%) | 闸开即通过 —— 运行期语义早已正确,S0.4 落地即拿下 |
| **S0.4-lower** | 210(63%) | 闸开后 `ssa_lower` panic —— 运行期路径**不存在** |
| **S0.4-crash** | 8(2%) | 段错误 / 挂死 —— 同上,更贵 |
| **S0.4-refused** | 2(0%) | 另一条拒收路(monomorphization) |
| **L1** | 63(19%) | 跑起来但答案仍不对 —— 运行期缺口,不在 S0 范围 |

**S0 的工作面因此是 220 条(66%)**,而不是 §3.1 读出来的那个数。

#### 这张表要两维读

桶只回答「运行期在不在」,回答不了「该不该拒」—— 后者来自族的规范依据。
两维交叉才是档位:

|  | 族档 = throw(B) | 族档 = refuse(A) |
|---|---|---|
| 闸开 **pass** | S0.4 emit 那次 throw 即拿下 | **⚠ 复核** —— 拒收可能是错的 |
| 闸开 **lower panic** | S0.4 要**建**这条路,不只是放行 | 符合预期:放行就崩,拒是对的 |
| 闸开 **答错** | L1:先修运行期 | L1 |

所以 F10(泛型,refuse)的 8/8 lower panic **不是待办**。

**而 L1 那一列有个盲点,r677 上撞到了:闸只把诊断降级,它不 emit
任何 throw。** 所以一条**尚未落地的档 B**,闸开后必然「跑起来但答错」——
该抛的没抛 —— 于是落进 L1 桶,与真正的运行期缺口混在一起。F6(const
重赋值)就是这样:5 条 L1 + 2 条 lower panic、0 条 B,读起来像「运行期
有缺口」,而实际上档 B 根本不需要运行期支持,编译期就知道要抛
(r677 `const-assign-throws-001` 落地后这 7 条应当整体转为通过)。

**判据不是桶,是那一族的规范依据。** L1 桶里凡是族档为 throw 的格,
都要先问「这条 throw 是不是编译期就能决定」——是,则它是未落地的档 B,
不是运行期缺口。已落地的档 B 不受影响:`spec_throws` 在 checker 里记,
闸开后照样记、照样 emit,所以 F1 的 5 条落在 B 桶。

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

refuse 族里 tsc **不**同判的条目,是这张表最值钱的地方:族的规则没错,
是这些条目本不该被它拦住。它与 §3.0 F4 那 11 条、§3.1 的 F3-b 同属一类。

**判据是 tsc,不是闸。** r677 把 F5 全部 51 条逐条最小化后过了一遍
`tsc --strict`,结果是 **39 条 tsc 同判(默认值 refuse 正确)、12 条 tsc
接受**。而本章此前写的那两个例子**都在 39 那一边** —— 先前是照着消息
文本推的,没有问 tsc:

- `let a = []` 确实被 tsc 推成 evolving `any[]` 并接受,**但语料里没有
  这个形状**:那四条 for-of case 写的是 `let f = [undefined, undefined,
  undefined]`,元素类型是 `undefined`,tsc 报 **TS2322**。
- `b ||= <只抛出的函数调用>` 那三条,tsc 同样报 **TS2322**(`void` 不可
  赋给 `boolean`)—— 「短路时不赋值」不是 tsc 的判据。

真正的 12 条分六条轴,每条都是一个独立的推断缺陷:

| 轴 | n | 形状 | tsc 怎么推 |
|---|--:|---|---|
| 裸声明后赋两种形状 **(r677 已落地一半)** | 7 | `var o; o = {join(){…}}; o = {};` | evolving `any`,每个赋值点各自成型 |
| 循环体内的裸 `let` | 1 | `for (let i of […]) { let s; s = i; }` | 同上(tr 在顶层通过,进了循环体就推成 `Undefined`) |
| 生成器函数表达式 | 1 | `let f = function*(){…}; f = function*(){…};` | 结构同型;tr 给每个生成器表达式铸一个名义 `ClassRef`,于是任意两个永不互赋 |
| `var x = {}` 后赋任意对象 | 1 | `var x = {}; x = Array;` | TS 的 `{}` 接受除 `null`/`undefined` 外的一切 —— 这是语义不是精度 |
| getter-only 改赋 setter-only | 1 | `var o = {get a(){…}}; o = {set a(b){}};` | 两边都是属性 `a`;tr 记成 `__getter_a` / `__setter_a` 两个不同字段名 |
| 只抛出的箭头函数 | 1 | `it.return = () => { throw … };` | 返回 `never`,可赋给任何返回类型 |

**第一条 r677 落地了它的一半**:`Ast::spliced_uninit_inits` 记下「这个 init 是
`desugar_uninit_let` 搬上来的」,`let_widen` 据此对裸声明按**字段名**比较对象字面量。
剩下的一半是**值类型**变化(`{toString(){return {}}}` 对 `{toString(){}}`,
`built-ins/Symbol/desc-to-string`)—— evolving any 同样接受,而分辨它需要那个 pass
拿不到的类型,所以按模块自己的规则留了 loud reject。

最后一条是一条**轴**而不是孤例,而且它解释了上面那三条 `||=` 为什么判
对:tsc 把「只抛出」的**箭头/函数表达式**推成 `never`(接受),把**函数
声明**推成 `void`(TS2322)。tr 对两种形式给同一个答案,所以那三条是
**形式偶合**判对的 —— 加宽这条轴时两处必须一起看,否则会把偶合拆成误拒。

#### 第四类:族的默认值可以与命中它的条目**整体**不一致

F7(重复声明)的规范依据无可挑剔 —— 重复声明是早错,规范要求在求值前
拒。**而 r677 逐条读源码的结果是:今天命中它的七条一条都不是真重复
声明。**

- 五条是模块机制:`instn-once` / `eval-self-once` /
  `export-star-as-dflt` / `own-property-keys-binding-types` /
  `import-defer-does-not-evaluate`。被点名的 `dflt1` / `named` /
  `ns_1_2` / `__nsdefault_ns` 在源码里**各只声明一次** —— 前三条是
  自引用模块(同一个文件被 import 九次)被处理了多遍,后两条是
  namespace desugar 把自己铸的名字放进了用户作用域。属 L1。
- 一条是 `export function evaluated` 铸了两遍(`verify-dfs`)。属 L1。
- 一条是 harness 与 case 各写一个 `var $262`
  (`detachArrayBuffer-host-detachArrayBuffer`)—— 同作用域的 `var`
  重复声明在 JS 里**合法**,tsc 对同类型的重复 `var` 也接受。这条是
  收窄。

所以这一族的默认档位一条也没兑现。**族给的是先验,不是结论** ——
而这一次连一条都没落在先验上,是逐条落档存在的最强理由。

#### 第五类:消息是二次症状,不是判断

F2(调用非函数)的族规则同样无可挑剔 —— §7.3.14 Call 要求 IsCallable。
而命中它的 15 条里,**14 条的消息逐字相同**:
`not callable: type Object("Function")`。

它们全是 `Function(p, "return …")` 这种**实参不是常量字符串**的动态函数
构造。`ast/desugar_eval/function_ctor.rs` 按 §20.2.1.1 在编译期做文本装配,
但只在实参全为常量串时;其余按它自己 doc 里写明的 honest-reject 留给今天
的 loud reject(运行期字符串要把编译器放进产物)。于是 `Function` 以
「一个不可调用的值」的形态落到通用调用检查上,而那句消息是**这件事的二次
症状**。实测:裸 `Function("a","return a+1")` 与 `new Function(…)` 今天都
跑得对,差别只在实参是不是常量。

**所以这一族是 gap(实现缺口),不是 throw。** 与第四类合看:F7 是「规则
对而条目不属于它」,这一条是「消息指向的地方不是出事的地方」—— 两种都让
按消息文本分族的默认值失效,而两种都只能逐条读源码才看得见。

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

**探针写漏了还有最后一道**:收尾 sweep 的 verdict diff 里,
`incompatible → bug` 就是「接受了但答错」的签名,一条一条读。这一轮它
抓到了本轮自己造的两条 —— `1n > true` 答 true(lowering 的类型清单没跟着
checker 放宽)与 `({[Symbol.toPrimitive]() { throw }}) >>> 0n` 抛错了类
(档 B 提前 emit,跳过了对象的 ToPrimitive)。**两条 gate 都是全绿的**:
这些形状只在 test262 里出现,本仓 fixture 一条都没有。所以这一格不是
「顺便看看」,是纪律 —— 而看完之后要补的正是那两条缺席的 fixture。

#### S0.4 的施工面 —— 210 条 lower panic 里,最大的一族长什么样

`S0.4-lower` 是最大的一桶,而其中 **F3 占 105 条**。r677 把这 105 条按
「**要建的那条路是什么**」而不是按消息文本分了一遍,得到 15 簇;前四簇:

| 簇 | n | 形状 | 要建的路 |
|---|--:|---|---|
| ToNumber / ToNumeric 的数值实参 | 23 | `copyWithin` / `fill` 的索引、`Math.max` 的变参、`~`、`x++` | 实参非 Number 时装箱成 `Any`,调已有的 any 车道内建 |
| ToObject 的 `Object.*` 首参 | 13 | `Object.assign` / `Object.values` | 同上;**其中 3 条 nullish 是档 B**(r677 已落地) |
| ToBigInt | 12 | `BigInt(v)` 的对象实参、`BigInt.asIntN` 的 value | 前 6 条装箱即可;后 6 条连 `any` 都被拒,要新建 ToBigInt 入口 |
| ToString 的字符串实参 | 11 | `String.p.indexOf` / `padStart` / `Symbol.for` / `parseFloat` | 同上(`parseFloat` 那条 r677 已落地) |

**比簇更有决策价值的是另一维**:这 105 条里 **87 条的 any 车道今天就答得
对**(实测 tr = bun),typed 车道要做的只是「装箱 → 调已有 kernel」或「直接
emit 那次 throw」;**18 条 any 车道自己也错或根本没路**(`BigInt.asIntN` 6 ·
`new Array(非 Number)` 5 · `Object.assign(基元, …)` 3 · Symbol 键与 String
下标写 3 · `Math.sumPrecise` 1),那 18 条要先修运行期。

判据钉死这一维的是一组 A/B:`Math.max(o as any, 1)` 崩在
`cannot coerce Obj(StructId(0)) to f64`,而把同一个值先赋给 `const b: any = o`
再 `Math.max(b, 1)` 答 `2` —— **kernel 对、装箱对,缺的只是调用点上那次装箱**。

**r677 实测到的边界,写下来给下一轮**:这类放宽**不是一律只改 checker**。
`String.padStart` 的 fill 槽放宽之后程序**静默退出**(第一行之后什么都没有,
无错误消息)—— 那条 lowering 只接受 `Str`,checker 的门是它唯一的保护。所以
每一条放宽都要**当场跑一条探针**,而且要看完整输出:少一行输出与「跑对了」
在 `head -1` 下长得一样。

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

**第三条看起来最省** —— 它绕开 checker(不必碰 `Diagnostic`),也绕开
前两条各自付过的死值代价(替身指针、接收者链上的 `read_raises` 走查),
求值顺序也已经有先例处理(逗号运算符 / `Expr::Sequence` 前置实参,
§13.3.6.2)。

**但它对档 B 用不上,而这一点只有动手才看得见**:那个 minter 跑在
`ast_pipeline` 里、**在 checker 之前**,而档 B 的判据按定义依赖类型 ——
「两个操作数的类型不同」这句话在类型存在之前问不出来。throw-IIFE 能覆盖
的只有字面量对(`1n & 1` 里两个字面量),覆盖不到
`let n: bigint = 1n; n & 1`,而后者才是这一族的主体。

**所以第四条先例是这一轮建的**,形状取自第二条(undeclared read):
checker 记侧表 `Ast::spec_throws` → 经 monomorphization 交棒 → lowering
读表并发射(`ssa_lower_spec_throw`)。它同时要在
`ast_throw_info` 登记 may-throw,否则调用方的检查被剪掉、throw 跨函数
边界静默消失 —— 前两条先例各自付过同一笔学费。

**四条落地时才看得见的约束**(后两条是这一轮的 sweep 事后抓到的,
不是设计时想到的):

1. **那个不可达的值仍然需要正确的形状。** 它永远不被读,但它要被
   materialize,而后端不肯把指针放进浮点寄存器:
   `function f(x: bigint): number { return +x }` 的返回槽是 f64,
   而 TDZ 读用的空指针不是 —— 程序当场
   `FPR materialization can't hold ConstPtrNull`。形状要取自 checker
   为该表达式记下的类型。
2. **该表达式的类型不该一律写成 `Any`。** 一元 `+` 只要产出就产出
   Number,所以它记 Number,外层表达式因此留在 typed 车道上。写 Any
   是更省事的局部答案,也正是 §7 要防的那次交换。
3. **档 B 的前提是「中间没有可观察的事」,而对象的 ToPrimitive 是用户
   代码。** §13.15.3 先对**两个**操作数做 ToPrimitive,再由运算符拒绝
   这一对 —— 那段用户代码要是先抛了,程序看到的就是它的错误。第一版
   的 `>>>` 规则只要求**一侧**是 BigInt、Symbol 规则只要求一侧是
   Symbol,于是另一侧可以是对象:
   `({[Symbol.toPrimitive]() { throw new MyError() }}) >>> 0n` 被提前
   emit 成 TypeError,而规范要的是 MyError(test262
   `unsigned-right-shift/bigint-toprimitive`)。**判据是「两个操作数都
   是 primitive」** —— 混合数值那一条规则从一开始就没这个洞,因为
   `to_numeric_kind` 对「valueOf 看不见」的类型答 `None`,那正是同一个
   性质写在了对的地方。`Any` 同样排除,理由相同而不是更弱:它背后可能
   是对象。
4. **放宽 checker 的接受面,必须同时问 lowering 认不认。** 这一轮把
   `1n > true` 从拒收改成接受,而 lowering 里另有一份手写的类型清单
   (`F64 | I64 | I32 | Str`)没跟上 —— tr 于是答 `true`,规范与它自己的
   any kernel 都答 `false`。**拒收变成错值比拒收更坏**,而 gate 全绿:
   这一对只在 test262 里出现。两处清单的正确修法不是把第二处也列全,
   是把它变成一个**性质**(「恰好一侧是 BigInt」),性质不会跟一份清单
   漂开。判据留在 §3.3 的施工纪律里:**落地一条之后,verdict diff 里
   `incompatible → bug` 的每一条都要当场读**,它正是「接受了但答错」的
   签名。

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
