//! AST — arena-allocated. Children referenced by `ExprId(u32)`, not Box.

mod apply_args;
mod arguments_object;
mod arguments_object_rewrite;
mod array_isarray_value;
mod class_globals;
mod consuming_flow;
mod desugar_async;
mod desugar_classes;
mod desugar_classes_abstract;
mod desugar_classes_dispatch;
mod desugar_classes_emit;
mod desugar_classes_field_inits;
mod desugar_classes_fields;
mod desugar_classes_method_owners;
mod desugar_classes_pass2;
mod desugar_classes_pass3;
mod desugar_classes_statics;
mod desugar_classes_super;
mod desugar_generators;
mod desugar_generators_class;
mod desugar_generators_methods;
mod desugar_generators_prep;
mod desugar_generators_rewrite;
mod desugar_generators_sm;
mod desugar_variadic_push;
mod escape_analyze;
mod forwarders;
mod forwarders_object;
mod free_vars;
mod implicit_generics_infer;
mod infer_closure_params;
mod inject_builtin_classes;
mod lift_arrow_fns;
mod nested_fns;
mod sfi_pass;
mod sfi_rewrite;
mod sfi_safe;
mod super_collect;
mod uninit_let;
mod var_hoist;
pub use apply_args::{apply_default_args, apply_rest_args};
pub use arguments_object::desugar_arguments_object;
pub use array_isarray_value::desugar_array_isarray_value;
pub use class_globals::synthesize_class_globals;
pub use consuming_flow::compute_consuming_params;
pub use desugar_async::desugar_async;
pub use desugar_generators::desugar_generators;
pub(crate) use desugar_generators::{expand_yield_into_in_stmt, lift_lets_in_stmt};
use desugar_async::{body_ends_in_return, rewrite_returns_for_async};
pub use desugar_classes::desugar_classes;
pub use desugar_variadic_push::desugar_variadic_push;
pub use escape_analyze::escape_analyze_array_literals;
pub use forwarders::synthesize_forwarders;
pub use forwarders_object::{synthesize_fn_to_closure_forwarders, tag_struct_field_closure_types};
pub(crate) use implicit_generics_infer::{
    AstExprsView, binds_to_params, body_has_value_return, infer_expr_ann_with, infer_return_ann,
    infer_return_ann_seeded,
};
pub use infer_closure_params::infer_anonymous_closure_params;
pub use inject_builtin_classes::inject_builtin_classes;
pub use lift_arrow_fns::lift_arrow_fns;
pub(crate) use lift_arrow_fns::{
    build_factory_body, default_init_for_field, default_init_for_type, is_fn_like_ann,
    method_owner_is_in_chain, rewrite_this_in_ann,
};
pub use nested_fns::desugar_nested_fns;
pub use sfi_pass::rewrite_split_for_i_to_iter;
pub use uninit_let::desugar_uninit_let;
pub use var_hoist::desugar_var_hoist;

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
    /// each capturing arrow becomes this expression: `fn_name` references
    /// the lifted top-level FnDecl (which expects an extra hidden first
    /// param `__env`); `captures` lists the outer-scope binding names that
    /// must be packed into the env at construction time, in the same order
    /// as the lifted FnDecl reads them. Non-capturing arrows still lower
    /// to `Expr::Ident` (FnAddr) — only capturing ones use this variant.
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
    New {
        class_name: String,
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
    /// `typeof x` — produces a string literal at runtime.
    /// Lowered to a fresh Type::Str whose contents are determined by
    /// the operand's static type ("number" / "string" / "boolean" /
    /// "object").
    TypeOf {
        expr: ExprId,
    },
    /// `expr instanceof ClassName` — compile-time class membership check.
    /// tr is statically typed: if `expr`'s declared type is the named
    /// class (or a subclass via `extends`), this lowers to ConstBool(true);
    /// otherwise ConstBool(false). The check itself never runs at
    /// runtime — desugar_classes records the class hierarchy, and check.rs
    /// resolves the answer during typechecking.
    InstanceOf {
        expr: ExprId,
        class_name: String,
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

/// One arm of a `switch` statement. `value` is the case label (must
/// be a literal in this subset — Number / String / Bool); `body` is
/// the statements that run when the scrutinee strict-equals `value`,
/// with the JS-shape fall-through to the next case unless interrupted
/// by `break` or `return`.
#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub value: ExprId,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(ExprId),
    LetDecl {
        mutable: bool,
        name: String,
        type_ann: Option<String>,
        init: ExprId,
        /// P2.1 — flag set when the source declared this with `var`
        /// instead of `let` / `const`. Drives the `desugar_var_hoist`
        /// pass which moves the declaration to the top of the
        /// enclosing fn-body / top-level script (per ES spec §14.3.2.1
        /// VariableStatement). Pre-P2.1 tora's lexer aliased `var`
        /// to `Token::Let` so var was indistinguishable from let post-
        /// parse; the hoisting pass needs the source intent to know
        /// which declarations to move. `let` / `const` keep this `false`
        /// (no hoist; block-scoped per spec).
        is_var: bool,
    },
    If {
        cond: ExprId,
        then_branch: Box<Stmt>,
        else_branch: Option<Box<Stmt>>,
    },
    While {
        cond: ExprId,
        body: Box<Stmt>,
    },
    /// `do { body } while (cond);` — body runs at least once, then
    /// `cond` decides whether to repeat.
    DoWhile {
        body: Box<Stmt>,
        cond: ExprId,
    },
    /// `switch (scrutinee) { case v: ... default: ... }` — strict-eq
    /// dispatch. Cases share fall-through; `break` exits the switch.
    Switch {
        scrutinee: ExprId,
        cases: Vec<SwitchCase>,
        default: Option<Vec<Stmt>>,
    },
    /// `for (init; cond; step) body` — C-style for-loop. M1.6.
    /// `init` is typically a LetDecl but can also be an Expr stmt or
    /// empty. `cond` is the loop condition (Boolean). `step` runs at
    /// the end of each iteration AND on `continue`. `body` is any stmt.
    For {
        init: Option<Box<Stmt>>,
        cond: Option<ExprId>,
        step: Option<ExprId>,
        body: Box<Stmt>,
    },
    /// P-iter — `for (let v of <parent>.split(<sep>)) body` — emitted
    /// by the parser when both source forms (parent expression and sep
    /// literal) match the SplitIter fast path. ssa_lower expands to
    /// stack alloca'd iter / substr slots + init / next-loop / drop
    /// calls. `var_name` binds the per-iter Substr borrow; type is
    /// always Substr (caller-side annotation `string` is honored).
    /// Falls back to the generic for-of (Array<Substr> walk) when the
    /// parser can't detect the split shape.
    ForOfSplitIter {
        var_name: String,
        parent: ExprId,
        sep: ExprId,
        body: Box<Stmt>,
    },
    /// P5.3 — generic `for (let v of <src>) body`. Parser hoists src
    /// to a fresh Ident binding (or reuses the source Ident if src
    /// was already one), mints an i counter Ident, and pre-allocates
    /// `elem_expr` = `Expr::Index { obj: src_ident, index: i_ident }`.
    /// ssa_lower routes element loads through that ExprId via the
    /// existing Expr::Index lowering path so Type::Any boxing /
    /// Substr borrowing / refcount semantics all stay consistent
    /// with the rest of the indexed-read substrate. For user iterables
    /// (P5.3 follow-up — `[Symbol.iterator]()`-implementing classes),
    /// ssa_lower ignores `elem_expr` and emits an iterator-protocol
    /// while-loop instead. The Block wrapping the optional `let
    /// src_ident = <src>` hoist + this Stmt::ForOf keeps the source
    /// binding scoped to the loop body only.
    ForOf {
        var_name: String,
        var_type_ann: Option<String>,
        src_ident: String,
        i_ident: String,
        elem_expr: ExprId,
        body: Box<Stmt>,
    },
    /// `break;` — exits the innermost enclosing loop. M1.7.
    Break,
    /// `continue;` — jumps to the innermost loop's step (for) or
    /// header (while). M1.7.
    Continue,
    /// `throw <expr>;` — M4. The thrown value's type is whatever
    /// `<expr>` resolves to (currently number-only at the SSA layer).
    /// Lowered to a write into `__torajs_throw_value` + an immediate
    /// return from the enclosing fn (with a sentinel result).
    Throw(ExprId),
    /// `try { body } catch (e) { catch_body } finally { finally_body }`.
    /// `had_catch` distinguishes `try {} catch {} finally {}` (where the
    /// catch swallows + finally runs) from `try {} finally {}` (where
    /// finally runs and the throw keeps propagating).
    /// `catch_param` is the binding name; `catch_type` is the optional
    /// `: Type` annotation; `catch_body` is empty if `had_catch=false`.
    Try {
        body: Vec<Stmt>,
        had_catch: bool,
        catch_param: Option<String>,
        catch_type: Option<String>,
        catch_body: Vec<Stmt>,
        finally_body: Option<Vec<Stmt>>,
    },
    Block(Vec<Stmt>),
    /// Compiler-generated sequence of statements that share the
    /// SURROUNDING scope (unlike `Block` which opens a fresh frame).
    /// Produced by parse-time desugars like destructuring (`let [a, b]
    /// = src` expands into 2-3 lets that must be visible in the outer
    /// scope, not buried in a child block). ssa_lower flattens it via
    /// a single recursive `lower_stmt` per child — no scope push, no
    /// drop emission of its own.
    Multi(Vec<Stmt>),
    FnDecl {
        name: String,
        /// M3 — type parameters declared by `function id<T, U>(...)`. Empty
        /// for non-generic fns. Each entry is the bare type-param name; uses
        /// of these names inside `params` / `return_type` / `body` resolve
        /// against this list at typecheck and trigger monomorphization at
        /// each concrete call site.
        type_params: Vec<String>,
        params: Vec<Param>,
        return_type: Option<String>,
        body: Vec<Stmt>,
        /// Phase J — `function*` generator. The post-parse `desugar_generators`
        /// pass rewrites generator FnDecls into a class with a `next()`
        /// state machine, then leaves a thin factory FnDecl that returns
        /// a fresh state-machine instance. Plain (non-generator) FnDecls
        /// stay false; rewritten factories also stay false.
        is_generator: bool,
    },
    /// `type Foo = { x: number, y: number };` — structural type alias.
    /// Field types are stored as raw annotation strings; `check.rs` is
    /// where they get resolved to `Type` values.
    /// M3.4 — `type_params` is non-empty for generic struct types
    /// `type Pair<A, B> = { fst: A, snd: B }`. Each use of `Pair<X, Y>`
    /// in a type annotation instantiates a fresh concrete struct by
    /// substituting `A→X, B→Y` in the field annotations.
    TypeDecl {
        name: String,
        type_params: Vec<String>,
        fields: Vec<(String, String)>,
    },
    /// M5.1 — `class ClassName { fields; constructor(...) {...} methods }`.
    /// Single-class, no inheritance / super / virtual dispatch yet.
    /// The `desugar_classes` pass (run before `lift_arrow_fns`) flattens
    /// this into a `TypeDecl` + a series of top-level `FnDecl`s, so the
    /// SSA layer never sees `ClassDecl`.
    ClassDecl {
        name: String,
        /// Generic type params: `class Map<K, V> { ... }`. Threaded through
        /// to the desugared TypeDecl + every method's FnDecl. Same
        /// monomorphization machinery as standalone generic fns +
        /// generic-struct aliases.
        type_params: Vec<String>,
        /// M5.2 — `class Sub extends Base { ... }`. None for root classes.
        /// Multi-level inheritance is supported (Sub extends Mid extends Root)
        /// as long as the chain is acyclic and every ancestor is declared
        /// before the descendant in source order.
        parent: Option<String>,
        /// M-OO.6 — `abstract class C { ... }`. Abstract classes can't be
        /// instantiated (`new C()` rejected at typecheck), and any
        /// concrete (non-abstract) subclass must override every abstract
        /// method along the inheritance chain.
        is_abstract: bool,
        fields: Vec<(String, String)>,
        /// P8.3 — ordered list of static initialization steps. Each entry
        /// is either `StaticInit::Field(StaticField)` (desugars to
        /// `let __sf_<Class>__<name>: T = init`) or `StaticInit::Block(Vec<Stmt>)`
        /// (desugars to `__sb_<Class>__<idx>()` named-fn + a top-level
        /// call at the entry's position). Order in this vec **is** the
        /// emit order — spec §15.7.10 requires interleaving with fields,
        /// so static fields and static blocks share one vec, not two.
        /// Static methods stay in `static_methods` (they're declarations,
        /// not init steps).
        static_init: Vec<StaticInit>,
        ctor: Option<ClassCtor>,
        methods: Vec<ClassMethod>,
        /// M-OO.4 — `static methodName(args): R { body }`. Each entry
        /// desugars to a top-level `function __sm_<Class>__<name>(...) {...}`
        /// (no `__this` param). Call-site `<Class>.<method>(args)` is
        /// rewritten by `desugar_classes` to `__sm_<Class>__<method>(args)`.
        static_methods: Vec<ClassMethod>,
    },
    Return(Option<ExprId>),
    /// Phase J — `yield e;` inside a generator body. The post-parse
    /// `desugar_generators` pass rewrites every Yield into a state-
    /// machine arm that returns `{value: e, done: false}`. Plain
    /// (non-generator) bodies reject Yield at parse-time / desugar-time.
    Yield(ExprId),
    /// Phase J.4 — `let <var>(:T)? = yield <value>;` inside a generator
    /// body. desugar_generators expands this to `yield <value>;` +
    /// `let <var>(:T)? = this.__sent;` so the bound variable receives
    /// whatever was passed to the next-most `g.next(arg)` call.
    /// The iterator class gains a `__sent: <yield_ty>` field and
    /// `next()` takes an optional `__yield_arg` parameter that is
    /// stored into `this.__sent` on every resume.
    YieldInto {
        var: String,
        type_ann: Option<String>,
        value: ExprId,
    },
    /// Phase K.1 — `import` declaration. Single-file mode: parsed into
    /// the AST so the syntax is accepted, but the lowerer treats it as
    /// a no-op. K.2 will wire in the cross-file symbol table.
    ///
    /// Variants captured:
    ///   - `import { a, b as c } from "./x"` → `named: [(a, None), (b, Some(c))]`
    ///   - `import x from "./x"`              → `default: Some("x")`
    ///   - `import * as ns from "./x"`         → `namespace: Some("ns")`
    ///   - `import "./x"` (side-effect-only)  → all None
    ImportDecl {
        // K.2 will read these to populate the cross-file symbol
        // table. K.1 just preserves the parse-time data.
        #[allow(dead_code)]
        default: Option<String>,
        #[allow(dead_code)]
        namespace: Option<String>,
        #[allow(dead_code)]
        named: Vec<(String, Option<String>)>,
        source: String,
    },
    /// Phase K.1 — `export` declaration. Single-file mode strips the
    /// modifier from a wrapped declaration; K.2 will record the export
    /// list in the per-file symbol table.
    ///
    /// Variants:
    ///   - `export function f() {}`   → `inner: Some(<the FnDecl>)`
    ///   - `export const x = 1`        → `inner: Some(<the LetDecl>)`
    ///   - `export class C {}`         → `inner: Some(<the ClassDecl>)`
    ///   - `export type T = ...`       → `inner: Some(<the TypeDecl>)`
    ///   - `export { a, b }`           → `named: [(a, None), (b, Some(c))]`
    ///   - `export { a } from "./b"`   → `named: [(a, None)], source: Some("./b")`
    ///                                    (P13-S4 re-export)
    ///   - `export default <expr>`     → `default_expr: Some(...)`
    ExportDecl {
        inner: Option<Box<Stmt>>,
        // K.2 will read these to populate the export list.
        #[allow(dead_code)]
        named: Vec<(String, Option<String>)>,
        #[allow(dead_code)]
        default_expr: Option<ExprId>,
        /// P13-S4 — `from "./b"` clause on a bare named export, making
        /// it a re-export. `None` for inline `export { a }` (subset-
        /// rejected) and for `export <decl>` modifier form.
        #[allow(dead_code)]
        source: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ClassCtor {
    pub params: Vec<Param>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub struct ClassMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<String>,
    pub body: Vec<Stmt>,
    /// M-OO.6 — `abstract method(): T;`. Body is empty (`Vec::new()`) when
    /// abstract. desugar_classes skips emitting `__cm_<C>__<m>` for
    /// abstract methods (no body to lower); the corresponding `__cm_*`
    /// must come from a concrete override in a subclass. Validation that
    /// concrete subclasses cover every inherited abstract is done in
    /// desugar_classes' chain walk.
    pub is_abstract: bool,
    /// M-OO.5 — visibility modifier (default `Public`). Enforced at
    /// typecheck (check.rs): `Private` rejects access from outside the
    /// declaring class; `Protected` rejects access from outside the
    /// declaring class + its descendants.
    pub visibility: Visibility,
    /// P8.2 — accessor kind for `get X(): T { ... }` / `set X(v: T) { ... }`
    /// class members (ECMA §10.1.7 — accessor descriptors). `None` for
    /// regular methods (no `get`/`set` keyword); `Some(Getter)` or
    /// `Some(Setter)` when the parser saw the contextual keyword.
    /// Downstream uses: check.rs resolves `c.X` (read) to the getter's
    /// `return_type`, and `c.X = v` (write) to the setter's first param
    /// type; ssa_lower emits a Call to `__cm_<Class>__<X>_get` /
    /// `_set` instead of a Load / Store at the member offset.
    pub accessor_kind: Option<AccessorKind>,
}

/// P8.2 — accessor descriptor kind (ES §10.1.7). Stored on
/// `ClassMethod` for members declared with the `get` / `set`
/// contextual keywords. Affects: parser member-name dispatch
/// (consume the keyword, parse the property name + body as a
/// method); check.rs Member type resolution (use accessor's
/// type signature, not field layout); ssa_lower Member load/store
/// (emit Call to the accessor instead of direct Load/Store at offset);
/// desugar (synthesize `__cm_<Class>__<prop>_get` / `_set` method
/// bindings so dispatch finds them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessorKind {
    Getter,
    Setter,
}

/// M-OO.5 — TypeScript-style visibility modifier on class members.
/// `Public` is the parse-time default and never appears explicitly in
/// the source; `Private` corresponds to `private`; `Protected` to
/// `protected`. The spec-defined hard-private `#name` form ships in
/// P8.1 (see [[P8.1]] in roadmap.md) via parse-time mangling to
/// `__priv_<Class>__<name>` and Visibility::Private with exact-class
/// enforcement; this enum carries both — the mangling prefix marks
/// `#`-style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    Public,
    Private,
    Protected,
}

/// M-OO.4 — `static fieldName: T = init` entry. Init is mandatory because
/// static fields aren't reachable from the constructor (they're per-class,
/// not per-instance). desugar_classes rewrites each into a top-level
/// `let __sf_<Class>__<name>: T = init`, where the K.3 / K.4 globals
/// machinery promotes the binding to a real LLVM data slot.
#[derive(Debug, Clone)]
pub struct StaticField {
    pub name: String,
    pub type_ann: String,
    pub init: ExprId,
}

/// P8.3 — one entry in a class's static initialization sequence, kept
/// in **source order** so the desugar emit preserves the spec-required
/// interleaving (ES2022 §15.7.10 ClassStaticBlockEvaluation):
/// `class C { static x = 1; static { f(); } static y = 2; }` runs
/// x-init, then the block, then y-init — three independent vecs would
/// lose this order. Static methods stay in `ClassDecl.static_methods`:
/// they're declarations, not init steps, and don't participate in the
/// interleave.
#[derive(Debug, Clone)]
pub enum StaticInit {
    /// `static name: T = init`
    Field(StaticField),
    /// `static { stmts; }` — body executed once at class-init time
    /// with `this` bound to the class itself (textbook static-context).
    Block(Vec<Stmt>),
}

#[derive(Debug, Clone, Default)]
pub struct Ast {
    pub stmts: Vec<Stmt>,
    pub exprs: Vec<Expr>,
    /// Recorded by `desugar_classes` so post-desugar passes (check, ssa_lower)
    /// can resolve `instanceof Parent` on a subclass instance: maps each
    /// declared class name to its parent (None if no `extends`). Empty
    /// before desugar runs and for programs with no class declarations.
    pub class_parents: std::collections::HashMap<String, Option<String>>,
    /// Phase H.3.b — method name → declaring classes in source order
    /// (deepest sub last). Used by ssa_lower's `__dispatch_<M>` Call
    /// interception to emit the runtime tag-switch and call the right
    /// owner's `__cm_<C>__M`. Single-owner methods aren't kept here
    /// since they go through static `__cm_<Owner>__M` dispatch directly.
    pub method_owners: std::collections::HashMap<String, Vec<String>>,
    /// Name-based class-method rewrites desugar made speculatively,
    /// `call ExprId → alt ExprId` where alt clones the original
    /// member-call shape. check.rs demotes a rewrite (types the alt
    /// instead) when the receiver checks as a builtin container —
    /// full mechanism in cm_demote.rs.
    pub speculative_cm_rewrites: std::collections::HashMap<ExprId, ExprId>,
    /// T-24 — virtual method index. Populated only for `chain_methods`
    /// (methods with multiple owners forming a single inheritance
    /// chain — the override case that goes through `__dispatch_<M>`).
    /// Each chain method gets a stable u32 slot in the per-class
    /// vtable; ssa_lower's dispatch interception loads
    /// `vtable_ptr[method_index] -> fn_ptr` and `CallIndirect`s. The
    /// indices are deterministic (sorted by method name) so codegen
    /// is reproducible across builds.
    pub method_index: std::collections::HashMap<String, u32>,
    /// M-OO.5 — `(class_name, member_name)` → visibility, populated by
    /// the parser when a `private` / `protected` modifier appears on a
    /// field or method. Public is the absent-default (no entry stored).
    /// `static_fields` and `static_methods` get the same treatment —
    /// the entry's `class_name` is the class's own name regardless of
    /// instance-vs-static. check.rs reads this map at every Member
    /// access site to enforce the modifier.
    pub member_visibility: std::collections::HashMap<(String, String), Visibility>,
    /// M-OO.5 — `(class_name, field_name)` set of `readonly` fields.
    /// Both instance and static fields can be readonly. check.rs rejects
    /// `obj.field = ...` (instance) and `Class.field = ...` (static)
    /// when the entry is present. Readonly inside the constructor /
    /// class init context is allowed; check.rs's caller-context tracking
    /// lifts the restriction for the same path that visibility uses.
    pub readonly_fields: std::collections::HashSet<(String, String)>,
    /// P8.2 — accessor descriptor side-channel maps (ES §10.1.7).
    /// `(class_name, property_name) → return_type` for getters and
    /// `(class_name, property_name) → param_type` for setters. Populated
    /// by `desugar_classes` while walking ClassDecl members tagged
    /// with `accessor_kind = Some(...)`. The desugared FnDecl is named
    /// `__cm_<class>__<prop>_get` / `__cm_<class>__<prop>_set` so it
    /// can't collide with a same-named regular method or with a
    /// pair of get/set on the same property. check.rs reads the
    /// getter map at Member type resolution (read side) and the
    /// setter map at Assign-Member typecheck (write side); ssa_lower
    /// reads both to emit a Call to the synthesised method instead of
    /// a direct field Load / Store at offset.
    pub accessor_getters: std::collections::HashMap<(String, String), String>,
    pub accessor_setters: std::collections::HashMap<(String, String), String>,
    /// Phase L.2 — names of `async function` declarations recorded by
    /// the parser. desugar_async iterates ast.stmts and, for any
    /// FnDecl whose name is in this set, wraps the return value in a
    /// Promise and shifts the surface return type from T to Promise<T>.
    /// Avoids adding an `is_async: bool` to every FnDecl construction
    /// site.
    pub async_fns: std::collections::HashSet<String>,
    /// Ownership pass — per-function bitmap of which params get
    /// "consumed" (transferred) by the call site instead of
    /// borrowed. A param consumes if its body passes the param into a
    /// `__new_*` constructor factory (which stores it into a class
    /// field) or into another fn already known to consume that
    /// position. Computed by `compute_consuming_params` after all
    /// desugars; check.rs / ssa_lower consult this map at call sites
    /// to decide whether to mark the caller's binding as moved.
    /// Without this, `let g = make_iter(arr); ... drop` creates a
    /// double-free because both `arr` and `g`'s field own the same
    /// heap.
    pub consuming_params: std::collections::HashMap<String, Vec<bool>>,
    /// Plan A — Array literal ExprIds that the escape verifier proved
    /// safe to emit on the stack instead of `__torajs_arr_alloc_pooled`.
    /// Populated by `escape_analyze_array_literals`. ssa_lower's
    /// `Expr::Array` arm checks membership and switches between the
    /// AllocaBytes path (stack) and the heap path. Empty before the
    /// pass runs and for programs with no qualifying literals.
    pub stack_array_literals: std::collections::HashSet<ExprId>,
    /// v0.3 #4 DWARF — per-Expr source byte ranges. Indexed by
    /// ExprId.0; `Span { start: 0, end: 0 }` is the sentinel for
    /// "not set" (parser fills these on key Expr-emit sites; fallback
    /// chain at panic time uses the nearest enclosing set-span when
    /// a leaf has none). Source-buffer `source` lets the byte-range
    /// translate to (line, col) on demand.
    pub expr_spans: Vec<crate::lexer::Span>,
    /// Original source text for the file this Ast was parsed from.
    /// Empty before the parser fills it. Used by `byte_to_line_col`
    /// to derive DWARF DILocation values without re-reading files.
    pub source: String,
    /// Cached newline byte offsets, lazily built on first
    /// `byte_to_line_col` call. Empty before that.
    pub newline_offsets: Vec<u32>,
}

/// Phase L.2 — rewrite each `async function f(args): T { body }` into
/// a regular FnDecl returning `Promise<T>` whose body wraps the
/// original return values in a Promise:
///
///   function f(args): Promise<T> {
///     let __async_p = new Promise(<default T>);
///     <body, with each `return e;` rewritten to `__async_p.do_resolve(e); return __async_p;`>
///     return __async_p;
///   }
///
/// MVP scope:
///   - `Promise` must be the user-declared L.1 class (or any class
///     with `do_resolve(v: T): void`); we don't synthesize one here.
///   - `await e` is already lowered to `e.value` at parse time, so
///     this pass doesn't need to touch it.
///   - The original return type annotation IS required (no inference).
///
/// T-19.m (v0.5.0) — rename a user-declared `function main()` to
/// `__user_main` so it doesn't collide with the synthesized OS-entry
/// `main` (i32 return, top-level statements as body) that ssa_lower
/// emits unconditionally. Both ended up in the same LLVM module
/// under the symbol `main` → verify error
/// `Function return type does not match operand type of return inst`
/// (the user's i64-returning body vs the entry's required i32).
///
/// Walks `ast.stmts` for any FnDecl with `name == "main"`, renames
/// it AND rewrites every Call/Ident reference in the program. Idents
/// in nested expression positions (object methods, struct fields,
/// import aliases) are intentionally left alone — only bare-name
/// callees and ident references count. After this pass, any user
/// code that called `main()` calls `__user_main()` with identical
/// semantics; the synthesized OS-entry retains the `main` symbol.
pub fn rename_user_main(ast: &mut Ast) {
    let has_user_main = ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == "main"));
    if !has_user_main {
        return;
    }
    /* Rename FnDecl. */
    for s in ast.stmts.iter_mut() {
        if let Stmt::FnDecl { name, .. } = s
            && name == "main"
        {
            *name = "__user_main".into();
        }
    }
    /* Rewrite every Expr::Ident("main") in the expression arena —
     * call sites resolve via Ident, so this catches both `main()`
     * and `let f = main; f()`. Member expressions like `obj.main`
     * stay untouched; their `.main` is a struct field name, not
     * a top-level fn. */
    let n = ast.exprs.len();
    for i in 0..n {
        if let Expr::Ident(ref mut name) = ast.exprs[i]
            && name == "main"
        {
            *name = "__user_main".into();
        }
    }
    /* Update async_fns side-table — `desugar_async` consults this
     * and would fail to find the renamed fn otherwise. */
    if ast.async_fns.remove("main") {
        ast.async_fns.insert("__user_main".into());
    }
}

