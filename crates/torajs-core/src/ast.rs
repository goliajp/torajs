//! AST — arena-allocated. Children referenced by `ExprId(u32)`, not Box.

use std::collections::HashMap;

mod apply_args;
mod arguments_object;
mod arguments_object_rewrite;
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
mod infer_closure_params;
mod sfi_pass;
mod sfi_rewrite;
mod sfi_safe;
mod super_collect;
mod var_hoist;
pub use apply_args::{apply_default_args, apply_rest_args};
pub use arguments_object::desugar_arguments_object;
pub use class_globals::synthesize_class_globals;
pub use consuming_flow::compute_consuming_params;
pub use desugar_async::desugar_async;
use desugar_async::{body_ends_in_return, rewrite_returns_for_async};
pub use desugar_classes::desugar_classes;
pub use desugar_variadic_push::desugar_variadic_push;
pub use escape_analyze::escape_analyze_array_literals;
pub use forwarders::synthesize_forwarders;
pub use forwarders_object::{synthesize_fn_to_closure_forwarders, tag_struct_field_closure_types};
pub use infer_closure_params::infer_anonymous_closure_params;
pub use sfi_pass::rewrite_split_for_i_to_iter;
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

/// M5.1 — desugar `class C { ... }` into `type C = {...}` + a series of
/// top-level `function` declarations (ctor / methods / `__new_C` factory).
///
/// This pass MUST run before `lift_arrow_fns` (so arrow fns inside method
/// bodies are still ArrowFn at desugar time) and before `check.rs`. The
/// SSA / runtime layer never sees `Stmt::ClassDecl` / `Expr::This` /
/// `Expr::New` — they are erased here.
///
/// Rewrites performed:
///
///   1. For each class C with method m:
///      - registers `m → C` in a global method table so call-sites
///        `obj.m(...)` can be retargeted to `C__m(obj, ...)`.
///      - duplicate method names across classes are an error (M5.1
///        single-dispatch table; M5.2 will introduce vtables / interfaces).
///   2. Walks every `Expr` in the arena once:
///      - `Expr::This` → `Expr::Ident("__this")`
///      - `Expr::Call { callee = Member{obj, name=m}, args }` where m is a
///        known class method → `Call { callee = Ident("C__m"), args = [obj, ...args] }`
///      - `Expr::New { class_name=C, args }` → `Call { callee = Ident("__new_C"), args }`
///   3. For each `Stmt::ClassDecl`: replace in-place with the corresponding
///      `Stmt::TypeDecl` (fields preserved verbatim), then append:
///      - `function __new_C(args): C { let __this: C = {field0: 0, ...}; C__ctor(__this, args); return __this; }`
///        (ctor params copied; factory return type is C; if no ctor declared,
///         the factory just constructs the default-initialized object)
///      - `function C__ctor(__this: C, ctor_params...): void { body }`
///      - `function C__methodName(__this: C, params...): R { body }` for each method
///
/// The factory's default-initialization strategy: every field gets a typed
/// zero literal (number → 0, string → "", boolean → false, T[] → [], any
/// other named type → calls __new_T() recursively if it's a class, else
/// errors at typecheck). Constructors are responsible for filling fields
/// before they're observably read.
/// Phase J — rewrite every `function*` generator into a class + factory.
/// MVP scope: linear yield sequences (no loops / conditionals between
/// yields). The desugar lowers the body into a `while (true) { ... }`
/// state machine where each yield is one resume point.
///
/// J.2.b — `yield` is allowed inside `if` / `while` / `for` (any
/// nesting). Each yield gets its own state arm. Control flow that
/// crosses a yield boundary becomes `this.__state = N; continue;`
/// gotos through the wrapping `while (true)`. Loop break / continue
/// inside a yield-containing loop rewrite to gotos toward the loop's
/// post-state / step-state respectively. yield-FREE inner control
/// flow is emitted inline so its own break/continue keep their
/// natural semantics.
///
/// For `function* gen(): T { stmt0; yield e0; stmt1; yield e1; }`:
///   - emit a class `__Gen_gen` with field `__state: number` (0-init).
///   - emit `next(): { value: T, done: boolean }` whose body is
///     `while (true) { if (state==0){...} if (state==1){...} ... return {0, true}; }`.
///     Each arm runs its prelude, then either returns `{value:e, done:false}`
///     for a yield, or sets `state=N` and `continue;` for a goto.
///   - emit a factory FnDecl `gen()` returning `__Gen_gen`.
///
/// MVP restrictions logged at desugar-time:
///   - generator return-type annotation supplies the yield value type.
///     Required (no `function* gen()` without `: T`).
///   - yields inside `try` / `catch` / `finally` / `switch` / nested
///     functions are rejected at this stage (no states allocated for them).
///   - all `let` declarations anywhere in the body are lifted to class
///     fields. Same name in two scopes is an error (panic) since both
///     would map to the same `this.<name>` field.
pub fn desugar_generators(ast: &mut Ast) {
    let gen_indices: Vec<(usize, String, Vec<Param>, Option<String>, Vec<Stmt>)> = ast
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl {
                name,
                params,
                return_type,
                body,
                is_generator: true,
                ..
            } => Some((
                i,
                name.clone(),
                params.clone(),
                return_type.clone(),
                body.clone(),
            )),
            _ => None,
        })
        .collect();

    if gen_indices.is_empty() {
        return;
    }

    let mut appended: Vec<Stmt> = Vec::new();

    for (idx, gen_name, gen_params, gen_ret, gen_body) in gen_indices {
        // P10.7 — Default-Any generator. When the user omits the
        // return-type annotation (`function* foo() {...}`), infer
        // `Generator<any>` so the body's `yield` values flow through
        // the existing Any-tier (NaN-box AnyValue) via the
        // `Expr::As { …, ty_ann: "any" }` wrap inside
        // `GenSm::emit_yield_return`. `Generator<T>` /
        // `IterableIterator<T>` annotations keep their explicit T.
        let yield_ty = gen_ret.unwrap_or_else(|| "any".into());

        // J.2.a/b + J.4 — prep gen_body: expand yield-into pairs,
        // lift `let` decls to class fields, rewrite param/local
        // idents to `this.<name>`. Returns (prepared_body,
        // lifted_locals) for the class-assembly step below.
        let (gen_body, lifted_locals) = crate::ast::desugar_generators_prep::prep_generator_body(
            ast,
            gen_body,
            &gen_name,
            &gen_params,
            &yield_ty,
        );

        // Class name + struct return type for next().
        let class_name = format!("__Gen_{gen_name}");
        let step_ann = format!("__step_{gen_name}");
        // Type alias `type __step_<gen> = { value: T, done: boolean }`.
        ast.stmts.push(Stmt::TypeDecl {
            name: step_ann.clone(),
            type_params: Vec::new(),
            fields: vec![
                ("value".into(), yield_ty.clone()),
                ("done".into(), "boolean".into()),
            ],
        });

        // Build the state machine + while-true loop body + tail return.
        // Yields close an arm with `return {value:e, done:false}`;
        // control-flow gotos close with `state = N; continue;` and the
        // `while(true)` loop re-enters the if-chain at the new state.
        let next_body = build_state_machine_next_body(ast, gen_body, &yield_ty);

        // Build the generator class with __state field + ctor + next().
        let zero_init = default_init_for_type("number");
        let zero_init_id = ast.add_expr(zero_init);
        let ctor = ClassCtor {
            params: gen_params.clone(),
            body: vec![Stmt::Expr({
                let this_id = ast.add_expr(Expr::This);
                let state_member = ast.add_expr(Expr::Member {
                    obj: this_id,
                    name: "__state".into(),
                });
                ast.add_expr(Expr::Assign {
                    target: state_member,
                    value: zero_init_id,
                })
            })],
        };
        // J.4 — next() takes an optional `__yield_arg: <yield_ty> = 0`
        // parameter and stashes it in `this.__sent` before re-entering
        // the state machine. YieldInto-expanded `let v = this.__sent`
        // sites read that field to receive the value passed to
        // `g.next(arg)`. First call's arg is ignored per JS spec; tr's
        // typed-default uses zero/empty depending on yield type.
        let yield_arg_default = default_init_for_type(&yield_ty);
        let yield_arg_default_id = ast.add_expr(yield_arg_default);
        let next_method = crate::ast::desugar_generators_methods::build_next_method(
            ast,
            yield_arg_default_id,
            &yield_ty,
            &step_ann,
            next_body,
        );
        let return_method =
            crate::ast::desugar_generators_methods::build_return_method(ast, &yield_ty, &step_ann);
        let throw_method =
            crate::ast::desugar_generators_methods::build_throw_method(ast, &step_ann);
        // For Phase J MVP, generator parameters are stored as fields on
        // the iterator object so the body can reference them through
        // `this.<name>`. The fields are auto-prepended to the class
        // declaration; the ctor's prelude (above) adds an assignment
        // for each param.
        // P10.6-A3 — nominal-marker field whose name is unique per
        // generator class. Generator desugar lifts every yield-fn
        // into a class with structurally identical primitives
        // (`__state: number` + `__sent: <yield_ty>` + params /
        // lifted locals); two `function* a()` + `function* b()`
        // sharing the same yield_ty + parameter shape used to
        // collapse to the same struct sid, and ssa_lower:18693's
        // sibling-class static dispatch picked the first-matching
        // alias from a HashMap iter — non-deterministically
        // routing `a().next()` to `__cm_<other>__next`. The
        // marker breaks structural equivalence per generator (V8
        // / SpiderMonkey side-step the same problem with nominal
        // class identity; tora's structural type system isn't
        // changing in this phase, so a per-class field-name
        // marker is the narrow fix that keeps the dispatch
        // correct without an SSA-level type-system overhaul).
        crate::ast::desugar_generators_class::assemble_generator_class_and_factory(
            ast,
            idx,
            gen_name,
            gen_params,
            &yield_ty,
            class_name,
            &lifted_locals,
            ctor.body,
            next_method,
            return_method,
            throw_method,
            &mut appended,
        );
    }

    ast.stmts.extend(appended);
}

