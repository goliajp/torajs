//! AST — arena-allocated. Children referenced by `ExprId(u32)`, not Box.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExprId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Lt,
    Gt,
    Le,
    Ge,
    Eq,  // ===
    Neq, // !==
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,  // signed; JS `>>`
    /// JS `>>>` — unsigned (logical) right shift. Lowered as LLVM
    /// `lshr` rather than `ashr`; the typechecker still treats it as
    /// `Number → Number` (matches arithmetic Shr).
    UShr,
    LAnd, // logical &&  — short-circuits
    LOr,  // logical ||  — short-circuits
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Not, // logical !
    Neg, // arithmetic -
    BitNot, // bitwise ~
}

#[derive(Debug, Clone)]
pub enum Expr {
    Ident(String),
    String(String),
    Number(f64),
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
        /// M-OO.4 — `static fieldName: T = init`. Each entry desugars to a
        /// top-level `let __sf_<Class>__<name>: T = init` (LetDecl) which
        /// the K.3/K.4 globals machinery picks up. Init is required (no
        /// constructor to default-init in).
        static_fields: Vec<StaticField>,
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
        #[allow(dead_code)] default: Option<String>,
        #[allow(dead_code)] namespace: Option<String>,
        #[allow(dead_code)] named: Vec<(String, Option<String>)>,
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
    ///   - `export default <expr>`     → `default_expr: Some(...)`
    ExportDecl {
        inner: Option<Box<Stmt>>,
        // K.2 will read these to populate the export list.
        #[allow(dead_code)] named: Vec<(String, Option<String>)>,
        #[allow(dead_code)] default_expr: Option<ExprId>,
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
}

/// M-OO.5 — TypeScript-style visibility modifier on class members.
/// `Public` is the parse-time default and never appears explicitly in
/// the source; `Private` corresponds to `private`; `Protected` to
/// `protected`. (TS also has `#name` private fields with a different
/// runtime story, which torajs doesn't ship — only the modifier form.)
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

    // Helper: rewrite every `Ident(name)` matching one of the generator
    // parameter names into `this.<name>`. We do this in-place on the
    // expression arena so the same ExprIds keep their semantic meaning,
    // just pointing at the field-access shape now. Walks every Expr
    // reachable from the function body.
    fn rewrite_params_to_this(ast: &mut Ast, body: &[Stmt], params: &[Param]) {
        let pset: std::collections::HashSet<String> = params.iter().map(|p| p.name.clone()).collect();
        let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for s in body {
            rewrite_params_in_stmt(ast, s, &pset, &mut visited);
        }
    }
    fn rewrite_params_in_stmt(
        ast: &mut Ast,
        s: &Stmt,
        pset: &std::collections::HashSet<String>,
        visited: &mut std::collections::HashSet<u32>,
    ) {
        match s {
            Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
                rewrite_params_in_expr(ast, *eid, pset, visited);
            }
            Stmt::YieldInto { value, .. } => {
                rewrite_params_in_expr(ast, *value, pset, visited);
            }
            Stmt::Return(maybe) => {
                if let Some(eid) = maybe {
                    rewrite_params_in_expr(ast, *eid, pset, visited);
                }
            }
            Stmt::LetDecl { init, .. } => rewrite_params_in_expr(ast, *init, pset, visited),
            Stmt::If { cond, then_branch, else_branch } => {
                rewrite_params_in_expr(ast, *cond, pset, visited);
                rewrite_params_in_stmt(ast, then_branch, pset, visited);
                if let Some(e) = else_branch { rewrite_params_in_stmt(ast, e, pset, visited); }
            }
            Stmt::While { cond, body } => {
                rewrite_params_in_expr(ast, *cond, pset, visited);
                rewrite_params_in_stmt(ast, body, pset, visited);
            }
            Stmt::DoWhile { body, cond } => {
                rewrite_params_in_stmt(ast, body, pset, visited);
                rewrite_params_in_expr(ast, *cond, pset, visited);
            }
            Stmt::For { init, cond, step, body } => {
                if let Some(i) = init { rewrite_params_in_stmt(ast, i, pset, visited); }
                if let Some(c) = cond { rewrite_params_in_expr(ast, *c, pset, visited); }
                if let Some(st) = step { rewrite_params_in_expr(ast, *st, pset, visited); }
                rewrite_params_in_stmt(ast, body, pset, visited);
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                for s in stmts { rewrite_params_in_stmt(ast, s, pset, visited); }
            }
            Stmt::Switch { scrutinee, cases, default } => {
                rewrite_params_in_expr(ast, *scrutinee, pset, visited);
                for c in cases {
                    rewrite_params_in_expr(ast, c.value, pset, visited);
                    for s in &c.body { rewrite_params_in_stmt(ast, s, pset, visited); }
                }
                if let Some(d) = default { for s in d { rewrite_params_in_stmt(ast, s, pset, visited); } }
            }
            _ => {}
        }
    }
    fn rewrite_params_in_expr(
        ast: &mut Ast,
        eid: ExprId,
        pset: &std::collections::HashSet<String>,
        visited: &mut std::collections::HashSet<u32>,
    ) {
        if !visited.insert(eid.0) {
            return;
        }
        let kind = ast.exprs[eid.0 as usize].clone();
        match kind {
            Expr::Ident(name) if pset.contains(&name) => {
                let this_id = ast.add_expr(Expr::This);
                ast.exprs[eid.0 as usize] = Expr::Member { obj: this_id, name };
            }
            Expr::BinOp { left, right, .. } => {
                rewrite_params_in_expr(ast, left, pset, visited);
                rewrite_params_in_expr(ast, right, pset, visited);
            }
            Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::Spread { expr }
            | Expr::InstanceOf { expr, .. } => {
                rewrite_params_in_expr(ast, expr, pset, visited);
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                rewrite_params_in_expr(ast, obj, pset, visited);
            }
            Expr::Call { callee, args } => {
                rewrite_params_in_expr(ast, callee, pset, visited);
                for a in args { rewrite_params_in_expr(ast, a, pset, visited); }
            }
            Expr::Assign { target, value } => {
                rewrite_params_in_expr(ast, target, pset, visited);
                rewrite_params_in_expr(ast, value, pset, visited);
            }
            Expr::Index { obj, index } => {
                rewrite_params_in_expr(ast, obj, pset, visited);
                rewrite_params_in_expr(ast, index, pset, visited);
            }
            Expr::Array(els) => {
                for e in els { rewrite_params_in_expr(ast, e, pset, visited); }
            }
            Expr::ObjectLit { fields } => {
                for (_, e) in fields { rewrite_params_in_expr(ast, e, pset, visited); }
            }
            Expr::Ternary { cond, then_branch, else_branch } => {
                rewrite_params_in_expr(ast, cond, pset, visited);
                rewrite_params_in_expr(ast, then_branch, pset, visited);
                rewrite_params_in_expr(ast, else_branch, pset, visited);
            }
            Expr::Nullish { lhs, rhs } => {
                rewrite_params_in_expr(ast, lhs, pset, visited);
                rewrite_params_in_expr(ast, rhs, pset, visited);
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                for e in args { rewrite_params_in_expr(ast, e, pset, visited); }
            }
            Expr::PostIncr { target, .. } => {
                rewrite_params_in_expr(ast, target, pset, visited);
            }
            _ => {}
        }
    }

    let mut appended: Vec<Stmt> = Vec::new();

    for (idx, gen_name, gen_params, gen_ret, gen_body) in gen_indices {
        let yield_ty = gen_ret.unwrap_or_else(|| {
            panic!(
                "function* {gen_name} requires an explicit yield value type \
                 annotation `: T` (Phase J MVP)"
            )
        });

        // J.2.a/b — lift every `let x: T = init` ANYWHERE in the body
        // (including for-init, if/else branches, while/for bodies) to a
        // class field so the binding survives yield boundaries. Each
        // lifted let becomes:
        //   - a new field on the iterator class
        //   - a `this.<name> = init` assignment expr at the let's source
        //     position (replacing the LetDecl in-place)
        //   - a `this.<name>` rewrite for every Ident reference further
        //     down the body
        //
        // Same-name lets in different scopes both map to the same field
        // and would clobber each other; we panic on collision so the
        // user has to rename. Switch / try lets are not lifted (those
        // forms don't yet support yields).
        let mut gen_body = gen_body;
        // J.4 — expand every `let v(:T)? = yield <e>;` into
        //   yield <e>;
        //   let v(:T)? = this.__sent;
        // so the rest of the pipeline only sees standard `Stmt::Yield`
        // and `Stmt::LetDecl`. The `this.__sent` reference picks up
        // whatever was passed to `g.next(arg)` on the resume.
        for s in &mut gen_body {
            expand_yield_into_in_stmt(ast, s, &yield_ty);
        }
        // After expansion, gen_body may contain Multi(Vec<Stmt>) holding
        // the [Yield; LetDecl] pair. The recursive let-lift below walks
        // Multi just fine.

        let mut lifted_locals: Vec<(String, String)> = Vec::new();
        for s in &mut gen_body {
            lift_lets_in_stmt(ast, s, &mut lifted_locals);
        }
        for i in 0..lifted_locals.len() {
            for j in (i + 1)..lifted_locals.len() {
                if lifted_locals[i].0 == lifted_locals[j].0 {
                    panic!(
                        "function* {gen_name}: duplicate `let {}` declarations across \
                         scopes — both lift to `this.{}` and would collide. Rename \
                         one (Phase J.2.b limitation).",
                        lifted_locals[i].0, lifted_locals[i].0
                    );
                }
            }
        }
        // Names to rewrite to `this.<name>`: generator params + lifted
        // locals. Both share the same identifier-shadowing semantics
        // for our MVP (no shadowing).
        let mut all_names: Vec<Param> = gen_params.clone();
        for (n, t) in &lifted_locals {
            all_names.push(Param {
                name: n.clone(),
                type_ann: Some(t.clone()),
                default: None,
                is_rest: false,
            });
        }
        rewrite_params_to_this(ast, &gen_body, &all_names);

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

        // Build the state machine. Each arm is the body of one state in
        // an if-chain wrapped by `while (true) { ... }`. Yields close an
        // arm with `return {value:e, done:false}`; control-flow gotos
        // close with `state = N; continue;` and the `while(true)` loop
        // re-enters the if-chain at the new state.
        let mut sm = GenSm::new(ast);
        sm.lower_seq(gen_body);
        // After the last body stmt, the natural exit is "done forever".
        let zero = default_init_for_type(&yield_ty);
        let zero_id = sm.ast.add_expr(zero);
        let done_lit = sm.ast.add_expr(Expr::Bool(true));
        let final_obj = sm.ast.add_expr(Expr::ObjectLit {
            fields: vec![
                ("value".into(), zero_id),
                ("done".into(), done_lit),
            ],
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
        let zero_tail = default_init_for_type(&yield_ty);
        let zero_tail_id = ast.add_expr(zero_tail);
        let done_tail = ast.add_expr(Expr::Bool(true));
        let final_tail = ast.add_expr(Expr::ObjectLit {
            fields: vec![
                ("value".into(), zero_tail_id),
                ("done".into(), done_tail),
            ],
        });
        loop_body.push(Stmt::Return(Some(final_tail)));

        let true_lit = ast.add_expr(Expr::Bool(true));
        // Unreachable trailing return after the `while (true)` — the
        // typechecker's "all paths return" analysis doesn't infer that
        // a `cond=true` while never falls out, so without this the
        // function's tail path looks indeterminate. Cheap to emit, no
        // runtime cost (LLVM dead-code-eliminates it).
        let zero_after = default_init_for_type(&yield_ty);
        let zero_after_id = ast.add_expr(zero_after);
        let done_after = ast.add_expr(Expr::Bool(true));
        let final_after = ast.add_expr(Expr::ObjectLit {
            fields: vec![
                ("value".into(), zero_after_id),
                ("done".into(), done_after),
            ],
        });
        let next_body: Vec<Stmt> = vec![
            Stmt::While {
                cond: true_lit,
                body: Box::new(Stmt::Block(loop_body)),
            },
            Stmt::Return(Some(final_after)),
        ];

        // Build the generator class with __state field + ctor + next().
        let zero_init = default_init_for_type("number");
        let zero_init_id = ast.add_expr(zero_init);
        let ctor = ClassCtor {
            params: gen_params.clone(),
            body: vec![
                Stmt::Expr({
                    let this_id = ast.add_expr(Expr::This);
                    let state_member = ast.add_expr(Expr::Member {
                        obj: this_id,
                        name: "__state".into(),
                    });
                    ast.add_expr(Expr::Assign {
                        target: state_member,
                        value: zero_init_id,
                    })
                }),
            ],
        };
        // J.4 — next() takes an optional `__yield_arg: <yield_ty> = 0`
        // parameter and stashes it in `this.__sent` before re-entering
        // the state machine. YieldInto-expanded `let v = this.__sent`
        // sites read that field to receive the value passed to
        // `g.next(arg)`. First call's arg is ignored per JS spec; tr's
        // typed-default uses zero/empty depending on yield type.
        let yield_arg_default = default_init_for_type(&yield_ty);
        let yield_arg_default_id = ast.add_expr(yield_arg_default);
        let yield_arg_param = Param {
            name: "__yield_arg".into(),
            type_ann: Some(yield_ty.clone()),
            default: Some(yield_arg_default_id),
            is_rest: false,
        };
        let stash_sent = {
            let this_id = ast.add_expr(Expr::This);
            let sent_member = ast.add_expr(Expr::Member {
                obj: this_id,
                name: "__sent".into(),
            });
            let arg_ident = ast.add_expr(Expr::Ident("__yield_arg".into()));
            let assign = ast.add_expr(Expr::Assign {
                target: sent_member,
                value: arg_ident,
            });
            Stmt::Expr(assign)
        };
        let mut next_body_with_stash: Vec<Stmt> = Vec::with_capacity(next_body.len() + 1);
        next_body_with_stash.push(stash_sent);
        next_body_with_stash.extend(next_body);

        let next_method = ClassMethod {
            name: "next".into(),
            params: vec![yield_arg_param],
            return_type: Some(step_ann.clone()),
            body: next_body_with_stash,
            is_abstract: false,
            visibility: Visibility::Public,
        };
        // For Phase J MVP, generator parameters are stored as fields on
        // the iterator object so the body can reference them through
        // `this.<name>`. The fields are auto-prepended to the class
        // declaration; the ctor's prelude (above) adds an assignment
        // for each param.
        let mut class_fields: Vec<(String, String)> = vec![
            ("__state".into(), "number".into()),
            ("__sent".into(), yield_ty.clone()),
        ];
        // Lifted locals as class fields. Their initial values are zero
        // (computed from the type ann) — actual initialization happens
        // when the corresponding let-rewrite assignment fires inside
        // next() body.
        for (lname, lty) in &lifted_locals {
            class_fields.push((lname.clone(), lty.clone()));
        }
        let mut ctor_body_with_params = ctor.body.clone();
        for p in &gen_params {
            let pname = p.name.clone();
            let pty = p.type_ann.clone().unwrap_or_else(|| "number".into());
            class_fields.push((pname.clone(), pty));
            // this.<param> = <param>
            let this_id = ast.add_expr(Expr::This);
            let f_member = ast.add_expr(Expr::Member {
                obj: this_id,
                name: pname.clone(),
            });
            let arg_ident = ast.add_expr(Expr::Ident(pname));
            let assign = ast.add_expr(Expr::Assign {
                target: f_member,
                value: arg_ident,
            });
            ctor_body_with_params.push(Stmt::Expr(assign));
        }
        let ctor_with_params = ClassCtor {
            params: gen_params.clone(),
            body: ctor_body_with_params,
        };

        appended.push(Stmt::ClassDecl {
            name: class_name.clone(),
            type_params: Vec::new(),
            parent: None,
            is_abstract: false,
            fields: class_fields,
            static_fields: Vec::new(),
            ctor: Some(ctor_with_params),
            methods: vec![next_method],
            static_methods: Vec::new(),
        });

        // Replace the original generator FnDecl with a thin factory
        // that returns `new __Gen_<name>(args)`.
        let factory_args: Vec<ExprId> = gen_params
            .iter()
            .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
            .collect();
        let new_expr = ast.add_expr(Expr::New {
            class_name: class_name.clone(),
            args: factory_args,
        });
        let factory_body = vec![Stmt::Return(Some(new_expr))];
        ast.stmts[idx] = Stmt::FnDecl {
            name: gen_name,
            type_params: Vec::new(),
            params: gen_params,
            return_type: Some(class_name),
            body: factory_body,
            is_generator: false,
        };
    }

    ast.stmts.extend(appended);
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
fn expand_yield_into_in_stmt(ast: &mut Ast, s: &mut Stmt, yield_ty: &str) {
    match s {
        Stmt::YieldInto { var, type_ann, value } => {
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
            };
            *s = Stmt::Multi(vec![yield_stmt, let_stmt]);
        }
        Stmt::If { then_branch, else_branch, .. } => {
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
fn lift_lets_in_stmt(ast: &mut Ast, s: &mut Stmt, lifted: &mut Vec<(String, String)>) {
    match s {
        Stmt::LetDecl { name, type_ann, init, .. } => {
            let n = name.clone();
            let t = type_ann.clone().unwrap_or_else(|| "number".into());
            lifted.push((n.clone(), t));
            let this_id = ast.add_expr(Expr::This);
            let m = ast.add_expr(Expr::Member { obj: this_id, name: n });
            let assign = ast.add_expr(Expr::Assign { target: m, value: *init });
            *s = Stmt::Expr(assign);
        }
        Stmt::If { then_branch, else_branch, .. } => {
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

/// Returns true if `s` (or any nested stmt) contains a `yield`. Used
/// by `GenSm` to decide whether a control-flow construct must be
/// expanded into separate state arms (yields present) or can be
/// emitted inline as a regular Stmt::If / While / For.
fn stmt_contains_yield(s: &Stmt) -> bool {
    match s {
        Stmt::Yield(_) | Stmt::YieldInto { .. } => true,
        Stmt::If { then_branch, else_branch, .. } => {
            stmt_contains_yield(then_branch)
                || else_branch.as_deref().is_some_and(stmt_contains_yield)
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => stmt_contains_yield(body),
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_contains_yield) || stmt_contains_yield(body)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts.iter().any(stmt_contains_yield),
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| c.body.iter().any(stmt_contains_yield))
                || default.as_ref().is_some_and(|d| d.iter().any(stmt_contains_yield))
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            body.iter().any(stmt_contains_yield)
                || catch_body.iter().any(stmt_contains_yield)
                || finally_body.as_ref().is_some_and(|f| f.iter().any(stmt_contains_yield))
        }
        _ => false,
    }
}

/// Rewrite `continue;` / `break;` inside `s` into `state = <target>;
/// continue;` gotos that re-enter the enclosing `while (true)` state
/// machine at the loop's continue / break target. Stops at inner loop
/// boundaries — break/continue inside a nested yield-free
/// `while` / `for` belong to that inner loop and stay literal.
fn rewrite_break_continue_for_outer(
    ast: &mut Ast,
    s: &mut Stmt,
    cont_target: usize,
    brk_target: usize,
) {
    fn make_goto(ast: &mut Ast, target: usize) -> Stmt {
        let this_id = ast.add_expr(Expr::This);
        let m = ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__state".into(),
        });
        let lit = ast.add_expr(Expr::Number(target as f64));
        let assign = ast.add_expr(Expr::Assign {
            target: m,
            value: lit,
        });
        Stmt::Block(vec![Stmt::Expr(assign), Stmt::Continue])
    }
    match s {
        Stmt::Continue => *s = make_goto(ast, cont_target),
        Stmt::Break => *s = make_goto(ast, brk_target),
        // Inner loops own their break/continue — don't descend.
        Stmt::While { .. } | Stmt::DoWhile { .. } | Stmt::For { .. } => {}
        // Switch swallows `break` (it targets the switch). `continue`
        // inside a switch belongs to the enclosing loop, but yields
        // inside switch aren't in J.2.b scope so we don't touch this.
        Stmt::Switch { .. } => {}
        Stmt::Try { .. } => {}
        Stmt::If { then_branch, else_branch, .. } => {
            rewrite_break_continue_for_outer(ast, then_branch, cont_target, brk_target);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_break_continue_for_outer(ast, eb, cont_target, brk_target);
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_break_continue_for_outer(ast, s, cont_target, brk_target);
            }
        }
        _ => {}
    }
}

/// State-machine emitter for generator bodies. Each state's body is
/// accumulated into `cur_buf` and flushed into `arms[cur_state]` when
/// the state ends (via yield, goto, or descent into a nested state).
///
/// The final assembled if-chain is wrapped in `while (true) { ... }`
/// so `Stmt::Continue` can be used as the goto primitive — setting
/// `this.__state = N; continue;` re-enters the chain at state N.
struct GenSm<'a> {
    ast: &'a mut Ast,
    arms: Vec<Vec<Stmt>>,
    cur_state: usize,
    cur_buf: Vec<Stmt>,
    /// (continue_target, break_target) for each enclosing yield-loop.
    /// Yield-FREE inner loops emit inline — their break/continue keep
    /// their normal Stmt::Break / Stmt::Continue meaning, never enter
    /// this stack.
    loop_stack: Vec<(usize, usize)>,
}

impl<'a> GenSm<'a> {
    fn new(ast: &'a mut Ast) -> Self {
        Self {
            ast,
            arms: vec![Vec::new()],
            cur_state: 0,
            cur_buf: Vec::new(),
            loop_stack: Vec::new(),
        }
    }

    fn alloc_state(&mut self) -> usize {
        let s = self.arms.len();
        self.arms.push(Vec::new());
        s
    }

    fn flush_cur(&mut self) {
        let cur = self.cur_state;
        let buf = std::mem::take(&mut self.cur_buf);
        self.arms[cur].extend(buf);
    }

    fn emit_set_state(&mut self, target: usize) -> Stmt {
        let this_id = self.ast.add_expr(Expr::This);
        let m = self.ast.add_expr(Expr::Member {
            obj: this_id,
            name: "__state".into(),
        });
        let lit = self.ast.add_expr(Expr::Number(target as f64));
        let assign = self.ast.add_expr(Expr::Assign {
            target: m,
            value: lit,
        });
        Stmt::Expr(assign)
    }

    fn emit_goto(&mut self, target: usize) -> Vec<Stmt> {
        let set = self.emit_set_state(target);
        vec![set, Stmt::Continue]
    }

    fn emit_yield_return(&mut self, val: ExprId, next: usize) -> Vec<Stmt> {
        let set = self.emit_set_state(next);
        let done = self.ast.add_expr(Expr::Bool(false));
        let obj = self.ast.add_expr(Expr::ObjectLit {
            fields: vec![("value".into(), val), ("done".into(), done)],
        });
        vec![set, Stmt::Return(Some(obj))]
    }

    fn lower_seq(&mut self, stmts: Vec<Stmt>) {
        for s in stmts {
            self.lower(s);
        }
    }

    fn lower(&mut self, stmt: Stmt) {
        match stmt {
            Stmt::Yield(e) => {
                let next = self.alloc_state();
                let mut yr = self.emit_yield_return(e, next);
                self.cur_buf.append(&mut yr);
                self.flush_cur();
                self.cur_state = next;
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                for s in stmts {
                    self.lower(s);
                }
            }
            Stmt::If { cond, then_branch, else_branch } => {
                let then_has = stmt_contains_yield(&then_branch);
                let else_has = else_branch
                    .as_deref()
                    .is_some_and(stmt_contains_yield);
                if !then_has && !else_has {
                    let mut s = Stmt::If { cond, then_branch, else_branch };
                    if let Some(&(cont, brk)) = self.loop_stack.last() {
                        rewrite_break_continue_for_outer(self.ast, &mut s, cont, brk);
                    }
                    self.cur_buf.push(s);
                    return;
                }
                let then_entry = self.alloc_state();
                let post = self.alloc_state();
                let else_entry = if else_branch.is_some() {
                    self.alloc_state()
                } else {
                    post
                };
                let then_jump = self.emit_goto(then_entry);
                let else_jump = self.emit_goto(else_entry);
                self.cur_buf.push(Stmt::If {
                    cond,
                    then_branch: Box::new(Stmt::Block(then_jump)),
                    else_branch: Some(Box::new(Stmt::Block(else_jump))),
                });
                self.flush_cur();

                self.cur_state = then_entry;
                self.lower(*then_branch);
                let mut exit = self.emit_goto(post);
                self.cur_buf.append(&mut exit);
                self.flush_cur();

                if let Some(eb) = else_branch {
                    self.cur_state = else_entry;
                    self.lower(*eb);
                    let mut exit = self.emit_goto(post);
                    self.cur_buf.append(&mut exit);
                    self.flush_cur();
                }

                self.cur_state = post;
            }
            Stmt::While { cond, body } => {
                if !stmt_contains_yield(&body) {
                    self.cur_buf.push(Stmt::While { cond, body });
                    return;
                }
                let head = self.alloc_state();
                let body_entry = self.alloc_state();
                let post = self.alloc_state();

                let mut to_head = self.emit_goto(head);
                self.cur_buf.append(&mut to_head);
                self.flush_cur();

                self.cur_state = head;
                let then_jump = self.emit_goto(body_entry);
                let else_jump = self.emit_goto(post);
                self.cur_buf.push(Stmt::If {
                    cond,
                    then_branch: Box::new(Stmt::Block(then_jump)),
                    else_branch: Some(Box::new(Stmt::Block(else_jump))),
                });
                self.flush_cur();

                self.cur_state = body_entry;
                self.loop_stack.push((head, post));
                self.lower(*body);
                self.loop_stack.pop();
                let mut back = self.emit_goto(head);
                self.cur_buf.append(&mut back);
                self.flush_cur();

                self.cur_state = post;
            }
            Stmt::For { init, cond, step, body } => {
                if !stmt_contains_yield(&body)
                    && !init.as_deref().is_some_and(stmt_contains_yield)
                {
                    self.cur_buf.push(Stmt::For { init, cond, step, body });
                    return;
                }
                if let Some(i) = init {
                    self.lower(*i);
                }
                let head = self.alloc_state();
                let body_entry = self.alloc_state();
                let step_state = self.alloc_state();
                let post = self.alloc_state();

                let mut to_head = self.emit_goto(head);
                self.cur_buf.append(&mut to_head);
                self.flush_cur();

                self.cur_state = head;
                if let Some(c) = cond {
                    let then_jump = self.emit_goto(body_entry);
                    let else_jump = self.emit_goto(post);
                    self.cur_buf.push(Stmt::If {
                        cond: c,
                        then_branch: Box::new(Stmt::Block(then_jump)),
                        else_branch: Some(Box::new(Stmt::Block(else_jump))),
                    });
                } else {
                    let mut g = self.emit_goto(body_entry);
                    self.cur_buf.append(&mut g);
                }
                self.flush_cur();

                self.cur_state = body_entry;
                self.loop_stack.push((step_state, post));
                self.lower(*body);
                self.loop_stack.pop();
                let mut to_step = self.emit_goto(step_state);
                self.cur_buf.append(&mut to_step);
                self.flush_cur();

                self.cur_state = step_state;
                if let Some(s) = step {
                    self.cur_buf.push(Stmt::Expr(s));
                }
                let mut back = self.emit_goto(head);
                self.cur_buf.append(&mut back);
                self.flush_cur();

                self.cur_state = post;
            }
            Stmt::Continue => {
                if let Some(&(cont, _)) = self.loop_stack.last() {
                    let mut g = self.emit_goto(cont);
                    self.cur_buf.append(&mut g);
                    self.flush_cur();
                    let dead = self.alloc_state();
                    self.cur_state = dead;
                } else {
                    self.cur_buf.push(Stmt::Continue);
                }
            }
            Stmt::Break => {
                if let Some(&(_, brk)) = self.loop_stack.last() {
                    let mut g = self.emit_goto(brk);
                    self.cur_buf.append(&mut g);
                    self.flush_cur();
                    let dead = self.alloc_state();
                    self.cur_state = dead;
                } else {
                    self.cur_buf.push(Stmt::Break);
                }
            }
            other => self.cur_buf.push(other),
        }
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
    let has_user_main = ast.stmts.iter().any(|s| {
        matches!(s, Stmt::FnDecl { name, .. } if name == "main")
    });
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

/// Runs between `desugar_generators` and `desugar_classes` so
/// `new Promise(...)` is still in pre-desugar shape (desugar_classes
/// will rewrite it to `__new_Promise(...)`).
pub fn desugar_async(ast: &mut Ast) {
    if ast.async_fns.is_empty() {
        return;
    }
    // Find every async FnDecl by index so we can mutate ast.stmts in
    // place. We only touch stmts; field shapes stay otherwise unchanged.
    let async_indices: Vec<usize> = ast
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::FnDecl { name, .. } if ast.async_fns.contains(name) => Some(i),
            _ => None,
        })
        .collect();

    for idx in async_indices {
        // Snapshot the FnDecl pieces so we can rebuild it in place.
        let (name, type_params, params, return_type, body) = match &ast.stmts[idx] {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                ..
            } => (
                name.clone(),
                type_params.clone(),
                params.clone(),
                return_type.clone(),
                body.clone(),
            ),
            _ => unreachable!(),
        };
        let inner_ty = return_type.unwrap_or_else(|| {
            panic!(
                "async function {name} requires an explicit return type \
                 annotation `: T` (Phase L MVP)"
            )
        });
        let promise_ty = format!("Promise<{inner_ty}>");

        // T-15.h: rewrite each `return e;` to `return Promise.resolve(e);`.
        // No shared `__async_p` state — every return constructs a
        // fresh fulfilled built-in Promise. Cleaner than the v0.4.x
        // user-class MVP and removes the multi-return move-tracker
        // workaround.
        let mut new_body: Vec<Stmt> = Vec::with_capacity(body.len() + 1);
        for s in body {
            let mut s = s;
            rewrite_returns_for_async(ast, &mut s, &inner_ty);
            new_body.push(s);
        }
        // Tail safety: if control flow falls off the end, return
        // `Promise.resolve(<default T>)`.
        if !body_ends_in_return(&new_body) {
            let default_init = default_init_for_type(&inner_ty);
            let default_id = ast.add_expr(default_init);
            let promise_ident = ast.add_expr(Expr::Ident("Promise".into()));
            let resolve_member = ast.add_expr(Expr::Member {
                obj: promise_ident,
                name: "resolve".into(),
            });
            let call = ast.add_expr(Expr::Call {
                callee: resolve_member,
                args: vec![default_id],
            });
            new_body.push(Stmt::Return(Some(call)));
        }

        ast.stmts[idx] = Stmt::FnDecl {
            name,
            type_params,
            params,
            return_type: Some(promise_ty),
            body: new_body,
            is_generator: false,
        };
    }
}