/// K.1 single-file desugar — strip every `Stmt::ExportDecl { inner }`
/// wrapper, replacing it in-place with `inner` so downstream check.rs
/// / ssa_lower see the wrapped FnDecl / TypeDecl / LetDecl as a normal
/// top-level declaration. `Stmt::ImportDecl` and the bare named-export
/// (`export { a, b }`) form are left as-is — they're parse-only at K.1
/// and will be picked up by K.2's cross-file symbol table pass.
pub fn unwrap_exports(ast: &mut Ast) {
    let mut new_stmts: Vec<Stmt> = Vec::with_capacity(ast.stmts.len());
    for s in std::mem::take(&mut ast.stmts) {
        if let Stmt::ExportDecl {
            inner: Some(boxed), ..
        } = s
        {
            new_stmts.push(*boxed);
        } else {
            new_stmts.push(s);
        }
    }
    ast.stmts = new_stmts;
}

/// Rewrite `new <BuiltinClass>(args)` into a direct call to the
/// matching `__torajs_<class>_*` intrinsic. Runs before
/// `desugar_classes` (which has an early-return when no user
/// `class` declarations exist) so built-in News still get rewritten
/// in pure-builtin programs. v0.2 #2 covers Date; future built-ins
/// (BigInt, Map, Set, ...) extend the match arm.
/* Built-in module names whose `import` statements register the
 * imported names as aliases for `<module>.<name>` member access.
 * E.g. `import { readFileSync } from "fs"` is desugared so any later
 * `readFileSync(path)` call lowers as `fs.readFileSync(path)` —
 * routed through the existing fs-namespace dispatch in ssa_lower.
 *
 * Cross-file user imports are unaffected; this pass only acts when
 * `source` is one of the known built-in module names. */