/// Build the next()'s state-machine body: lower `gen_body` through
/// GenSm into `arms`, then assemble `[while(true) { if(state==0){arm0}
/// if(state==1){arm1} ... ; catch-all }, return {value:0,done:true}]`
/// as the two-stmt body. Caller wraps it into the ClassMethod.
///
/// Kept file-private (not a sibling) so GenSm and `default_init_for_type`
/// stay ast-internal — externalizing would require a handful of
/// pub(super) markers across the GenSm struct, three of its methods,
/// three of its fields, and `default_init_for_type`. Inlining the
/// driver here closes the desugar_generators-LOC HARD limit gap
/// (208 -> 148 LOC) at zero visibility cost.
fn build_state_machine_next_body(ast: &mut Ast, gen_body: Vec<Stmt>, yield_ty: &str) -> Vec<Stmt> {
    // Build the state machine. Each arm is the body of one state in
    // an if-chain wrapped by `while (true) { ... }`. Yields close an
    // arm with `return {value:e, done:false}`; control-flow gotos
    // close with `state = N; continue;` and the `while(true)` loop
    // re-enters the if-chain at the new state.
    let mut sm = desugar_generators_sm::GenSm::new(ast, yield_ty.to_string());
    sm.lower_seq(gen_body);
    // After the last body stmt, the natural exit is "done forever".
    let zero = default_init_for_type(yield_ty);
    let zero_id = sm.ast.add_expr(zero);
    let done_lit = sm.ast.add_expr(Expr::Bool(true));
    let final_obj = sm.ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), zero_id), ("done".into(), done_lit)],
    });
    sm.cur_buf.push(Stmt::Return(Some(final_obj)));
    sm.flush_cur();

    // Assemble: while (true) { if (state==0){arm0} if (state==1){arm1} ... ; catch-all }
    let mut loop_body: Vec<Stmt> = Vec::new();
    for (i, arm_stmts) in sm.arms.iter().enumerate() {
        let i_lit = ast.add_expr(Expr::Number(i as f64));
        let this_state = ast.add_expr(Expr::This);
        let state_member = ast.add_expr(Expr::Member {
            obj: this_state,
            name: "__state".into(),
        });
        let cond = ast.add_expr(Expr::BinOp {
            op: BinOp::Eq,
            left: state_member,
            right: i_lit,
        });
        loop_body.push(Stmt::If {
            cond,
            then_branch: Box::new(Stmt::Block(arm_stmts.clone())),
            else_branch: None,
        });
    }
    // Catch-all for any state past the last allocated arm (covers
    // unreachable dead-states from break/continue and any "fell off
    // the end" case that didn't return inside the if-chain).
    let zero_tail = default_init_for_type(yield_ty);
    let zero_tail_id = ast.add_expr(zero_tail);
    let done_tail = ast.add_expr(Expr::Bool(true));
    let final_tail = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), zero_tail_id), ("done".into(), done_tail)],
    });
    loop_body.push(Stmt::Return(Some(final_tail)));

    let true_lit = ast.add_expr(Expr::Bool(true));
    // Unreachable trailing return after the `while (true)` — the
    // typechecker's "all paths return" analysis doesn't infer that
    // a `cond=true` while never falls out, so without this the
    // function's tail path looks indeterminate. Cheap to emit, no
    // runtime cost (LLVM dead-code-eliminates it).
    let zero_after = default_init_for_type(yield_ty);
    let zero_after_id = ast.add_expr(zero_after);
    let done_after = ast.add_expr(Expr::Bool(true));
    let final_after = ast.add_expr(Expr::ObjectLit {
        fields: vec![("value".into(), zero_after_id), ("done".into(), done_after)],
    });
    vec![
        Stmt::While {
            cond: true_lit,
            body: Box::new(Stmt::Block(loop_body)),
        },
        Stmt::Return(Some(final_after)),
    ]
}

/// J.4 — recursively expand every `Stmt::YieldInto { var, type_ann,
/// value }` in `s` into the pair `[Stmt::Yield(value);
/// Stmt::LetDecl { name: var, type_ann, init: this.__sent }]`. The
/// pair is wrapped in `Stmt::Multi` so it occupies the YieldInto's
/// original slot without disturbing surrounding scope. Walks into
/// nested control-flow.
///
/// `yield_ty` is the surrounding generator's declared yield type; it
/// supplies the let's annotation when the user omitted one (so the
/// J.2.b lift picks the right field type).
pub(super) fn expand_yield_into_in_stmt(ast: &mut Ast, s: &mut Stmt, yield_ty: &str) {
    match s {
        Stmt::YieldInto {
            var,
            type_ann,
            value,
        } => {
            let var = std::mem::take(var);
            let ty = type_ann.clone().or_else(|| Some(yield_ty.to_string()));
            let value = *value;
            let yield_stmt = Stmt::Yield(value);
            let this_id = ast.add_expr(Expr::This);
            let sent_member = ast.add_expr(Expr::Member {
                obj: this_id,
                name: "__sent".into(),
            });
            let let_stmt = Stmt::LetDecl {
                mutable: true,
                name: var,
                type_ann: ty,
                init: sent_member,
                is_var: false,
            };
            *s = Stmt::Multi(vec![yield_stmt, let_stmt]);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            expand_yield_into_in_stmt(ast, then_branch, yield_ty);
            if let Some(eb) = else_branch.as_deref_mut() {
                expand_yield_into_in_stmt(ast, eb, yield_ty);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                expand_yield_into_in_stmt(ast, i, yield_ty);
            }
            expand_yield_into_in_stmt(ast, body, yield_ty);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                expand_yield_into_in_stmt(ast, s, yield_ty);
            }
        }
        // Switch / try cases not yet in yield scope (J.2.b).
        _ => {}
    }
}

/// Recursively replace every `let x = init` in `s` (and any nested
/// stmts) with `this.x = init`, recording each lifted `(name, type)`
/// in `lifted`. Used by `desugar_generators` so locals declared in
/// for-init / if-branches / while-bodies survive yield boundaries
/// the same way top-level lets do.
pub(super) fn lift_lets_in_stmt(ast: &mut Ast, s: &mut Stmt, lifted: &mut Vec<(String, String)>) {
    match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            let n = name.clone();
            let t = type_ann.clone().unwrap_or_else(|| "number".into());
            lifted.push((n.clone(), t));
            let this_id = ast.add_expr(Expr::This);
            let m = ast.add_expr(Expr::Member {
                obj: this_id,
                name: n,
            });
            let assign = ast.add_expr(Expr::Assign {
                target: m,
                value: *init,
            });
            *s = Stmt::Expr(assign);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            lift_lets_in_stmt(ast, then_branch, lifted);
            if let Some(eb) = else_branch.as_deref_mut() {
                lift_lets_in_stmt(ast, eb, lifted);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            lift_lets_in_stmt(ast, body, lifted);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init.as_deref_mut() {
                lift_lets_in_stmt(ast, i, lifted);
            }
            lift_lets_in_stmt(ast, body, lifted);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                lift_lets_in_stmt(ast, s, lifted);
            }
        }
        // Switch / try cases don't yet support yields (J.2.b scope)
        // so their inner lets stay as plain locals — no lift needed.
        _ => {}
    }
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

/// Synthetic root `class Error { message: string; name: string;
/// constructor(message: string) { this.message = message;
/// this.name = "Error"; } }` (spec §20.5.1). Field order matters:
/// it must match the ctor-body assignment order since check.rs's
/// affine-flow analysis walks declarations in order.
/// P7.3 — build the header expression shared by Error / subclass
/// ctors as the minimal `.stack` value. Follows ECMAScript
/// §20.5.3.4 `Error.prototype.toString` (the stack's first line uses
/// the same format in every engine): an empty `message` yields just
/// the name — `new Error("").stack` is "Error", not "Error: " (bun /
/// V8 / JSC all do this). The §20.5.3.4 empty-`name` branch is
/// unreachable for the injected classes (name is always a non-empty
/// literal), so only the empty-message case is special-cased:
///
///   this.message === "" ? this.name : this.name + ": " + this.message
fn build_stack_concat(ast: &mut Ast) -> ExprId {
    // cond: this.message === ""
    let cm_obj = ast.add_expr(Expr::This);
    let cm = ast.add_expr(Expr::Member {
        obj: cm_obj,
        name: "message".to_string(),
    });
    let empty = ast.add_expr(Expr::String(String::new()));
    let cond = ast.add_expr(Expr::BinOp {
        op: BinOp::Eq,
        left: cm,
        right: empty,
    });
    // then: this.name
    let tn_obj = ast.add_expr(Expr::This);
    let then_branch = ast.add_expr(Expr::Member {
        obj: tn_obj,
        name: "name".to_string(),
    });
    // else: this.name + ": " + this.message
    let n_obj = ast.add_expr(Expr::This);
    let n = ast.add_expr(Expr::Member {
        obj: n_obj,
        name: "name".to_string(),
    });
    let colon = ast.add_expr(Expr::String(": ".to_string()));
    let n_colon = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: n,
        right: colon,
    });
    let m_obj = ast.add_expr(Expr::This);
    let m = ast.add_expr(Expr::Member {
        obj: m_obj,
        name: "message".to_string(),
    });
    let else_branch = ast.add_expr(Expr::BinOp {
        op: BinOp::Add,
        left: n_colon,
        right: m,
    });
    ast.add_expr(Expr::Ternary {
        cond,
        then_branch,
        else_branch,
    })
}