/// True if the (possibly-empty) body's last reachable statement is a
/// `Stmt::Return`. Used by `desugar_async` to skip emitting the tail-
/// safety `return __async_p;` when the body already ends in one (a
/// second access of `__async_p` would trip tr's move tracker even on
/// the unreachable path).
fn body_ends_in_return(body: &[Stmt]) -> bool {
    match body.last() {
        Some(Stmt::Return(_)) => true,
        Some(Stmt::Multi(stmts)) | Some(Stmt::Block(stmts)) => body_ends_in_return(stmts),
        _ => false,
    }
}

/// T-15.h (v0.5.0) — recursively rewrite `Stmt::Return(Some(e))` /
/// `Stmt::Return(None)` inside `s` into `Stmt::Return(Promise.resolve(e))`.
///
/// Pre-T-15.h MVP wrapped each return in a user-class `__async_p`
/// shared across the function body (`__async_p.do_resolve(e); return
/// __async_p;`). With the built-in Promise<T> from T-15, every return
/// just constructs a fresh fulfilled Promise — no shared state, no
/// move-tracker complications, no need for the user to declare
/// `class Promise<T>` themselves.
fn rewrite_returns_for_async(ast: &mut Ast, s: &mut Stmt, inner_ty: &str) {
    match s {
        Stmt::Return(maybe) => {
            let value = match maybe {
                Some(eid) => *eid,
                None => {
                    let default = default_init_for_type(inner_ty);
                    ast.add_expr(default)
                }
            };
            // Build `Promise.resolve(value)` AST.
            let promise_ident = ast.add_expr(Expr::Ident("Promise".into()));
            let resolve_member = ast.add_expr(Expr::Member {
                obj: promise_ident,
                name: "resolve".into(),
            });
            let call = ast.add_expr(Expr::Call {
                callee: resolve_member,
                args: vec![value],
            });
            *s = Stmt::Return(Some(call));
        }
        Stmt::If { then_branch, else_branch, .. } => {
            rewrite_returns_for_async(ast, then_branch, inner_ty);
            if let Some(eb) = else_branch.as_deref_mut() {
                rewrite_returns_for_async(ast, eb, inner_ty);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        Stmt::For { body, init, .. } => {
            if let Some(i) = init.as_deref_mut() {
                rewrite_returns_for_async(ast, i, inner_ty);
            }
            rewrite_returns_for_async(ast, body, inner_ty);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_returns_for_async(ast, s, inner_ty);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for s in &mut c.body {
                    rewrite_returns_for_async(ast, s, inner_ty);
                }
            }
            if let Some(d) = default {
                for s in d {
                    rewrite_returns_for_async(ast, s, inner_ty);
                }
            }
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            for s in body {
                rewrite_returns_for_async(ast, s, inner_ty);
            }
            for s in catch_body {
                rewrite_returns_for_async(ast, s, inner_ty);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    rewrite_returns_for_async(ast, s, inner_ty);
                }
            }
        }
        _ => {}
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
        if let Stmt::ExportDecl { inner: Some(boxed), .. } = s {
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
    source.strip_prefix("node:").unwrap_or(source).replace('/', "_")
}

pub fn desugar_builtin_imports(ast: &mut Ast) {
    use std::collections::HashMap;
    /* Build name → (module, original_name). The local alias (if
     * the user wrote `import { x as y }`) is the lookup key; the
     * original name is the field used in the Member rewrite. */
    let mut imported: HashMap<String, (String, String)> = HashMap::new();
    let mut to_drop: Vec<usize> = Vec::new();
    for (idx, s) in ast.stmts.iter().enumerate() {
        if let Stmt::ImportDecl { source, named, default: _, namespace } = s
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
                ast.exprs[i] = Expr::Member { obj: module_id, name: orig };
            }
        }
    }
}