fn is_builtin_module(source: &str) -> bool {
    matches!(
        source,
        "fs" | "node:fs" | "fs/promises" | "node:fs/promises"
    )
}

/// T-18.a (v0.5.0) — sanitize the module name for the Ident-based
/// desugar lookup. Slash isn't a valid Ident; rewrite "fs/promises"
/// → "__fs_promises" so the Member rewrite produces a parseable
/// `__fs_promises.readFile(...)` shape. check.rs / ssa_lower
/// recognize the sanitized name.
fn sanitize_module_name(source: &str) -> String {
    source
        .strip_prefix("node:")
        .unwrap_or(source)
        .replace('/', "_")
}

pub fn desugar_builtin_imports(ast: &mut Ast) {
    use std::collections::HashMap;
    /* Build name → (module, original_name). The local alias (if
     * the user wrote `import { x as y }`) is the lookup key; the
     * original name is the field used in the Member rewrite. */
    let mut imported: HashMap<String, (String, String)> = HashMap::new();
    let mut to_drop: Vec<usize> = Vec::new();
    for (idx, s) in ast.stmts.iter().enumerate() {
        if let Stmt::ImportDecl {
            source,
            named,
            default: _,
            namespace,
        } = s
            && is_builtin_module(source)
        {
            let module_name = sanitize_module_name(source);
            for (orig, alias) in named {
                let local = alias.clone().unwrap_or_else(|| orig.clone());
                imported.insert(local, (module_name.clone(), orig.clone()));
            }
            /* `import * as ns from "fs"` — bind ns directly to the
             * fs namespace ident. */
            if let Some(ns) = namespace {
                imported.insert(ns.clone(), (module_name.clone(), String::new()));
            }
            to_drop.push(idx);
        }
    }
    if imported.is_empty() {
        return;
    }
    /* Drop the import stmts in reverse so indices stay valid. */
    for &idx in to_drop.iter().rev() {
        ast.stmts.remove(idx);
    }
    /* Rewrite Ident(local) → Member(Ident(module), original) across
     * the whole expr arena. Skip the rewrite when the Ident is the
     * `obj` field of a Member (already a member-access target —
     * leave shape alone). */
    let n = ast.exprs.len();
    for i in 0..n {
        let plan = match &ast.exprs[i] {
            Expr::Ident(name) => imported.get(name).cloned(),
            _ => None,
        };
        if let Some((module, orig)) = plan {
            if orig.is_empty() {
                /* Namespace import — bind to the module ident. */
                ast.exprs[i] = Expr::Ident(module);
            } else {
                let module_id = ast.add_expr(Expr::Ident(module));
                ast.exprs[i] = Expr::Member {
                    obj: module_id,
                    name: orig,
                };
            }
        }
    }
}