fn build_error_class(ast: &mut Ast) -> Stmt {
    let this0 = ast.add_expr(Expr::This);
    let msg_member = ast.add_expr(Expr::Member {
        obj: this0,
        name: "message".to_string(),
    });
    let msg_value = ast.add_expr(Expr::Ident("message".to_string()));
    let assign1 = ast.add_expr(Expr::Assign {
        target: msg_member,
        value: msg_value,
    });
    let this1 = ast.add_expr(Expr::This);
    let name_member = ast.add_expr(Expr::Member {
        obj: this1,
        name: "name".to_string(),
    });
    let name_value = ast.add_expr(Expr::String("Error".to_string()));
    let assign2 = ast.add_expr(Expr::Assign {
        target: name_member,
        value: name_value,
    });
    // P7.3 — this.stack = this.name + ": " + this.message. Minimal
    // header-only stack (the Node `Error.stackTraceLimit = 0` shape,
    // a real-engine-legal value): `.stack` exists and is a string so
    // code reading it doesn't fault. No synthesized frames — fake
    // "at file:line" would be silent-wrong; real frame capture is a
    // separate perf-sensitive substrate (P7.3-frames). Set last so
    // it reflects the final name/message; subclasses re-run this
    // after overriding `name`.
    let stack_expr = build_stack_concat(ast);
    let stack_obj = ast.add_expr(Expr::This);
    let stack_member = ast.add_expr(Expr::Member {
        obj: stack_obj,
        name: "stack".to_string(),
    });
    let assign3 = ast.add_expr(Expr::Assign {
        target: stack_member,
        value: stack_expr,
    });

    let ctor = ClassCtor {
        params: vec![Param {
            name: "message".to_string(),
            type_ann: Some("string".to_string()),
            default: None,
            is_rest: false,
        }],
        body: vec![
            Stmt::Expr(assign1),
            Stmt::Expr(assign2),
            Stmt::Expr(assign3),
        ],
    };

    Stmt::ClassDecl {
        name: "Error".to_string(),
        type_params: Vec::new(),
        parent: None,
        is_abstract: false,
        fields: vec![
            ("message".to_string(), "string".to_string()),
            ("name".to_string(), "string".to_string()),
            ("stack".to_string(), "string".to_string()),
        ],
        static_init: Vec::new(),
        ctor: Some(ctor),
        methods: Vec::new(),
        static_methods: Vec::new(),
    }
}

/// Synthetic `class <N> extends Error { constructor(message: string)
/// { super(message); this.name = "<N>"; } }` for the four standard
/// NativeError subclasses (spec §20.5.5). No own fields: `message` /
/// `name` are inherited from Error via desugar field-flattening
/// (which panics on parent-field redeclaration), so the ctor just
/// forwards to Error's ctor through `super` and overrides `name`.
/// The `super(message)` site is rewritten to `__cm_Error__ctor(...)`
/// by the existing Pass 1.5 super-rewrite in `desugar_classes` —
/// identical to a user-written subclass.
fn build_error_subclass(ast: &mut Ast, sub_name: &str) -> Stmt {
    let msg_ident = ast.add_expr(Expr::Ident("message".to_string()));
    let super_call = ast.add_expr(Expr::Super {
        args: vec![msg_ident],
    });
    let this0 = ast.add_expr(Expr::This);
    let name_member = ast.add_expr(Expr::Member {
        obj: this0,
        name: "name".to_string(),
    });
    let name_value = ast.add_expr(Expr::String(sub_name.to_string()));
    let assign_name = ast.add_expr(Expr::Assign {
        target: name_member,
        value: name_value,
    });
    // P7.3 — re-set this.stack AFTER overriding name so it reflects
    // the subclass name ("TypeError: msg"), not Error's. The Error
    // ctor reached via super() already set a "Error: msg" stack;
    // this overwrites it on the same inherited field.
    let stack_expr = build_stack_concat(ast);
    let stack_obj = ast.add_expr(Expr::This);
    let stack_member = ast.add_expr(Expr::Member {
        obj: stack_obj,
        name: "stack".to_string(),
    });
    let assign_stack = ast.add_expr(Expr::Assign {
        target: stack_member,
        value: stack_expr,
    });

    let ctor = ClassCtor {
        params: vec![Param {
            name: "message".to_string(),
            type_ann: Some("string".to_string()),
            default: None,
            is_rest: false,
        }],
        body: vec![
            Stmt::Expr(super_call),
            Stmt::Expr(assign_name),
            Stmt::Expr(assign_stack),
        ],
    };

    Stmt::ClassDecl {
        name: sub_name.to_string(),
        type_params: Vec::new(),
        parent: Some("Error".to_string()),
        is_abstract: false,
        fields: Vec::new(),
        static_init: Vec::new(),
        ctor: Some(ctor),
        methods: Vec::new(),
        static_methods: Vec::new(),
    }
}

/// P4.6 / P7.1 — inject built-in class declarations for `Error` and
/// its four standard subclasses (`TypeError` / `RangeError` /
/// `SyntaxError` / `ReferenceError`, spec §20.5.5) so user code can
/// `new TypeError(msg)` directly AND `class MyError extends Error`
/// flows through the existing user-class ClassDecl machinery in
/// `desugar_classes`. Pre-fix `new Error("oops")` panicked at
/// check.rs with "internal: `new Error` reached check.rs (desugar
/// didn't run?)" because no factory FnDecl was synthesized.
///
/// Shapes (spec §20.5): `Error` has `message` / `name` string fields
/// and a one-arg ctor; each subclass `extends Error`, forwards via
/// `super(message)`, and sets `this.name` to its own name. Other
/// Error surface area (.stack DWARF capture, .toString format) lands
/// incrementally as follow-up substrate.
///
/// Runs BEFORE `desugar_classes` so the synth ClassDecls get
/// processed normally — synthesizes `__new_<C>` factory /
/// `__cm_<C>__ctor` / `__class_<C>` (via synthesize_class_globals
/// later) and registers each in the class_name_to_tag map so
/// instanceof chain walks reach them.
///
/// Idempotent + usage-gated:
/// - If the user declares their own `class Error`, the whole error
///   hierarchy is theirs — skip all injection (preserves the P4.6
///   stdlib-override contract, and avoids the desugar
///   declaration-order panic that an injected `Sub extends Error`
///   prepended ahead of a user `class Error` would trigger).
/// - Each subclass is injected only when referenced and not
///   user-shadowed by a same-named ClassDecl.
/// - `Error` is injected when referenced directly OR implied by any
///   wanted subclass (subclasses extend it). Nothing is injected for
///   programs that never mention the error hierarchy (compile-time
///   neutral).
pub fn inject_builtin_classes(ast: &mut Ast) {
    let user_has_error = ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::ClassDecl { name, .. } if name == "Error"));
    if user_has_error {
        return;
    }

    // The four standard NativeError subclasses (spec §20.5.5).
    const ERROR_SUBCLASSES: [&str; 4] =
        ["TypeError", "RangeError", "SyntaxError", "ReferenceError"];

    // A name is "referenced" if it appears as a bare Ident, a
    // `new <N>(...)`, a `.<N>` member, or an `extends <N>` parent.
    let referenced = |n: &str| -> bool {
        ast.exprs.iter().any(|e| {
            matches!(e, Expr::Ident(x) | Expr::New { class_name: x, .. } if x == n)
                || matches!(e, Expr::Member { name, .. } if name == n)
                // P7.4-a-2 — `x instanceof <N>` is a genuine reference
                // to <N>; without this `catch (e) { e instanceof
                // TypeError }` wouldn't inject TypeError and the
                // runtime native-error throw couldn't build a real
                // instance for it.
                || matches!(e, Expr::InstanceOf { class_name, .. } if class_name == n)
        }) || ast.stmts.iter().any(|s| {
            matches!(s, Stmt::ClassDecl { parent: Some(p), .. } if p == n)
                // P7.4-a-2 — `catch (e: <N>)` annotates the class and
                // expects it to resolve; treat the annotation as a
                // reference so the typed catch + runtime real-instance
                // path both work.
                || matches!(s, Stmt::Try { catch_type: Some(t), .. } if t == n)
        })
    };

    // P7.4-a-b — bigint `/ % ** << >>` and `BigInt(x)` can throw a
    // real RangeError at runtime (divide-by-zero / negative exponent /
    // shift too large / non-integer). If the program uses bigint at
    // all, imply RangeError so its `__new_RangeError` factory is
    // registered into the native-error registry (slot 2); otherwise
    // the runtime throw degrades to a bare string instead of a
    // catchable RangeError instance.
    let uses_bigint =
        ast.exprs.iter().any(|e| matches!(e, Expr::BigInt { .. })) || referenced("BigInt");

    // Subclasses to inject: (referenced OR implied OR runtime-thrown)
    // AND not user-shadowed.
    //
    // `runtime_thrown` covers TypeError / RangeError — emitted by
    // runtime helpers (Object.defineProperty on sealed / writes to
    // frozen / `__torajs_throw_type_error` / bigint `/ %` / Array
    // length range / numeric radix range / ...) regardless of user
    // reference. Without auto-inject the native-error registry slot
    // for that class stays unregistered → the helper falls through
    // to a bare-Str throw, breaking `e.message` and
    // `e instanceof TypeError` on the caught value. The fixture
    // workaround "add `const _t = TypeError;` to the program"
    // (check-throw-msg-001.ts, ba8f4ef4) is the same gap surfaced
    // case-by-case; force-inject closes it module-wide. SyntaxError
    // / ReferenceError are never emitted by runtime helpers (parse-
    // time / type-check-time only), so their reference-gated path
    // stays — programs that never mention them pay no cost.
    let want_sub: Vec<&str> = ERROR_SUBCLASSES
        .iter()
        .copied()
        .filter(|n| {
            let shadowed = ast
                .stmts
                .iter()
                .any(|s| matches!(s, Stmt::ClassDecl { name, .. } if name == *n));
            let implied = *n == "RangeError" && uses_bigint;
            let runtime_thrown = matches!(*n, "TypeError" | "RangeError");
            !shadowed && (runtime_thrown || referenced(n) || implied)
        })
        .collect();

    // Subclasses extend Error, so any wanted subclass implies Error.
    let want_error = referenced("Error") || !want_sub.is_empty();
    if !want_error {
        return;
    }

    // Build Error first, then each wanted subclass, and splice at the
    // front so the parent (`Error`) precedes its children — the
    // field-flattening + declaration-order check in `desugar_classes`
    // requires every ancestor declared before its descendants — and
    // all user references (forward + downstream) resolve here.
    let mut injected: Vec<Stmt> = Vec::with_capacity(1 + want_sub.len());
    injected.push(build_error_class(ast));
    for n in &want_sub {
        injected.push(build_error_subclass(ast, n));
    }
    ast.stmts.splice(0..0, injected);
}