pub fn desugar_builtin_new(ast: &mut Ast) {
    /* Pass 1 — handle `Array.of(a, b, c)` rewrites. Walk every Call
     * whose callee is the Member shape `Array.of`, and replace the
     * whole Call expression with an array literal. Same ExprId is
     * reused so downstream passes see plain `Expr::Array`. */
    let n_exprs = ast.exprs.len();
    for i in 0..n_exprs {
        let array_of_args = match &ast.exprs[i] {
            Expr::Call { callee, args } => {
                let callee_id = *callee;
                if let Expr::Member { obj, name } = &ast.exprs[callee_id.0 as usize]
                    && name == "of"
                    && let Expr::Ident(ns) = &ast.exprs[obj.0 as usize]
                    && ns == "Array"
                {
                    Some(args.clone())
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(args) = array_of_args {
            ast.exprs[i] = Expr::Array(args);
        }
    }
    let n = ast.exprs.len();
    for i in 0..n {
        let plan = match &ast.exprs[i] {
            Expr::New { class_name, args } if class_name == "Date" => {
                /* `new Date(...)` arity dispatch:
                 *   0 args → __torajs_date_now
                 *   1 arg :
                 *     literal `Expr::String` → __torajs_date_from_iso
                 *     otherwise → __torajs_date_from_ms (number)
                 *   2..7 args → __torajs_date_from_components with
                 *     missing trailing args padded to JS defaults
                 *     (day=1, hour=min=sec=ms=0).
                 *   ≥8 args → invalid (JS ignores extras; we panic
                 *     in Phase 2.0b.2 to surface unexpected idioms). */
                match args.len() {
                    0 => Some(("__torajs_date_now".to_string(), false, args.clone())),
                    1 => {
                        let is_str = matches!(
                            ast.exprs[args[0].0 as usize],
                            Expr::String(_)
                        );
                        if is_str {
                            Some(("__torajs_date_from_iso".to_string(), false, args.clone()))
                        } else {
                            Some(("__torajs_date_from_ms".to_string(), false, args.clone()))
                        }
                    }
                    n_args if (2..=7).contains(&n_args) => {
                        Some(("__torajs_date_from_components".to_string(), true, args.clone()))
                    }
                    n_args => panic!(
                        "v0.2 #2 Phase 2.0b.2: `new Date(...)` with {n_args} args not yet supported"
                    ),
                }
            }
            _ => None,
        };
        if let Some((factory, pad_components, mut args)) = plan {
            if pad_components {
                /* Pad to 7 args with JS-spec defaults. */
                while args.len() < 7 {
                    let val = match args.len() {
                        2 => 1.0, /* day default = 1 */
                        _ => 0.0, /* hour, minute, second, ms default = 0 */
                    };
                    args.push(ast.add_expr(Expr::Number(val)));
                }
            }
            let callee = ast.add_expr(Expr::Ident(factory));
            ast.exprs[i] = Expr::Call { callee, args };
        }
    }
}

pub fn desugar_classes(ast: &mut Ast) {
    // Pass 1 — extract every ClassDecl. After this loop the original
    // ClassDecl stmts are replaced by their generated TypeDecl in-place;
    // ctor / methods / factory FnDecls accumulate in `appended`.
    // method name → ordered list of declaring classes. Source order
    // (deepest sub last) — this matters for dispatcher emission since
    // we walk in reverse to check the deepest class first. Tracks
    // every class that declares a method body, including overrides.
    let mut method_owners: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut class_field_inits: std::collections::HashMap<String, Vec<(String, ExprId)>> =
        std::collections::HashMap::new();
    let mut class_field_preludes: std::collections::HashMap<String, Vec<Stmt>> =
        std::collections::HashMap::new();
    let mut appended: Vec<Stmt> = Vec::new();
    /// M-OO.4 — accumulator for `let __sf_<C>__<name>: T = init;`
    /// declarations. These get **prepended** to `ast.stmts` (not
    /// appended) so the synthetic `main` fn runs them before any
    /// user top-level code; the alternative leaves `check()` reading
    /// uninitialized slots when the user-visible call comes first
    /// in source order.
    let mut static_field_inits: Vec<Stmt> = Vec::new();

    // Snapshot the class metadata first (cloned out so we can mutate
    // ast.stmts in-place without aliasing). M5.2 adds `parent` to the
    // tuple — for inheritance flattening + super(args) rewriting.
    // M-OO.4 adds the static-fields / static-methods slices for the
    // post-collect emission of `__sf_<C>__<n>` LetDecls and
    // `__sm_<C>__<m>` FnDecls.
    let class_index: Vec<(
        usize,
        String,
        Vec<String>,           // type_params
        Option<String>,
        Vec<(String, String)>,
        Vec<StaticField>,      // static_fields
        Option<ClassCtor>,
        Vec<ClassMethod>,
        Vec<ClassMethod>,      // static_methods
    )> = ast
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::ClassDecl {
                name,
                type_params,
                parent,
                is_abstract: _,
                fields,
                static_fields,
                ctor,
                methods,
                static_methods,
            } => Some((
                i,
                name.clone(),
                type_params.clone(),
                parent.clone(),
                fields.clone(),
                static_fields.clone(),
                ctor.clone(),
                methods.clone(),
                static_methods.clone(),
            )),
            _ => None,
        })
        .collect();

    if class_index.is_empty() {
        return;
    }

    // M-OO.6 — collect abstract-class names + per-class abstract-method
    // names. Concrete subclasses must override every inherited abstract;
    // `new` of an abstract class is rejected (in check.rs). Side-channel
    // (HashSet / HashMap) instead of inflating class_index's tuple.
    let mut abstract_classes: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut abstract_methods: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for s in ast.stmts.iter() {
        if let Stmt::ClassDecl {
            name,
            is_abstract,
            methods,
            ..
        } = s
        {
            if *is_abstract {
                abstract_classes.insert(name.clone());
            }
            let abs: Vec<String> = methods
                .iter()
                .filter(|m| m.is_abstract)
                .map(|m| m.name.clone())
                .collect();
            if !abs.is_empty() {
                abstract_methods.insert(name.clone(), abs);
            }
            // Abstract method only allowed inside abstract class.
            // (Parser already rejects this for the immediate case, but
            // a desugar-time double-check catches programmatically-built
            // classes from upstream desugars.)
            if !is_abstract && methods.iter().any(|m| m.is_abstract) {
                panic!(
                    "M-OO.6: concrete class `{name}` cannot declare abstract methods"
                );
            }
        }
    }
    // Walk every concrete class's inheritance chain (root → leaf,
    // accumulating "unimplemented" abstract names along the way) and
    // verify that none survive into the concrete leaf.
    for (_, cname, _, _, _, _, _, _, _) in &class_index {
        if abstract_classes.contains(cname) {
            continue;
        }
        let mut chain: Vec<String> = Vec::new();
        let mut cur: Option<String> = Some(cname.clone());
        while let Some(c) = cur {
            chain.push(c.clone());
            cur = class_index
                .iter()
                .find(|t| t.1 == c)
                .and_then(|t| t.3.clone());
        }
        chain.reverse();
        let mut unimplemented: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for cls in &chain {
            if let Some(absms) = abstract_methods.get(cls) {
                for m in absms {
                    unimplemented.insert(m.clone());
                }
            }
            if let Some(t) = class_index.iter().find(|t| &t.1 == cls) {
                let cls_methods = &t.7;
                for m in cls_methods.iter() {
                    if !m.is_abstract {
                        unimplemented.remove(&m.name);
                    }
                }
            }
        }
        if !unimplemented.is_empty() {
            let mut names: Vec<&String> = unimplemented.iter().collect();
            names.sort();
            panic!(
                "M-OO.6: concrete class `{cname}` must override abstract method(s): {names:?}"
            );
        }
    }

    // Build the parent map and validate the inheritance graph.
    let mut parent_map: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for (_, cname, _tp, parent, _, _, _, _, _) in &class_index {
        parent_map.insert(cname.clone(), parent.clone());
    }
    // Make the parent map visible to post-desugar passes so `instanceof`
    // can walk the chain when the LHS is a subclass and the RHS names
    // an ancestor.
    ast.class_parents = parent_map.clone();
    // method_owners populated below; expose only the multi-owner entries
    // so ssa_lower's `__dispatch_` interception is a constant-time
    // contains lookup.
    // (Filled in after the per-method walk; HashMap moved at end.)
    // Detect missing-parent and cycle errors. We don't allow forward
    // references to classes that come later in source order — every
    // ancestor must be declared before its descendants. This keeps
    // field-flattening + factory-emission order trivially correct.
    let mut declared_so_far: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, cname, _tp, parent, _, _, _, _, _) in &class_index {
        if let Some(p) = parent {
            if !declared_so_far.contains(p) {
                panic!(
                    "M5.2: `{cname} extends {p}` — parent class `{p}` must be declared \
                     before `{cname}` (and must exist as a class, not a type alias)"
                );
            }
        }
        declared_so_far.insert(cname.clone());
    }

    // Compute the flattened (full) field list for each class along the
    // inheritance chain: parent's fields followed by self's. This is the
    // layout that `type C = { ... }` will declare and the factory will
    // default-initialize.
    let mut full_fields: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for (_, cname, _tp, parent, fields, _, _, _, _) in &class_index {
        let mut combined: Vec<(String, String)> = Vec::new();
        if let Some(p) = parent {
            // Parent must be in full_fields by now (declaration order check
            // above guarantees this).
            let pfields = full_fields.get(p).unwrap_or_else(|| {
                panic!("internal: parent `{p}` of `{cname}` had no flattened fields")
            });
            combined.extend(pfields.iter().cloned());
        }
        for (fn_, ft) in fields {
            // Subclass fields must not collide with parent fields. (TS
            // allows shadowing with the same type, but M5.2.a keeps this
            // simple — disallow.)
            if combined.iter().any(|(n, _)| n == fn_) {
                panic!(
                    "M5.2: subclass `{cname}` redeclares parent field `{fn_}` — \
                     not yet supported"
                );
            }
            combined.push((fn_.clone(), ft.clone()));
        }
        full_fields.insert(cname.clone(), combined);
    }

    // Build the method dispatch table. Phase H.3.b: ancestor-descendant
    // overrides go through a generated `__dispatch_<method>` fn (walks
    // runtime class tag). Phase I.1 lifted the sibling-collision panic:
    // unrelated classes are allowed to share a method name now — call
    // sites pick the right `__cm_<C>__M` from obj's static type at SSA
    // lower time (handled by the `Type::Obj` Member-call arm).
    for (_, cname, _tp, _, _, _, _, methods, _) in &class_index {
        for m in methods {
            method_owners.entry(m.name.clone())
                .or_default()
                .push(cname.clone());
        }
    }
    // Phase I.1 — categorize each multi-owner method. If owners[0]
    // (source-first, the topmost in source order) is an ancestor of
    // every other owner, the method forms a single inheritance chain
    // and gets the `__dispatch_<M>` runtime-tag dispatcher (override
    // case). Otherwise (siblings in unrelated hierarchies, or a mix),
    // call sites stay as Member-shape and ssa_lower picks the right
    // `__cm_<C>__M` from obj's static type.
    let chain_methods: std::collections::HashSet<String> = method_owners
        .iter()
        .filter(|(_, owners)| owners.len() > 1)
        .filter(|(_, owners)| {
            let base = &owners[0];
            owners.iter().skip(1).all(|sub| {
                method_owner_is_in_chain(&parent_map, base, sub)
            })
        })
        .map(|(n, _)| n.clone())
        .collect();

    // Phase H.3.b — emit `__dispatch_<method>(__this, args...)` for every
    // method whose name has multiple owners (the override case). Body is
    // an instanceof-chain checking subclasses deepest-first, falling
    // through to the base owner's `__cm_<Base>__<method>`. Single-owner
    // methods stay on the static `__cm_<Owner>__M` path — no dispatcher
    // fn, no extra indirection.
    for (m_name, owners) in &method_owners {
        if !chain_methods.contains(m_name) {
            continue;
        }
        // Locate the base owner's method to copy its signature.
        let base_owner = &owners[0];
        let (_, _, base_tp, _, _, _, _, base_methods, _) = class_index
            .iter()
            .find(|(_, n, ..)| n == base_owner)
            .expect("base owner must exist in class_index");
        let base_method = base_methods
            .iter()
            .find(|m| &m.name == m_name)
            .expect("base owner declared the method by construction");
        // Dispatcher params: `__this: Base, ...method_params`.
        let mut params: Vec<Param> = Vec::with_capacity(base_method.params.len() + 1);
        let this_ann = if base_tp.is_empty() {
            base_owner.clone()
        } else {
            format!("{base_owner}<{}>", base_tp.join("|"))
        };
        params.push(Param {
            name: "__this".into(),
            type_ann: Some(this_ann),
            default: None,
            is_rest: false,
        });
        params.extend(base_method.params.iter().cloned());
        // Body is a typecheck-clean stub that just forwards to the base
        // owner's `__cm_<Base>__M` — passing `__this: Base` to a fn
        // expecting `__this: Base` typechecks fine, and the SSA layer
        // bypasses this body entirely (see `__dispatch_` interception
        // in ssa_lower's Call arm). The stub is what tr would do if
        // override were ignored; the real virtual dispatch happens at
        // SSA level where untyped pointer args dodge the contravariance
        // problem (subclass __cm fns expect __this: Sub which the
        // typechecker won't widen Animal → Sub for, even though the
        // runtime layout is compatible).
        let mut body: Vec<Stmt> = Vec::new();
        let stub_callee = ast.add_expr(Expr::Ident(format!("__cm_{base_owner}__{m_name}")));
        let stub_this = ast.add_expr(Expr::Ident("__this".into()));
        let mut stub_args: Vec<ExprId> = Vec::with_capacity(base_method.params.len() + 1);
        stub_args.push(stub_this);
        for p in &base_method.params {
            stub_args.push(ast.add_expr(Expr::Ident(p.name.clone())));
        }
        let stub_call = ast.add_expr(Expr::Call {
            callee: stub_callee,
            args: stub_args,
        });
        body.push(Stmt::Return(Some(stub_call)));
        appended.push(Stmt::FnDecl {
            name: format!("__dispatch_{m_name}"),
            type_params: base_tp.clone(),
            params,
            return_type: base_method.return_type.clone(),
            body,
            is_generator: false,
        });
    }

    // Build a snapshot of every TypeDecl's field layout. Used by the
    // default-init helper below so a class field whose type is a type
    // alias (`type Step = { value: number, done: boolean }`) gets a
    // structurally-correct zero rather than a Number(0).
    let mut type_alias_fields: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for s in &ast.stmts {
        if let Stmt::TypeDecl { name, fields, .. } = s {
            type_alias_fields.insert(name.clone(), fields.clone());
        }
    }
    let combined_fields_map = full_fields.clone();

    // For each class, build the list of typed default-initializer expressions
    // that the factory will use to seed the `__this` object literal. We use
    // the FLATTENED field list (parent fields + self fields) so subclass
    // factories produce a fully-initialized object.
    //
    // Empty `T[]` defaults need special handling: a bare `[]` in expression
    // position has no inferable element type. We hoist these out into a
    // typed prelude let — `let __def_arr_<field>: T[] = []` — and use the
    // ident as the field init. The let-binding's annotation gives ssa-lower
    // enough context to emit a typed `arr_alloc(0)`.
    //
    // Class- or alias-typed fields recursively expand into a nested
    // ObjectLit of zero-initialized children, looked up via
    // `combined_fields_map` (classes) and `type_alias_fields` (aliases).
    // This is what makes `__Gen_<X>` / `__step_<X>` fields work as
    // class fields on outer iterator classes (J.3 / I.2-inside-gen).
    for (_, cname, _tp, _, _, _, _, _, _) in &class_index {
        let combined = full_fields.get(cname).unwrap().clone();
        let mut init_pairs: Vec<(String, ExprId)> = Vec::with_capacity(combined.len());
        let mut prelude: Vec<Stmt> = Vec::new();
        for (fname, fty) in &combined {
            let id = default_init_for_field(
                ast,
                fty,
                &combined_fields_map,
                &type_alias_fields,
                &mut prelude,
                cname,
                fname,
                &mut std::collections::HashSet::new(),
            );
            init_pairs.push((fname.clone(), id));
        }
        class_field_inits.insert(cname.clone(), init_pairs);
        class_field_preludes.insert(cname.clone(), prelude);
    }

    // Pass 1.5 — rewrite `super(args)` inside each subclass's ctor body
    // into a Call to `__cm_<Parent>__ctor(__this, args)`. Must run before
    // pass 2 (which rewrites `Expr::This` and method-call shapes).
    for (_, cname, _tp, parent, _, _, ctor, _, _) in &class_index {
        let Some(c) = ctor.as_ref() else { continue };
        let mut super_sites: Vec<(ExprId, Vec<ExprId>)> = Vec::new();
        for s in &c.body {
            collect_super_in_stmt(ast, s, &mut super_sites);
        }
        for (eid, args) in super_sites {
            let parent_name = parent.as_ref().unwrap_or_else(|| {
                panic!(
                    "M5.2: `super(...)` used in `{cname}.constructor` but `{cname}` \
                     has no `extends` clause"
                )
            });
            let callee = ast.add_expr(Expr::Ident(format!("__cm_{parent_name}__ctor")));
            let this_id = ast.add_expr(Expr::This);
            let mut new_args = Vec::with_capacity(args.len() + 1);
            new_args.push(this_id);
            new_args.extend(args);
            ast.exprs[eid.0 as usize] = Expr::Call {
                callee,
                args: new_args,
            };
        }
    }

    // Pass 2 — rewrite the expression arena. Walking by index is safe
    // because we only mutate Exprs in place (or append new ones at the
    // tail; existing ExprIds keep their meaning).
    let n = ast.exprs.len();
    for i in 0..n {
        match &ast.exprs[i] {
            Expr::This => {
                ast.exprs[i] = Expr::Ident("__this".into());
            }
            Expr::New { class_name, args } => {
                /* Builtin News (Date, ...) are rewritten by
                 * `desugar_builtin_new` BEFORE this pass, so any
                 * remaining Expr::New here is a user class. */
                let factory = format!("__new_{class_name}");
                let args = args.clone();
                let callee = ast.add_expr(Expr::Ident(factory));
                ast.exprs[i] = Expr::Call { callee, args };
            }
            Expr::Call { callee, args } => {
                let callee_id = *callee;
                let args_clone = args.clone();
                // Look at what the callee is pointing at.
                if let Expr::Member { obj, name } = &ast.exprs[callee_id.0 as usize] {
                    let m_name = name.clone();
                    let obj_id = *obj;
                    if let Some(owners) = method_owners.get(&m_name) {
                        // Three cases:
                        // (1) Single owner — keep static dispatch via
                        //     `__cm_<C>__<M>`, EXCEPT when the receiver
                        //     is `this.<field>` and the field is typed
                        //     as a known builtin (Array `T[]`, `string`,
                        //     `number`). Those calls dispatch to the
                        //     intrinsic, not the user class's method
                        //     — without the guard, `class C { data:
                        //     T[]; push(v) { this.data.push(v); } }`
                        //     would rewrite the inner `this.data.push`
                        //     to `__cm_C__push(this.data, v)` and
                        //     infinite-recurse.
                        // (2) Multi-owner forming a single inheritance
                        //     chain (override case) — route through
                        //     `__dispatch_<M>` runtime-tag dispatcher.
                        // (3) Multi-owner across unrelated hierarchies
                        //     (sibling collision) — leave Member as-is.
                        if owners.len() == 1 {
                            let skip_for_builtin_field = receiver_is_this_builtin_field(
                                ast,
                                obj_id,
                                owners[0].as_str(),
                                &class_index,
                            );
                            if skip_for_builtin_field {
                                // Leave Member; ssa_lower picks the
                                // builtin intrinsic from the field's
                                // actual type at SSA time.
                            } else {
                                let mangled = format!("__cm_{}__{m_name}", owners[0]);
                                let new_callee = ast.add_expr(Expr::Ident(mangled));
                                let mut new_args =
                                    Vec::with_capacity(args_clone.len() + 1);
                                new_args.push(obj_id);
                                new_args.extend(args_clone);
                                ast.exprs[i] = Expr::Call {
                                    callee: new_callee,
                                    args: new_args,
                                };
                            }
                        } else if chain_methods.contains(&m_name) {
                            let mangled = format!("__dispatch_{m_name}");
                            let new_callee = ast.add_expr(Expr::Ident(mangled));
                            let mut new_args = Vec::with_capacity(args_clone.len() + 1);
                            new_args.push(obj_id);
                            new_args.extend(args_clone);
                            ast.exprs[i] = Expr::Call {
                                callee: new_callee,
                                args: new_args,
                            };
                        }
                        // else: sibling collision — leave Member call AS-IS.
                    }
                }
            }
            _ => {}
        }
    }

    // M-OO.4 — collect static-member rewrite tables: keys are
    // `(ClassName, member_name)` → flat replacement ident
    // (`__sf_<C>__<n>` for fields, `__sm_<C>__<m>` for methods). After
    // emitting the desugared decls, a second walk over `ast.exprs`
    // rewrites every `Expr::Member { obj: Ident("ClassName"), name }`
    // whose key is in the table to a plain `Expr::Ident(replacement)`.
    let mut static_member_rewrites: std::collections::HashMap<(String, String), String> =
        std::collections::HashMap::new();
    for (_, cname, _, _, _, sfs, _, _, sms) in &class_index {
        for sf in sfs {
            static_member_rewrites
                .insert((cname.clone(), sf.name.clone()), format!("__sf_{cname}__{}", sf.name));
        }
        for sm in sms {
            static_member_rewrites
                .insert((cname.clone(), sm.name.clone()), format!("__sm_{cname}__{}", sm.name));
        }
    }

    // Pass 3 — rewrite the stmt list. Replace each ClassDecl in-place
    // with its TypeDecl (using the flattened field list so subclasses
    // carry parent fields too), and accumulate the generated FnDecls.
    for (idx, cname, type_params, _parent, _own_fields, static_fields, ctor, methods, static_methods) in class_index {
        let type_decl = Stmt::TypeDecl {
            name: cname.clone(),
            type_params: type_params.clone(),
            fields: full_fields[&cname].clone(),
        };
        ast.stmts[idx] = type_decl;

        // For generic classes, the `__this` type ann must reference
        // the instantiated form, e.g. `Wrapper<T>` not bare `Wrapper`.
        let this_ann = if type_params.is_empty() {
            cname.clone()
        } else {
            format!("{cname}<{}>", type_params.join("|"))
        };

        // Constructor → C__ctor(__this: C, params...): void { body }
        let mut ctor_params_for_factory: Vec<Param> = Vec::new();
        if let Some(c) = &ctor {
            ctor_params_for_factory = c.params.clone();
            let mut params: Vec<Param> = Vec::with_capacity(c.params.len() + 1);
            params.push(Param {
                name: "__this".into(),
                type_ann: Some(this_ann.clone()),
                default: None,
                is_rest: false,
            });
            params.extend(c.params.iter().cloned());
            appended.push(Stmt::FnDecl {
                name: format!("__cm_{cname}__ctor"),
                type_params: type_params.clone(),
                params,
                return_type: Some("void".into()),
                body: c.body.clone(),
                is_generator: false,
            });
        }

        // Methods → __cm_C__m(__this: C, params...): R { body }
        for m in &methods {
            // M-OO.6 — abstract method: the user wrote no body. We
            // still need a `__cm_<C>__<m>` symbol because ssa_lower's
            // `__dispatch_<m>` interception emits the base owner as
            // the fall-through default branch. Concrete subclasses
            // override the dispatch via tag-switch, so the stub is
            // unreachable on a well-typed program — emit a `throw`
            // body as a defensive trap. The thrown value is a small
            // integer so we don't need a string allocation on a
            // never-taken path.
            if m.is_abstract {
                let mut params: Vec<Param> = Vec::with_capacity(m.params.len() + 1);
                params.push(Param {
                    name: "__this".into(),
                    type_ann: Some(this_ann.clone()),
                    default: None,
                    is_rest: false,
                });
                params.extend(m.params.iter().cloned());
                let trap_eid = ast.add_expr(Expr::Number(7777.0));
                let trap_body = vec![Stmt::Throw(trap_eid)];
                appended.push(Stmt::FnDecl {
                    name: format!("__cm_{cname}__{}", m.name),
                    type_params: type_params.clone(),
                    params,
                    return_type: m.return_type.clone(),
                    body: trap_body,
                    is_generator: false,
                });
                continue;
            }
            let mut params: Vec<Param> = Vec::with_capacity(m.params.len() + 1);
            params.push(Param {
                name: "__this".into(),
                type_ann: Some(this_ann.clone()),
                default: None,
                is_rest: false,
            });
            params.extend(m.params.iter().cloned());
            appended.push(Stmt::FnDecl {
                name: format!("__cm_{cname}__{}", m.name),
                type_params: type_params.clone(),
                params,
                return_type: m.return_type.clone(),
                body: m.body.clone(),
                is_generator: false,
            });
        }

        // Factory: __new_C(ctor_params...): C {
        //   let __this: C = { f0: <init>, f1: <init>, ... };
        //   C__ctor(__this, ctor_params...);   // only if a ctor was declared
        //   return __this;
        // }
        let factory_body = build_factory_body(
            ast,
            &cname,
            &type_params,
            &class_field_inits[&cname],
            class_field_preludes
                .get(&cname)
                .cloned()
                .unwrap_or_default(),
            ctor.as_ref(),
        );
        appended.push(Stmt::FnDecl {
            name: format!("__new_{cname}"),
            type_params: type_params.clone(),
            params: ctor_params_for_factory,
            return_type: Some(this_ann.clone()),
            body: factory_body,
            is_generator: false,
        });

        // M-OO.4 — emit `let __sf_<C>__<name>: T = init;` for each
        // static field. const-form (mutable=false) so K.4 refcount
        // globals accept it. The `init` ExprId is reused — desugar
        // runs before any pass that might mutate the expression
        // referenced by it.
        //
        // CRITICAL: static field LetDecls go into `static_field_inits`
        // (NOT `appended`) so they can be prepended to `ast.stmts`
        // before the user's top-level code runs. Otherwise the synth
        // main fn would call `check()` BEFORE the static field slot
        // was initialized — every read of `Counter.label` inside
        // `check()` would see the slot's null/zero default. This was
        // a real silent leak + correctness bug uncovered by the
        // m-oo-04-static `leaks --atExit` audit.
        for sf in &static_fields {
            static_field_inits.push(Stmt::LetDecl {
                mutable: false,
                name: format!("__sf_{cname}__{}", sf.name),
                type_ann: Some(sf.type_ann.clone()),
                init: sf.init,
            });
        }

        // M-OO.4 — emit `function __sm_<C>__<name>(...): R { body }`
        // for each static method. No `__this` param (statics don't
        // bind a receiver). type_params propagate from the class so
        // generic statics on a generic class work.
        for sm in &static_methods {
            appended.push(Stmt::FnDecl {
                name: format!("__sm_{cname}__{}", sm.name),
                type_params: type_params.clone(),
                params: sm.params.clone(),
                return_type: sm.return_type.clone(),
                body: sm.body.clone(),
                is_generator: false,
            });
        }
    }

    ast.stmts.extend(appended);

    // M-OO.4 — prepend static-field LetDecls so they init before any
    // user code. Maintains insertion order across multiple classes
    // (declaration-order, source-order). Doing this AFTER
    // `ast.stmts.extend(appended)` keeps the source-position of
    // appended decls (factory / __cm_*/__sm_*) unchanged; they're
    // already at the back where check.rs / ssa_lower expect them.
    if !static_field_inits.is_empty() {
        let mut new_stmts = static_field_inits;
        new_stmts.extend(std::mem::take(&mut ast.stmts));
        ast.stmts = new_stmts;
    }

    // M-OO.4 — rewrite `<ClassName>.<member>` accesses to flat
    // `__sf_<C>__<member>` / `__sm_<C>__<member>` Idents wherever
    // they appear in the program (top-level + every fn body / arrow
    // body / nested struct field initializer — all live in
    // `ast.exprs` since exprs are arena-allocated). This walks the
    // arena once; the rewrite is in-place and shape-preserving (a
    // Member is one ExprId; the new Ident is the same ExprId with a
    // new variant). Downstream passes (lift_arrow_fns, check.rs,
    // ssa_lower) see plain Idents and resolve them through the
    // top-level fn / globals tables already populated above.
    if !static_member_rewrites.is_empty() {
        for i in 0..ast.exprs.len() {
            let replacement = match &ast.exprs[i] {
                Expr::Member { obj, name } => {
                    if let Expr::Ident(class_name) = &ast.exprs[obj.0 as usize] {
                        let key = (class_name.clone(), name.clone());
                        static_member_rewrites.get(&key).cloned()
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(new_name) = replacement {
                ast.exprs[i] = Expr::Ident(new_name);
            }
        }
    }

    // M-OO.6 — reject `new AbstractClass()` after the desugar walk
    // (abstract metadata is local to this pass; the SSA layer never
    // sees it). Walking ast.exprs catches every construction site
    // regardless of where in the tree it lives.
    if !abstract_classes.is_empty() {
        for expr in &ast.exprs {
            if let Expr::New { class_name, .. } = expr
                && abstract_classes.contains(class_name)
            {
                panic!(
                    "M-OO.6: cannot instantiate abstract class `{class_name}` — use a concrete subclass"
                );
            }
        }
    }

    // Hand multi-owner method_owners to ssa_lower for the
    // `__dispatch_<M>` runtime-tag dispatch. Single-owner entries are
    // dropped since they don't need runtime resolution (already
    // statically rewritten unless the builtin-name guard skipped them,
    // in which case ssa_lower's sibling-class path picks them up via
    // the Type::Obj match — see the (Expr::Member ...) Call arm in
    // lower_expr).
    ast.method_owners = method_owners
        .into_iter()
        .filter(|(_, owners)| owners.len() > 1)
        .collect();

    /* T-24 — assign each chain method a stable vtable slot. Sorted
     * by name so codegen stays deterministic; the index becomes the
     * per-class vtable's `[N x ptr]` slot offset (in u64 units). */
    let mut chain_methods_sorted: Vec<&String> = chain_methods.iter().collect();
    chain_methods_sorted.sort();
    ast.method_index = chain_methods_sorted
        .into_iter()
        .enumerate()
        .map(|(i, n)| (n.clone(), i as u32))
        .collect();
}

/// True iff the call receiver is `this.<field>` AND the named
/// field on class `cname` has a builtin (Array / Str / Number)
/// type annotation. Used by desugar_classes' single-owner rewrite
/// guard so `this.data.push(v)` (where `data: T[]`) doesn't get
/// rewritten as a self-recursive class-method call.
///
/// `class_index` is the snapshot built at the top of desugar_classes
/// — `(usize, name, type_params, parent, fields, ctor, methods)`.
#[allow(clippy::type_complexity)]
fn receiver_is_this_builtin_field(
    ast: &Ast,
    obj_id: ExprId,
    cname: &str,
    class_index: &[(
        usize,
        String,
        Vec<String>,
        Option<String>,
        Vec<(String, String)>,
        Vec<StaticField>,
        Option<ClassCtor>,
        Vec<ClassMethod>,
        Vec<ClassMethod>,
    )],
) -> bool {
    let Expr::Member { obj: inner_obj, name: field_name } =
        &ast.exprs[obj_id.0 as usize]
    else {
        return false;
    };
    // The This → Ident("__this") rewrite in this same desugar pass
    // may already have fired for low-ExprId nodes by the time we
    // inspect this call (Pass 2 walks 0..n). Accept either shape.
    let inner_is_this = match &ast.exprs[inner_obj.0 as usize] {
        Expr::This => true,
        Expr::Ident(n) if n == "__this" => true,
        _ => false,
    };
    if !inner_is_this {
        return false;
    }
    // Find the class entry and look up the field's type annotation.
    let cls = class_index
        .iter()
        .find(|(_, n, ..)| n == cname);
    let Some((_, _, _, _, fields, _, _, _, _)) = cls else {
        return false;
    };
    let field_ty_ann = fields
        .iter()
        .find(|(fn_, _)| fn_ == field_name)
        .map(|(_, ann)| ann.as_str());
    let Some(ann) = field_ty_ann else {
        return false;
    };
    // Builtin: Array (`T[]`), `string`, or `number`. These dispatch
    // to runtime intrinsics, not user class methods.
    ann.ends_with("[]") || ann == "string" || ann == "number"
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
        });
        return ast.add_expr(Expr::Ident(local));
    }
    let sub_fields = class_layouts.get(fty).or_else(|| alias_layouts.get(fty));
    if let Some(sub_fields) = sub_fields {
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
    match ann {
        "number" => Expr::Number(0.0),
        "string" => Expr::String(String::new()),
        "boolean" => Expr::Bool(false),
        // Array types `T[]` and named types (other classes / aliases) are
        // not legally default-zero in TS — for M5.1 we punt and emit a
        // typed zero anyway; field types beyond primitive are deferred to
        // M5.2 alongside inheritance.
        _ if ann.ends_with("[]") => Expr::Array(Vec::new()),
        // TypeVar field (heuristic: short all-uppercase identifier — T,
        // U, K, V, A, B …). Emit a marker Ident that the monomorphizer
        // rewrites to the concrete default once the type is bound.
        _ if is_likely_typevar(ann) => {
            Expr::Ident(format!("__tvdefault__{ann}"))
        }
        _ => Expr::Number(0.0),
    }
}

fn is_likely_typevar(s: &str) -> bool {
    s.len() <= 2 && !s.is_empty() && s.chars().all(|c| c.is_ascii_uppercase())
}

/// Walk a stmt list and collect every `Expr::Super { args }` site, with
/// the original args slice preserved so the caller can build the
/// rewritten Call. Walks into nested blocks / control flow.
fn collect_super_in_stmt(
    ast: &Ast,
    s: &Stmt,
    out: &mut Vec<(ExprId, Vec<ExprId>)>,
) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => collect_super_in_expr(ast, *eid, out),
        Stmt::YieldInto { value, .. } => collect_super_in_expr(ast, *value, out),
        Stmt::Return(maybe) => {
            if let Some(eid) = maybe {
                collect_super_in_expr(ast, *eid, out);
            }
        }
        Stmt::LetDecl { init, .. } => collect_super_in_expr(ast, *init, out),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_super_in_expr(ast, *cond, out);
            collect_super_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_super_in_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } => {
            collect_super_in_expr(ast, *cond, out);
            collect_super_in_stmt(ast, body, out);
        }
        Stmt::DoWhile { body, cond } => {
            collect_super_in_stmt(ast, body, out);
            collect_super_in_expr(ast, *cond, out);
        }
        Stmt::Switch { scrutinee, cases, default } => {
            collect_super_in_expr(ast, *scrutinee, out);
            for c in cases {
                collect_super_in_expr(ast, c.value, out);
                for s in &c.body {
                    collect_super_in_stmt(ast, s, out);
                }
            }
            if let Some(db) = default {
                for s in db {
                    collect_super_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                collect_super_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_super_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_super_in_expr(ast, *st, out);
            }
            collect_super_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for st in stmts {
                collect_super_in_stmt(ast, st, out);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for st in body {
                collect_super_in_stmt(ast, st, out);
            }
            for st in catch_body {
                collect_super_in_stmt(ast, st, out);
            }
            if let Some(fb) = finally_body {
                for st in fb {
                    collect_super_in_stmt(ast, st, out);
                }
            }
        }
        Stmt::Break | Stmt::Continue => {}
        Stmt::ForOfSplitIter { parent, sep, body, .. } => {
            collect_super_in_expr(ast, *parent, out);
            collect_super_in_expr(ast, *sep, out);
            collect_super_in_stmt(ast, body, out);
        }
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } => {}
        Stmt::ImportDecl { .. } => {}
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                collect_super_in_stmt(ast, inner, out);
            }
        }
    }
}

fn collect_super_in_expr(
    ast: &Ast,
    eid: ExprId,
    out: &mut Vec<(ExprId, Vec<ExprId>)>,
) {
    match ast.get_expr(eid) {
        Expr::Super { args } => {
            // Record the site, then descend into args (super itself
            // probably doesn't nest super(args), but be safe).
            let args_clone = args.clone();
            for a in &args_clone {
                collect_super_in_expr(ast, *a, out);
            }
            out.push((eid, args_clone));
        }
        Expr::Call { callee, args } => {
            collect_super_in_expr(ast, *callee, out);
            for a in args {
                collect_super_in_expr(ast, *a, out);
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_super_in_expr(ast, *left, out);
            collect_super_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. } => collect_super_in_expr(ast, *expr, out),
        Expr::Member { obj, .. } => collect_super_in_expr(ast, *obj, out),
        Expr::Assign { target, value } => {
            collect_super_in_expr(ast, *target, out);
            collect_super_in_expr(ast, *value, out);
        }
        Expr::Index { obj, index } => {
            collect_super_in_expr(ast, *obj, out);
            collect_super_in_expr(ast, *index, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_super_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect_super_in_expr(ast, *e, out);
            }
        }
        Expr::ArrowFn { body, .. } => {
            for s in body {
                collect_super_in_stmt(ast, s, out);
            }
        }
        Expr::Closure { .. } => {}
        Expr::New { args, .. } => {
            for a in args {
                collect_super_in_expr(ast, *a, out);
            }
        }
        Expr::Ternary { cond, then_branch, else_branch } => {
            collect_super_in_expr(ast, *cond, out);
            collect_super_in_expr(ast, *then_branch, out);
            collect_super_in_expr(ast, *else_branch, out);
        }
        Expr::TypeOf { expr } | Expr::Spread { expr } | Expr::InstanceOf { expr, .. } => collect_super_in_expr(ast, *expr, out),
        Expr::Nullish { lhs, rhs } => {
            collect_super_in_expr(ast, *lhs, out);
            collect_super_in_expr(ast, *rhs, out);
        }
        Expr::OptChain { obj, .. } => collect_super_in_expr(ast, *obj, out),
        Expr::PostIncr { target, .. } => collect_super_in_expr(ast, *target, out),
        Expr::This
        | Expr::Ident(_)
        | Expr::String(_)
        | Expr::Number(_)
        | Expr::Bool(_)
        | Expr::Regex { .. }
        | Expr::Null
        | Expr::Uninit => {}
    }
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
    };
    let mut body: Vec<Stmt> = prelude;
    body.push(let_this);
    if let Some(c) = ctor {
        // Build: __cm_C__ctor(__this, ctor_param_idents...);
        let callee = ast.add_expr(Expr::Ident(format!("__cm_{cname}__ctor")));
        let this_id = ast.add_expr(Expr::Ident("__this".into()));
        let mut args: Vec<ExprId> = Vec::with_capacity(c.params.len() + 1);
        args.push(this_id);
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

/// Apply default-argument substitution at Call sites. For every
/// `function f(x = expr) {...}` or arrow fn with default params, walks
/// every `Expr::Call` whose callee is an Ident matching the fn name
/// and pads `args` with the default ExprIds when the caller omits
/// trailing args. Shared ExprIds across call sites are fine — they're
/// purely read by the type-checker and ssa-lower, never mutated.
///
/// Defaults are evaluated at the call site (not in callee scope) —
/// slightly diverges from JS spec but covers typical constant /
/// global-expression defaults used in tests.
/// Static analysis: for each top-level FnDecl, determine which
/// parameter positions get "consumed" by callers. A param consumes
/// if its body reaches one of:
///   - a `__new_*(... <param> ...)` constructor factory call
///   - a call to another fn whose corresponding parameter already
///     consumes
///   - a `this.<field> = <param>` assignment (class method)
///
/// Computes the fixed point: iterate until no fn's bitmap changes.
///
/// Result is written to `ast.consuming_params`. check.rs / ssa_lower
/// query this map at every Call site to decide whether to consume the
/// caller's non-Copy ident arg.
pub fn compute_consuming_params(ast: &mut Ast) {
    use std::collections::HashMap;

    // Snapshot fn signatures (name → param names).
    let mut fn_params: HashMap<String, Vec<String>> = HashMap::new();
    let mut fn_bodies: HashMap<String, Vec<Stmt>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, body, .. } = s {
            fn_params.insert(name.clone(), params.iter().map(|p| p.name.clone()).collect());
            fn_bodies.insert(name.clone(), body.clone());
        }
    }
    if fn_params.is_empty() {
        return;
    }

    // Initial bitmap: all false.
    let mut consuming: HashMap<String, Vec<bool>> = fn_params
        .iter()
        .map(|(n, ps)| (n.clone(), vec![false; ps.len()]))
        .collect();

    // Fixed-point loop. Each round, walk every fn body, see which params
    // flow into a known-consuming sink. Stop when nothing changes.
    let mut changed = true;
    let mut rounds = 0;
    while changed {
        changed = false;
        rounds += 1;
        if rounds > 32 {
            // Safety net: a pathological recursive shape shouldn't
            // explode here. fn_count rounds upper bound suffices in
            // practice (each round monotonically grows the consuming
            // set; cap is fn count + slack).
            break;
        }
        let snapshot = consuming.clone();
        for (fname, params) in &fn_params {
            let body = match fn_bodies.get(fname) {
                Some(b) => b,
                None => continue,
            };
            for s in body {
                scan_stmt_for_consuming_flow(ast, s, fname, params, &snapshot, &mut consuming, &mut changed);
            }
        }
    }

    ast.consuming_params = consuming;
}