/// Thin wrapper preserving the `ast::desugar_builtin_new` public
/// surface for `torajs-cli` callers; body moved into
/// `ast_desugar_builtin_new` sibling (chunk 139).
pub fn desugar_builtin_new(ast: &mut Ast) {
    crate::ast_desugar_builtin_new::run(ast);
}

/// V3-18 m2.f — rewrite `X.prototype.foo.call(recv, ...args)` to
/// the equivalent direct-method form `recv.foo(...args)`. Tora has
/// no real prototype object so the literal traversal would fail at
/// `Number.prototype.toString` (Type::Null doesn't have .toString).
/// Pattern-matched at the AST level so check.rs / ssa_lower see only
/// the rewritten form. Ns coverage: every constructor namespace
/// listed (Number / String / Boolean / Object / Array / BigInt /
/// Symbol / Function / Date / RegExp / Error). `.apply(recv, args)`
/// is similar but takes args as an array — handled in a follow-up
/// when an array-spread call shape lands.
pub fn desugar_prototype_call(ast: &mut Ast) {
    let n = ast.exprs.len();
    for i in 0..n {
        let Expr::Call { callee, args } = ast.exprs[i].clone() else {
            continue;
        };
        let Expr::Member {
            obj: outer_obj,
            name: outer_name,
        } = ast.get_expr(callee).clone()
        else {
            continue;
        };
        if outer_name != "call" || args.is_empty() {
            continue;
        }
        let Expr::Member {
            obj: inner_obj,
            name: method_name,
        } = ast.get_expr(outer_obj).clone()
        else {
            continue;
        };
        let Expr::Member {
            obj: ns_id,
            name: proto_name,
        } = ast.get_expr(inner_obj).clone()
        else {
            continue;
        };
        if proto_name != "prototype" {
            continue;
        }
        let Expr::Ident(ns) = ast.get_expr(ns_id).clone() else {
            continue;
        };
        let known_ns = matches!(
            ns.as_str(),
            "Number"
                | "String"
                | "Boolean"
                | "BigInt"
                | "Symbol"
                | "Object"
                | "Array"
                | "Function"
                | "Date"
                | "RegExp"
                | "Error"
                | "Promise"
                | "Map"
                | "Set"
        );
        if !known_ns {
            continue;
        }
        let recv = args[0];
        let rest = args[1..].to_vec();
        let new_callee = ast.add_expr(Expr::Member {
            obj: recv,
            name: method_name,
        });
        ast.exprs[i] = Expr::Call {
            callee: new_callee,
            args: rest,
        };
    }
}