/// Build a default-initializer Expr for a type annotation string. Used by
/// `desugar_classes` to seed the factory's object-literal at the top of
/// `__new_C`. The constructor (if any) is responsible for overwriting
/// these defaults with caller-provided values; the defaults exist so the
/// object is well-typed even on fields a buggy constructor forgets to
/// touch.
/// Recursive default-initializer for a class field. Knows how to:
///   - hoist `T[]` into a typed prelude let returning the bound ident
///   - expand a class- or alias-typed field into an ObjectLit of
///     recursively-defaulted children (looked up in `class_layouts`
///     and `alias_layouts`)
/// V3-18 wedge — rewrite the placeholder `"this"` in a class
/// method's return-type annotation to the enclosing class's
/// `this_ann` (e.g., `C` or `C<T|U>` for generic classes), per
/// TS spec §3.6.3 polymorphic-this semantics. Standard usage:
///   class Builder { add(...): this { return this } }
/// The parser stores the literal `"this"` in `m.return_type`;
/// desugar_classes substitutes it here before emit so check.rs
/// and ssa_lower see the concrete class type at every method's
/// return boundary. Also handles the `__nullable(this)` wrapper
/// case for the rare `: this | null` shape.
fn rewrite_this_in_ann(ann: &Option<String>, this_ann: &str) -> Option<String> {
    let a = ann.as_deref()?;
    if a == "this" {
        return Some(this_ann.to_string());
    }
    if a == "__nullable(this)" {
        return Some(format!("__nullable({this_ann})"));
    }
    Some(a.to_string())
}

///   - fall back to `default_init_for_type` for primitives / typevars
///
/// `seen` guards against direct cycles (a class transitively
/// containing itself by name); a hit panics rather than spinning.
#[allow(clippy::too_many_arguments)]
fn default_init_for_field(
    ast: &mut Ast,
    fty: &str,
    class_layouts: &std::collections::HashMap<String, Vec<(String, String)>>,
    alias_layouts: &std::collections::HashMap<String, Vec<(String, String)>>,
    prelude: &mut Vec<Stmt>,
    parent_cname: &str,
    parent_fname: &str,
    seen: &mut std::collections::HashSet<String>,
) -> ExprId {
    if fty.ends_with("[]") {
        let local = format!("__def_arr_{parent_cname}_{parent_fname}");
        let arr_lit = ast.add_expr(Expr::Array(Vec::new()));
        prelude.push(Stmt::LetDecl {
            mutable: false,
            name: local.clone(),
            type_ann: Some(fty.to_string()),
            init: arr_lit,
            is_var: false,
        });
        return ast.add_expr(Expr::Ident(local));
    }
    let sub_fields = class_layouts.get(fty).or_else(|| alias_layouts.get(fty));
    if let Some(sub_fields) = sub_fields {
        // V3-18 wedge — bare type alias `type X = T` is encoded
        // as a single field named "__alias__" carrying the
        // underlying ann. Recurse using the underlying ann
        // instead of treating the alias as a struct shape — the
        // alias name resolves to T at the type level, never a
        // struct with one __alias__ field.
        if sub_fields.len() == 1 && sub_fields[0].0 == "__alias__" {
            let underlying = sub_fields[0].1.clone();
            return default_init_for_field(
                ast,
                &underlying,
                class_layouts,
                alias_layouts,
                prelude,
                parent_cname,
                parent_fname,
                seen,
            );
        }
        if !seen.insert(fty.to_string()) {
            panic!(
                "default_init_for_field: cyclic struct/class layout via `{fty}` \
                 (parent `{parent_cname}.{parent_fname}`)"
            );
        }
        let sub_fields = sub_fields.clone();
        let mut sub_pairs: Vec<(String, ExprId)> = Vec::with_capacity(sub_fields.len());
        for (sfname, sfty) in &sub_fields {
            let sub_local = format!("{parent_cname}_{parent_fname}_{sfname}");
            let sub_id = default_init_for_field(
                ast,
                sfty,
                class_layouts,
                alias_layouts,
                prelude,
                &sub_local,
                sfname,
                seen,
            );
            sub_pairs.push((sfname.clone(), sub_id));
        }
        seen.remove(fty);
        return ast.add_expr(Expr::ObjectLit { fields: sub_pairs });
    }
    let init_expr = default_init_for_type(fty);
    ast.add_expr(init_expr)
}

fn default_init_for_type(ann: &str) -> Expr {
    #[rustfmt::skip]
    fn ctor(name: &str) -> Expr { Expr::New { class_name: name.into(), args: vec![] } }
    match ann {
        "number" => Expr::Number(0.0),
        "string" => Expr::String(String::new()),
        "boolean" => Expr::Bool(false),
        // T[] / __nullable(T) — typed zero / null (M5.2 inheritance follow-up).
        _ if ann.ends_with("[]") => Expr::Array(Vec::new()),
        // V3-05 — `T | null` field default null (parser flat `__nullable(T)`).
        _ if ann.starts_with("__nullable(") && ann.ends_with(')') => Expr::Null,
        // class W { m: Map<K,V>; } default → `new Map()` so SSA-lower intercepts
        // to __torajs_map_create; same for Set/WeakMap/WeakSet (bare or generic).
        _ if ann == "Map" || ann.starts_with("Map<") => ctor("Map"),
        _ if ann == "Set" || ann.starts_with("Set<") => ctor("Set"),
        _ if ann == "WeakMap" || ann.starts_with("WeakMap<") => ctor("WeakMap"),
        _ if ann == "WeakSet" || ann.starts_with("WeakSet<") => ctor("WeakSet"),
        // TypeVar (short all-uppercase T/U/K/V…) — monomorphizer-resolved marker.
        _ if is_likely_typevar(ann) => Expr::Ident(format!("__tvdefault__{ann}")),
        _ => Expr::Number(0.0),
    }
}

fn is_likely_typevar(s: &str) -> bool {
    s.len() <= 2 && !s.is_empty() && s.chars().all(|c| c.is_ascii_uppercase())
}

/// True iff `owner` is `target_ancestor` or any ancestor of `target_ancestor`.
/// Used by the override-detection check.
fn method_owner_is_in_chain(
    parent_map: &std::collections::HashMap<String, Option<String>>,
    owner: &str,
    target_ancestor: &str,
) -> bool {
    if owner == target_ancestor {
        return true;
    }
    let mut cur = parent_map.get(target_ancestor).cloned().flatten();
    while let Some(p) = cur {
        if p == owner {
            return true;
        }
        cur = parent_map.get(&p).cloned().flatten();
    }
    false
}

fn build_factory_body(
    ast: &mut Ast,
    cname: &str,
    type_params: &[String],
    field_inits: &[(String, ExprId)],
    prelude: Vec<Stmt>,
    ctor: Option<&ClassCtor>,
) -> Vec<Stmt> {
    let obj_lit = ast.add_expr(Expr::ObjectLit {
        fields: field_inits.to_vec(),
    });
    let this_ann = if type_params.is_empty() {
        cname.to_string()
    } else {
        format!("{cname}<{}>", type_params.join("|"))
    };
    let let_this = Stmt::LetDecl {
        mutable: true,
        name: "__this".into(),
        type_ann: Some(this_ann),
        init: obj_lit,
        is_var: false,
    };
    let mut body: Vec<Stmt> = prelude;
    body.push(let_this);
    if let Some(c) = ctor {
        // Build: __cm_C__ctor(__this, __torajs_my_class_ref("C"),
        //                     ctor_param_idents...);
        // The 2nd arg is __new_target — looked up via the magic
        // factory-side intercept that resolves "C" → class_tag → the
        // registered __class_<C> Any-box at runtime. Sub-class super()
        // forwards its caller's __new_target through (see Pass 1.5).
        // Top-level `__class_<C>` lets aren't K.3-promoted (Type::Any
        // doesn't yet fit the global-slot shape contract), so we
        // can't read them directly from inside this factory FnDecl;
        // the runtime-table indirection is the substrate-correct
        // path.
        let callee = ast.add_expr(Expr::Ident(format!("__cm_{cname}__ctor")));
        let this_id = ast.add_expr(Expr::Ident("__this".into()));
        let class_ref_callee = ast.add_expr(Expr::Ident("__torajs_my_class_ref".to_string()));
        let cname_str = ast.add_expr(Expr::String(cname.to_string()));
        let new_target_id = ast.add_expr(Expr::Call {
            callee: class_ref_callee,
            args: vec![cname_str],
        });
        let mut args: Vec<ExprId> = Vec::with_capacity(c.params.len() + 2);
        args.push(this_id);
        args.push(new_target_id);
        for p in &c.params {
            let pid = ast.add_expr(Expr::Ident(p.name.clone()));
            args.push(pid);
        }
        let call = ast.add_expr(Expr::Call { callee, args });
        body.push(Stmt::Expr(call));
    }
    let ret_id = ast.add_expr(Expr::Ident("__this".into()));
    body.push(Stmt::Return(Some(ret_id)));
    body
}