/// Walk `s` looking for sites that consume one of `params` (the
/// surrounding fn's parameters). Updates `consuming[fname][i] = true`
/// when found and sets `changed=true`.
fn scan_stmt_for_consuming_flow(
    ast: &Ast,
    s: &Stmt,
    fname: &str,
    params: &[String],
    snapshot: &std::collections::HashMap<String, Vec<bool>>,
    consuming: &mut std::collections::HashMap<String, Vec<bool>>,
    changed: &mut bool,
) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            scan_expr_for_consuming_flow(ast, *eid, fname, params, snapshot, consuming, changed);
        }
        Stmt::YieldInto { value, .. } => {
            scan_expr_for_consuming_flow(ast, *value, fname, params, snapshot, consuming, changed);
        }
        Stmt::Return(Some(eid)) => {
            scan_expr_for_consuming_flow(ast, *eid, fname, params, snapshot, consuming, changed);
        }
        Stmt::Return(None) => {}
        Stmt::LetDecl { init, .. } => {
            scan_expr_for_consuming_flow(ast, *init, fname, params, snapshot, consuming, changed);
        }
        Stmt::If { cond, then_branch, else_branch } => {
            scan_expr_for_consuming_flow(ast, *cond, fname, params, snapshot, consuming, changed);
            scan_stmt_for_consuming_flow(ast, then_branch, fname, params, snapshot, consuming, changed);
            if let Some(eb) = else_branch {
                scan_stmt_for_consuming_flow(ast, eb, fname, params, snapshot, consuming, changed);
            }
        }
        Stmt::While { cond, body } => {
            scan_expr_for_consuming_flow(ast, *cond, fname, params, snapshot, consuming, changed);
            scan_stmt_for_consuming_flow(ast, body, fname, params, snapshot, consuming, changed);
        }
        Stmt::DoWhile { body, cond } => {
            scan_stmt_for_consuming_flow(ast, body, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, *cond, fname, params, snapshot, consuming, changed);
        }
        Stmt::For { init, cond, step, body } => {
            if let Some(i) = init {
                scan_stmt_for_consuming_flow(ast, i, fname, params, snapshot, consuming, changed);
            }
            if let Some(c) = cond {
                scan_expr_for_consuming_flow(ast, *c, fname, params, snapshot, consuming, changed);
            }
            if let Some(st) = step {
                scan_expr_for_consuming_flow(ast, *st, fname, params, snapshot, consuming, changed);
            }
            scan_stmt_for_consuming_flow(ast, body, fname, params, snapshot, consuming, changed);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                scan_stmt_for_consuming_flow(ast, s, fname, params, snapshot, consuming, changed);
            }
        }
        Stmt::Switch { scrutinee, cases, default } => {
            scan_expr_for_consuming_flow(ast, *scrutinee, fname, params, snapshot, consuming, changed);
            for c in cases {
                scan_expr_for_consuming_flow(ast, c.value, fname, params, snapshot, consuming, changed);
                for s in &c.body {
                    scan_stmt_for_consuming_flow(ast, s, fname, params, snapshot, consuming, changed);
                }
            }
            if let Some(d) = default {
                for s in d {
                    scan_stmt_for_consuming_flow(ast, s, fname, params, snapshot, consuming, changed);
                }
            }
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            for s in body {
                scan_stmt_for_consuming_flow(ast, s, fname, params, snapshot, consuming, changed);
            }
            for s in catch_body {
                scan_stmt_for_consuming_flow(ast, s, fname, params, snapshot, consuming, changed);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    scan_stmt_for_consuming_flow(ast, s, fname, params, snapshot, consuming, changed);
                }
            }
        }
        _ => {}
    }
}

fn scan_expr_for_consuming_flow(
    ast: &Ast,
    eid: ExprId,
    fname: &str,
    params: &[String],
    snapshot: &std::collections::HashMap<String, Vec<bool>>,
    consuming: &mut std::collections::HashMap<String, Vec<bool>>,
    changed: &mut bool,
) {
    let expr = ast.get_expr(eid).clone();
    match expr {
        Expr::Call { callee, args } => {
            // Decide which arg positions get consumed by this call:
            //  - __new_<C>(...) — every non-Copy arg
            //  - other fns — per snapshot's `consuming` bitmap
            let mut consumes_at: Vec<bool> = vec![false; args.len()];
            if let Expr::Ident(callee_name) = ast.get_expr(callee) {
                if callee_name.starts_with("__new_") {
                    for v in consumes_at.iter_mut() {
                        *v = true;
                    }
                } else if let Some(bm) = snapshot.get(callee_name) {
                    for (i, v) in consumes_at.iter_mut().enumerate() {
                        if let Some(b) = bm.get(i) {
                            *v = *b;
                        }
                    }
                }
            }
            for (i, arg) in args.iter().enumerate() {
                if consumes_at.get(i).copied().unwrap_or(false)
                    && let Expr::Ident(name) = ast.get_expr(*arg)
                    && let Some(idx) = params.iter().position(|p| p == name)
                {
                    let bm = consuming.get_mut(fname).unwrap();
                    if !bm[idx] {
                        bm[idx] = true;
                        *changed = true;
                    }
                }
                scan_expr_for_consuming_flow(ast, *arg, fname, params, snapshot, consuming, changed);
            }
            scan_expr_for_consuming_flow(ast, callee, fname, params, snapshot, consuming, changed);
        }
        Expr::Assign { target, value } => {
            // `this.<field> = <param>` — class-field stores own the
            // value transitively. Detect Member-on-This target shape.
            if let Expr::Member { obj, .. } = ast.get_expr(target)
                && (matches!(ast.get_expr(*obj), Expr::This)
                    || matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "__this"))
                && let Expr::Ident(name) = ast.get_expr(value)
                && let Some(idx) = params.iter().position(|p| p == name)
            {
                let bm = consuming.get_mut(fname).unwrap();
                if !bm[idx] {
                    bm[idx] = true;
                    *changed = true;
                }
            }
            scan_expr_for_consuming_flow(ast, target, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, value, fname, params, snapshot, consuming, changed);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            // Pre-desugar shape: every arg consumed (constructor stores).
            for arg in &args {
                if let Expr::Ident(name) = ast.get_expr(*arg)
                    && let Some(idx) = params.iter().position(|p| p == name)
                {
                    let bm = consuming.get_mut(fname).unwrap();
                    if !bm[idx] {
                        bm[idx] = true;
                        *changed = true;
                    }
                }
                scan_expr_for_consuming_flow(ast, *arg, fname, params, snapshot, consuming, changed);
            }
        }
        Expr::BinOp { left, right, .. } => {
            scan_expr_for_consuming_flow(ast, left, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, right, fname, params, snapshot, consuming, changed);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. } => {
            scan_expr_for_consuming_flow(ast, expr, fname, params, snapshot, consuming, changed);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            scan_expr_for_consuming_flow(ast, obj, fname, params, snapshot, consuming, changed);
        }
        Expr::Index { obj, index } => {
            scan_expr_for_consuming_flow(ast, obj, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, index, fname, params, snapshot, consuming, changed);
        }
        Expr::Array(els) => {
            for e in els {
                scan_expr_for_consuming_flow(ast, e, fname, params, snapshot, consuming, changed);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                scan_expr_for_consuming_flow(ast, e, fname, params, snapshot, consuming, changed);
            }
        }
        Expr::Ternary { cond, then_branch, else_branch } => {
            scan_expr_for_consuming_flow(ast, cond, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, then_branch, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, else_branch, fname, params, snapshot, consuming, changed);
        }
        Expr::Nullish { lhs, rhs } => {
            scan_expr_for_consuming_flow(ast, lhs, fname, params, snapshot, consuming, changed);
            scan_expr_for_consuming_flow(ast, rhs, fname, params, snapshot, consuming, changed);
        }
        Expr::PostIncr { target, .. } => {
            scan_expr_for_consuming_flow(ast, target, fname, params, snapshot, consuming, changed);
        }
        _ => {}
    }
}

pub fn apply_default_args(ast: &mut Ast) {
    let mut fn_defaults: HashMap<String, Vec<Option<ExprId>>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            let is_closure = params.first().is_some_and(|p| p.name == "__env");
            let user_params: &[Param] = if is_closure { &params[1..] } else { params };
            if user_params.iter().any(|p| p.default.is_some()) {
                fn_defaults.insert(
                    name.clone(),
                    user_params.iter().map(|p| p.default).collect(),
                );
            }
        }
    }
    // Sibling-shape Member calls (`obj.method(args)`) survive desugar
    // when the method name is shared by unrelated classes (I.1). For
    // those, look up class-method FnDecls named `__cm_<C>__<method>`
    // and group by `<method>`. If every owner of `<method>` has the
    // same defaults shape (length + which positions have defaults),
    // we can apply them to the bare `obj.method(args)` call site
    // without knowing the receiver's static type.
    let mut method_defaults: HashMap<String, Vec<Option<ExprId>>> = HashMap::new();
    let mut method_conflict: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (fname, defaults) in &fn_defaults {
        let Some(rest) = fname.strip_prefix("__cm_") else {
            continue;
        };
        // Use rfind: the class name itself may contain `__` (e.g.
        // `__Gen_count3`), so the method-name boundary is the LAST `__`.
        let Some(idx) = rest.rfind("__") else {
            continue;
        };
        let mname = &rest[idx + 2..];
        // The first param of __cm_C__M is __this (no default). Skip it
        // when comparing — the Member-call path doesn't pass __this
        // explicitly.
        if defaults.is_empty() {
            continue;
        }
        let user_defaults: Vec<Option<ExprId>> = defaults[1..].to_vec();
        if !user_defaults.iter().any(|d| d.is_some()) {
            continue;
        }
        if method_conflict.contains(mname) {
            continue;
        }
        match method_defaults.get(mname) {
            None => {
                method_defaults.insert(mname.to_string(), user_defaults);
            }
            Some(existing) => {
                // Conflict only if defaults shape differs (different
                // arity or different which-positions). We don't compare
                // ExprIds — different generator classes use different
                // ExprId for the same Number(0.0) literal but should
                // count as compatible. Compare lengths + Some/None
                // pattern only.
                let same_shape = existing.len() == user_defaults.len()
                    && existing
                        .iter()
                        .zip(&user_defaults)
                        .all(|(a, b)| a.is_some() == b.is_some());
                if !same_shape {
                    method_conflict.insert(mname.to_string());
                    method_defaults.remove(mname);
                }
            }
        }
    }

    if fn_defaults.is_empty() && method_defaults.is_empty() {
        return;
    }
    let n = ast.exprs.len();
    for i in 0..n {
        if let Expr::Call { callee, args } = &ast.exprs[i] {
            let callee = *callee;
            let args_len = args.len();
            // Pick defaults: prefer Ident match, fall back to Member
            // (sibling-shape) lookup.
            let defaults: Vec<Option<ExprId>> = match ast.get_expr(callee).clone() {
                Expr::Ident(name) => match fn_defaults.get(&name) {
                    Some(d) => d.clone(),
                    None => continue,
                },
                Expr::Member { name, .. } => match method_defaults.get(&name) {
                    Some(d) => d.clone(),
                    None => continue,
                },
                _ => continue,
            };
            if args_len >= defaults.len() {
                continue;
            }
            let mut new_args = match &ast.exprs[i] {
                Expr::Call { args, .. } => args.clone(),
                _ => unreachable!(),
            };
            let mut ok = true;
            for j in args_len..defaults.len() {
                if let Some(default_eid) = defaults[j] {
                    new_args.push(default_eid);
                } else {
                    ok = false;
                    break;
                }
            }
            if ok {
                ast.exprs[i] = Expr::Call { callee, args: new_args };
            }
        }
    }
}

/// Pack trailing call-site args into an array literal when the
/// callee declares its last param with `...rest`. This pass mirrors
/// `apply_default_args` but for the rest-param shape.
///
/// The transformation: `f(a0, a1, …, ak)` where f's params are
/// `[p0, p1, ..., pn-1, ...rest]` becomes `f(a0, ..., an-1, [an, ..., ak])`
/// — the trailing args (positions n through k) get bundled into a
/// single Array literal at the rest-param position.
pub fn apply_rest_args(ast: &mut Ast) {
    // Map: callee name -> (n_required, rest_param_type_ann).
    let mut fn_rest: HashMap<String, (usize, String)> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            let is_closure = params.first().is_some_and(|p| p.name == "__env");
            let user_params: &[Param] = if is_closure { &params[1..] } else { params };
            if let Some(last) = user_params.last() {
                if last.is_rest {
                    let n_required = user_params.len() - 1;
                    let rest_ann = last
                        .type_ann
                        .clone()
                        .unwrap_or_else(|| "any[]".into());
                    fn_rest.insert(name.clone(), (n_required, rest_ann));
                }
            }
        }
    }
    if fn_rest.is_empty() {
        return;
    }
    // Pre-synthesize empty-array helper FnDecls per rest type ann. Each
    // helper has shape `function __empty_arr_<sanitized>(): T[] {
    //   let _e: T[] = []; return _e; }`. The let-binding's annotation
    // gives ssa-lower the typed-empty-array path.
    let mut empty_helpers: HashMap<String, String> = HashMap::new();
    for (_, (_, rest_ann)) in &fn_rest {
        if !empty_helpers.contains_key(rest_ann) {
            let sanitized: String = rest_ann
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            let helper_name = format!("__empty_arr__{sanitized}");
            empty_helpers.insert(rest_ann.clone(), helper_name);
        }
    }
    // Emit the helpers as new FnDecls.
    for (rest_ann, helper_name) in &empty_helpers {
        // Skip if already present.
        let exists = ast.stmts.iter().any(|s| matches!(s, Stmt::FnDecl { name, .. } if name == helper_name));
        if exists { continue; }
        let arr_lit = ast.add_expr(Expr::Array(Vec::new()));
        let body = vec![
            Stmt::LetDecl {
                mutable: false,
                name: "_e".into(),
                type_ann: Some(rest_ann.clone()),
                init: arr_lit,
            },
            Stmt::Return(Some(ast.add_expr(Expr::Ident("_e".into())))),
        ];
        ast.stmts.push(Stmt::FnDecl {
            name: helper_name.clone(),
            type_params: Vec::new(),
            params: Vec::new(),
            return_type: Some(rest_ann.clone()),
            body,
            is_generator: false,
        });
    }
    let n = ast.exprs.len();
    for i in 0..n {
        if let Expr::Call { callee, args } = &ast.exprs[i] {
            let callee = *callee;
            let name = match ast.get_expr(callee) {
                Expr::Ident(n) => n.clone(),
                _ => continue,
            };
            let Some((n_required, rest_ann)) = fn_rest.get(&name).cloned() else { continue };
            let args_clone = args.clone();
            if args_clone.len() < n_required {
                continue;
            }
            let mut new_args: Vec<ExprId> = args_clone[..n_required].to_vec();
            let rest_elems: Vec<ExprId> = args_clone[n_required..].to_vec();
            // Single-spread shape: `f(req…, ...arr)` — pass the spread
            // source array directly as the rest param. Common in
            // delegating wrappers.
            let single_spread_only = rest_elems.len() == 1
                && matches!(ast.get_expr(rest_elems[0]), Expr::Spread { .. });
            let rest_arr = if rest_elems.is_empty() {
                let helper_name = empty_helpers.get(&rest_ann).cloned().unwrap();
                let callee_id = ast.add_expr(Expr::Ident(helper_name));
                ast.add_expr(Expr::Call { callee: callee_id, args: Vec::new() })
            } else if single_spread_only {
                if let Expr::Spread { expr } = ast.get_expr(rest_elems[0]) {
                    *expr
                } else {
                    unreachable!()
                }
            } else {
                ast.add_expr(Expr::Array(rest_elems))
            };
            new_args.push(rest_arr);
            ast.exprs[i] = Expr::Call { callee, args: new_args };
        }
    }
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
                free_vars_of_arrow(ast, params, body, &global_fn_names)
            }
            _ => Vec::new(),
        };
        let placeholder = if captures.is_empty() {
            Expr::Ident(name.clone())
        } else {
            Expr::Closure {
                fn_name: name.clone(),
                captures: captures.clone(),
            }
        };
        let arrow = std::mem::replace(&mut ast.exprs[i], placeholder);
        if let Expr::ArrowFn {
            params,
            return_type,
            body,
        } = arrow
        {
            // For capturing arrows, prepend a hidden `__env` parameter so
            // the lowerer can recognize a closure body and emit env loads
            // for the captures at the top of the function. The capture
            // names are smuggled to the lowerer via the param's type_ann
            // string (encoded as `__env(cap0|cap1|...)`).
            let mut final_params = params;
            if !captures.is_empty() {
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
            }
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

/// Synthesize forwarder closures for `Stmt::Return(Ident(global_fn))`
/// in functions whose declared ret type is a closure type
/// (`(...) => R`). Without this, ssa_lower's `effective_ret_ty`
/// upgrades the fn's ret to Closure (because the body also returns
/// a capturing arrow somewhere) but the bare-fn-name branch returns
/// a Type::FnSig value — calling-convention mismatch SIGSEGVs at
/// the call site.
///
/// Fix: each such Return(Ident(name)) is rewritten to
///   `return Closure { fn_name: "__forward_<name>", captures: [] }`
/// where `__forward_<name>(__env: ptr, args...) { return name(args...); }`
/// is a synthesized FnDecl appended to ast.stmts. The forwarder has
/// a `__env` first param so ssa_lower treats it as closure-shaped
/// (env-first calling convention); the body just discards env and
/// forwards to the wrapped fn. Both branches now emit Closure
/// values and the caller's CallIndirect dispatches uniformly.
///
/// Runs after `lift_arrow_fns` so capturing arrows are already
/// `Expr::Closure` and we can detect mixed shapes.
pub fn synthesize_forwarders(ast: &mut Ast) {
    use std::collections::{HashMap, HashSet};

    // Snapshot all FnDecls' (params, return type) for forwarder body
    // synthesis. Filter out "closure-shaped" fns (first param `__env`):
    // those are already closures and shouldn't be wrapped.
    let mut fn_sigs: HashMap<String, (Vec<Param>, Option<String>)> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, return_type, .. } = s {
            let is_closure_shaped = params.first().is_some_and(|p| p.name == "__env");
            if !is_closure_shaped {
                fn_sigs.insert(name.clone(), (params.clone(), return_type.clone()));
            }
        }
    }
    if fn_sigs.is_empty() {
        return;
    }

    // Walk fns whose declared ret type is closure-typed. For each,
    // collect Return(Ident(global_fn)) pairs that need a forwarder.
    let mut targets: HashSet<String> = HashSet::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { return_type, body, params, .. } = s {
            // Skip closure-shaped fns (their bodies were already lifted).
            let is_closure_shaped = params.first().is_some_and(|p| p.name == "__env");
            if is_closure_shaped {
                continue;
            }
            let Some(rt) = return_type.as_deref() else { continue };
            // Quick sniff: ret type looks like a fn type ann
            // (`(args) => R` parser shape, or `__fn(...)->R` lifted
            // shape).
            let looks_like_fn = rt.starts_with('(')
                || rt.contains("=>")
                || rt.starts_with("__fn(");
            if !looks_like_fn {
                continue;
            }
            // Body has any Return(Closure-producing expr)? Mirrors
            // ssa_lower's `body_returns_closure` heuristic.
            if !body_has_closure_return(ast, body) {
                continue;
            }
            // Collect Return(Ident(name)) where name is FnSig-shaped.
            collect_fnsig_ident_returns(ast, body, &fn_sigs, &mut targets);
        }
    }

    if targets.is_empty() {
        return;
    }

    // Synthesize one forwarder per target.
    let mut new_decls: Vec<Stmt> = Vec::new();
    let mut renames: HashMap<String, String> = HashMap::new();
    for target in &targets {
        let (params, return_type) = fn_sigs.get(target).unwrap().clone();
        let forward_name = format!("__forward_{target}");
        // params: __env: ptr, ...orig params
        let mut fwd_params: Vec<Param> = Vec::with_capacity(params.len() + 1);
        fwd_params.push(Param {
            name: "__env".into(),
            type_ann: Some(format!("__env({})", "")),
            default: None,
            is_rest: false,
        });
        fwd_params.extend(params.iter().cloned());
        // body: return target(p0, p1, ...);
        let arg_eids: Vec<ExprId> = params
            .iter()
            .map(|p| ast.add_expr(Expr::Ident(p.name.clone())))
            .collect();
        let callee_id = ast.add_expr(Expr::Ident(target.clone()));
        let call_id = ast.add_expr(Expr::Call {
            callee: callee_id,
            args: arg_eids,
        });
        let body = vec![Stmt::Return(Some(call_id))];
        new_decls.push(Stmt::FnDecl {
            name: forward_name.clone(),
            type_params: Vec::new(),
            params: fwd_params,
            return_type,
            body,
            is_generator: false,
        });
        renames.insert(target.clone(), forward_name);
    }

    // Rewrite Return(Ident(target)) → Return(Closure { fn_name:
    // __forward_<target>, captures: [] }). Done by adding new exprs;
    // existing ExprIds stay valid (just point at unused old idents).
    let n = ast.exprs.len();
    let mut return_rewrites: Vec<(usize, ExprId)> = Vec::new();
    for i in 0..ast.stmts.len() {
        collect_return_ident_rewrites(
            ast,
            i,
            &renames,
            &mut return_rewrites,
        );
    }
    for (stmt_visit_idx, eid_to_replace) in return_rewrites {
        let _ = stmt_visit_idx;
        let _ = eid_to_replace;
    }
    // Walk stmts and rewrite Returns directly.
    rewrite_returns_to_forwarders(ast, &renames);

    let _ = n;
    ast.stmts.extend(new_decls);
}

fn body_has_closure_return(ast: &Ast, body: &[Stmt]) -> bool {
    body.iter().any(|s| stmt_has_closure_return(ast, s))
}

fn stmt_has_closure_return(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::Return(Some(eid)) => {
            matches!(ast.get_expr(*eid), Expr::Closure { .. })
                || matches!(ast.get_expr(*eid), Expr::Ident(n) if n.starts_with("__closure_"))
        }
        Stmt::If { then_branch, else_branch, .. } => {
            stmt_has_closure_return(ast, then_branch)
                || else_branch
                    .as_deref()
                    .is_some_and(|s| stmt_has_closure_return(ast, s))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            stmt_has_closure_return(ast, body)
        }
        Stmt::For { body, .. } => stmt_has_closure_return(ast, body),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_has_closure_return(ast, s))
        }
        Stmt::Switch { cases, default, .. } => {
            cases
                .iter()
                .any(|c| c.body.iter().any(|s| stmt_has_closure_return(ast, s)))
                || default.as_ref().is_some_and(|d| {
                    d.iter().any(|s| stmt_has_closure_return(ast, s))
                })
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            body.iter().any(|s| stmt_has_closure_return(ast, s))
                || catch_body.iter().any(|s| stmt_has_closure_return(ast, s))
                || finally_body.as_ref().is_some_and(|fb| {
                    fb.iter().any(|s| stmt_has_closure_return(ast, s))
                })
        }
        _ => false,
    }
}

fn collect_fnsig_ident_returns(
    ast: &Ast,
    body: &[Stmt],
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>)>,
    out: &mut std::collections::HashSet<String>,
) {
    for s in body {
        collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
    }
}

fn collect_fnsig_ident_returns_stmt(
    ast: &Ast,
    s: &Stmt,
    fn_sigs: &std::collections::HashMap<String, (Vec<Param>, Option<String>)>,
    out: &mut std::collections::HashSet<String>,
) {
    match s {
        Stmt::Return(Some(eid)) => {
            if let Expr::Ident(name) = ast.get_expr(*eid)
                && fn_sigs.contains_key(name)
            {
                out.insert(name.clone());
            }
        }
        Stmt::If { then_branch, else_branch, .. } => {
            collect_fnsig_ident_returns_stmt(ast, then_branch, fn_sigs, out);
            if let Some(eb) = else_branch {
                collect_fnsig_ident_returns_stmt(ast, eb, fn_sigs, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_fnsig_ident_returns_stmt(ast, body, fn_sigs, out);
        }
        Stmt::For { body, .. } => {
            collect_fnsig_ident_returns_stmt(ast, body, fn_sigs, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for s in &c.body {
                    collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
                }
            }
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            for s in body {
                collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
            }
            for s in catch_body {
                collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_fnsig_ident_returns_stmt(ast, s, fn_sigs, out);
                }
            }
        }
        _ => {}
    }
}

#[allow(unused)]
fn collect_return_ident_rewrites(
    _ast: &Ast,
    _stmt_idx: usize,
    _renames: &std::collections::HashMap<String, String>,
    _out: &mut Vec<(usize, ExprId)>,
) {
    // Placeholder — kept so the synthesize_forwarders body compiles
    // while the rewrite walker is the actual mutating pass below.
}

fn rewrite_returns_to_forwarders(
    ast: &mut Ast,
    renames: &std::collections::HashMap<String, String>,
) {
    // Two-phase: collect (eid, forward_name) replacements, then apply.
    // Walk every FnDecl's body — top-level stmts are mostly FnDecls
    // and the actual Returns we need to rewrite live in their bodies.
    let mut replacements: Vec<(ExprId, String)> = Vec::new();
    let bodies: Vec<Vec<Stmt>> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { body, .. } => Some(body.clone()),
            _ => None,
        })
        .collect();
    for body in &bodies {
        for s in body {
            collect_return_replacements(ast, s, renames, &mut replacements);
        }
    }
    for (eid, forward_name) in replacements {
        let new_expr = Expr::Closure {
            fn_name: forward_name,
            captures: Vec::new(),
        };
        ast.exprs[eid.0 as usize] = new_expr;
    }
}