/// Walk one Stmt (and any Stmts / Exprs it contains) looking for
/// store-sites where a bare top-level FnDecl reference appears in a
/// Closure-typed position. Mutates `targets` / `rewrites` in place.

/// Walk an Expr looking for nested store-sites (Call args, nested
/// ObjectLits not directly under a typed LetDecl, etc.).
pub(crate) fn collect_expr_fn_to_closure(
    ast: &Ast,
    eid: ExprId,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>)>,
    targets: &mut std::collections::HashSet<String>,
    rewrites: &mut Vec<(ExprId, String)>,
) {
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            // Call args are FnSig-typed (parse_type maps `__fn` →
            // FnSig); bare top-FnDecl Idents passed as callbacks
            // stay raw FnSig values, matching the FnSig param ABI
            // directly. We only need to recurse for nested ObjectLits
            // (e.g. `f({k: top_fn})` inside a typed param position
            // would still need the ObjectLit-field arm).
            collect_expr_fn_to_closure(ast, *callee, fn_sigs, targets, rewrites);
            for arg in args {
                collect_expr_fn_to_closure(ast, *arg, fn_sigs, targets, rewrites);
            }
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            collect_expr_fn_to_closure(ast, *obj, fn_sigs, targets, rewrites);
        }
        Expr::Index { obj, index } => {
            collect_expr_fn_to_closure(ast, *obj, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *index, fn_sigs, targets, rewrites);
        }
        Expr::Assign { target, value } => {
            collect_expr_fn_to_closure(ast, *target, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *value, fn_sigs, targets, rewrites);
        }
        Expr::BinOp { left, right, .. } => {
            collect_expr_fn_to_closure(ast, *left, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *right, fn_sigs, targets, rewrites);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. }
        | Expr::As { expr, .. } => {
            collect_expr_fn_to_closure(ast, *expr, fn_sigs, targets, rewrites);
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_fn_to_closure(ast, *cond, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *then_branch, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *else_branch, fn_sigs, targets, rewrites);
        }
        Expr::Sequence { left, right }
        | Expr::Nullish {
            lhs: left,
            rhs: right,
        } => {
            collect_expr_fn_to_closure(ast, *left, fn_sigs, targets, rewrites);
            collect_expr_fn_to_closure(ast, *right, fn_sigs, targets, rewrites);
        }
        Expr::Array(eids) => {
            for e in eids {
                collect_expr_fn_to_closure(ast, *e, fn_sigs, targets, rewrites);
            }
        }
        Expr::ObjectLit { fields } => {
            // Untyped ObjectLit — only recurse into fields (no
            // closure-typed signal available without surrounding
            // LetDecl context).
            for (_, eid) in fields {
                collect_expr_fn_to_closure(ast, *eid, fn_sigs, targets, rewrites);
            }
        }
        Expr::PostIncr { target, .. } => {
            collect_expr_fn_to_closure(ast, *target, fn_sigs, targets, rewrites);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                collect_expr_fn_to_closure(ast, *a, fn_sigs, targets, rewrites);
            }
        }
        _ => {}
    }
}