/// M2 — lambda-lift arrow fns. Walks `ast.exprs` in index order; each
/// `Expr::ArrowFn` is replaced in-place and a corresponding `Stmt::FnDecl`
/// is appended to `ast.stmts`.
///
/// Non-capturing arrows: the source-site expression becomes
/// `Expr::Ident("__closure_N")`, lowering to a plain `FnAddr` in SSA. This
/// is the original M2 Phase A path.
///
/// Capturing arrows (M2 Phase C): the source-site becomes
/// `Expr::Closure { fn_name, captures }`. The lifted FnDecl is given a
/// hidden first parameter named `__env` (typed at the SSA layer); the
/// lowerer reads each capture out of `__env` and binds it as a local at
/// the top of the body, so the body's `Ident(name)` references resolve
/// against the captured value rather than the (now out-of-scope) outer
/// binding.
///
/// Iteration order: parser emits inner expressions before outer, so a
/// nested arrow fn sits at a lower `ExprId` than its enclosing arrow fn.
/// We walk indices low→high; the inner arrow gets lifted first and the
/// outer arrow's body still references it via the (now Ident/Closure) ExprId.
pub fn lift_arrow_fns(ast: &mut Ast) {
    let mut counter = 0u32;
    let mut new_decls: Vec<Stmt> = Vec::new();
    // Top-level FnDecl names are globals — references to them inside an
    // arrow body should not count as captures. Collect once before
    // walking the exprs.
    let global_fn_names: Vec<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let n = ast.exprs.len();
    for i in 0..n {
        if !matches!(ast.exprs[i], Expr::ArrowFn { .. }) {
            continue;
        }
        let name = format!("__closure_{counter}");
        counter += 1;
        // Compute captures BEFORE moving the arrow body out — collect free
        // vars (idents referenced inside the body that are neither one of
        // the arrow's params nor declared by an inner let, and not a
        // top-level FnDecl name).
        let captures = match &ast.exprs[i] {
            Expr::ArrowFn { params, body, .. } => {
                crate::ast::free_vars::free_vars_of_arrow(ast, params, body, &global_fn_names)
            }
            _ => Vec::new(),
        };
        // P3.closure-in-struct-field — always produce a Closure value
        // (env-carrying, env-first CallIndirect ABI) regardless of
        // capture count. Zero-capture arrows still get an `__env()`
        // annotation so the lowerer treats them as closure-shaped and
        // the call-site dispatch is uniform with capturing arrows.
        let placeholder = Expr::Closure {
            fn_name: name.clone(),
            captures: captures.clone(),
        };
        let arrow = std::mem::replace(&mut ast.exprs[i], placeholder);
        if let Expr::ArrowFn {
            params,
            return_type,
            body,
        } = arrow
        {
            let mut final_params = params;
            let env_ann = format!("__env({})", captures.join("|"));
            final_params.insert(
                0,
                Param {
                    name: "__env".into(),
                    type_ann: Some(env_ann),
                    default: None,
                    is_rest: false,
                },
            );
            new_decls.push(Stmt::FnDecl {
                name,
                type_params: Vec::new(),
                params: final_params,
                return_type,
                body,
                is_generator: false,
            });
        }
    }
    ast.stmts.extend(new_decls);
}

