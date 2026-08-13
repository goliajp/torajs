//! Expression AST types (chunk 432), extracted verbatim from
//! ast.rs:
//! - ExprId — u32 index into `Ast::exprs`
//! - BinOp / UnaryOp — operator enums
//! - Expr — the expression tree
//! - Param — fn/method parameter payload
//!
//! Re-exported from `crate::ast` so every consumer keeps the
//! canonical `ast::Expr` / `ast::ExprId` / `ast::Param` paths.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    /// V3-01 — `**` exponent. Right-associative; precedence above
    /// mul/div/mod. Number ** Number → Number (libm pow);
    /// BigInt ** BigInt → BigInt (square-and-multiply, RangeError
    /// on negative exponent per spec).
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,  // ===
    Neq, // !==
    /// V3-18 m3 — JS loose equality (`==` / `!=`) per §7.2.13
    /// IsLooselyEqual. Different from `Eq` / `Neq` (strict): does
    /// type coercion across Number/Boolean/Null/String pairs and
    /// treats null==undefined as true.
    LooseEq,
    LooseNeq,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr, // signed; JS `>>`
    /// JS `>>>` — unsigned (logical) right shift. Lowered as LLVM
    /// `lshr` rather than `ashr`; the typechecker still treats it as
    /// `Number → Number` (matches arithmetic Shr).
    UShr,
    LAnd, // logical &&  — short-circuits
    LOr,  // logical ||  — short-circuits
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not,    // logical !
    Neg,    // arithmetic -
    BitNot, // bitwise ~
    /// V3-18 m1.h.4 — unary `+x` per spec §13.5.4. Equivalent to
    /// `ToNumber(x)` — coerces strings/booleans/null to a Number
    /// without changing sign. Common in test262 for converting
    /// values to Number explicitly.
    Plus,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(String),
    String(String),
    Number(f64),
    /// T-25 — BigInt literal. Carries the digits + radix exactly
    /// as the lexer parsed them (e.g. `123n` → digits="123" radix=10;
    /// `0xffn` → digits="ff" radix=16). The runtime parses the
    /// digits at allocation time via `__torajs_bigint_alloc_from_str`.
    BigInt {
        digits: String,
        radix: u32,
    },
    Bool(bool),
    /// `let x;` / `let x: T;` — placeholder init the parser emits when
    /// no `= EXPR` is provided. `desugar_uninit_let` walks the
    /// declaring scope for the first `x = EXPR;` shape, splices that
    /// EXPR into the let's init, and removes the assignment. Anything
    /// that resists rewrite (no follow-up assignment) keeps `Uninit`,
    /// which the typechecker rejects with a clear "declared but never
    /// assigned" message — better than the previous parse-error wall.
    Uninit,
    /// `/pattern/flags` regex literal. Lexer carries the raw pattern
    /// + flag bytes; the parser wraps them here so check.rs can give
    /// a clean roadmap-phase rejection. Actual matching engine is
    /// future work — the typechecker today rejects regex use with a
    /// "regex literals not yet implemented (planned)" message.
    Regex {
        pattern: String,
        flags: String,
    },
    /// `null` — the in-band 0 sentinel for any pointer-shaped slot.
    /// Lowered to `Operand::ConstPtrNull`. Comparable against pointer
    /// values via `=== null` / `!== null` and the implicit `?.`/`??`
    /// shapes.
    Null,
    BinOp {
        op: BinOp,
        left: ExprId,
        right: ExprId,
    },
    /// Unary prefix op — currently just `!` (logical not). M1.5.
    Unary {
        op: UnaryOp,
        expr: ExprId,
    },
    Member {
        obj: ExprId,
        name: String,
    },
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    Assign {
        target: ExprId,
        value: ExprId,
    },
    Index {
        obj: ExprId,
        index: ExprId,
    },
    Array(Vec<ExprId>),
    /// Object literal: `{ x: 1, y: 2 }`. Field order is preserved as written
    /// (matters for struct layout decisions in P2.4.c).
    ObjectLit {
        fields: Vec<(String, ExprId)>,
    },
    ArrowFn {
        params: Vec<Param>,
        return_type: Option<String>,
        body: Vec<Stmt>,
    },
    /// Lifted closure with implicit captures (M2). After `lift_arrow_fns`,
    /// each arrow becomes this expression: `fn_name` references the
    /// lifted top-level FnDecl (which expects an extra hidden first
    /// param `__env`); `captures` lists the outer-scope binding names that
    /// must be packed into the env at construction time, in the same order
    /// as the lifted FnDecl reads them. Since P3.closure-in-struct-field
    /// zero-capture arrows use this variant too (empty `captures`, an
    /// `__env()` annotation) so call-site dispatch stays uniform.
    Closure {
        fn_name: String,
        captures: Vec<String>,
    },
    /// M5.1 — `this` inside a class method body. Rewritten by the
    /// `desugar_classes` pass into `Expr::Ident("__this")` once methods
    /// are flattened into top-level FnDecls.
    This,
    /// P4.5 — `new.target` meta-property (spec §13.3.10). Inside a
    /// ctor body, evaluates to the class function object that was
    /// invoked via `new` (typically the same class as the ctor's
    /// owner, but a subclass if the ctor was inherited / super-called).
    /// Outside any ctor body, evaluates to `undefined`. Rewritten by
    /// `desugar_classes` into `Expr::Ident("__new_target")` inside ctor
    /// bodies; outside ctors it lowers to `Operand::ConstPtrNull` (the
    /// ANY_UNDEF tag at the SSA layer).
    NewTarget,
    /// M5.1 — `new ClassName(args)`. Rewritten by `desugar_classes` into
    /// a Call to the synthesized `__new_ClassName` factory FnDecl.
    /// `type_args` carries the explicit instantiation spellings of
    /// `new Map<string, number>()` (flat ann strings, same format as
    /// `parse_type_ann`); empty when the source wrote none. Consumed by
    /// `infer_anonymous_closure_params` to give container receivers an
    /// element type for callback-param seeding.
    New {
        class_name: String,
        args: Vec<ExprId>,
        type_args: Vec<String>,
    },
    /// S-NEW 刀 1 — `new <expr>(args)` where the callee is a full
    /// MemberExpression per spec §13.3, not a name the compiler can
    /// resolve to a class: `new a.b()`, `new a[k]()`, `new (f())()`.
    ///
    /// Kept separate from [`Expr::New`] on purpose. `New` names its
    /// target, so the whole desugar chain can bind it to a synthesized
    /// `__new_<C>` factory at compile time; this one cannot know its
    /// target until the callee has been evaluated, so it has to go
    /// through a runtime construct. That is the ordinary direct-call
    /// versus indirect-call split, and keeping the fast path untouched
    /// is what `torajs-build-and-port.md` B-4 asks for.
    ///
    /// Before this variant existed the parser stopped at the head
    /// identifier, so `new a.b()` silently became `(new a).b()` — a
    /// different program, quietly.
    NewDynamic {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    /// M5.2 — `super(args)` inside a subclass constructor. Rewritten by
    /// `desugar_classes` into `__cm_<Parent>__ctor(__this, args)` once
    /// the surrounding class's parent is known.
    Super {
        args: Vec<ExprId>,
    },
    /// `cond ? then_branch : else_branch` — TS / JS ternary.
    /// Lowered to a CondBr at SSA layer with a phi-style result via
    /// an alloca slot (consistent with how the rest of tr handles
    /// branch results today).
    Ternary {
        cond: ExprId,
        then_branch: ExprId,
        else_branch: ExprId,
    },
    /// V3-18 m1.h.6 — comma operator `(a, b)` per spec §13.16.
    /// Evaluates `left` for side effects (value discarded), then
    /// returns `right`. Result type = right's type.
    Sequence {
        left: ExprId,
        right: ExprId,
    },
    /// V3-07 — `expr as T` TS type cast. Parser-level type assertion;
    /// at runtime the cast is identity (the inner value's bits flow
    /// through). Typecheck uses `ty_ann` to widen / narrow the inner
    /// expression's type — the common shape is `as any` for widening
    /// into a heterogeneous container slot. Currently lowered as
    /// `inner` with the cast type honored only when widening to
    /// `Type::Any` (via the existing Any-box machinery); other casts
    /// are typecheck-only with no IR side effect.
    As {
        expr: ExprId,
        ty_ann: String,
    },
    /// An array-literal elision slot (`[0, , 2]` — §13.2.4). Values
    /// like `undefined` (reads through the hole answer undefined),
    /// but the slot is NOT an own property: `lower_array` marks it a
    /// hole after the dense store. Only ever appears as an
    /// `Expr::Array` element.
    Elision,
    /// `typeof x` — produces a string literal at runtime.
    /// Lowered to a fresh Type::Str whose contents are determined by
    /// the operand's static type ("number" / "string" / "boolean" /
    /// "object").
    TypeOf {
        expr: ExprId,
    },
    /// `delete obj.k` / `delete obj["k"]` — ES §13.5.1 property
    /// removal, answering a Boolean. The parser only accepts a
    /// Member / Index operand (deleting a variable is the strict-
    /// mode SyntaxError; other operand shapes are a recorded loud
    /// boundary). Checker admits an `any`-typed receiver with a
    /// string key; lowering dispatches through
    /// `__torajs_any_prop_delete` (dynobj OrdinaryDelete, expando
    /// props, non-object receivers answer true per spec).
    Delete {
        expr: ExprId,
    },
    /// `expr instanceof rhs` (ES §13.10.2) — both operands are
    /// expressions, as the grammar has them.
    ///
    /// The right-hand side used to be a `String` because the answer was
    /// read off the class hierarchy at compile time. That fold is still
    /// the fast path and still runs, but it is now an OPTIMISATION the
    /// lowering applies when `rhs` happens to be an `Expr::Ident`, not
    /// the shape of the node: a target can be any value, and §13.10.2
    /// step 2 lets it answer for itself through `@@hasInstance`.
    ///
    /// A bare name still arrives here as `Expr::Ident` carrying the
    /// same class-value alias the parser applied before, so every
    /// static fold keyed on the name reads back exactly what it did.
    InstanceOf {
        expr: ExprId,
        rhs: ExprId,
    },
    /// `...expr` — array spread. Only valid as a child of `Expr::Array`.
    /// ssa_lower's Array arm pre-computes total length (sum of spread
    /// source `.length`s + non-spread element count) at runtime, allocs
    /// once, fills via `arr_push_unchecked` — no cap-doubling realloc.
    Spread {
        expr: ExprId,
    },
    /// `lhs ?? rhs` — nullish coalescing. ssa_lower stores `lhs` into a
    /// temp slot, compares the slot value against null, and branches —
    /// `lhs` evaluates exactly once even if it has side effects.
    Nullish {
        lhs: ExprId,
        rhs: ExprId,
    },
    /// `obj?.field` — optional chaining for member access. ssa_lower
    /// stores `obj` in a temp, branches on null, returns null on the
    /// null path or `obj.field` otherwise. Single eval of `obj`.
    OptChain {
        obj: ExprId,
        name: String,
    },
    /// `obj?.[index]` — optional chaining for element access (ES2020
    /// §13.3.9, chunk 703). Same null-guard shape as OptChain: `obj`
    /// evaluates once; nullish short-circuits to undefined WITHOUT
    /// evaluating `index`; otherwise `obj[index]`.
    OptIndex {
        obj: ExprId,
        index: ExprId,
    },
    /// `callee?.(args…)` — optional call (ES2020 §13.3.9, chunk 705).
    /// Same null-guard shape as OptChain/OptIndex: `callee` evaluates
    /// once; nullish short-circuits to undefined WITHOUT evaluating
    /// the arguments; otherwise the callee value is invoked (a
    /// non-callable non-nullish callee is a catchable TypeError).
    OptCall {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    /// `x++` / `x--` — JS-spec-compliant post-increment / post-decrement.
    /// Yields the OLD value, then mutates the target. ssa_lower captures
    /// `target`'s value into a temp SSA value, computes new = old ± 1,
    /// stores new into target, and returns the temp.  Pre-increment is
    /// the simpler `x = x + 1` shape and is already handled by Assign.
    PostIncr {
        target: ExprId,
        is_inc: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub type_ann: Option<String>,
    /// Default value expression: `function f(x: number = 0)`. Evaluated
    /// at the call site (not in callee scope) when the caller omits
    /// the argument. None for required params.
    pub default: Option<ExprId>,
    /// Rest parameter: `function f(...args: number[])`. Only valid as
    /// the last param. The receiver sees `args` as an Array<T>; the
    /// `apply_rest_args` AST pass packs trailing call-site args into
    /// an array literal at every call site.
    pub is_rest: bool,
}