/// When a LetDecl has shape `const o: T = { k: v, ... }` and `T`
/// resolves to a known TypeDecl whose field `k` is fn-typed, and `v`
/// is a bare Ident referring to a top-level FnDecl, mark `v` as
/// rewrite-target.
pub(crate) fn collect_objectlit_fn_to_closure(
    ast: &Ast,
    init: ExprId,
    type_ann: Option<&str>,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>)>,
    struct_field_anns: &std::collections::HashMap<
        String,
        std::collections::HashMap<String, String>,
    >,
    targets: &mut std::collections::HashSet<String>,
    rewrites: &mut Vec<(ExprId, String)>,
) {
    let Some(ann) = type_ann else { return };
    let Some(field_anns) = struct_field_anns.get(ann.trim()) else {
        return;
    };
    if let Expr::ObjectLit { fields } = ast.get_expr(init) {
        for (fname, feid) in fields {
            if let Some(fann) = field_anns.get(fname)
                && is_fn_like_ann(fann)
                && let Expr::Ident(name) = ast.get_expr(*feid)
                && fn_sigs.contains_key(name)
            {
                targets.insert(name.clone());
                rewrites.push((*feid, name.clone()));
            }
        }
    }
}

/// P4.4 — `Function.prototype.bind / call / apply` desugar pass.
/// Rewrites Call exprs whose callee is `Member { obj: Ident(f), name: m }`
/// (m ∈ {"bind", "call", "apply"}) into substrate-correct shapes that
/// piggyback on tora's existing FnSig / Closure ABIs. Source fn must
/// be a top-level FnDecl Ident — typed-tier substrate only. thisArg
/// is silently discarded (tora has no `this` for top-level fns;
/// class-method `this` is a separate substrate, not in P4.4 scope).
///
///   .call(thisArg, a1, a2)       →  f(a1, a2)
///   .apply(thisArg, [a1, a2])    →  f(a1, a2)            [array-lit only]
///   .bind(thisArg, p0, p1)       →  __bind_create_<id>(p0, p1)
///
/// `__bind_create_<id>` is a synthesized factory FnDecl: takes
/// partials as positional params, returns an arrow that captures
/// them. The arrow's body forwards to the source fn with
/// (partials..., remaining_args...). The capture mechanism is the
/// existing lift_arrow_fns substrate — the arrow inside the factory
/// gets lifted automatically.
///
/// Subset limits at this phase:
///   - `.apply` with non-literal array → left as-is, fails later
///     with informative typecheck error (full dynamic spread is
///     P5.5).
///   - `.bind` source must be Ident referring to a known FnDecl
///     (closure or runtime-Any source is post-P4.4 substrate).
///   - thisArg is silently dropped — `this` semantics arrive with
///     full class substrate (post-P4 / P8 class spec).
///
/// Runs AFTER `lift_arrow_fns` / `synthesize_*_forwarders` so the
/// synthesized factory's inner arrow goes through the normal
/// closure-lift pipeline on the SECOND lift pass... actually we
/// need to be careful here — once lift_arrow_fns has run, new
/// arrows we emit won't be lifted. Solution: emit the FACTORY's
/// inner arrow as a SEPARATE pre-lifted FnDecl `__bound_<id>`
/// with explicit `__env(p0|p1|...)` annotation, and the factory
/// body returns `Expr::Closure { fn_name: "__bound_<id>",
/// captures: ["p0", "p1", ...] }` directly. Matches the post-
/// lift_arrow_fns invariant.
/// Thin wrapper preserving the `ast::desugar_function_prototype_methods`
/// public surface for `torajs-cli` callers (main / lsp / repl /
/// cmd_build); body moved into `ast_desugar_function_prototype_methods`
/// sibling (chunk 138).
pub fn desugar_function_prototype_methods(ast: &mut Ast) {
    crate::ast_desugar_function_prototype_methods::run(ast);
}