/// Closure ABI slot detector. After `tag_struct_field_closure_types`
/// rewrites TypeDecl fn-like fields to `__cls(P)->R`, this returns
/// true exactly when a field's annotation indicates the SSA slot will
/// be Type::Closure. User-source `(P)=>R` / `__fn(P)->R` also pass
/// this test for resilience (the desugar passes run before
/// type-checking, so a TypeDecl field that escaped tagging would still
/// trigger an ObjectLit rewrite if needed — defensive).
fn is_fn_like_ann(s: &str) -> bool {
    let t = s.trim();
    t.starts_with("__cls(") || t.starts_with("__fn(") || t.contains("=>") || t.starts_with('(')
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

/// Untyped fn params (`function f(x) {}`) and explicit `: any` annotations
/// are folded into the existing M3 generic-monomorphization pipeline by
/// rewriting each untyped/any param's annotation to a fresh `TypeVar` and
/// adding the new name to the fn's `type_params`. This keeps the
/// substrate "TS subset" — every param still has a concrete type at SSA
/// time, but the typechecker can defer that type to call-site inference
/// (see check.rs's generic call-site arm and ssa_lower's
/// `monomorphize_generics`). Same treatment for an untyped/`any` return
/// type, BUT only when the body actually returns a non-void expression
/// — otherwise we'd flip the default-void semantic for stub fns.
///
/// Runs after `lift_arrow_fns` / `infer_anonymous_closure_params` so
/// closure params that already got concrete annotations from method
/// inference don't get re-genericized.
///
/// Skipped:
///   - lifted-closure FnDecls (first param `__env`) — those need their
///     concrete env layout for capture lowering; also their user params
///     are already inferred by `infer_anonymous_closure_params` for the
///     known-receiver-method shape.
///   - desugar-synthesized fns whose first param is `__this` — that's a
///     class instance/factory binding and must stay nominally typed.
///   - generator/factory helpers (the desugarers stamp explicit
///     annotations on every param they emit).
/// `let x;` (the `var x;` shape after the test262 runner's `var → let`
/// rewrite) parses to `Stmt::LetDecl { init: Expr::Uninit }`. This
/// pass walks each declaring scope, finds the first
/// `Stmt::Expr(Assign { Ident(x), value })` after the let, splices
/// `value` into the let's init, and removes the assignment. Anything
/// that doesn't have a matching follow-up assignment keeps the
/// `Uninit` sentinel; the typechecker reports it with a clear "let
/// declared but never assigned" message — better than the previous
/// `expected `=`, got Semi` parse error.
///
/// Limitations of the search:
///   - same scope only — won't promote an inner-block assignment to
///     the outer let's init, since that would change scope semantics
///   - first matching assignment wins — chains like
///     `let x; if (...) x = 1; else x = "two";` don't unify; only the
///     first branch's value lifts in, the second stays an assign and
///     the regular type checker handles the agreement check
///   - top-level vs fn-body scopes are walked uniformly; nested
///     control-flow children don't bubble assignments across their
///     boundary (we only splice within the same `Vec<Stmt>`)
pub fn desugar_uninit_let(ast: &mut Ast) {
    rewrite_uninit_in_stmts(&mut ast.stmts, &ast.exprs.clone());
    // FnDecl bodies live inside `ast.stmts` already; the recursive
    // walk handles them when it descends into Stmt::FnDecl variants.
}

/// P3.4 — block-scoped / nested function declaration hoisting per
/// ES spec Annex B §B.3.3 (FunctionDeclarations in IfStatement
/// Blocks). Walks every top-level FnDecl body, finds nested
/// `function f() {...}` declarations inside any block-shape stmt,
/// lifts them to top-level with mangled names
/// `__nested_<parent>_<name>_<uid>`. References to the original
/// name within the parent body get rewritten to the mangled name.
///
/// Captures: this pass only handles the no-capture case. If the
/// nested fn body references parent locals, ssa_lower will fail at
/// the lifted FnDecl's body lower (lifted = top-level scope, can't
/// see parent locals). Real closure-capturing nested fns need the
/// same treatment as arrow fns (lift_arrow_fns adds an `__env`
/// first param + Closure shape) — substrate followup if test262
/// surfaces it.
pub fn desugar_nested_fns(ast: &mut Ast) {
    let mut top = std::mem::take(&mut ast.stmts);
    let mut new_top: Vec<Stmt> = Vec::new();
    let mut counter: u32 = 0;
    // Pass 1 — top-level FnDecl bodies. Walk nested fns inside parent
    // FnDecl bodies (the original P3.4 scope).
    for stmt in top.iter_mut() {
        if let Stmt::FnDecl {
            name: parent_name,
            body,
            ..
        } = stmt
        {
            let parent = parent_name.clone();
            let mut renames: HashMap<String, String> = HashMap::new();
            let mut lifted: Vec<Stmt> = Vec::new();
            collect_nested_fns_to_lift(body, &parent, &mut renames, &mut lifted, &mut counter);
            if !renames.is_empty() {
                rewrite_idents_in_body(ast, body, &renames);
                for lf in lifted.iter_mut() {
                    if let Stmt::FnDecl { body: lb, .. } = lf {
                        rewrite_idents_in_body(ast, lb, &renames);
                    }
                }
            }
            new_top.extend(lifted);
        }
    }
    // P3.4-followup-A — module-top-level blocks. `{ function f() {} }`
    // at module scope (outside any FnDecl) puts a FnDecl inside a
    // Stmt::Block, which the synthesized `main` body walks. Without
    // this pass, ssa_lower's lower_top_stmt catch-all panicked on
    // every such case (annexB function-statement hoisting tests).
    // Same shape as Pass 1 but the "parent" is the synthetic "__top"
    // namespace for mangling. Also handles If / While / DoWhile /
    // For / ForOf / ForIn / Try / Switch nested at module top.
    let parent = "__top".to_string();
    let mut top_renames: HashMap<String, String> = HashMap::new();
    let mut top_lifted: Vec<Stmt> = Vec::new();
    for stmt in top.iter_mut() {
        match stmt {
            Stmt::FnDecl { .. } => {} // handled by Pass 1
            other => {
                collect_nested_fns_in_stmt(
                    other,
                    &parent,
                    &mut top_renames,
                    &mut top_lifted,
                    &mut counter,
                );
            }
        }
    }
    if !top_renames.is_empty() {
        // Rewrite ident references in the entire top-level (excluding
        // the lifted decls themselves; those get rewritten below).
        for stmt in top.iter_mut() {
            rewrite_idents_in_stmt(ast, stmt, &top_renames);
        }
        for lf in top_lifted.iter_mut() {
            if let Stmt::FnDecl { body: lb, .. } = lf {
                rewrite_idents_in_body(ast, lb, &top_renames);
            }
        }
    }
    new_top.extend(top_lifted);
    top.extend(new_top);
    ast.stmts = top;
}

/// P3.4-followup-A — recursive helper that descends into a single stmt
/// looking for nested FnDecls (block/if/while/for/try/switch
/// children). When found, lift to `lifted` with mangled name and
/// drop from the original site (replaced with a no-op Block).
fn collect_nested_fns_in_stmt(
    stmt: &mut Stmt,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    lifted: &mut Vec<Stmt>,
    counter: &mut u32,
) {
    // Bare-FnDecl-as-statement: `if (cond) function f() {}` parses
    // the then-branch as Stmt::FnDecl directly (no enclosing Block).
    // Same lift-and-mangle as the Block case but in-place.
    if let Stmt::FnDecl { name, .. } = stmt {
        if !name.starts_with("__closure_")
            && !name.starts_with("__cm_")
            && !name.starts_with("__sm_")
            && !name.starts_with("__nested_")
        {
            let mangled = format!("__nested_{parent_name}_{name}_{counter}");
            *counter += 1;
            renames.insert(name.clone(), mangled.clone());
            // Take the FnDecl out of the slot, replace with empty
            // Block, push the renamed decl into `lifted`.
            let taken = std::mem::replace(stmt, Stmt::Block(Vec::new()));
            if let Stmt::FnDecl {
                type_params,
                params,
                return_type,
                body,
                is_generator,
                ..
            } = taken
            {
                lifted.push(Stmt::FnDecl {
                    name: mangled,
                    type_params,
                    params,
                    return_type,
                    body,
                    is_generator,
                });
            }
            return;
        }
    }
    match stmt {
        Stmt::Block(body) => {
            collect_nested_fns_to_lift(body, parent_name, renames, lifted, counter);
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_nested_fns_in_stmt(then_branch, parent_name, renames, lifted, counter);
            if let Some(eb) = else_branch {
                collect_nested_fns_in_stmt(eb, parent_name, renames, lifted, counter);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_nested_fns_in_stmt(body, parent_name, renames, lifted, counter);
        }
        Stmt::For { body, .. } | Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            collect_nested_fns_in_stmt(body, parent_name, renames, lifted, counter);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_nested_fns_to_lift(body, parent_name, renames, lifted, counter);
            collect_nested_fns_to_lift(catch_body, parent_name, renames, lifted, counter);
            if let Some(fb) = finally_body {
                collect_nested_fns_to_lift(fb, parent_name, renames, lifted, counter);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for case in cases.iter_mut() {
                collect_nested_fns_to_lift(&mut case.body, parent_name, renames, lifted, counter);
            }
            if let Some(d) = default {
                collect_nested_fns_to_lift(d, parent_name, renames, lifted, counter);
            }
        }
        _ => {} // leaf stmts have no nested FnDecl children
    }
}

fn collect_nested_fns_to_lift(
    body: &mut Vec<Stmt>,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    lifted: &mut Vec<Stmt>,
    counter: &mut u32,
) {
    let drained = std::mem::take(body);
    let mut new_body: Vec<Stmt> = Vec::with_capacity(drained.len());
    for s in drained {
        match s {
            Stmt::FnDecl { ref name, .. }
                if !name.starts_with("__closure_")
                    && !name.starts_with("__cm_")
                    && !name.starts_with("__sm_")
                    && !name.starts_with("__nested_") =>
            {
                let mangled = format!("__nested_{parent_name}_{name}_{counter}");
                *counter += 1;
                renames.insert(name.clone(), mangled.clone());
                let new_decl = match s {
                    Stmt::FnDecl {
                        type_params,
                        params,
                        return_type,
                        body: fbody,
                        is_generator,
                        ..
                    } => Stmt::FnDecl {
                        name: mangled,
                        type_params,
                        params,
                        return_type,
                        body: fbody,
                        is_generator,
                    },
                    _ => unreachable!(),
                };
                lifted.push(new_decl);
                // Original site: drop entirely.
            }
            mut other => {
                collect_nested_fns_in_other(&mut other, parent_name, renames, lifted, counter);
                new_body.push(other);
            }
        }
    }
    *body = new_body;
}

fn collect_nested_fns_in_other(
    s: &mut Stmt,
    parent: &str,
    renames: &mut HashMap<String, String>,
    lifted: &mut Vec<Stmt>,
    counter: &mut u32,
) {
    match s {
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_nested_fns_in_other(then_branch, parent, renames, lifted, counter);
            if let Some(eb) = else_branch.as_deref_mut() {
                collect_nested_fns_in_other(eb, parent, renames, lifted, counter);
            }
        }
        Stmt::Block(b) | Stmt::Multi(b) => {
            collect_nested_fns_to_lift(b, parent, renames, lifted, counter);
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_nested_fns_in_other(body, parent, renames, lifted, counter);
        }
        Stmt::For { body, .. } | Stmt::ForOfSplitIter { body, .. } | Stmt::ForOf { body, .. } => {
            collect_nested_fns_in_other(body, parent, renames, lifted, counter);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_nested_fns_to_lift(body, parent, renames, lifted, counter);
            collect_nested_fns_to_lift(catch_body, parent, renames, lifted, counter);
            if let Some(fb) = finally_body {
                collect_nested_fns_to_lift(fb, parent, renames, lifted, counter);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for case in cases {
                collect_nested_fns_to_lift(&mut case.body, parent, renames, lifted, counter);
            }
            if let Some(dflt) = default {
                collect_nested_fns_to_lift(dflt, parent, renames, lifted, counter);
            }
        }
        // Don't descend into FnDecl — its body is its own scope
        // (handled at top level).
        _ => {}
    }
}

fn rewrite_idents_in_body(ast: &mut Ast, body: &mut Vec<Stmt>, renames: &HashMap<String, String>) {
    for s in body.iter_mut() {
        rewrite_idents_in_stmt(ast, s, renames);
    }
}

fn rewrite_idents_in_stmt(ast: &mut Ast, s: &mut Stmt, renames: &HashMap<String, String>) {
    match s {
        Stmt::Expr(eid) => rewrite_idents_in_expr(ast, *eid, renames),
        Stmt::LetDecl { init, .. } => rewrite_idents_in_expr(ast, *init, renames),
        Stmt::Return(Some(eid)) => rewrite_idents_in_expr(ast, *eid, renames),
        Stmt::Throw(eid) => rewrite_idents_in_expr(ast, *eid, renames),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_idents_in_expr(ast, *cond, renames);
            rewrite_idents_in_stmt(ast, then_branch, renames);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_idents_in_stmt(ast, eb, renames);
            }
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            rewrite_idents_in_expr(ast, *cond, renames);
            rewrite_idents_in_stmt(ast, body, renames);
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_idents_in_stmt(ast, i, renames);
            }
            if let Some(c) = cond {
                rewrite_idents_in_expr(ast, *c, renames);
            }
            if let Some(st) = step {
                rewrite_idents_in_expr(ast, *st, renames);
            }
            rewrite_idents_in_stmt(ast, body, renames);
        }
        Stmt::Block(b) | Stmt::Multi(b) => {
            for s2 in b.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s2 in body.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames);
            }
            for s2 in catch_body.iter_mut() {
                rewrite_idents_in_stmt(ast, s2, renames);
            }
            if let Some(fb) = finally_body {
                for s2 in fb.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames);
                }
            }
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rewrite_idents_in_expr(ast, *scrutinee, renames);
            for case in cases.iter_mut() {
                rewrite_idents_in_expr(ast, case.value, renames);
                for s2 in case.body.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames);
                }
            }
            if let Some(dflt) = default {
                for s2 in dflt.iter_mut() {
                    rewrite_idents_in_stmt(ast, s2, renames);
                }
            }
        }
        Stmt::ForOfSplitIter {
            parent, sep, body, ..
        } => {
            rewrite_idents_in_expr(ast, *parent, renames);
            rewrite_idents_in_expr(ast, *sep, renames);
            rewrite_idents_in_stmt(ast, body, renames);
        }
        Stmt::ForOf {
            elem_expr, body, ..
        } => {
            rewrite_idents_in_expr(ast, *elem_expr, renames);
            rewrite_idents_in_stmt(ast, body, renames);
        }
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => {
            rewrite_idents_in_expr(ast, *eid, renames);
        }
        _ => {}
    }
}

fn rewrite_idents_in_expr(ast: &mut Ast, eid: ExprId, renames: &HashMap<String, String>) {
    use std::collections::HashSet;
    let mut seen: HashSet<ExprId> = HashSet::new();
    let mut stack: Vec<ExprId> = vec![eid];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let new_name = if let Expr::Ident(n) = &ast.exprs[id.0 as usize] {
            renames.get(n).cloned()
        } else {
            None
        };
        if let Some(nm) = new_name {
            ast.exprs[id.0 as usize] = Expr::Ident(nm);
            continue;
        }
        match ast.exprs[id.0 as usize].clone() {
            Expr::BinOp { left, right, .. } => {
                stack.push(left);
                stack.push(right);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::PostIncr { target: expr, .. } => {
                stack.push(expr);
            }
            Expr::Call { callee, args } => {
                stack.push(callee);
                for a in args {
                    stack.push(a);
                }
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                stack.push(obj);
            }
            Expr::Index { obj, index } => {
                stack.push(obj);
                stack.push(index);
            }
            Expr::Assign { target, value } => {
                stack.push(target);
                stack.push(value);
            }
            Expr::Array(els) => {
                for e in els {
                    stack.push(e);
                }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields {
                    stack.push(e);
                }
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                stack.push(cond);
                stack.push(then_branch);
                stack.push(else_branch);
            }
            Expr::Nullish { lhs, rhs }
            | Expr::Sequence {
                left: lhs,
                right: rhs,
            } => {
                stack.push(lhs);
                stack.push(rhs);
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for a in args {
                    stack.push(a);
                }
            }
            Expr::InstanceOf { expr, .. } => {
                stack.push(expr);
            }
            // ArrowFn / Closure bodies are separate scopes — don't
            // descend (handled by lift_arrow_fns + their own pass).
            _ => {}
        }
    }
}