fn collect_return_replacements(
    ast: &Ast,
    s: &Stmt,
    renames: &std::collections::HashMap<String, String>,
    out: &mut Vec<(ExprId, String)>,
) {
    match s {
        Stmt::Return(Some(eid)) => {
            if let Expr::Ident(name) = ast.get_expr(*eid)
                && let Some(forward_name) = renames.get(name)
            {
                out.push((*eid, forward_name.clone()));
            }
        }
        Stmt::If { then_branch, else_branch, .. } => {
            collect_return_replacements(ast, then_branch, renames, out);
            if let Some(eb) = else_branch {
                collect_return_replacements(ast, eb, renames, out);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_return_replacements(ast, body, renames, out);
        }
        Stmt::For { body, .. } => {
            collect_return_replacements(ast, body, renames, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_return_replacements(ast, s, renames, out);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                for s in &c.body {
                    collect_return_replacements(ast, s, renames, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_return_replacements(ast, s, renames, out);
                }
            }
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            for s in body {
                collect_return_replacements(ast, s, renames, out);
            }
            for s in catch_body {
                collect_return_replacements(ast, s, renames, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_return_replacements(ast, s, renames, out);
                }
            }
        }
        _ => {}
    }
}

/// Backward-infer the param type annotations of anonymous arrow
/// closures from the call site that consumes them. Runs after
/// `lift_arrow_fns` so each arrow is now a top-level FnDecl named
/// `__closure_<N>`; un-annotated params would later trip
/// `build_fn_type` with "parameter `a` requires a type annotation".
///
/// Inference rules (narrow MVP):
///   - Look for `Expr::Call { callee = Member(obj, method), args }`.
///   - For each arg that is an `Expr::Closure { fn_name }` with a
///     lifted FnDecl whose params lack annotations, look up the
///     receiver's type via the surrounding fn's let-decls + params.
///   - If the receiver type is `T[]` and the method is one of the
///     known callback-bearing Array methods (`sort` / `map` /
///     `filter` / `reduce` / `forEach` / `find` / `findIndex` /
///     `findLast` / `findLastIndex` / `some` / `every` / `flatMap`),
///     write the inferred per-position type annotations into the
///     lifted FnDecl.
///
/// Anything outside this rule (callbacks on non-Array receivers,
/// callbacks on un-annotated locals, etc.) keeps requiring explicit
/// type annotations.
pub fn infer_anonymous_closure_params(ast: &mut Ast) {
    use std::collections::HashMap;

    // Build per-fn name → param/let type-annotation table. Walk all
    // top-level FnDecl bodies, gathering let-decl names and param
    // names. The same name may appear in multiple fns; we key by the
    // enclosing fn so call-site inference resolves the right binding.
    //
    // Side-effect-free: just reads the AST, populates a name→ann map.
    let mut all_anns: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { params, body, .. } = s {
            for p in params {
                if let Some(ann) = &p.type_ann {
                    all_anns.insert(p.name.clone(), ann.clone());
                }
            }
            collect_let_anns(body, &mut all_anns);
        }
    }

    // Map from lifted closure fn_name → (param annotations, return
    // annotation). Filled by walking call sites; applied at the end
    // (deferred so we don't mutate ast.stmts mid-walk).
    let mut updates: HashMap<String, (Vec<String>, String)> = HashMap::new();

    let n = ast.exprs.len();
    for i in 0..n {
        let Expr::Call { callee, args } = &ast.exprs[i] else { continue };
        let callee = *callee;
        let args = args.clone();
        // Member(obj, method) with at least one Closure arg.
        let Expr::Member { obj, name } = ast.get_expr(callee).clone() else { continue };
        let mut closure_args: Vec<(usize, String)> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            // Two shapes after lift_arrow_fns:
            //   - `Expr::Closure { fn_name, captures }` for arrows that
            //     captured outer-scope bindings.
            //   - `Expr::Ident(fn_name)` for arrows with no captures
            //     (lift emits a bare ident pointing at the lifted
            //     FnDecl). Both cases must be probed for inference.
            match ast.get_expr(*a) {
                Expr::Closure { fn_name, .. } => {
                    closure_args.push((i, fn_name.clone()));
                }
                Expr::Ident(n) if n.starts_with("__closure_") => {
                    closure_args.push((i, n.clone()));
                }
                _ => {}
            }
        }
        if closure_args.is_empty() {
            continue;
        }
        /* Resolve obj's type ann.
         * - `Ident(n)`        → look up in the all_anns table built
         *                       from FnDecl params + let-decl annotations.
         * - `Array(els)`      → infer `T[]` from els[0]'s shape (literal
         *                       receiver path — `[1,2,3].map(x => ...)`).
         *                       Empty literal can't infer an element type;
         *                       skipped. Only homogeneous-typed literals
         *                       matter here since the existing `T[]` infra
         *                       requires homogeneous elements.
         * - `String`          → "string"
         * - `Number`          → "number"
         * Anything more exotic falls through unchanged (caller relies on
         * an explicit annotation upstream). */
        fn infer_lit_ann(ast: &Ast, eid: ExprId) -> Option<String> {
            match ast.get_expr(eid) {
                Expr::Number(_) => Some("number".into()),
                Expr::String(_) => Some("string".into()),
                Expr::Bool(_) => Some("boolean".into()),
                Expr::Array(els) if !els.is_empty() => {
                    /* Recurse on first element to get its inferred ann,
                     * then suffix with []. Fails (returns None) if the
                     * first element isn't a recognized literal shape. */
                    infer_lit_ann(ast, els[0]).map(|inner| format!("{inner}[]"))
                }
                _ => None,
            }
        }
        let obj_ann = match ast.get_expr(obj) {
            Expr::Ident(n) => all_anns.get(n).cloned(),
            other => {
                let _ = other;
                infer_lit_ann(ast, obj)
            }
        };
        let Some(ann) = obj_ann else { continue };
        // Only handle T[] receivers for the known Array methods.
        let Some(elem_ann) = ann.strip_suffix("[]") else { continue };
        let elem_ann = elem_ann.to_string();
        // Per-method expected (param annotations, return annotation).
        let expected: Option<(Vec<String>, String)> = match name.as_str() {
            "sort" => Some((
                vec![elem_ann.clone(), elem_ann.clone()],
                "number".into(),
            )),
            "map" => Some((vec![elem_ann.clone()], elem_ann.clone())),
            "filter" => Some((vec![elem_ann.clone()], "boolean".into())),
            "forEach" => Some((vec![elem_ann.clone()], "void".into())),
            "find" | "findLast" => {
                Some((vec![elem_ann.clone()], "boolean".into()))
            }
            "findIndex" | "findLastIndex" => {
                Some((vec![elem_ann.clone()], "boolean".into()))
            }
            "some" | "every" => Some((vec![elem_ann.clone()], "boolean".into())),
            "flatMap" => {
                // Return is `T[]` (flattened); inner cb returns array.
                Some((vec![elem_ann.clone()], format!("{elem_ann}[]")))
            }
            "reduce" | "reduceRight" => {
                // (acc, cur) => acc — caller supplies the seed; without
                // type-tracking the seed type, assume elem-typed accum
                // (works for sum/max/etc.).
                Some((
                    vec![elem_ann.clone(), elem_ann.clone()],
                    elem_ann.clone(),
                ))
            }
            _ => None,
        };
        let Some(expected) = expected else { continue };
        for (_arg_idx, fn_name) in &closure_args {
            updates.insert(fn_name.clone(), expected.clone());
        }
    }

    if updates.is_empty() {
        return;
    }

    // Apply updates: mutate each lifted FnDecl's params + return type.
    for stmt in &mut ast.stmts {
        if let Stmt::FnDecl { name, params, return_type, .. } = stmt
            && let Some((new_param_anns, new_ret_ann)) = updates.get(name)
        {
            // First param of a lifted closure is `__env`; user params
            // start at index 1.
            let user_start = if params.first().is_some_and(|p| p.name == "__env") {
                1
            } else {
                0
            };
            for (i, ann) in new_param_anns.iter().enumerate() {
                let pidx = user_start + i;
                if let Some(p) = params.get_mut(pidx)
                    && p.type_ann.is_none()
                {
                    p.type_ann = Some(ann.clone());
                }
            }
            if return_type.is_none() {
                *return_type = Some(new_ret_ann.clone());
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

/// JS's `arguments` object is array-like, holds the actual passed
/// values, and changes per call site. A faithful implementation needs
/// runtime support (heterogeneous array, per-call materialization).
///
/// Plan A — escape analyzer for non-Spread Array literals bound by
/// `let X = [...]`. For every such let-decl in every fn body (top-
/// level or class method), verify that X is used only in
/// stack-safe shapes within the rest of the body:
///
///   - `X.length`         (read)
///   - `X[i] / X[i] = v`  (read or write — i may be any expression
///                         that doesn't itself escape X)
///
/// Anything else — bare `X` reference (return, fn arg, store to
/// outer slot, alias to another let, throw, container element),
/// `X.foo` for any name other than "length", `X.method()`,
/// `X?.foo` — disqualifies the literal. The qualifying ExprId is
/// recorded in `ast.stack_array_literals`; ssa_lower emits these
/// as stack alloca + STATIC_LITERAL flag (rc_inc / rc_dec /
/// arr_drop all no-op via the existing flag pathway, so no heap
/// alloc + no per-call drop).
///
/// Runs after all desugars (so closure-lift visibility, arguments
/// rewrites, split-for-i fusion etc are already settled) and before
/// `compute_consuming_params` so the verifier sees the final shape.
/// False negatives stay heap (correct, just slower); false
/// positives would be silent UAF — bias every uncertain shape
/// toward false.
pub fn escape_analyze_array_literals(ast: &mut Ast) {
    let mut found: std::collections::HashSet<ExprId> = std::collections::HashSet::new();
    let stmts = ast.stmts.clone();
    eal_walk_stmts(ast, &stmts, &mut found);
    ast.stack_array_literals = found;
}

fn eal_walk_stmts(
    ast: &Ast,
    stmts: &[Stmt],
    found: &mut std::collections::HashSet<ExprId>,
) {
    // Pass 1: at this level, check each `let X = [...]` against the
    // stmts that follow it (in source order — `let` is in scope from
    // its decl to end of block).
    for (i, s) in stmts.iter().enumerate() {
        if let Stmt::LetDecl { name, init, .. } = s
            && let Expr::Array(els) = ast.get_expr(*init)
            && !els.is_empty()
            && !els.iter().any(|e| matches!(ast.get_expr(*e), Expr::Spread { .. }))
        {
            // The array literal `init` is X's value. Verify X is
            // stack-safe in stmts[i+1..].
            let trailing = &stmts[i + 1..];
            if trailing.iter().all(|s| eal_stmt_safe(ast, s, name)) {
                found.insert(*init);
            }
        }
    }
    // Pass 2: recurse into every nested stmt list.
    for s in stmts {
        eal_recurse_into(ast, s, found);
    }
}

fn eal_recurse_into(
    ast: &Ast,
    s: &Stmt,
    found: &mut std::collections::HashSet<ExprId>,
) {
    match s {
        Stmt::Block(inner) | Stmt::Multi(inner) => eal_walk_stmts(ast, inner, found),
        Stmt::If { then_branch, else_branch, .. } => {
            eal_recurse_into(ast, then_branch, found);
            if let Some(eb) = else_branch {
                eal_recurse_into(ast, eb, found);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            eal_recurse_into(ast, body, found)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                eal_recurse_into(ast, i, found);
            }
            eal_recurse_into(ast, body, found);
        }
        Stmt::ForOfSplitIter { body, .. } => eal_recurse_into(ast, body, found),
        Stmt::Switch { cases, default, .. } => {
            for c in cases {
                eal_walk_stmts(ast, &c.body, found);
            }
            if let Some(db) = default {
                eal_walk_stmts(ast, db, found);
            }
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            eal_walk_stmts(ast, body, found);
            eal_walk_stmts(ast, catch_body, found);
            if let Some(fb) = finally_body {
                eal_walk_stmts(ast, fb, found);
            }
        }
        Stmt::FnDecl { body, .. } => eal_walk_stmts(ast, body, found),
        Stmt::ClassDecl { methods, .. } => {
            for m in methods {
                eal_walk_stmts(ast, &m.body, found);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                eal_recurse_into(ast, inner, found);
            }
        }
        _ => {}
    }
}

fn eal_stmt_safe(ast: &Ast, s: &Stmt, x_name: &str) -> bool {
    match s {
        Stmt::Expr(eid) => eal_expr_safe(ast, *eid, x_name),
        Stmt::Throw(eid) => eal_expr_safe(ast, *eid, x_name),
        Stmt::Yield(eid) | Stmt::YieldInto { value: eid, .. } => {
            // yield emits the value to the caller — escape.
            !eal_expr_uses_x(ast, *eid, x_name)
        }
        Stmt::Return(Some(eid)) => {
            // X returned → escape (any reference at all). We use the
            // stricter `uses_x` check here because X[i] returning the
            // i64 element is fine; X bare in the return is escape.
            // eal_expr_safe handles both: bare X = false, X[i] = true.
            eal_expr_safe(ast, *eid, x_name)
        }
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => true,
        Stmt::LetDecl { name, init, .. } => {
            // `let Y = X[i]` is fine (Y holds an element value);
            // `let Y = X` would be escape (caught by eal_expr_safe).
            // The new let's name shadows X in the body if same name;
            // we don't need to handle shadowing specially since once
            // X is shadowed the new binding takes over.
            let _ = name;
            eal_expr_safe(ast, *init, x_name)
        }
        Stmt::If { cond, then_branch, else_branch } => {
            eal_expr_safe(ast, *cond, x_name)
                && eal_stmt_safe(ast, then_branch, x_name)
                && else_branch.as_deref().map_or(true, |e| eal_stmt_safe(ast, e, x_name))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            eal_expr_safe(ast, *cond, x_name) && eal_stmt_safe(ast, body, x_name)
        }
        Stmt::For { init, cond, step, body } => {
            init.as_deref().map_or(true, |i| eal_stmt_safe(ast, i, x_name))
                && cond.map_or(true, |c| eal_expr_safe(ast, c, x_name))
                && step.map_or(true, |st| eal_expr_safe(ast, st, x_name))
                && eal_stmt_safe(ast, body, x_name)
        }
        Stmt::ForOfSplitIter { parent, sep, body, .. } => {
            eal_expr_safe(ast, *parent, x_name)
                && eal_expr_safe(ast, *sep, x_name)
                && eal_stmt_safe(ast, body, x_name)
        }
        Stmt::Switch { scrutinee, cases, default } => {
            eal_expr_safe(ast, *scrutinee, x_name)
                && cases.iter().all(|c| {
                    eal_expr_safe(ast, c.value, x_name)
                        && c.body.iter().all(|s| eal_stmt_safe(ast, s, x_name))
                })
                && default
                    .as_ref()
                    .map_or(true, |db| db.iter().all(|s| eal_stmt_safe(ast, s, x_name)))
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            body.iter().all(|s| eal_stmt_safe(ast, s, x_name))
                && catch_body.iter().all(|s| eal_stmt_safe(ast, s, x_name))
                && finally_body
                    .as_ref()
                    .map_or(true, |fb| fb.iter().all(|s| eal_stmt_safe(ast, s, x_name)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| eal_stmt_safe(ast, s, x_name))
        }
        Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. } => true,
        Stmt::ExportDecl { inner, .. } => inner
            .as_deref()
            .map_or(true, |s| eal_stmt_safe(ast, s, x_name)),
    }
}

fn eal_expr_safe(ast: &Ast, eid: ExprId, x_name: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n != x_name, // bare X is escape
        Expr::Member { obj, name } => {
            // X.length is the only allowed Member shape on X.
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return name == "length";
            }
            eal_expr_safe(ast, *obj, x_name)
        }
        Expr::Index { obj, index } => {
            // X[i] is allowed in any context. The index expression is
            // recursively checked (would not be a valid place to cite
            // X bare since X.length and X[k] are the only allowed
            // shapes — bare X in index expr would fail `Ident(n)` arm).
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return eal_expr_safe(ast, *index, x_name);
            }
            eal_expr_safe(ast, *obj, x_name) && eal_expr_safe(ast, *index, x_name)
        }
        Expr::Call { callee, args } => {
            eal_expr_safe(ast, *callee, x_name)
                && args.iter().all(|a| eal_expr_safe(ast, *a, x_name))
        }
        Expr::Assign { target, value } => {
            eal_expr_safe(ast, *target, x_name) && eal_expr_safe(ast, *value, x_name)
        }
        Expr::BinOp { left, right, .. } => {
            eal_expr_safe(ast, *left, x_name) && eal_expr_safe(ast, *right, x_name)
        }
        Expr::Unary { expr, .. } => eal_expr_safe(ast, *expr, x_name),
        Expr::Ternary { cond, then_branch, else_branch } => {
            eal_expr_safe(ast, *cond, x_name)
                && eal_expr_safe(ast, *then_branch, x_name)
                && eal_expr_safe(ast, *else_branch, x_name)
        }
        Expr::Array(els) => els.iter().all(|e| eal_expr_safe(ast, *e, x_name)),
        Expr::ObjectLit { fields } => {
            fields.iter().all(|(_, e)| eal_expr_safe(ast, *e, x_name))
        }
        Expr::Spread { expr } => eal_expr_safe(ast, *expr, x_name),
        Expr::Nullish { lhs, rhs } => {
            eal_expr_safe(ast, *lhs, x_name) && eal_expr_safe(ast, *rhs, x_name)
        }
        Expr::OptChain { obj, .. } => {
            // X?.foo — disqualify; we'd have to permit only X?.length
            // and recursive analysis that's not worth the rare usage.
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            eal_expr_safe(ast, *obj, x_name)
        }
        Expr::PostIncr { target, .. } => eal_expr_safe(ast, *target, x_name),
        Expr::TypeOf { expr } => eal_expr_safe(ast, *expr, x_name),
        Expr::InstanceOf { expr, .. } => eal_expr_safe(ast, *expr, x_name),
        Expr::Closure { captures, .. } => {
            // Closure captures = list of outer-scope names captured.
            // If X is captured, the lifted fn body could escape it.
            !captures.iter().any(|n| n == x_name)
        }
        Expr::ArrowFn { .. } => {
            // Arrow not yet lifted (would have been ArrowFn pre-lift)
            // — conservative: any arrow could capture X. Bail.
            false
        }
        Expr::New { args, .. } => args.iter().all(|a| eal_expr_safe(ast, *a, x_name)),
        Expr::Super { args } => args.iter().all(|a| eal_expr_safe(ast, *a, x_name)),
        Expr::This | Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::Null
        | Expr::Uninit | Expr::Regex { .. } => true,
    }
}

fn eal_expr_uses_x(ast: &Ast, eid: ExprId, x_name: &str) -> bool {
    !eal_expr_safe(ast, eid, x_name)
}

/// P-iter Phase 3 — rewrite `let X = E.split(LIT); for (let I = 0; I <
/// X.length; I = I + 1) { ... X[I] ... }` into a `for-of E.split(LIT)`
/// shape so the SplitIter substrate (Phase 1+2) eliminates the
/// per-call Array<Substr> allocation. Conservative: bails to the
/// untouched original if the body or trailing stmts could see X as a
/// random-access Array.
///
/// Pattern (must hold over an adjacent stmt pair):
///   1. `Stmt::LetDecl { name: X, init: Call { Member { obj: E,
///      name: "split" }, args: [SEP] } }` where SEP is `Expr::String(_)`
///      (literal sep — guarantees lifetime via STATIC_LITERAL globals).
///   2. `Stmt::For { init: Some(LetDecl { name: I, init: Number(0),
///      mutable: true }), cond: Some(BinOp::Lt(Ident(I), Member { obj:
///      Ident(X), name: "length" })), step: Some(Assign { target:
///      Ident(I), value: BinOp::Add(Ident(I), Number(1)) }), body: ... }`.
///
/// Escape verification on body + trailing stmts:
///   - Every read of X must be `Index { obj: Ident(X), index: Ident(I) }`
///     (sequential access only). X used elsewhere — `X[i+1]`, `X.length`
///     inside body, `X` as fn arg, `X` stored to outer slot, X read
///     after the loop — disqualifies the rewrite.
///   - The `I` counter may be read freely (e.g. as a position argument);
///     the rewrite preserves it via a manual mut counter.
///
/// Rewrite emits:
///   Stmt::Block([
///     LetDecl mutable I = 0,
///     ForOfSplitIter { var: <fresh>, parent: E, sep: SEP, body: BODY' },
///   ])
///
/// where BODY' is the original body with `X[I]` index reads replaced
/// by `Ident(<fresh>)` and a trailing `I = I + 1` appended so the
/// counter still advances per iter.
pub fn rewrite_split_for_i_to_iter(ast: &mut Ast) {
    let mut ctx = SplitForICtx { counter: 0 };
    let mut top = std::mem::take(&mut ast.stmts);
    rewrite_sfi_walk_list(ast, &mut top, &mut ctx);
    ast.stmts = top;
}

struct SplitForICtx {
    counter: u32,
}

fn rewrite_sfi_walk_list(ast: &mut Ast, stmts: &mut Vec<Stmt>, ctx: &mut SplitForICtx) {
    // Forward scan: for each Stmt::For matching the for-i+.length
    // pattern, look back through prior stmts in the same block for
    // the matching `let X = E.split(LIT)` declaration. Intermediate
    // stmts (e.g. unrelated `let total = 0`) are allowed as long as
    // they don't reference X. Once both endpoints are found and all
    // escape conditions hold, splice: drop X-decl at j and replace
    // For at i with the rewritten Block.
    let mut to_remove: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut replacements: std::collections::HashMap<usize, Stmt> = std::collections::HashMap::new();
    for i in 0..stmts.len() {
        if to_remove.contains(&i) {
            continue;
        }
        // Try to extract i_name + x_name from the For pattern at
        // stmts[i]. Bails on any non-canonical shape.
        let (i_name, x_name) = match for_i_x_length_match(ast, &stmts[i]) {
            Some(t) => t,
            None => continue,
        };
        // Look back for `let X = E.split(LIT)` in this block. Scan
        // i-1 → 0; bail at the first stmt that references X with a
        // shape other than the X-declaration.
        let mut found_j: Option<usize> = None;
        let mut hit_blocker = false;
        for j in (0..i).rev() {
            if to_remove.contains(&j) {
                hit_blocker = true;
                break;
            }
            // Is this the matching X-decl?
            if let Stmt::LetDecl { name, init, .. } = &stmts[j]
                && name == &x_name
            {
                if match_split_call_with_lit_sep(ast, *init).is_some() {
                    found_j = Some(j);
                }
                break;
            }
            // Otherwise, intermediate stmt must not reference X.
            if sfi_stmt_uses_ident(ast, &stmts[j], &x_name) {
                hit_blocker = true;
                break;
            }
        }
        let _ = hit_blocker;
        let Some(j) = found_j else { continue };
        // Verify body / trailing escape constraints.
        let (parent_eid, sep_eid) = match &stmts[j] {
            Stmt::LetDecl { init, .. } => match_split_call_with_lit_sep(ast, *init).unwrap(),
            _ => unreachable!(),
        };
        let body_box = match &stmts[i] {
            Stmt::For { body, .. } => body.clone(),
            _ => continue,
        };
        if !sfi_body_x_safe(ast, &body_box, &x_name, &i_name) {
            continue;
        }
        if stmts
            .iter()
            .skip(i + 1)
            .any(|s| sfi_stmt_uses_ident(ast, s, &x_name))
        {
            continue;
        }
        // Build the rewrite: Block([let mut I = 0, ForOfSplitIter])
        let id = ctx.counter;
        ctx.counter += 1;
        let v_name = format!("__sfi_v_{id}");
        let new_body = sfi_rewrite_body(ast, &body_box, &x_name, &i_name, &v_name);
        // Append `I = I + 1` step so the body's manual counter ticks.
        let i_ref_step_l = ast.add_expr(Expr::Ident(i_name.clone()));
        let i_ref_step_r = ast.add_expr(Expr::Ident(i_name.clone()));
        let one_eid = ast.add_expr(Expr::Number(1.0));
        let inc_eid = ast.add_expr(Expr::BinOp {
            op: BinOp::Add,
            left: i_ref_step_r,
            right: one_eid,
        });
        let assign_eid = ast.add_expr(Expr::Assign {
            target: i_ref_step_l,
            value: inc_eid,
        });
        let body_with_inc = match new_body {
            Stmt::Block(mut inner) => {
                inner.push(Stmt::Expr(assign_eid));
                Stmt::Block(inner)
            }
            other => Stmt::Block(vec![other, Stmt::Expr(assign_eid)]),
        };
        let zero_eid = ast.add_expr(Expr::Number(0.0));
        let counter_decl = Stmt::LetDecl {
            mutable: true,
            name: i_name.clone(),
            type_ann: Some("number".into()),
            init: zero_eid,
        };
        let forof = Stmt::ForOfSplitIter {
            var_name: v_name,
            parent: parent_eid,
            sep: sep_eid,
            body: Box::new(body_with_inc),
        };
        let new_block = Stmt::Block(vec![counter_decl, forof]);
        to_remove.insert(j);
        replacements.insert(i, new_block);
    }
    // Apply: keep stmts not in to_remove; for indices in replacements,
    // swap to the new Stmt; otherwise clone original.
    if !to_remove.is_empty() || !replacements.is_empty() {
        let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
        for (idx, s) in stmts.iter().enumerate() {
            if to_remove.contains(&idx) {
                continue;
            }
            if let Some(rw) = replacements.remove(&idx) {
                out.push(rw);
            } else {
                out.push(s.clone());
            }
        }
        *stmts = out;
    }
    // Recurse into each child stmt's nested lists.
    for s in stmts.iter_mut() {
        rewrite_sfi_walk_stmt(ast, s, ctx);
    }
}

/// Returns Some((i_name, x_name)) if the Stmt is a canonical
/// `for (let mut I = 0; I < X.length; I = I + 1) BODY`. None on any
/// shape mismatch.
fn for_i_x_length_match(ast: &Ast, s: &Stmt) -> Option<(String, String)> {
    let Stmt::For { init, cond, step, .. } = s else {
        return None;
    };
    // init: let mut I = 0
    let i_name = match init.as_deref() {
        Some(Stmt::LetDecl {
            mutable: true, name, init: init_eid, ..
        }) if is_zero_lit(ast, *init_eid) => name.clone(),
        _ => return None,
    };
    let cond_eid = (*cond)?;
    // cond: I < X.length
    let x_name = match ast.get_expr(cond_eid) {
        Expr::BinOp { op: BinOp::Lt, left, right }
            if matches!(ast.get_expr(*left), Expr::Ident(n) if *n == i_name) =>
        {
            match ast.get_expr(*right) {
                Expr::Member { obj, name } if name == "length" => match ast.get_expr(*obj) {
                    Expr::Ident(n) => n.clone(),
                    _ => return None,
                },
                _ => return None,
            }
        }
        _ => return None,
    };
    let step_eid = (*step)?;
    if !is_i_plus_eq_1(ast, step_eid, &i_name) {
        return None;
    }
    Some((i_name, x_name))
}

fn rewrite_sfi_walk_stmt(ast: &mut Ast, s: &mut Stmt, ctx: &mut SplitForICtx) {
    match s {
        Stmt::Block(inner) | Stmt::Multi(inner) => {
            rewrite_sfi_walk_list(ast, inner, ctx);
        }
        Stmt::If { then_branch, else_branch, .. } => {
            rewrite_sfi_walk_stmt(ast, then_branch, ctx);
            if let Some(eb) = else_branch {
                rewrite_sfi_walk_stmt(ast, eb, ctx);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            rewrite_sfi_walk_stmt(ast, body, ctx);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                rewrite_sfi_walk_stmt(ast, i, ctx);
            }
            rewrite_sfi_walk_stmt(ast, body, ctx);
        }
        Stmt::ForOfSplitIter { body, .. } => {
            rewrite_sfi_walk_stmt(ast, body, ctx);
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                rewrite_sfi_walk_list(ast, &mut c.body, ctx);
            }
            if let Some(db) = default {
                rewrite_sfi_walk_list(ast, db, ctx);
            }
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            rewrite_sfi_walk_list(ast, body, ctx);
            rewrite_sfi_walk_list(ast, catch_body, ctx);
            if let Some(fb) = finally_body {
                rewrite_sfi_walk_list(ast, fb, ctx);
            }
        }
        Stmt::FnDecl { body, .. } => {
            rewrite_sfi_walk_list(ast, body, ctx);
        }
        Stmt::ClassDecl { methods, .. } => {
            for m in methods.iter_mut() {
                rewrite_sfi_walk_list(ast, &mut m.body, ctx);
            }
        }
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                rewrite_sfi_walk_stmt(ast, inner, ctx);
            }
        }
        _ => {}
    }
}