/// Thin wrapper preserving the `ast::desugar_implicit_generics`
/// public surface for `torajs-cli` callers; body moved into
/// `ast_desugar_implicit_generics` sibling (chunk 140).
pub fn desugar_implicit_generics(ast: &mut Ast) {
    crate::ast_desugar_implicit_generics::run(ast);
}

impl Ast {
    pub fn add_expr(&mut self, e: Expr) -> ExprId {
        let id = ExprId(self.exprs.len() as u32);
        self.exprs.push(e);
        // Sentinel span until parser (or a desugar pass) sets a real
        // one via `set_expr_span`. Both fields zero means "unknown".
        self.expr_spans
            .push(crate::lexer::Span { start: 0, end: 0 });
        id
    }

    /// v0.3 #4 — record the source byte range of `eid`'s originating
    /// token (or sub-token range). Idempotent in the sense that
    /// later calls overwrite earlier ones; parser is the canonical
    /// caller, but desugar passes that emit synthetic Exprs may also
    /// inherit a span from their originating user node.
    pub fn set_expr_span(&mut self, eid: ExprId, span: crate::lexer::Span) {
        if (eid.0 as usize) < self.expr_spans.len() {
            self.expr_spans[eid.0 as usize] = span;
        }
    }

    /// Build the newline offset table once. Idempotent. Call this
    /// after setting `self.source`; subsequent `byte_to_line_col`
    /// lookups become `&self`-only and can be invoked from
    /// borrow-restricted contexts (codegen, etc).
    pub fn warm_newline_cache(&mut self) {
        if !self.newline_offsets.is_empty() || self.source.is_empty() {
            return;
        }
        for (i, b) in self.source.as_bytes().iter().enumerate() {
            if *b == b'\n' {
                self.newline_offsets.push(i as u32);
            }
        }
    }

    /// Translate a source byte offset into a (line, col) pair, both
    /// 1-indexed (DWARF / editor convention). Returns (0, 0) if the
    /// offset is past end-of-source (sentinel for "no location").
    /// Requires `warm_newline_cache` to have been called when source
    /// is non-empty; otherwise returns (0, 0) for non-zero offsets.
    pub fn byte_to_line_col(&self, byte: u32) -> (u32, u32) {
        if byte == 0 || (byte as usize) > self.source.len() {
            return (0, 0);
        }
        let nl = &self.newline_offsets;
        if nl.is_empty() && !self.source.is_empty() {
            // Cache not warmed; line 1, col=byte+1 as fallback.
            return (1, byte + 1);
        }
        let line = match nl.binary_search(&byte) {
            Ok(k) => (k as u32) + 1,
            Err(k) => (k as u32) + 1,
        };
        let line_start = if line == 1 {
            0u32
        } else {
            nl[(line - 2) as usize] + 1
        };
        let col = byte - line_start + 1;
        (line, col)
    }

    pub fn get_expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.0 as usize]
    }

    pub fn print(&self) {
        for s in &self.stmts {
            crate::ast_printer::print_stmt(self, s, 0);
        }
    }
}