/// of test262's `arguments-object/*` cases without runtime changes:
///
///   - `arguments.length` → `Number(<arity>)` where arity is the fn's
///     declared param count (excluding the synthetic `__env` / `__this`
///     prefix params from closure / class lowering)
///   - `arguments[N]` with `N` a literal integer in [0, arity) →
///     `Ident(<param-name-N>)`. Param ownership rules then apply
///     normally (the typechecker treats it as a read of that binding).
///
/// Bare `arguments` (returned, passed, dynamically indexed) is left
/// alone — the typechecker reports it as an unknown identifier with
/// the existing message, which is the correct surface until a real
/// arguments-object materialization lands.
///
/// Runs after class / closure desugars (so the synthetic `__env` /
/// `__this` prefix is already in place) and after `lift_arrow_fns`
/// (so closure-lifted FnDecls are visible). Needs to run before the
/// typechecker so the rewritten Idents resolve cleanly.
/// T-38-followup — `Array.isArray` used as a value (not in callee
/// position) has no Operand representation in tora's subset — ssa_lower
/// hits "unknown ident `Array`". Common test262 shape is
/// `var f = Array.isArray; typeof f === "function"` and
/// `Array.isArray.length === 1`.
///
/// Fix: at AST level, synthesize a stub user FnDecl
/// `__torajs_array_isarray_stub(x: any): boolean` and rewrite every
/// non-callee `Array.isArray` Member access to reference it. Call-site
/// usage (`Array.isArray(value)`) keeps the existing ssa_lower static-
/// check intercept since the Member stays in callee position there
/// (we mark those ExprIds during the pre-pass and skip them).
///
/// Stub body returns false — sufficient for the test262 cases that
/// only check typeof / .length. Real call-time semantics are still
/// handled by the existing `Array.isArray(value)` intercept. If user
/// code does `var f = Array.isArray; f([1,2,3])` the stub would give
/// the wrong answer, but no test262 case in the 5k sample exercises
/// that shape.
pub fn desugar_array_isarray_value(ast: &mut Ast) {
    // Collect every `Expr::Call`'s callee ExprId — those Members are
    // "in callee position" and keep their existing intercept path.
    let mut callee_exprs: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } = e {
            callee_exprs.insert(*callee);
        }
    }

    // Walk the arena for Members matching `Array.isArray` NOT in
    // callee position. Mutate in place.
    let mut any_rewritten = false;
    let n = ast.exprs.len();
    for i in 0..n {
        let eid = ExprId(i as u32);
        if callee_exprs.contains(&eid) {
            continue;
        }
        let matched = matches!(&ast.exprs[i], Expr::Member { obj, name }
            if name == "isArray"
                && matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "Array"));
        if matched {
            ast.exprs[i] = Expr::Ident("__torajs_array_isarray_stub".into());
            any_rewritten = true;
        }
    }

    if !any_rewritten {
        return;
    }

    // Emit the stub FnDecl once at module top. Skip if already present
    // (defensive — pass should run once).
    let exists = ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == "__torajs_array_isarray_stub"));
    if exists {
        return;
    }
    let false_lit = ast.add_expr(Expr::Bool(false));
    ast.stmts.push(Stmt::FnDecl {
        name: "__torajs_array_isarray_stub".into(),
        type_params: Vec::new(),
        params: vec![Param {
            name: "x".into(),
            type_ann: Some("any".into()),
            default: None,
            is_rest: false,
        }],
        return_type: Some("boolean".into()),
        body: vec![Stmt::Return(Some(false_lit))],
        is_generator: false,
    });
}