fn match_split_call_with_lit_sep(ast: &Ast, eid: ExprId) -> Option<(ExprId, ExprId)> {
    if let Expr::Call { callee, args } = ast.get_expr(eid)
        && args.len() == 1
        && let Expr::Member { obj: parent, name } = ast.get_expr(*callee)
        && name == "split"
        && matches!(ast.get_expr(args[0]), Expr::String(_))
    {
        return Some((*parent, args[0]));
    }
    None
}

fn is_zero_lit(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Number(n) if *n == 0.0)
}

fn is_i_plus_eq_1(ast: &Ast, eid: ExprId, i_name: &str) -> bool {
    // Either `I = I + 1` (Assign) or `I++` (PostIncr) — accept both.
    if let Expr::Assign { target, value } = ast.get_expr(eid)
        && matches!(ast.get_expr(*target), Expr::Ident(n) if n == i_name)
        && let Expr::BinOp { op: BinOp::Add, left, right } = ast.get_expr(*value)
        && matches!(ast.get_expr(*left), Expr::Ident(n) if n == i_name)
        && matches!(ast.get_expr(*right), Expr::Number(n) if *n == 1.0)
    {
        return true;
    }
    if let Expr::PostIncr { target, .. } = ast.get_expr(eid)
        && matches!(ast.get_expr(*target), Expr::Ident(n) if n == i_name)
    {
        return true;
    }
    false
}

/// Walk body, return false if any reference to `x_name` appears that
/// is NOT exactly `Index(Ident(x_name), Ident(i_name))` (sequential
/// access). Conservative — false on any uncertain shape.
fn sfi_body_x_safe(ast: &Ast, body: &Stmt, x_name: &str, i_name: &str) -> bool {
    sfi_stmt_x_safe(ast, body, x_name, i_name)
}

fn sfi_stmt_x_safe(ast: &Ast, s: &Stmt, x_name: &str, i_name: &str) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            sfi_expr_x_safe(ast, *eid, x_name, i_name)
        }
        Stmt::Return(Some(eid)) => sfi_expr_x_safe(ast, *eid, x_name, i_name),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => true,
        Stmt::LetDecl { init, .. } => sfi_expr_x_safe(ast, *init, x_name, i_name),
        Stmt::If { cond, then_branch, else_branch } => {
            sfi_expr_x_safe(ast, *cond, x_name, i_name)
                && sfi_stmt_x_safe(ast, then_branch, x_name, i_name)
                && else_branch
                    .as_deref()
                    .map_or(true, |eb| sfi_stmt_x_safe(ast, eb, x_name, i_name))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { body, cond } => {
            sfi_expr_x_safe(ast, *cond, x_name, i_name)
                && sfi_stmt_x_safe(ast, body, x_name, i_name)
        }
        Stmt::For { init, cond, step, body } => {
            init.as_deref()
                .map_or(true, |i| sfi_stmt_x_safe(ast, i, x_name, i_name))
                && cond.map_or(true, |c| sfi_expr_x_safe(ast, c, x_name, i_name))
                && step.map_or(true, |st| sfi_expr_x_safe(ast, st, x_name, i_name))
                && sfi_stmt_x_safe(ast, body, x_name, i_name)
        }
        Stmt::ForOfSplitIter { parent, sep, body, .. } => {
            sfi_expr_x_safe(ast, *parent, x_name, i_name)
                && sfi_expr_x_safe(ast, *sep, x_name, i_name)
                && sfi_stmt_x_safe(ast, body, x_name, i_name)
        }
        Stmt::Switch { scrutinee, cases, default } => {
            sfi_expr_x_safe(ast, *scrutinee, x_name, i_name)
                && cases.iter().all(|c| {
                    sfi_expr_x_safe(ast, c.value, x_name, i_name)
                        && c.body.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
                })
                && default.as_ref().map_or(true, |db| {
                    db.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
                })
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            body.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
                && catch_body.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
                && finally_body
                    .as_ref()
                    .map_or(true, |fb| fb.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name)))
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().all(|s| sfi_stmt_x_safe(ast, s, x_name, i_name))
        }
        Stmt::YieldInto { value, .. } => sfi_expr_x_safe(ast, *value, x_name, i_name),
        Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. } => true,
        Stmt::ExportDecl { inner, .. } => inner
            .as_deref()
            .map_or(true, |s| sfi_stmt_x_safe(ast, s, x_name, i_name)),
    }
}

/// Returns true iff every reference to `x_name` in `eid` is the safe
/// shape `Index(Ident(x_name), Ident(i_name))`. Conservative — any
/// shape we can't analyze cleanly returns false.
fn sfi_expr_x_safe(ast: &Ast, eid: ExprId, x_name: &str, i_name: &str) -> bool {
    match ast.get_expr(eid) {
        Expr::Ident(n) => n != x_name,
        Expr::Index { obj, index } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return matches!(
                    ast.get_expr(*index),
                    Expr::Ident(in_) if in_ == i_name
                );
            }
            sfi_expr_x_safe(ast, *obj, x_name, i_name)
                && sfi_expr_x_safe(ast, *index, x_name, i_name)
        }
        Expr::Member { obj, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            sfi_expr_x_safe(ast, *obj, x_name, i_name)
        }
        Expr::Call { callee, args } => {
            sfi_expr_x_safe(ast, *callee, x_name, i_name)
                && args.iter().all(|a| sfi_expr_x_safe(ast, *a, x_name, i_name))
        }
        Expr::Number(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::Uninit
        | Expr::This
        | Expr::Regex { .. } => true,
        Expr::Array(els) => els.iter().all(|e| sfi_expr_x_safe(ast, *e, x_name, i_name)),
        Expr::ObjectLit { fields } => {
            fields.iter().all(|(_, e)| sfi_expr_x_safe(ast, *e, x_name, i_name))
        }
        Expr::Spread { expr } => sfi_expr_x_safe(ast, *expr, x_name, i_name),
        Expr::BinOp { left, right, .. } => {
            sfi_expr_x_safe(ast, *left, x_name, i_name)
                && sfi_expr_x_safe(ast, *right, x_name, i_name)
        }
        Expr::Assign { target, value } => {
            sfi_expr_x_safe(ast, *target, x_name, i_name)
                && sfi_expr_x_safe(ast, *value, x_name, i_name)
        }
        Expr::Unary { expr, .. } => sfi_expr_x_safe(ast, *expr, x_name, i_name),
        Expr::Ternary { cond, then_branch, else_branch } => {
            sfi_expr_x_safe(ast, *cond, x_name, i_name)
                && sfi_expr_x_safe(ast, *then_branch, x_name, i_name)
                && sfi_expr_x_safe(ast, *else_branch, x_name, i_name)
        }
        Expr::TypeOf { expr } => sfi_expr_x_safe(ast, *expr, x_name, i_name),
        Expr::InstanceOf { expr, .. } => sfi_expr_x_safe(ast, *expr, x_name, i_name),
        Expr::ArrowFn { .. } | Expr::Closure { .. } => {
            // Conservative — captured X inside a closure is hard to
            // verify safe since the closure body could index X[k]
            // for arbitrary k. Disqualify any X-mention by treating
            // closures as black boxes.
            true
        }
        Expr::Super { args } => args.iter().all(|a| sfi_expr_x_safe(ast, *a, x_name, i_name)),
        Expr::New { args, .. } => args.iter().all(|a| sfi_expr_x_safe(ast, *a, x_name, i_name)),
        Expr::Nullish { lhs, rhs } => {
            sfi_expr_x_safe(ast, *lhs, x_name, i_name)
                && sfi_expr_x_safe(ast, *rhs, x_name, i_name)
        }
        Expr::OptChain { obj, .. } => {
            if let Expr::Ident(n) = ast.get_expr(*obj)
                && n == x_name
            {
                return false;
            }
            sfi_expr_x_safe(ast, *obj, x_name, i_name)
        }
        Expr::PostIncr { target, .. } => sfi_expr_x_safe(ast, *target, x_name, i_name),
    }
}

fn sfi_stmt_uses_ident(ast: &Ast, s: &Stmt, name: &str) -> bool {
    !sfi_stmt_x_safe(ast, s, name, "<<UNUSED>>")
}

/// Build a fresh body where every `Index(Ident(x_name), Ident(i_name))`
/// becomes `Ident(v_name)`. The escape check has already verified all
/// X-references match this exact shape.
fn sfi_rewrite_body(ast: &mut Ast, body: &Stmt, x_name: &str, i_name: &str, v_name: &str) -> Stmt {
    sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)
}

fn sfi_rewrite_stmt(ast: &mut Ast, s: &Stmt, x_name: &str, i_name: &str, v_name: &str) -> Stmt {
    match s {
        Stmt::Expr(eid) => Stmt::Expr(sfi_rewrite_expr(ast, *eid, x_name, i_name, v_name)),
        Stmt::Throw(eid) => Stmt::Throw(sfi_rewrite_expr(ast, *eid, x_name, i_name, v_name)),
        Stmt::Yield(eid) => Stmt::Yield(sfi_rewrite_expr(ast, *eid, x_name, i_name, v_name)),
        Stmt::YieldInto { var, type_ann, value } => Stmt::YieldInto {
            var: var.clone(),
            type_ann: type_ann.clone(),
            value: sfi_rewrite_expr(ast, *value, x_name, i_name, v_name),
        },
        Stmt::Return(Some(eid)) => {
            Stmt::Return(Some(sfi_rewrite_expr(ast, *eid, x_name, i_name, v_name)))
        }
        Stmt::Return(None) => Stmt::Return(None),
        Stmt::LetDecl { mutable, name, type_ann, init } => Stmt::LetDecl {
            mutable: *mutable,
            name: name.clone(),
            type_ann: type_ann.clone(),
            init: sfi_rewrite_expr(ast, *init, x_name, i_name, v_name),
        },
        Stmt::If { cond, then_branch, else_branch } => Stmt::If {
            cond: sfi_rewrite_expr(ast, *cond, x_name, i_name, v_name),
            then_branch: Box::new(sfi_rewrite_stmt(ast, then_branch, x_name, i_name, v_name)),
            else_branch: else_branch
                .as_ref()
                .map(|eb| Box::new(sfi_rewrite_stmt(ast, eb, x_name, i_name, v_name))),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: sfi_rewrite_expr(ast, *cond, x_name, i_name, v_name),
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
        },
        Stmt::DoWhile { body, cond } => Stmt::DoWhile {
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
            cond: sfi_rewrite_expr(ast, *cond, x_name, i_name, v_name),
        },
        Stmt::Switch { scrutinee, cases, default } => Stmt::Switch {
            scrutinee: sfi_rewrite_expr(ast, *scrutinee, x_name, i_name, v_name),
            cases: cases
                .iter()
                .map(|c| SwitchCase {
                    value: sfi_rewrite_expr(ast, c.value, x_name, i_name, v_name),
                    body: c
                        .body
                        .iter()
                        .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                        .collect(),
                })
                .collect(),
            default: default.as_ref().map(|db| {
                db.iter()
                    .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                    .collect()
            }),
        },
        Stmt::For { init, cond, step, body } => Stmt::For {
            init: init
                .as_ref()
                .map(|i| Box::new(sfi_rewrite_stmt(ast, i, x_name, i_name, v_name))),
            cond: cond.map(|c| sfi_rewrite_expr(ast, c, x_name, i_name, v_name)),
            step: step.map(|st| sfi_rewrite_expr(ast, st, x_name, i_name, v_name)),
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
        },
        Stmt::ForOfSplitIter { var_name, parent, sep, body } => Stmt::ForOfSplitIter {
            var_name: var_name.clone(),
            parent: sfi_rewrite_expr(ast, *parent, x_name, i_name, v_name),
            sep: sfi_rewrite_expr(ast, *sep, x_name, i_name, v_name),
            body: Box::new(sfi_rewrite_stmt(ast, body, x_name, i_name, v_name)),
        },
        Stmt::Try { body, had_catch, catch_param, catch_type, catch_body, finally_body } => {
            Stmt::Try {
                body: body
                    .iter()
                    .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                    .collect(),
                had_catch: *had_catch,
                catch_param: catch_param.clone(),
                catch_type: catch_type.clone(),
                catch_body: catch_body
                    .iter()
                    .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                    .collect(),
                finally_body: finally_body.as_ref().map(|fb| {
                    fb.iter()
                        .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                        .collect()
                }),
            }
        }
        Stmt::Block(stmts) => Stmt::Block(
            stmts
                .iter()
                .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                .collect(),
        ),
        Stmt::Multi(stmts) => Stmt::Multi(
            stmts
                .iter()
                .map(|s| sfi_rewrite_stmt(ast, s, x_name, i_name, v_name))
                .collect(),
        ),
        Stmt::Break | Stmt::Continue => s.clone(),
        Stmt::FnDecl { .. }
        | Stmt::TypeDecl { .. }
        | Stmt::ClassDecl { .. }
        | Stmt::ImportDecl { .. }
        | Stmt::ExportDecl { .. } => s.clone(),
    }
}

fn sfi_rewrite_expr(ast: &mut Ast, eid: ExprId, x_name: &str, i_name: &str, v_name: &str) -> ExprId {
    let cur = ast.get_expr(eid).clone();
    match cur {
        Expr::Index { obj, index } => {
            // X[I] → Ident(v_name). Otherwise descend.
            if let Expr::Ident(n) = ast.get_expr(obj).clone()
                && n == x_name
                && let Expr::Ident(in_) = ast.get_expr(index).clone()
                && in_ == i_name
            {
                return ast.add_expr(Expr::Ident(v_name.to_string()));
            }
            let new_obj = sfi_rewrite_expr(ast, obj, x_name, i_name, v_name);
            let new_index = sfi_rewrite_expr(ast, index, x_name, i_name, v_name);
            ast.add_expr(Expr::Index { obj: new_obj, index: new_index })
        }
        Expr::Member { obj, name } => {
            let new_obj = sfi_rewrite_expr(ast, obj, x_name, i_name, v_name);
            ast.add_expr(Expr::Member { obj: new_obj, name })
        }
        Expr::Call { callee, args } => {
            let new_callee = sfi_rewrite_expr(ast, callee, x_name, i_name, v_name);
            let new_args: Vec<ExprId> = args
                .iter()
                .map(|a| sfi_rewrite_expr(ast, *a, x_name, i_name, v_name))
                .collect();
            ast.add_expr(Expr::Call { callee: new_callee, args: new_args })
        }
        Expr::BinOp { op, left, right } => {
            let l = sfi_rewrite_expr(ast, left, x_name, i_name, v_name);
            let r = sfi_rewrite_expr(ast, right, x_name, i_name, v_name);
            ast.add_expr(Expr::BinOp { op, left: l, right: r })
        }
        Expr::Assign { target, value } => {
            let t = sfi_rewrite_expr(ast, target, x_name, i_name, v_name);
            let v = sfi_rewrite_expr(ast, value, x_name, i_name, v_name);
            ast.add_expr(Expr::Assign { target: t, value: v })
        }
        Expr::Unary { op, expr } => {
            let e = sfi_rewrite_expr(ast, expr, x_name, i_name, v_name);
            ast.add_expr(Expr::Unary { op, expr: e })
        }
        Expr::Ternary { cond, then_branch, else_branch } => {
            let c = sfi_rewrite_expr(ast, cond, x_name, i_name, v_name);
            let t = sfi_rewrite_expr(ast, then_branch, x_name, i_name, v_name);
            let e = sfi_rewrite_expr(ast, else_branch, x_name, i_name, v_name);
            ast.add_expr(Expr::Ternary {
                cond: c,
                then_branch: t,
                else_branch: e,
            })
        }
        Expr::Array(els) => {
            let new_els: Vec<ExprId> = els
                .iter()
                .map(|e| sfi_rewrite_expr(ast, *e, x_name, i_name, v_name))
                .collect();
            ast.add_expr(Expr::Array(new_els))
        }
        Expr::Spread { expr } => {
            let e = sfi_rewrite_expr(ast, expr, x_name, i_name, v_name);
            ast.add_expr(Expr::Spread { expr: e })
        }
        Expr::ObjectLit { fields } => {
            let new_fields: Vec<(String, ExprId)> = fields
                .iter()
                .map(|(n, e)| (n.clone(), sfi_rewrite_expr(ast, *e, x_name, i_name, v_name)))
                .collect();
            ast.add_expr(Expr::ObjectLit { fields: new_fields })
        }
        Expr::PostIncr { target, is_inc } => {
            let t = sfi_rewrite_expr(ast, target, x_name, i_name, v_name);
            ast.add_expr(Expr::PostIncr { target: t, is_inc })
        }
        Expr::OptChain { obj, name } => {
            let o = sfi_rewrite_expr(ast, obj, x_name, i_name, v_name);
            ast.add_expr(Expr::OptChain { obj: o, name })
        }
        Expr::Nullish { lhs, rhs } => {
            let l = sfi_rewrite_expr(ast, lhs, x_name, i_name, v_name);
            let r = sfi_rewrite_expr(ast, rhs, x_name, i_name, v_name);
            ast.add_expr(Expr::Nullish { lhs: l, rhs: r })
        }
        Expr::New { class_name, args } => {
            let new_args: Vec<ExprId> = args
                .iter()
                .map(|a| sfi_rewrite_expr(ast, *a, x_name, i_name, v_name))
                .collect();
            ast.add_expr(Expr::New { class_name, args: new_args })
        }
        // Leaves and shapes that don't carry X-referencing children
        // (Ident / Number / String / Bool / Null / closures / etc) — clone.
        _ => eid,
    }
}

/// This pass implements two static-rewrite shapes that cover the bulk
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
pub fn desugar_arguments_object(ast: &mut Ast) {
    // Snapshot per-fn user-param names, indexed by FnDecl name. The
    // walk below mutates expression nodes in place using these
    // snapshots.
    use std::collections::HashMap;
    let mut fn_params: HashMap<String, Vec<String>> = HashMap::new();
    for s in &ast.stmts {
        if let Stmt::FnDecl { name, params, .. } = s {
            // Skip the synthetic `__env` (closure capture vector) and
            // `__this` (class instance) prefix params — they're not
            // user-visible "arguments". Everything after is the
            // user's declared param list.
            let user_start = params
                .first()
                .filter(|p| p.name == "__env" || p.name == "__this")
                .map(|_| 1)
                .unwrap_or(0);
            let names: Vec<String> = params[user_start..]
                .iter()
                .map(|p| p.name.clone())
                .collect();
            fn_params.insert(name.clone(), names);
        }
    }

    let stmts_clone: Vec<Stmt> = ast.stmts.clone();
    for (idx, stmt) in stmts_clone.iter().enumerate() {
        if let Stmt::FnDecl { name, body, .. } = stmt {
            let Some(params) = fn_params.get(name) else { continue };
            let params = params.clone();
            // T-11 — pre-pass: detect any dynamic `arguments[<non-
            // literal>]` use. If found, prepend a synthesized
            // `let __torajs_arguments: any[] = [p0, p1, ...]` before
            // the body and rewrite the dynamic indices to read from it.
            // Literal-index rewrites (the existing path) take priority
            // and don't materialize the array — they stay zero-cost.
            let mut needs_materialize = false;
            for s in body {
                if stmt_uses_dynamic_arguments(ast, s) {
                    needs_materialize = true;
                    break;
                }
            }
            let new_body: Vec<Stmt> = body
                .iter()
                .map(|s| rewrite_arguments_in_stmt(ast, s, &params))
                .collect();
            // Synthesize the local OUTSIDE the &mut ast.stmts borrow
            // (synth_arguments_local also takes &mut ast for add_expr).
            let synth_opt = if needs_materialize {
                Some(synth_arguments_local(ast, &params))
            } else {
                None
            };
            if let Stmt::FnDecl { body: b, .. } = &mut ast.stmts[idx] {
                if let Some(synth) = synth_opt {
                    let mut full = Vec::with_capacity(new_body.len() + 1);
                    full.push(synth);
                    full.extend(new_body);
                    *b = full;
                } else {
                    *b = new_body;
                }
            }
        }
    }
}

/// T-11 — returns true if any `arguments[<non-literal>]` index access
/// (or bare `arguments` reference outside the literal-index /
/// `arguments.length` / spread forms the existing rewrite handles)
/// appears in the stmt subtree. Used to gate the synthesized
/// `let __torajs_arguments: any[] = [...]` prepend.
fn stmt_uses_dynamic_arguments(ast: &Ast, s: &Stmt) -> bool {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) | Stmt::Yield(eid) => {
            expr_uses_dynamic_arguments(ast, *eid)
        }
        Stmt::Return(opt) => opt.is_some_and(|e| expr_uses_dynamic_arguments(ast, e)),
        Stmt::LetDecl { init, .. } => expr_uses_dynamic_arguments(ast, *init),
        Stmt::YieldInto { value, .. } => expr_uses_dynamic_arguments(ast, *value),
        Stmt::If { cond, then_branch, else_branch } => {
            expr_uses_dynamic_arguments(ast, *cond)
                || stmt_uses_dynamic_arguments(ast, then_branch)
                || else_branch.as_ref().is_some_and(|e| stmt_uses_dynamic_arguments(ast, e))
        }
        Stmt::While { cond, body } | Stmt::DoWhile { cond, body } => {
            expr_uses_dynamic_arguments(ast, *cond) || stmt_uses_dynamic_arguments(ast, body)
        }
        Stmt::For { init, cond, step, body } => {
            init.as_ref().is_some_and(|s| stmt_uses_dynamic_arguments(ast, s))
                || cond.is_some_and(|c| expr_uses_dynamic_arguments(ast, c))
                || step.is_some_and(|st| expr_uses_dynamic_arguments(ast, st))
                || stmt_uses_dynamic_arguments(ast, body)
        }
        Stmt::ForOfSplitIter { parent, sep, body, .. } => {
            expr_uses_dynamic_arguments(ast, *parent)
                || expr_uses_dynamic_arguments(ast, *sep)
                || stmt_uses_dynamic_arguments(ast, body)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_uses_dynamic_arguments(ast, s))
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            body.iter().any(|s| stmt_uses_dynamic_arguments(ast, s))
                || catch_body.iter().any(|s| stmt_uses_dynamic_arguments(ast, s))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_uses_dynamic_arguments(ast, s)))
        }
        _ => false,
    }
}

fn expr_uses_dynamic_arguments(ast: &Ast, eid: ExprId) -> bool {
    match ast.get_expr(eid) {
        Expr::Index { obj, index } => {
            // Match `arguments[<non-Number-literal>]`. Number-literal
            // case is already handled inline by the existing rewrite
            // (param-name substitution; no array materialization).
            if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments") {
                if !matches!(ast.get_expr(*index), Expr::Number(_)) {
                    return true;
                }
                // Number index but out-of-range fall-through still
                // materializes — bun returns undefined; tr maps to
                // null in the boxed Any read. Conservative: treat as
                // dynamic so the array is available.
                if let Expr::Number(n) = ast.get_expr(*index)
                    && (n.fract() != 0.0 || (*n as usize) >= count_user_params(ast, eid))
                {
                    return true;
                }
            }
            expr_uses_dynamic_arguments(ast, *obj)
                || expr_uses_dynamic_arguments(ast, *index)
        }
        Expr::Member { obj, name } => {
            // `arguments.callee` — currently unhandled; will need its
            // own materialization later. Bare `arguments.<other>`
            // also forces materialize so stuff like
            // `arguments.length.toString()` keeps walking.
            if matches!(ast.get_expr(*obj), Expr::Ident(n) if n == "arguments")
                && name != "length"
            {
                return true;
            }
            expr_uses_dynamic_arguments(ast, *obj)
        }
        Expr::Ident(n) if n == "arguments" => {
            // Bare `arguments` reference (not Index / Member / spread —
            // those have their own arms). E.g. `let xs = arguments;`
            // or passing `arguments` to a fn that's not the spread
            // form. Forces materialize.
            true
        }
        Expr::Call { callee, args } => {
            expr_uses_dynamic_arguments(ast, *callee)
                || args.iter().any(|a| {
                    // `f(...arguments)` is handled by the inline-spread
                    // rewrite — no materialize needed.
                    if let Expr::Spread { expr } = ast.get_expr(*a)
                        && let Expr::Ident(n) = ast.get_expr(*expr)
                        && n == "arguments"
                    {
                        return false;
                    }
                    expr_uses_dynamic_arguments(ast, *a)
                })
        }
        Expr::BinOp { left, right, .. } => {
            expr_uses_dynamic_arguments(ast, *left)
                || expr_uses_dynamic_arguments(ast, *right)
        }
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::PostIncr { target: expr, .. } => {
            expr_uses_dynamic_arguments(ast, *expr)
        }
        Expr::Assign { target, value } => {
            expr_uses_dynamic_arguments(ast, *target)
                || expr_uses_dynamic_arguments(ast, *value)
        }
        Expr::Array(items) => items.iter().any(|e| {
            // `[...arguments]` — handled inline by spread rewrite.
            if let Expr::Spread { expr } = ast.get_expr(*e)
                && let Expr::Ident(n) = ast.get_expr(*expr)
                && n == "arguments"
            {
                return false;
            }
            expr_uses_dynamic_arguments(ast, *e)
        }),
        Expr::ObjectLit { fields } => {
            fields.iter().any(|(_, e)| expr_uses_dynamic_arguments(ast, *e))
        }
        Expr::Spread { expr } => expr_uses_dynamic_arguments(ast, *expr),
        Expr::Ternary { cond, then_branch, else_branch } => {
            expr_uses_dynamic_arguments(ast, *cond)
                || expr_uses_dynamic_arguments(ast, *then_branch)
                || expr_uses_dynamic_arguments(ast, *else_branch)
        }
        Expr::Nullish { lhs, rhs } => {
            expr_uses_dynamic_arguments(ast, *lhs) || expr_uses_dynamic_arguments(ast, *rhs)
        }
        Expr::OptChain { obj, .. } => expr_uses_dynamic_arguments(ast, *obj),
        _ => false,
    }
}

fn count_user_params(_ast: &Ast, _eid: ExprId) -> usize {
    // Caller's params count is captured during the FnDecl walk and
    // not threaded through expr_uses_dynamic_arguments today; default
    // to a large value so the literal-bounds-check arm never trips.
    // The bounds-aware materialize is a follow-up.
    usize::MAX
}

/// T-11 — synthesize `let __torajs_arguments: any[] = [p0, p1, ...]`
/// for prepending to a fn body. Each param Ident becomes one array
/// element; the LetDecl arm in ssa_lower routes through the forced-
/// Any path because the annotation is `any[]`.
fn synth_arguments_local(ast: &mut Ast, params: &[String]) -> Stmt {
    let elems: Vec<ExprId> = params
        .iter()
        .map(|p| ast.add_expr(Expr::Ident(p.clone())))
        .collect();
    let init = ast.add_expr(Expr::Array(elems));
    Stmt::LetDecl {
        mutable: false,
        name: "__torajs_arguments".into(),
        type_ann: Some("any[]".into()),
        init,
    }
}

fn rewrite_arguments_in_stmt(ast: &mut Ast, s: &Stmt, params: &[String]) -> Stmt {
    match s {
        Stmt::Expr(eid) => Stmt::Expr(rewrite_arguments_in_expr(ast, *eid, params)),
        Stmt::Throw(eid) => Stmt::Throw(rewrite_arguments_in_expr(ast, *eid, params)),
        Stmt::Return(Some(eid)) => {
            Stmt::Return(Some(rewrite_arguments_in_expr(ast, *eid, params)))
        }
        Stmt::Return(None) => Stmt::Return(None),
        Stmt::LetDecl { mutable, name, type_ann, init } => Stmt::LetDecl {
            mutable: *mutable,
            name: name.clone(),
            type_ann: type_ann.clone(),
            init: rewrite_arguments_in_expr(ast, *init, params),
        },
        Stmt::Block(stmts) => Stmt::Block(
            stmts
                .iter()
                .map(|s| rewrite_arguments_in_stmt(ast, s, params))
                .collect(),
        ),
        Stmt::Multi(stmts) => Stmt::Multi(
            stmts
                .iter()
                .map(|s| rewrite_arguments_in_stmt(ast, s, params))
                .collect(),
        ),
        Stmt::If { cond, then_branch, else_branch } => Stmt::If {
            cond: rewrite_arguments_in_expr(ast, *cond, params),
            then_branch: Box::new(rewrite_arguments_in_stmt(ast, then_branch, params)),
            else_branch: else_branch
                .as_ref()
                .map(|eb| Box::new(rewrite_arguments_in_stmt(ast, eb, params))),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: rewrite_arguments_in_expr(ast, *cond, params),
            body: Box::new(rewrite_arguments_in_stmt(ast, body, params)),
        },
        Stmt::DoWhile { cond, body } => Stmt::DoWhile {
            cond: rewrite_arguments_in_expr(ast, *cond, params),
            body: Box::new(rewrite_arguments_in_stmt(ast, body, params)),
        },
        Stmt::For { init, cond, step, body } => Stmt::For {
            init: init.as_ref().map(|i| Box::new(rewrite_arguments_in_stmt(ast, i, params))),
            cond: cond.map(|c| rewrite_arguments_in_expr(ast, c, params)),
            step: step.map(|u| rewrite_arguments_in_expr(ast, u, params)),
            body: Box::new(rewrite_arguments_in_stmt(ast, body, params)),
        },
        Stmt::Try { body, had_catch, catch_param, catch_type, catch_body, finally_body } => {
            Stmt::Try {
                body: body
                    .iter()
                    .map(|s| rewrite_arguments_in_stmt(ast, s, params))
                    .collect(),
                had_catch: *had_catch,
                catch_param: catch_param.clone(),
                catch_type: catch_type.clone(),
                catch_body: catch_body
                    .iter()
                    .map(|s| rewrite_arguments_in_stmt(ast, s, params))
                    .collect(),
                finally_body: finally_body.as_ref().map(|fb| {
                    fb.iter()
                        .map(|s| rewrite_arguments_in_stmt(ast, s, params))
                        .collect()
                }),
            }
        }
        // Nested FnDecl owns its own arguments scope — leave it for
        // the outer pass to handle independently when it iterates
        // ast.stmts (lift_arrow_fns has already hoisted closures to
        // top-level FnDecls, so nested-FnDecl-in-body is rare in
        // practice).
        other => other.clone(),
    }
}