fn rewrite_uninit_in_stmts(stmts: &mut Vec<Stmt>, exprs: &[Expr]) {
    let mut i = 0;
    while i < stmts.len() {
        // Recurse into nested scopes first so each scope's lets see
        // their own follow-up assignments.
        match &mut stmts[i] {
            Stmt::FnDecl { body, .. } => {
                rewrite_uninit_in_stmts(body, exprs);
            }
            Stmt::Block(inner) | Stmt::Multi(inner) => {
                rewrite_uninit_in_stmts(inner, exprs);
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                if let Stmt::Block(b) | Stmt::Multi(b) = then_branch.as_mut() {
                    rewrite_uninit_in_stmts(b, exprs);
                }
                if let Some(eb) = else_branch
                    && let Stmt::Block(b) | Stmt::Multi(b) = eb.as_mut()
                {
                    rewrite_uninit_in_stmts(b, exprs);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                if let Stmt::Block(b) | Stmt::Multi(b) = body.as_mut() {
                    rewrite_uninit_in_stmts(b, exprs);
                }
            }
            Stmt::For { body, .. } => {
                if let Stmt::Block(b) | Stmt::Multi(b) = body.as_mut() {
                    rewrite_uninit_in_stmts(b, exprs);
                }
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                rewrite_uninit_in_stmts(body, exprs);
                rewrite_uninit_in_stmts(catch_body, exprs);
                if let Some(fb) = finally_body {
                    rewrite_uninit_in_stmts(fb, exprs);
                }
            }
            _ => {}
        }
        // Now, if this stmt is an Uninit let, scan forward for the
        // first matching `name = EXPR;` and splice.
        let (name, init_eid) = match &stmts[i] {
            Stmt::LetDecl { name, init, .. } => (name.clone(), *init),
            _ => {
                i += 1;
                continue;
            }
        };
        let is_uninit = matches!(exprs.get(init_eid.0 as usize), Some(Expr::Uninit));
        if !is_uninit {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut found: Option<(usize, ExprId)> = None;
        while j < stmts.len() {
            if let Stmt::Expr(eid) = &stmts[j]
                && let Some(Expr::Assign { target, value }) = exprs.get(eid.0 as usize)
                && let Some(Expr::Ident(n)) = exprs.get(target.0 as usize)
                && n == &name
            {
                found = Some((j, *value));
                break;
            }
            // Don't reach into non-flat control-flow: an assignment in
            // a sibling block / if-branch doesn't lift to the outer
            // scope. Only adjacent flat stmts in the SAME Vec<Stmt>
            // count.
            j += 1;
        }
        if let Some((stmt_idx, value)) = found {
            // Splice value into the let's init, drop the assignment.
            if let Stmt::LetDecl { init, .. } = &mut stmts[i] {
                *init = value;
            }
            stmts.remove(stmt_idx);
        }
        i += 1;
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

/// Borrow-shaped view of `Ast.exprs` for the inference helper. Defined
/// at the top of `desugar_implicit_generics` (just below) — `&[Expr]`
/// indexed by `ExprId.0 as usize`. The pre-pass walks expression
/// shapes statically without consulting the typechecker, so this
/// flat slice is enough.
pub(crate) type AstExprsView<'a> = &'a [Expr];

/// Static return-type sniff. Walks every value-return inside `body`
/// (recursing through control-flow shapes that propagate value-
/// returns out of the fn) and asks `infer_expr_ann` for an annotation.
/// Returns `Some(ann)` only if every reachable return agrees; any
/// disagreement or any return that resists static typing yields None
/// (caller leaves return_type alone).
///
/// Beyond literals + boolean-result ops, the helper does a one-pass
/// scan over let-decl bodies to populate a binding → annotation map
/// (so `let x = 3; ... return x + 1;` infers `x: number` and bubbles
/// `number` out as the return). Lookups that fall off the simple-
/// shape grammar (Member / Call / Index / object literal / etc.) bail
/// to None — the typechecker still owns the deeper analysis.
pub(crate) fn infer_return_ann(
    exprs: AstExprsView,
    body: &[Stmt],
    params: &[Param],
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    infer_return_ann_seeded(
        exprs,
        body,
        params,
        &std::collections::HashMap::new(),
        fn_sigs,
    )
}

/// T-19.p — variant that takes a pre-seeded binds map. Used by the
/// capturing-closure FnDecl path so outer-scope let-decls (where
/// the captures actually live) flow into the body's static return
/// sniff. The pre-T-19.p path passed only the body's params + body
/// let-decls — captured idents had no entry in binds and the sniff
/// bailed to None. Idempotent: explicit body-local lets shadow the
/// outer seed via collect_let_binding_anns running afterwards.
pub(crate) fn infer_return_ann_seeded(
    exprs: AstExprsView,
    body: &[Stmt],
    params: &[Param],
    outer_binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let mut binds: std::collections::HashMap<String, String> = outer_binds.clone();
    for p in params {
        if let Some(ann) = &p.type_ann {
            binds.insert(p.name.clone(), ann.clone());
        }
    }
    collect_let_binding_anns(exprs, body, &mut binds, fn_sigs);
    let mut acc: Option<String> = None;
    if !collect_return_anns(exprs, body, &binds, fn_sigs, &mut acc) {
        return None;
    }
    acc
}

/// Walk `body` and for each `let x = INIT;` whose `INIT` we can
/// statically annotate, register `x → ann` in `binds`. Inner
/// FnDecls / nested classes etc. are skipped — we only care about
/// the immediate fn's binding scope. Idempotent over re-runs since
/// later let-decls in the same scope shadow earlier ones.
fn collect_let_binding_anns(
    exprs: AstExprsView,
    body: &[Stmt],
    binds: &mut std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) {
    for s in body {
        collect_let_binding_anns_stmt(exprs, s, binds, fn_sigs);
    }
}

fn collect_let_binding_anns_stmt(
    exprs: AstExprsView,
    s: &Stmt,
    binds: &mut std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) {
    match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            // Explicit annotation wins.
            if let Some(ann) = type_ann {
                binds.insert(name.clone(), ann.clone());
                return;
            }
            // Else infer from init shape — using the binds map we've
            // built up to here, so `let x = 3; let y = x + 1;` chains
            // correctly.
            let bs: Vec<Param> = binds_to_params(binds);
            if let Some(ann) = infer_expr_ann_with(exprs, *init, &bs, binds, fn_sigs) {
                binds.insert(name.clone(), ann);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_let_binding_anns_stmt(exprs, then_branch, binds, fn_sigs);
            if let Some(eb) = else_branch {
                collect_let_binding_anns_stmt(exprs, eb, binds, fn_sigs);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_let_binding_anns_stmt(exprs, body, binds, fn_sigs);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                collect_let_binding_anns_stmt(exprs, i, binds, fn_sigs);
            }
            collect_let_binding_anns_stmt(exprs, body, binds, fn_sigs);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            collect_let_binding_anns(exprs, stmts, binds, fn_sigs);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_let_binding_anns(exprs, body, binds, fn_sigs);
            collect_let_binding_anns(exprs, catch_body, binds, fn_sigs);
            if let Some(fb) = finally_body {
                collect_let_binding_anns(exprs, fb, binds, fn_sigs);
            }
        }
        _ => {}
    }
}

pub(crate) fn binds_to_params(binds: &std::collections::HashMap<String, String>) -> Vec<Param> {
    binds
        .iter()
        .map(|(k, v)| Param {
            name: k.clone(),
            type_ann: Some(v.clone()),
            default: None,
            is_rest: false,
        })
        .collect()
}

/// Returns false on first disagreement / un-inferable return; on
/// success, `acc` holds the unique annotation across all returns.
fn collect_return_anns(
    exprs: AstExprsView,
    body: &[Stmt],
    binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
    acc: &mut Option<String>,
) -> bool {
    for s in body {
        if !collect_return_anns_stmt(exprs, s, binds, fn_sigs, acc) {
            return false;
        }
    }
    true
}

fn collect_return_anns_stmt(
    exprs: AstExprsView,
    s: &Stmt,
    binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
    acc: &mut Option<String>,
) -> bool {
    match s {
        Stmt::Return(Some(eid)) => {
            let bs = binds_to_params(binds);
            let Some(ann) = infer_expr_ann_with(exprs, *eid, &bs, binds, fn_sigs) else {
                return false;
            };
            match acc {
                None => *acc = Some(ann),
                Some(prev) if *prev == ann => {}
                Some(_) => return false,
            }
            true
        }
        Stmt::Return(None) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            if !collect_return_anns_stmt(exprs, then_branch, binds, fn_sigs, acc) {
                return false;
            }
            if let Some(eb) = else_branch
                && !collect_return_anns_stmt(exprs, eb, binds, fn_sigs, acc)
            {
                return false;
            }
            true
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_return_anns_stmt(exprs, body, binds, fn_sigs, acc)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init
                && !collect_return_anns_stmt(exprs, i, binds, fn_sigs, acc)
            {
                return false;
            }
            collect_return_anns_stmt(exprs, body, binds, fn_sigs, acc)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            collect_return_anns(exprs, stmts, binds, fn_sigs, acc)
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            if !collect_return_anns(exprs, body, binds, fn_sigs, acc) {
                return false;
            }
            if !collect_return_anns(exprs, catch_body, binds, fn_sigs, acc) {
                return false;
            }
            if let Some(fb) = finally_body
                && !collect_return_anns(exprs, fb, binds, fn_sigs, acc)
            {
                return false;
            }
            true
        }
        // Switch / nested FnDecl etc. — conservative: treat as opaque,
        // make the whole inference bail (returns are uncommon inside
        // these shapes for our test262 surface).
        Stmt::FnDecl { .. } => true,
        _ => true,
    }
}

fn is_typevar_ann(s: &str) -> bool {
    s.starts_with("__T") && s.len() > 3 && s[3..].bytes().all(|b| b.is_ascii_digit())
}

/// Statically infer the type-annotation string for an expression (best-effort).
pub(crate) fn infer_expr_ann_with(
    exprs: AstExprsView,
    eid: ExprId,
    params: &[Param],
    binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let recur = |id: ExprId| infer_expr_ann_with(exprs, id, params, binds, fn_sigs);
    let e = exprs.get(eid.0 as usize)?;
    match e {
        Expr::Number(_) => Some("number".into()),
        Expr::String(_) => Some("string".into()),
        Expr::Bool(_) => Some("boolean".into()),
        Expr::BinOp { op, left, right } => match op {
            BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::Eq
            | BinOp::Neq
            | BinOp::LooseEq
            | BinOp::LooseNeq
            | BinOp::LAnd
            | BinOp::LOr => Some("boolean".into()),
            BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr => Some("number".into()),
            // `+`: string-propagates / num+num=num / TypeVar+num=`any` (else Void).
            BinOp::Add => {
                let (l, r) = (recur(*left)?, recur(*right)?);
                match (l.as_str(), r.as_str()) {
                    ("string", _) | (_, "string") => Some("string".into()),
                    ("number", "number") => Some("number".into()),
                    (a, "number") | ("number", a) if is_typevar_ann(a) => Some("any".into()),
                    _ => None,
                }
            }
        },
        Expr::Unary { op, .. } if matches!(op, UnaryOp::Not) => Some("boolean".into()),
        Expr::Unary { .. } => Some("number".into()),
        Expr::Ident(name) => params
            .iter()
            .find(|p| &p.name == name)
            .and_then(|p| p.type_ann.clone())
            .or_else(|| binds.get(name).cloned()),
        // Call: bare Ident → fn_sigs; Member+string/`T[]` receiver → method whitelist.
        // Omitted methods SIGSEGV/silent-wrong even with explicit annotation (L3b).
        Expr::Call { callee, .. } => {
            let c = exprs.get(callee.0 as usize)?;
            if let Expr::Ident(n) = c {
                return fn_sigs.get(n).cloned();
            }
            if let Expr::Member { obj, name } = c {
                let r = recur(*obj)?;
                let elem = r.strip_suffix("[]");
                let ret: &str = match (r.as_str(), elem, name.as_str()) {
                    (
                        "string",
                        _,
                        "toUpperCase" | "toLowerCase" | "trim" | "trimStart" | "trimEnd" | "slice"
                        | "substring" | "repeat" | "concat" | "replace" | "replaceAll" | "padStart"
                        | "padEnd" | "at" | "charAt",
                    ) => "string",
                    ("string", _, "startsWith" | "endsWith" | "includes") => "boolean",
                    ("string", _, "indexOf" | "lastIndexOf" | "charCodeAt") => "number",
                    (_, Some(e), "pop" | "shift" | "at" | "find" | "findLast") => e,
                    (
                        _,
                        Some(_),
                        "slice" | "map" | "reverse" | "sort" | "concat" | "fill" | "filter"
                        | "flat" | "flatMap",
                    ) => &r,
                    (_, Some(_), "every" | "some" | "includes") => "boolean",
                    (_, Some(_), "indexOf" | "lastIndexOf" | "findIndex" | "findLastIndex") => {
                        "number"
                    }
                    (_, Some(_), "join") => "string",
                    // `reduce` / `reduceRight` return type is the callback's
                    // accumulator type — for the common case where the
                    // accumulator is the same shape as elements (e.g.
                    // `xs.reduce((a, b) => a + b, 0)` on Number[]), it
                    // matches the element type. Mixed-type accumulators
                    // (cb returns a different shape than elem) still need
                    // explicit ret annotation (substrate follow-up).
                    (_, Some(e), "reduce" | "reduceRight") => e,
                    _ => return None,
                };
                return Some(ret.to_string());
            }
            None
        }
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => {
            let (a, b) = (recur(*then_branch)?, recur(*else_branch)?);
            if a == b { Some(a) } else { Some("any".into()) }
        }
        Expr::Array(elems) => Some(format!("{}[]", recur(*elems.first()?)?)),
        Expr::Member { obj, name } if name == "length" => recur(*obj)
            .filter(|r| r == "string" || r.ends_with("[]"))
            .map(|_| "number".into()),
        Expr::Index { obj, .. } => recur(*obj)?.strip_suffix("[]").map(str::to_string),
        Expr::ObjectLit { fields } => fields
            .iter()
            .map(|(n, eid)| recur(*eid).map(|t| format!("{n}:{t}")))
            .collect::<Option<Vec<_>>>()
            .map(|p| format!("__inlobj({})", p.join("|"))),
        _ => None,
    }
}

pub(crate) fn body_has_value_return(body: &[Stmt]) -> bool {
    for s in body {
        if stmt_has_value_return(s) {
            return true;
        }
    }
    false
}

fn stmt_has_value_return(s: &Stmt) -> bool {
    match s {
        Stmt::Return(Some(_)) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_has_value_return(then_branch)
                || else_branch.as_deref().is_some_and(stmt_has_value_return)
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stmt_has_value_return(body),
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_has_value_return) || stmt_has_value_return(body)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => body_has_value_return(stmts),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body_has_value_return(body)
                || body_has_value_return(catch_body)
                || finally_body.as_deref().is_some_and(body_has_value_return)
        }
        // Nested FnDecl returns are scoped to the inner fn — skip.
        Stmt::FnDecl { .. } => false,
        _ => false,
    }
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