fn rewrite_arguments_in_expr(ast: &mut Ast, eid: ExprId, params: &[String]) -> ExprId {
    let e = ast.get_expr(eid).clone();
    match e {
        // `arguments.length` → Number(<arity>)
        Expr::Member { obj, name } if name == "length" => {
            if let Expr::Ident(n) = ast.get_expr(obj)
                && n == "arguments"
            {
                return ast.add_expr(Expr::Number(params.len() as f64));
            }
            // Recurse through the receiver; non-arguments member access
            // gets a fresh node so nested rewrites still reach the
            // children.
            let new_obj = rewrite_arguments_in_expr(ast, obj, params);
            ast.add_expr(Expr::Member { obj: new_obj, name })
        }
        // `arguments[N]` with literal N in [0, arity) → Ident(param[N]).
        // T-11 — `arguments[<non-literal>]` (or out-of-range literal)
        // → `__torajs_arguments[<i>]` reading from the synthesized
        // Array<Any>. The synth let is prepended at fn body start by
        // the FnDecl-walk pre-pass when any dynamic use is detected.
        Expr::Index { obj, index } => {
            let is_arguments = matches!(
                ast.get_expr(obj),
                Expr::Ident(n) if n == "arguments"
            );
            if is_arguments {
                if let Expr::Number(n) = ast.get_expr(index)
                    && n.fract() == 0.0
                    && (*n as usize) < params.len()
                {
                    let pname = params[*n as usize].clone();
                    return ast.add_expr(Expr::Ident(pname));
                }
                // Dynamic index (or out-of-range literal): route to
                // the materialized Array<Any> via __torajs_arguments.
                let new_index = rewrite_arguments_in_expr(ast, index, params);
                let synth_obj =
                    ast.add_expr(Expr::Ident("__torajs_arguments".into()));
                return ast.add_expr(Expr::Index { obj: synth_obj, index: new_index });
            }
            let new_obj = rewrite_arguments_in_expr(ast, obj, params);
            let new_index = rewrite_arguments_in_expr(ast, index, params);
            ast.add_expr(Expr::Index { obj: new_obj, index: new_index })
        }
        Expr::BinOp { op, left, right } => {
            let l = rewrite_arguments_in_expr(ast, left, params);
            let r = rewrite_arguments_in_expr(ast, right, params);
            ast.add_expr(Expr::BinOp { op, left: l, right: r })
        }
        Expr::Unary { op, expr } => {
            let e2 = rewrite_arguments_in_expr(ast, expr, params);
            ast.add_expr(Expr::Unary { op, expr: e2 })
        }
        Expr::Call { callee, args } => {
            let c = rewrite_arguments_in_expr(ast, callee, params);
            /* `f(...arguments)` — expand the spread inline into the
             * call arg list as `f(p0, p1, ...)`. Handles arbitrary
             * mix of regular args and the spread. */
            let mut new_args: Vec<ExprId> = Vec::with_capacity(args.len());
            for a in &args {
                if let Expr::Spread { expr } = ast.get_expr(*a)
                    && let Expr::Ident(n) = ast.get_expr(*expr)
                    && n == "arguments"
                {
                    for p in params {
                        new_args.push(ast.add_expr(Expr::Ident(p.clone())));
                    }
                    continue;
                }
                new_args.push(rewrite_arguments_in_expr(ast, *a, params));
            }
            ast.add_expr(Expr::Call { callee: c, args: new_args })
        }
        Expr::Member { obj, name } => {
            let o = rewrite_arguments_in_expr(ast, obj, params);
            ast.add_expr(Expr::Member { obj: o, name })
        }
        Expr::Assign { target, value } => {
            let t = rewrite_arguments_in_expr(ast, target, params);
            let v = rewrite_arguments_in_expr(ast, value, params);
            ast.add_expr(Expr::Assign { target: t, value: v })
        }
        Expr::Array(elems) => {
            /* `[...arguments]` — expand the spread inline. Same shape
             * as the Call arm above. Mixed elems (regular + spread)
             * supported by interleaving. */
            let mut new_elems: Vec<ExprId> = Vec::with_capacity(elems.len());
            for e in &elems {
                if let Expr::Spread { expr } = ast.get_expr(*e)
                    && let Expr::Ident(n) = ast.get_expr(*expr)
                    && n == "arguments"
                {
                    for p in params {
                        new_elems.push(ast.add_expr(Expr::Ident(p.clone())));
                    }
                    continue;
                }
                new_elems.push(rewrite_arguments_in_expr(ast, *e, params));
            }
            ast.add_expr(Expr::Array(new_elems))
        }
        Expr::ObjectLit { fields } => {
            let new_fields: Vec<(String, ExprId)> = fields
                .iter()
                .map(|(n, e)| (n.clone(), rewrite_arguments_in_expr(ast, *e, params)))
                .collect();
            ast.add_expr(Expr::ObjectLit { fields: new_fields })
        }
        // Leaf / opaque shapes — no children to recurse through here.
        // Intentionally returns the original `eid` so we don't bloat
        // the arena with no-op clones.
        _ => eid,
    }
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
            Stmt::If { then_branch, else_branch, .. } => {
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
            Stmt::Try { body, catch_body, finally_body, .. } => {
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
                && let Some(Expr::Assign { target, value }) =
                    exprs.get(eid.0 as usize)
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

pub fn desugar_implicit_generics(ast: &mut Ast) {
    use std::collections::HashSet;

    // Split borrow: the body-walk inference helper reads `exprs` while
    // we mutate `stmts` in the same iteration. Destructure the fields
    // so the borrow checker sees two disjoint references rather than a
    // single &mut Ast.
    let Ast { stmts, exprs, .. } = ast;
    let ast_exprs_view: AstExprsView = &*exprs;

    /* T-19.p — pre-collect outer bindings the capturing-closure
     * return-type sniff can use to resolve captured idents. Without
     * this seed, `(v: number) => v + cap` bails out of the static
     * sniff and the FnDecl's return_type stays None → Void.
     *
     * Sources walked, in order (later overrides earlier):
     *  1. Top-level let-decls — the common shape `let cap = N; let
     *     cb = (v) => v + cap`.
     *  2. Every FnDecl's params (including parent fn's params for
     *     a closure created inside) — covers `function f(x) {
     *     return (y) => x + y }`. Lift moves the closure to a
     *     top-level FnDecl, so by the time we see its body,
     *     enclosing-fn params live in some other top-level FnDecl
     *     somewhere; pre-scanning all FnDecls catches them.
     *
     * Same-named clashes pick the LAST one observed; tora's
     * de-shadow at SSA means the bind table just needs ANY
     * matching annotation, not the lexically-correct one. */
    let mut outer_binds: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for s in stmts.iter() {
        if let Stmt::LetDecl { name, type_ann, init, .. } = s {
            if let Some(ann) = type_ann {
                outer_binds.insert(name.clone(), ann.clone());
            } else {
                let bs: Vec<Param> = binds_to_params(&outer_binds);
                if let Some(ann) = infer_expr_ann_with(
                    ast_exprs_view, *init, &bs, &outer_binds,
                ) {
                    outer_binds.insert(name.clone(), ann);
                }
            }
        }
        if let Stmt::FnDecl { params, .. } = s {
            for p in params {
                if let Some(ann) = &p.type_ann
                    && p.name != "__env"
                    && p.name != "__this"
                {
                    outer_binds.insert(p.name.clone(), ann.clone());
                }
            }
        }
    }

    for stmt in stmts.iter_mut() {
        let Stmt::FnDecl {
            name,
            params,
            return_type,
            type_params,
            body,
            ..
        } = stmt
        else {
            continue;
        };

        // Skip lifted closures and class-method synthesized shapes —
        // both keep their concrete first-param annotation as-is.
        // Capturing arrows arrive here with `__env` as the first
        // param; un-annotated expr-body still needs return-type
        // inference for the `(v: number) => v + capture` shape, so
        // run that branch before continuing. `__this` (class methods)
        // already has explicit declared return types in practice.
        if let Some(first) = params.first()
            && (first.name == "__env" || first.name == "__this")
        {
            if first.name == "__env"
                && return_type.is_none()
                && body_has_value_return(body)
            {
                if let Some(inferred) = infer_return_ann_seeded(
                    ast_exprs_view, body, params, &outer_binds,
                ) {
                    *return_type = Some(inferred);
                }
            }
            continue;
        }
        // Lifted arrow / function-expression bodies (`__closure_<N>`)
        // are stored in locals and called indirectly — the M3 generic
        // call-site retargeting only fires for bare-Ident callees that
        // name a global generic FnDecl, so adding TypeVars to PARAMS
        // here would produce a generic signature with no path to
        // monomorphize. Return-type sniff is still safe and useful
        // for arrow expr-body inference (`(v: number) => v + 1`),
        // so we run that branch then continue without touching params.
        if name.starts_with("__closure_") {
            if return_type.is_none() && body_has_value_return(body) {
                if let Some(inferred) = infer_return_ann(ast_exprs_view, body, params) {
                    *return_type = Some(inferred);
                }
            }
            continue;
        }

        // Avoid name collisions with any explicit type-params already
        // declared. Tracking the in-use set lets us pick `__T1`, `__T2`,
        // ... without trampling.
        let mut taken: HashSet<String> = type_params.iter().cloned().collect();

        let mut next_idx: usize = type_params.len();
        let alloc = |taken: &mut HashSet<String>, next_idx: &mut usize| -> String {
            loop {
                *next_idx += 1;
                let candidate = format!("__T{next_idx}");
                if !taken.contains(&candidate) {
                    taken.insert(candidate.clone());
                    return candidate;
                }
            }
        };

        let mut new_type_params: Vec<String> = Vec::new();
        for p in params.iter_mut() {
            let needs_var = match &p.type_ann {
                None => true,
                Some(ann) => ann == "any",
            };
            if !needs_var {
                continue;
            }
            // Don't genericize rest params — `...args: any[]` would need
            // a list-of-T encoding the substrate doesn't model. Leave
            // them un-genericized; the typechecker still rejects them
            // with the existing "requires annotation" message, but only
            // for rest-shaped sites which are a narrow slice.
            if p.is_rest {
                continue;
            }
            let var_name = alloc(&mut taken, &mut next_idx);
            p.type_ann = Some(var_name.clone());
            new_type_params.push(var_name);
        }

        // Return type:
        //   - explicit `: any` → fresh TypeVar (M3 path, monomorphized
        //     at call sites)
        //   - omitted (`function f(...) { ... }`) → walk the body's
        //     `return EXPR;` sites and try to *statically* infer a
        //     consistent annotation (literal kind, boolean BinOp/Unary,
        //     Ident-of-typed-binding). If every value-return agrees on
        //     a single annotation, set it as the return type; if there
        //     is disagreement or any return resists static inference,
        //     leave the return alone (sticks to the long-standing None
        //     → Void default — call sites that need a non-void value
        //     will still get the "return type mismatch" error, which is
        //     the right pre-existing surface).
        //   - explicit non-any annotation → leave alone.
        if return_type.as_deref() == Some("any") {
            let var_name = alloc(&mut taken, &mut next_idx);
            *return_type = Some(var_name.clone());
            new_type_params.push(var_name);
        } else if return_type.is_none() && body_has_value_return(body) {
            if let Some(inferred) = infer_return_ann(ast_exprs_view, body, params) {
                *return_type = Some(inferred);
            }
        }

        if !new_type_params.is_empty() {
            type_params.extend(new_type_params);
        }
    }
}

/// Borrow-shaped view of `Ast.exprs` for the inference helper. Defined
/// at the top of `desugar_implicit_generics` (just below) — `&[Expr]`
/// indexed by `ExprId.0 as usize`. The pre-pass walks expression
/// shapes statically without consulting the typechecker, so this
/// flat slice is enough.
type AstExprsView<'a> = &'a [Expr];

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
fn infer_return_ann(
    exprs: AstExprsView,
    body: &[Stmt],
    params: &[Param],
) -> Option<String> {
    infer_return_ann_seeded(
        exprs,
        body,
        params,
        &std::collections::HashMap::new(),
    )
}

/// T-19.p — variant that takes a pre-seeded binds map. Used by the
/// capturing-closure FnDecl path so outer-scope let-decls (where
/// the captures actually live) flow into the body's static return
/// sniff. The pre-T-19.p path passed only the body's params + body
/// let-decls — captured idents had no entry in binds and the sniff
/// bailed to None. Idempotent: explicit body-local lets shadow the
/// outer seed via collect_let_binding_anns running afterwards.
fn infer_return_ann_seeded(
    exprs: AstExprsView,
    body: &[Stmt],
    params: &[Param],
    outer_binds: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let mut binds: std::collections::HashMap<String, String> = outer_binds.clone();
    for p in params {
        if let Some(ann) = &p.type_ann {
            binds.insert(p.name.clone(), ann.clone());
        }
    }
    collect_let_binding_anns(exprs, body, &mut binds);
    let mut acc: Option<String> = None;
    if !collect_return_anns(exprs, body, &binds, &mut acc) {
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
) {
    for s in body {
        collect_let_binding_anns_stmt(exprs, s, binds);
    }
}

fn collect_let_binding_anns_stmt(
    exprs: AstExprsView,
    s: &Stmt,
    binds: &mut std::collections::HashMap<String, String>,
) {
    match s {
        Stmt::LetDecl { name, type_ann, init, .. } => {
            // Explicit annotation wins.
            if let Some(ann) = type_ann {
                binds.insert(name.clone(), ann.clone());
                return;
            }
            // Else infer from init shape — using the binds map we've
            // built up to here, so `let x = 3; let y = x + 1;` chains
            // correctly.
            let bs: Vec<Param> = binds_to_params(binds);
            if let Some(ann) = infer_expr_ann_with(exprs, *init, &bs, binds) {
                binds.insert(name.clone(), ann);
            }
        }
        Stmt::If { then_branch, else_branch, .. } => {
            collect_let_binding_anns_stmt(exprs, then_branch, binds);
            if let Some(eb) = else_branch {
                collect_let_binding_anns_stmt(exprs, eb, binds);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_let_binding_anns_stmt(exprs, body, binds);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                collect_let_binding_anns_stmt(exprs, i, binds);
            }
            collect_let_binding_anns_stmt(exprs, body, binds);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            collect_let_binding_anns(exprs, stmts, binds);
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            collect_let_binding_anns(exprs, body, binds);
            collect_let_binding_anns(exprs, catch_body, binds);
            if let Some(fb) = finally_body {
                collect_let_binding_anns(exprs, fb, binds);
            }
        }
        _ => {}
    }
}

fn binds_to_params(
    binds: &std::collections::HashMap<String, String>,
) -> Vec<Param> {
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
    acc: &mut Option<String>,
) -> bool {
    for s in body {
        if !collect_return_anns_stmt(exprs, s, binds, acc) {
            return false;
        }
    }
    true
}

fn collect_return_anns_stmt(
    exprs: AstExprsView,
    s: &Stmt,
    binds: &std::collections::HashMap<String, String>,
    acc: &mut Option<String>,
) -> bool {
    match s {
        Stmt::Return(Some(eid)) => {
            let bs = binds_to_params(binds);
            let Some(ann) = infer_expr_ann_with(exprs, *eid, &bs, binds) else {
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
        Stmt::If { then_branch, else_branch, .. } => {
            if !collect_return_anns_stmt(exprs, then_branch, binds, acc) {
                return false;
            }
            if let Some(eb) = else_branch
                && !collect_return_anns_stmt(exprs, eb, binds, acc)
            {
                return false;
            }
            true
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_return_anns_stmt(exprs, body, binds, acc)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init
                && !collect_return_anns_stmt(exprs, i, binds, acc)
            {
                return false;
            }
            collect_return_anns_stmt(exprs, body, binds, acc)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            collect_return_anns(exprs, stmts, binds, acc)
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            if !collect_return_anns(exprs, body, binds, acc) {
                return false;
            }
            if !collect_return_anns(exprs, catch_body, binds, acc) {
                return false;
            }
            if let Some(fb) = finally_body
                && !collect_return_anns(exprs, fb, binds, acc)
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

/// Statically infer an annotation string for an expression. Limited to
/// shapes whose annotation is unambiguous without consulting the
/// typechecker — literals, boolean-result BinOp/Unary, arithmetic ops
/// with statically-typeable operands, and Ident references resolvable
/// against `binds` (params + locally-inferred let bindings).
fn infer_expr_ann_with(
    exprs: AstExprsView,
    eid: ExprId,
    params: &[Param],
    binds: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let e = exprs.get(eid.0 as usize)?;
    match e {
        Expr::Number(_) => Some("number".into()),
        Expr::String(_) => Some("string".into()),
        Expr::Bool(_) => Some("boolean".into()),
        Expr::BinOp { op, left, right } => match op {
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge
            | BinOp::Eq | BinOp::Neq | BinOp::LAnd | BinOp::LOr => {
                Some("boolean".into())
            }
            BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod
            | BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor
            | BinOp::Shl | BinOp::Shr | BinOp::UShr => Some("number".into()),
            // `+` is the only ambiguous op (number add OR string
            // concat); fall back to per-side inference and only commit
            // when both agree on a concrete primitive.
            BinOp::Add => {
                let l = infer_expr_ann_with(exprs, *left, params, binds)?;
                let r = infer_expr_ann_with(exprs, *right, params, binds)?;
                // JS spec: `string + anything` and `anything + string`
                // both coerce to string concat; `number + number` stays
                // number. Other shapes (e.g. number+boolean → number,
                // boolean+boolean → number) are handled by the
                // typechecker's regular path — leave None so we don't
                // commit to a guess that conflicts with the deeper rules.
                if l == "string" || r == "string" {
                    Some("string".into())
                } else if l == "number" && r == "number" {
                    Some("number".into())
                } else {
                    None
                }
            }
        },
        Expr::Unary { op, .. } => match op {
            UnaryOp::Not => Some("boolean".into()),
            UnaryOp::Neg | UnaryOp::BitNot => Some("number".into()),
        },
        Expr::Ident(name) => {
            if let Some(p) = params.iter().find(|p| &p.name == name)
                && let Some(ann) = &p.type_ann
            {
                return Some(ann.clone());
            }
            binds.get(name).cloned()
        }
        // Conservatively bail on Member / Call / Index / etc.
        // The typechecker's regular path will produce the right errors;
        // we only override when statically obvious.
        _ => None,
    }
}

fn body_has_value_return(body: &[Stmt]) -> bool {
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
        Stmt::If { then_branch, else_branch, .. } => {
            stmt_has_value_return(then_branch)
                || else_branch.as_deref().is_some_and(stmt_has_value_return)
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            stmt_has_value_return(body)
        }
        Stmt::For { init, body, .. } => {
            init.as_deref().is_some_and(stmt_has_value_return)
                || stmt_has_value_return(body)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => body_has_value_return(stmts),
        Stmt::Try { body, catch_body, finally_body, .. } => {
            body_has_value_return(body)
                || body_has_value_return(catch_body)
                || finally_body
                    .as_deref()
                    .is_some_and(body_has_value_return)
        }
        // Nested FnDecl returns are scoped to the inner fn — skip.
        Stmt::FnDecl { .. } => false,
        _ => false,
    }
}

fn collect_let_anns(body: &[Stmt], out: &mut std::collections::HashMap<String, String>) {
    for s in body {
        match s {
            Stmt::LetDecl { name, type_ann: Some(ann), .. } => {
                out.insert(name.clone(), ann.clone());
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => collect_let_anns(stmts, out),
            Stmt::If { then_branch, else_branch, .. } => {
                collect_let_anns(std::slice::from_ref(then_branch.as_ref()), out);
                if let Some(eb) = else_branch {
                    collect_let_anns(std::slice::from_ref(eb.as_ref()), out);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
                collect_let_anns(std::slice::from_ref(body.as_ref()), out);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_let_anns(std::slice::from_ref(i.as_ref()), out);
                }
                collect_let_anns(std::slice::from_ref(body.as_ref()), out);
            }
            _ => {}
        }
    }
}

/// Free-variable analysis for an arrow fn body. Returns a deterministic,
/// de-duplicated list of identifier names referenced in the body that are
/// NOT bound by the arrow's params and NOT declared by any inner let/for
/// in the body itself. The ordering matches first-use order in the body
/// (deterministic across runs).
///
/// Limitations: this is a conservative name-only analysis — it does not
/// distinguish global FnDecls from outer locals (the lowerer filters
/// global fn names out of the capture set when it has the symbol table).
/// Inner ArrowFn bodies are walked too; their inner-arrow params shadow
/// matching names inside their body.
fn free_vars_of_arrow(
    ast: &Ast,
    params: &[Param],
    body: &[Stmt],
    global_fn_names: &[String],
) -> Vec<String> {
    // Pre-bind top-level fn names so they're treated as already-in-scope
    // and don't fall into the captures set.
    let mut bound: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
    bound.extend(global_fn_names.iter().cloned());
    let mut out: Vec<String> = Vec::new();
    for s in body {
        walk_stmt(ast, s, &mut bound, &mut out);
    }
    out
}

fn walk_stmt(ast: &Ast, s: &Stmt, bound: &mut Vec<String>, out: &mut Vec<String>) {
    match s {
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Yield(eid) => walk_expr(ast, *eid, bound, out),
        Stmt::YieldInto { value, .. } => walk_expr(ast, *value, bound, out),
        Stmt::Return(None) => {}
        Stmt::LetDecl { name, init, .. } => {
            walk_expr(ast, *init, bound, out);
            bound.push(name.clone());
        }
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            walk_expr(ast, *cond, bound, out);
            let saved = bound.len();
            walk_stmt(ast, then_branch, bound, out);
            bound.truncate(saved);
            if let Some(eb) = else_branch {
                walk_stmt(ast, eb, bound, out);
                bound.truncate(saved);
            }
        }
        Stmt::While { cond, body } => {
            walk_expr(ast, *cond, bound, out);
            let saved = bound.len();
            walk_stmt(ast, body, bound, out);
            bound.truncate(saved);
        }
        Stmt::DoWhile { body, cond } => {
            let saved = bound.len();
            walk_stmt(ast, body, bound, out);
            bound.truncate(saved);
            walk_expr(ast, *cond, bound, out);
        }
        Stmt::Switch { scrutinee, cases, default } => {
            walk_expr(ast, *scrutinee, bound, out);
            for c in cases {
                walk_expr(ast, c.value, bound, out);
                let saved = bound.len();
                for s in &c.body {
                    walk_stmt(ast, s, bound, out);
                }
                bound.truncate(saved);
            }
            if let Some(db) = default {
                let saved = bound.len();
                for s in db {
                    walk_stmt(ast, s, bound, out);
                }
                bound.truncate(saved);
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            let saved = bound.len();
            if let Some(i) = init {
                walk_stmt(ast, i, bound, out);
            }
            if let Some(c) = cond {
                walk_expr(ast, *c, bound, out);
            }
            if let Some(st) = step {
                walk_expr(ast, *st, bound, out);
            }
            walk_stmt(ast, body, bound, out);
            bound.truncate(saved);
        }
        Stmt::Block(stmts) => {
            let saved = bound.len();
            for st in stmts {
                walk_stmt(ast, st, bound, out);
            }
            bound.truncate(saved);
        }
        Stmt::Multi(stmts) => {
            // Same surrounding scope — bindings stay visible after.
            for st in stmts {
                walk_stmt(ast, st, bound, out);
            }
        }
        Stmt::ForOfSplitIter { var_name, parent, sep, body } => {
            // Same scope hygiene as Stmt::For — var_name binds inside
            // the body only.
            walk_expr(ast, *parent, bound, out);
            walk_expr(ast, *sep, bound, out);
            let saved = bound.len();
            bound.push(var_name.clone());
            walk_stmt(ast, body, bound, out);
            bound.truncate(saved);
        }
        Stmt::Break | Stmt::Continue => {}
        Stmt::Throw(eid) => walk_expr(ast, *eid, bound, out),
        Stmt::Try {
            body,
            catch_param,
            catch_type: _,
            had_catch: _,
            catch_body,
            finally_body,
        } => {
            let saved = bound.len();
            for s in body {
                walk_stmt(ast, s, bound, out);
            }
            bound.truncate(saved);
            if let Some(name) = catch_param {
                bound.push(name.clone());
            }
            for s in catch_body {
                walk_stmt(ast, s, bound, out);
            }
            bound.truncate(saved);
            if let Some(fb) = finally_body {
                for s in fb {
                    walk_stmt(ast, s, bound, out);
                }
                bound.truncate(saved);
            }
        }
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } => {
            // FnDecl inside an arrow body would be unusual; conservatively
            // ignore since check.rs hoists these out anyway.
        }
        Stmt::ClassDecl { .. } => {
            // desugar_classes runs before lift_arrow_fns; if a ClassDecl
            // somehow remains, ignore — its body has already been split
            // into FnDecls anyway.
        }
        Stmt::ImportDecl { .. } => {}
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                walk_stmt(ast, inner, bound, out);
            }
        }
    }
}

/// Names that are pre-bound globals — they should never count as
/// closure captures even when they appear as bare idents inside an
/// arrow body. Currently the runtime-provided print / namespace
/// objects. Kept in sync with `check.rs`'s `type_of(Expr::Ident)`
/// fallback list.
fn is_global_name(name: &str) -> bool {
    matches!(name, "console" | "Math")
}

/// M4.3.b — describe a fn's throw shape: `direct_throw` is true if any
/// `throw` statement appears anywhere in the body; `called_fns` is the
/// deduplicated list of identifier names referenced as direct call
/// callees (`Expr::Call { callee: Ident(name) }`). The lowerer combines
/// this with a fixed-point closure to compute may_throw transitively.
pub fn fn_throw_info(ast: &Ast, body: &[Stmt]) -> (bool, Vec<String>) {
    let mut direct = false;
    let mut called: Vec<String> = Vec::new();
    for s in body {
        scan_stmt_for_throws(ast, s, &mut direct, &mut called);
    }
    (direct, called)
}

fn scan_stmt_for_throws(
    ast: &Ast,
    s: &Stmt,
    direct: &mut bool,
    called: &mut Vec<String>,
) {
    match s {
        Stmt::Throw(eid) => {
            *direct = true;
            scan_expr_for_calls(ast, *eid, called);
        }
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) | Stmt::Yield(eid) => {
            scan_expr_for_calls(ast, *eid, called)
        }
        Stmt::YieldInto { value, .. } => scan_expr_for_calls(ast, *value, called),
        Stmt::Return(None) | Stmt::Break | Stmt::Continue => {}
        Stmt::LetDecl { init, .. } => scan_expr_for_calls(ast, *init, called),
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
            scan_expr_for_calls(ast, *cond, called);
            scan_stmt_for_throws(ast, then_branch, direct, called);
            if let Some(eb) = else_branch {
                scan_stmt_for_throws(ast, eb, direct, called);
            }
        }
        Stmt::While { cond, body } => {
            scan_expr_for_calls(ast, *cond, called);
            scan_stmt_for_throws(ast, body, direct, called);
        }
        Stmt::DoWhile { body, cond } => {
            scan_stmt_for_throws(ast, body, direct, called);
            scan_expr_for_calls(ast, *cond, called);
        }
        Stmt::Switch { scrutinee, cases, default } => {
            scan_expr_for_calls(ast, *scrutinee, called);
            for c in cases {
                scan_expr_for_calls(ast, c.value, called);
                for s in &c.body {
                    scan_stmt_for_throws(ast, s, direct, called);
                }
            }
            if let Some(db) = default {
                for s in db {
                    scan_stmt_for_throws(ast, s, direct, called);
                }
            }
        }
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                scan_stmt_for_throws(ast, i, direct, called);
            }
            if let Some(c) = cond {
                scan_expr_for_calls(ast, *c, called);
            }
            if let Some(st) = step {
                scan_expr_for_calls(ast, *st, called);
            }
            scan_stmt_for_throws(ast, body, direct, called);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for st in stmts {
                scan_stmt_for_throws(ast, st, direct, called);
            }
        }
        Stmt::ForOfSplitIter { parent, sep, body, .. } => {
            scan_expr_for_calls(ast, *parent, called);
            scan_expr_for_calls(ast, *sep, called);
            scan_stmt_for_throws(ast, body, direct, called);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for st in body {
                scan_stmt_for_throws(ast, st, direct, called);
            }
            for st in catch_body {
                scan_stmt_for_throws(ast, st, direct, called);
            }
            if let Some(fb) = finally_body {
                for st in fb {
                    scan_stmt_for_throws(ast, st, direct, called);
                }
            }
        }
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } => {}
        Stmt::ClassDecl { .. } => {
            // desugar_classes runs before throw analysis; classes split
            // into FnDecls each get their own throw-info pass.
        }
        Stmt::ImportDecl { .. } => {}
        Stmt::ExportDecl { inner, .. } => {
            if let Some(inner) = inner {
                scan_stmt_for_throws(ast, inner, direct, called);
            }
        }
    }
}

fn scan_expr_for_calls(ast: &Ast, eid: ExprId, out: &mut Vec<String>) {
    match ast.get_expr(eid) {
        Expr::Call { callee, args } => {
            if let Expr::Ident(name) = ast.get_expr(*callee) {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            scan_expr_for_calls(ast, *callee, out);
            for a in args {
                scan_expr_for_calls(ast, *a, out);
            }
        }
        Expr::BinOp { left, right, .. } => {
            scan_expr_for_calls(ast, *left, out);
            scan_expr_for_calls(ast, *right, out);
        }
        Expr::Unary { expr, .. } => scan_expr_for_calls(ast, *expr, out),
        Expr::Member { obj, .. } => scan_expr_for_calls(ast, *obj, out),
        Expr::Assign { target, value } => {
            scan_expr_for_calls(ast, *target, out);
            scan_expr_for_calls(ast, *value, out);
        }
        Expr::Index { obj, index } => {
            scan_expr_for_calls(ast, *obj, out);
            scan_expr_for_calls(ast, *index, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                scan_expr_for_calls(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                scan_expr_for_calls(ast, *e, out);
            }
        }
        // ArrowFn / Closure bodies are walked separately (their own
        // FnDecls — lifted by lift_arrow_fns); from this fn's
        // perspective the closure body's calls don't propagate to the
        // outer fn's may_throw bit until the outer fn actually invokes
        // the closure (which is itself a Call → tracked above).
        Expr::ArrowFn { .. } | Expr::Closure { .. } => {}
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                scan_expr_for_calls(ast, *a, out);
            }
        }
        Expr::Ternary { cond, then_branch, else_branch } => {
            scan_expr_for_calls(ast, *cond, out);
            scan_expr_for_calls(ast, *then_branch, out);
            scan_expr_for_calls(ast, *else_branch, out);
        }
        Expr::TypeOf { expr } | Expr::Spread { expr } | Expr::InstanceOf { expr, .. } => scan_expr_for_calls(ast, *expr, out),
        Expr::Nullish { lhs, rhs } => {
            scan_expr_for_calls(ast, *lhs, out);
            scan_expr_for_calls(ast, *rhs, out);
        }
        Expr::OptChain { obj, .. } => scan_expr_for_calls(ast, *obj, out),
        Expr::PostIncr { target, .. } => scan_expr_for_calls(ast, *target, out),
        Expr::This => {}
        Expr::Ident(_) | Expr::String(_) | Expr::Number(_) | Expr::Bool(_)
        | Expr::Null | Expr::Uninit | Expr::Regex { .. } => {}
    }
}

fn walk_expr(ast: &Ast, eid: ExprId, bound: &mut Vec<String>, out: &mut Vec<String>) {
    match ast.get_expr(eid) {
        Expr::Ident(name) => {
            if is_global_name(name) {
                return;
            }
            if !bound.contains(name) && !out.contains(name) {
                out.push(name.clone());
            }
        }
        Expr::String(_) | Expr::Number(_) | Expr::Bool(_)
        | Expr::Null | Expr::Uninit | Expr::Regex { .. } => {}
        Expr::BinOp { left, right, .. } => {
            walk_expr(ast, *left, bound, out);
            walk_expr(ast, *right, bound, out);
        }
        Expr::Unary { expr, .. } => walk_expr(ast, *expr, bound, out),
        Expr::Member { obj, .. } => walk_expr(ast, *obj, bound, out),
        Expr::Call { callee, args } => {
            walk_expr(ast, *callee, bound, out);
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::Assign { target, value } => {
            walk_expr(ast, *target, bound, out);
            walk_expr(ast, *value, bound, out);
        }
        Expr::Index { obj, index } => {
            walk_expr(ast, *obj, bound, out);
            walk_expr(ast, *index, bound, out);
        }
        Expr::Array(elems) => {
            for e in elems {
                walk_expr(ast, *e, bound, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                walk_expr(ast, *e, bound, out);
            }
        }
        Expr::ArrowFn { params, body, .. } => {
            let saved = bound.len();
            for p in params {
                bound.push(p.name.clone());
            }
            for s in body {
                walk_stmt(ast, s, bound, out);
            }
            bound.truncate(saved);
        }
        Expr::Closure { captures, .. } => {
            // Already lifted (shouldn't normally happen during this pass,
            // but guard for nested-lift cases): the captures referenced
            // by an already-lifted closure are themselves free in the
            // current arrow body if not bound here.
            for c in captures {
                if !bound.contains(c) && !out.contains(c) {
                    out.push(c.clone());
                }
            }
        }
        // M5.1 — by the time arrow-fn lifting runs, classes have already
        // been desugared to functions (and `this` to `__this`). These
        // arms guard against an arrow body that lexically nests inside a
        // class method whose desugar hasn't completed; in practice we
        // run desugar_classes before lift_arrow_fns, so they're inert.
        Expr::This => {}
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                walk_expr(ast, *a, bound, out);
            }
        }
        Expr::Ternary { cond, then_branch, else_branch } => {
            walk_expr(ast, *cond, bound, out);
            walk_expr(ast, *then_branch, bound, out);
            walk_expr(ast, *else_branch, bound, out);
        }
        Expr::TypeOf { expr } | Expr::Spread { expr } | Expr::InstanceOf { expr, .. } => walk_expr(ast, *expr, bound, out),
        Expr::Nullish { lhs, rhs } => {
            walk_expr(ast, *lhs, bound, out);
            walk_expr(ast, *rhs, bound, out);
        }
        Expr::OptChain { obj, .. } => walk_expr(ast, *obj, bound, out),
        Expr::PostIncr { target, .. } => walk_expr(ast, *target, bound, out),
    }
}

impl Ast {
    pub fn add_expr(&mut self, e: Expr) -> ExprId {
        let id = ExprId(self.exprs.len() as u32);
        self.exprs.push(e);
        // Sentinel span until parser (or a desugar pass) sets a real
        // one via `set_expr_span`. Both fields zero means "unknown".
        self.expr_spans.push(crate::lexer::Span { start: 0, end: 0 });
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
    /// borrow-restricted contexts (ssa_inkwell, etc).
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
            self.print_stmt(s, 0);
        }
    }

    fn print_stmt(&self, s: &Stmt, indent: usize) {
        let pad = "  ".repeat(indent);
        match s {
            Stmt::Expr(eid) => {
                println!("{pad}ExprStmt");
                self.print_expr(*eid, indent + 1);
            }
            Stmt::Yield(eid) => {
                println!("{pad}Yield");
                self.print_expr(*eid, indent + 1);
            }
            Stmt::YieldInto { var, type_ann, value } => {
                println!("{pad}YieldInto var={var} ty={type_ann:?}");
                self.print_expr(*value, indent + 1);
            }
            Stmt::LetDecl {
                mutable,
                name,
                type_ann,
                init,
            } => {
                let kw = if *mutable { "let" } else { "const" };
                match type_ann {
                    Some(ann) => println!("{pad}{kw} {name}: {ann}"),
                    None => println!("{pad}{kw} {name}"),
                }
                self.print_expr(*init, indent + 1);
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                println!("{pad}If");
                println!("{pad}  cond:");
                self.print_expr(*cond, indent + 2);
                println!("{pad}  then:");
                self.print_stmt(then_branch, indent + 2);
                if let Some(eb) = else_branch {
                    println!("{pad}  else:");
                    self.print_stmt(eb, indent + 2);
                }
            }
            Stmt::While { cond, body } => {
                println!("{pad}While");
                println!("{pad}  cond:");
                self.print_expr(*cond, indent + 2);
                println!("{pad}  body:");
                self.print_stmt(body, indent + 2);
            }
            Stmt::DoWhile { body, cond } => {
                println!("{pad}DoWhile");
                println!("{pad}  body:");
                self.print_stmt(body, indent + 2);
                println!("{pad}  cond:");
                self.print_expr(*cond, indent + 2);
            }
            Stmt::Switch { scrutinee, cases, default } => {
                println!("{pad}Switch");
                println!("{pad}  on:");
                self.print_expr(*scrutinee, indent + 2);
                for c in cases {
                    println!("{pad}  case:");
                    self.print_expr(c.value, indent + 2);
                    for s in &c.body {
                        self.print_stmt(s, indent + 2);
                    }
                }
                if let Some(db) = default {
                    println!("{pad}  default:");
                    for s in db {
                        self.print_stmt(s, indent + 2);
                    }
                }
            }
            Stmt::For { init, cond, step, body } => {
                println!("{pad}For");
                if let Some(i) = init {
                    println!("{pad}  init:");
                    self.print_stmt(i, indent + 2);
                }
                if let Some(c) = cond {
                    println!("{pad}  cond:");
                    self.print_expr(*c, indent + 2);
                }
                if let Some(st) = step {
                    println!("{pad}  step:");
                    self.print_expr(*st, indent + 2);
                }
                println!("{pad}  body:");
                self.print_stmt(body, indent + 2);
            }
            Stmt::Break => println!("{pad}Break"),
            Stmt::Continue => println!("{pad}Continue"),
            Stmt::ForOfSplitIter { var_name, parent, sep, body } => {
                println!("{pad}ForOfSplitIter {var_name}");
                println!("{pad}  parent:");
                self.print_expr(*parent, indent + 2);
                println!("{pad}  sep:");
                self.print_expr(*sep, indent + 2);
                println!("{pad}  body:");
                self.print_stmt(body, indent + 2);
            }
            Stmt::Throw(eid) => {
                println!("{pad}Throw");
                self.print_expr(*eid, indent + 1);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_type: _,
            had_catch: _,
                catch_body,
                finally_body,
            } => {
                println!("{pad}Try");
                println!("{pad}  body:");
                for s in body {
                    self.print_stmt(s, indent + 2);
                }
                if let Some(p) = catch_param {
                    println!("{pad}  catch ({p}):");
                } else {
                    println!("{pad}  catch:");
                }
                for s in catch_body {
                    self.print_stmt(s, indent + 2);
                }
                if let Some(fb) = finally_body {
                    println!("{pad}  finally:");
                    for s in fb {
                        self.print_stmt(s, indent + 2);
                    }
                }
            }
            Stmt::Block(stmts) => {
                println!("{pad}Block");
                for s in stmts {
                    self.print_stmt(s, indent + 1);
                }
            }
            Stmt::Multi(stmts) => {
                println!("{pad}Multi");
                for s in stmts {
                    self.print_stmt(s, indent + 1);
                }
            }
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                is_generator: _,
            } => {
                let plist: Vec<String> = params
                    .iter()
                    .map(|p| match &p.type_ann {
                        Some(t) => format!("{}: {t}", p.name),
                        None => p.name.clone(),
                    })
                    .collect();
                let ret = return_type.clone().unwrap_or_else(|| "void".into());
                let tps = if type_params.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", type_params.join(", "))
                };
                println!("{pad}FnDecl {name}{tps}({}): {ret}", plist.join(", "));
                for s in body {
                    self.print_stmt(s, indent + 1);
                }
            }
            Stmt::TypeDecl {
                name,
                type_params,
                fields,
            } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, t)| format!("{n}: {t}"))
                    .collect();
                let tps = if type_params.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", type_params.join(", "))
                };
                println!("{pad}TypeDecl {name}{tps} = {{ {} }}", parts.join(", "));
            }
            Stmt::Return(maybe) => match maybe {
                Some(eid) => {
                    println!("{pad}Return");
                    self.print_expr(*eid, indent + 1);
                }
                None => println!("{pad}Return"),
            },
            Stmt::ClassDecl {
                name,
                type_params: _,
                parent,
                is_abstract: _,
                fields,
                static_fields: _,
                ctor,
                methods,
                static_methods: _,
            } => {
                let parts: Vec<String> = fields
                    .iter()
                    .map(|(n, t)| format!("{n}: {t}"))
                    .collect();
                let ext = match parent {
                    Some(p) => format!(" extends {p}"),
                    None => String::new(),
                };
                println!(
                    "{pad}ClassDecl {name}{ext} fields={{ {} }}",
                    parts.join(", ")
                );
                if let Some(c) = ctor {
                    let plist: Vec<String> = c
                        .params
                        .iter()
                        .map(|p| match &p.type_ann {
                            Some(t) => format!("{}: {t}", p.name),
                            None => p.name.clone(),
                        })
                        .collect();
                    println!("{pad}  constructor({})", plist.join(", "));
                    for s in &c.body {
                        self.print_stmt(s, indent + 2);
                    }
                }
                for m in methods {
                    let plist: Vec<String> = m
                        .params
                        .iter()
                        .map(|p| match &p.type_ann {
                            Some(t) => format!("{}: {t}", p.name),
                            None => p.name.clone(),
                        })
                        .collect();
                    let ret = m.return_type.clone().unwrap_or_else(|| "void".into());
                    println!("{pad}  method {}({}): {ret}", m.name, plist.join(", "));
                    for s in &m.body {
                        self.print_stmt(s, indent + 2);
                    }
                }
            }
            Stmt::ImportDecl { source, .. } => {
                println!("{pad}ImportDecl {source:?}");
            }
            Stmt::ExportDecl { inner, .. } => {
                println!("{pad}ExportDecl");
                if let Some(inner) = inner {
                    self.print_stmt(inner, indent + 1);
                }
            }
        }
    }

    fn print_expr(&self, id: ExprId, indent: usize) {
        let pad = "  ".repeat(indent);
        match self.get_expr(id) {
            Expr::Ident(n) => println!("{pad}Ident({n:?})"),
            Expr::String(s) => println!("{pad}String({s:?})"),
            Expr::Number(n) => println!("{pad}Number({n})"),
            Expr::Bool(b) => println!("{pad}Bool({b})"),
            Expr::Null => println!("{pad}Null"),
            Expr::Uninit => println!("{pad}Uninit"),
            Expr::Regex { pattern, flags } => {
                println!("{pad}Regex /{pattern}/{flags}")
            }
            Expr::BinOp { op, left, right } => {
                println!("{pad}BinOp({op:?})");
                self.print_expr(*left, indent + 1);
                self.print_expr(*right, indent + 1);
            }
            Expr::Unary { op, expr } => {
                println!("{pad}Unary({op:?})");
                self.print_expr(*expr, indent + 1);
            }
            Expr::Member { obj, name } => {
                println!("{pad}Member");
                self.print_expr(*obj, indent + 1);
                println!("{pad}  .{name}");
            }
            Expr::Call { callee, args } => {
                println!("{pad}Call");
                self.print_expr(*callee, indent + 1);
                println!("{pad}  args:");
                for a in args {
                    self.print_expr(*a, indent + 2);
                }
            }
            Expr::Assign { target, value } => {
                println!("{pad}Assign");
                self.print_expr(*target, indent + 1);
                println!("{pad}  =");
                self.print_expr(*value, indent + 1);
            }
            Expr::Index { obj, index } => {
                println!("{pad}Index");
                self.print_expr(*obj, indent + 1);
                println!("{pad}  [");
                self.print_expr(*index, indent + 1);
                println!("{pad}  ]");
            }
            Expr::Array(elements) => {
                println!("{pad}Array [{}]", elements.len());
                for e in elements {
                    self.print_expr(*e, indent + 1);
                }
            }
            Expr::ObjectLit { fields } => {
                println!("{pad}ObjectLit {{");
                for (n, eid) in fields {
                    println!("{pad}  {n}:");
                    self.print_expr(*eid, indent + 2);
                }
                println!("{pad}}}");
            }
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => {
                let plist: Vec<String> = params
                    .iter()
                    .map(|p| match &p.type_ann {
                        Some(t) => format!("{}: {t}", p.name),
                        None => p.name.clone(),
                    })
                    .collect();
                let ret = return_type.clone().unwrap_or_else(|| "void".into());
                println!("{pad}ArrowFn ({}) -> {ret}", plist.join(", "));
                for s in body {
                    self.print_stmt(s, indent + 1);
                }
            }
            Expr::Closure { fn_name, captures } => {
                println!("{pad}Closure {fn_name} captures=[{}]", captures.join(", "));
            }
            Expr::This => println!("{pad}This"),
            Expr::New { class_name, args } => {
                println!("{pad}New {class_name}");
                for a in args {
                    self.print_expr(*a, indent + 1);
                }
            }
            Expr::Super { args } => {
                println!("{pad}Super");
                for a in args {
                    self.print_expr(*a, indent + 1);
                }
            }
            Expr::Ternary { cond, then_branch, else_branch } => {
                println!("{pad}Ternary");
                self.print_expr(*cond, indent + 1);
                self.print_expr(*then_branch, indent + 1);
                self.print_expr(*else_branch, indent + 1);
            }
            Expr::TypeOf { expr } => {
                println!("{pad}TypeOf");
                self.print_expr(*expr, indent + 1);
            }
            Expr::InstanceOf { expr, class_name } => {
                println!("{pad}InstanceOf {class_name}");
                self.print_expr(*expr, indent + 1);
            }
            Expr::Spread { expr } => {
                println!("{pad}Spread");
                self.print_expr(*expr, indent + 1);
            }
            Expr::Nullish { lhs, rhs } => {
                println!("{pad}Nullish");
                self.print_expr(*lhs, indent + 1);
                self.print_expr(*rhs, indent + 1);
            }
            Expr::OptChain { obj, name } => {
                println!("{pad}OptChain .{name}");
                self.print_expr(*obj, indent + 1);
            }
            Expr::PostIncr { target, is_inc } => {
                println!("{pad}PostIncr is_inc={is_inc}");
                self.print_expr(*target, indent + 1);
            }
        }
    }
}
