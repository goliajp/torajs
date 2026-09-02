//! Statement AST types (chunk 431), extracted verbatim from ast.rs:
//! - SwitchCase / Stmt — the statement tree
//! - ExportStar — the `*` head of an `export * from` clause
//!
//! The class-declaration payloads `Stmt::ClassDecl` carries (ClassCtor
//! / ClassMethod / AccessorKind / Visibility / StaticField /
//! StaticInit) live in the `stmt_class.rs` sibling.
//!
//! Re-exported from `crate::ast` so every consumer keeps the
//! canonical `ast::Stmt` / `ast::ClassMethod` paths.

use super::*;

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
        /// chunk B3 (RFC 20260711 for-in) — `Some(obj Ident ExprId)`
        /// when this loop is a for-in desugar walking the object's
        /// key snapshot. The lowering re-checks each key against the
        /// live object (`any_prop_has`) before the body runs so
        /// properties deleted mid-loop are not visited (ES §14.7.5).
        forin_obj: Option<ExprId>,
        /// The loop was written `for await` (ES §14.7.5.10 — the
        /// async form). This flag is the ONLY marker (hole Z removed
        /// the `.value` Member wrap on `elem_expr` — it conflated the
        /// await unwrap with a real user member). The checker unwraps
        /// a Promise(T) element to T under this flag (every
        /// non-thenable awaits to itself per §27.2); the array lane
        /// routes Promise-typed elements through promise_get_value;
        /// the iterator-protocol lane awaits each `next()` — an async
        /// generator's step methods answer `Promise<IteratorResult>`
        /// per §27.6.
        is_await: bool,
    },
    /// `break;` / `break label;` — exits the innermost enclosing loop
    /// or switch when unlabeled; with a label, exits the enclosing
    /// statement carrying that label (ES §14.9). M1.7.
    Break(Option<String>),
    /// `continue;` / `continue label;` — jumps to the innermost loop's
    /// step (for) or header (while) when unlabeled; with a label, to
    /// the labeled enclosing *iteration* statement's continuation
    /// point (ES §14.8). M1.7.
    Continue(Option<String>),
    /// `label: stmt` — ES §13.13 labeled statement. The label is
    /// retained (rather than stripped at parse time) so `break label`
    /// / `continue label` inside `body` can target it. Stacked labels
    /// (`a: b: stmt`) nest as `Labeled { a, Labeled { b, stmt } }`.
    Labeled {
        label: String,
        body: Box<Stmt>,
    },
    /// `throw <expr>;` — M4. The thrown value's type is whatever
    /// `<expr>` resolves to (currently number-only at the SSA layer).
    /// Lowered to a write into `__torajs_throw_value` + an immediate
    /// return from the enclosing fn (with a sentinel result).
    Throw(ExprId),
    /// `using x = expr;` — Explicit Resource Management (RFC
    /// 20260809). A const-natured lexical binding that registers the
    /// value's `[Symbol.dispose]` method AT BIND TIME and disposes
    /// registered resources in reverse order when the enclosing
    /// lexical scope exits (normal fall-through, throw, break,
    /// return). `desugar_using` — which runs in the AST prelude
    /// BEFORE the generator / async desugars, so state-machine
    /// bodies inherit the try/finally shape — rewrites every one of
    /// these into try/catch/finally + `__torajs_using_*` helper
    /// calls; no later pass and no lowering ever sees this variant.
    /// Multi-binding heads (`using a = x, b = y`) parse to one
    /// UsingDecl per binding under `Stmt::Multi` (LetDecl
    /// convention).
    UsingDecl {
        name: String,
        type_ann: Option<String>,
        init: ExprId,
        /// `await using x = …` (knife 2) — hint async-dispose: the
        /// bind reads `@@asyncDispose` first (`@@dispose` sync
        /// fallback), and the scope's disposal awaits (implicitly,
        /// even for sync methods and null/undefined bindings). Only
        /// parses where `await` itself is legal (§15.8.1 gate).
        is_await: bool,
    },
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
        /// RFC 20260719-fn-tostring-source B1b — source byte range of
        /// the whole declaration (`function` keyword — or its `async `
        /// prefix — through the body's closing brace). (0,0) sentinel
        /// = "no user source": every synthesized FnDecl (forwarders,
        /// desugar products, monomorph clones) writes it, while the
        /// in-place rewrite passes (generators / async) carry the
        /// original declaration's span so toString answers the source
        /// text the user wrote.
        span: crate::lexer::Span,
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
        fields: Vec<(PropKey, String)>,
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
        ///
        /// Per §15.7 the heritage is a LeftHandSideExpression, so this is
        /// an ExprId. A bare name arrives as `Expr::Ident` carrying the
        /// same class-value alias the parser applied before, so every
        /// static path keyed on the name (class_index / class_parents /
        /// super rewrites) reads back exactly what it did — static
        /// resolution is an OPTIMISATION the desugar applies when the
        /// heritage happens to be an `Expr::Ident`, not the shape of the
        /// node. Read the name via `Ast::parent_ident_name`.
        parent: Option<ExprId>,
        /// M-OO.6 — `abstract class C { ... }`. Abstract classes can't be
        /// instantiated (`new C()` rejected at typecheck), and any
        /// concrete (non-abstract) subclass must override every abstract
        /// method along the inheritance chain.
        is_abstract: bool,
        fields: Vec<(PropKey, String)>,
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
    ///   - `export * from "./b"`       → `star: Some(All), source: Some("./b")`
    ///   - `export * as ns from "./b"` → `star: Some(AsNamespace("ns"))`
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
        /// §16.2.3 ExportFromClause — the `*` head. Always paired with
        /// `source`; `named` stays empty because a star export binds
        /// nothing locally.
        #[allow(dead_code)]
        star: Option<ExportStar>,
    },
}

/// §16.2.3 ExportFromClause — the `*` head of `export * from "m"`.
/// Kept out of `named` because the two clauses answer different
/// questions: `named` lists the names this module hands on, the star
/// says "whatever `m` hands on".
#[derive(Debug, Clone, PartialEq)]
pub enum ExportStar {
    /// `export * from "m"` — every named export of `m` except
    /// `default` becomes a named export here, under its own name. A
    /// name this module exports itself shadows the star's.
    All,
    /// `export * as ns from "m"` — `m`'s namespace object becomes one
    /// named export, spelled `ns`. `default` is NOT skipped here: the
    /// namespace object is the export, not its members.
    AsNamespace(String),
}

/// P-SURF S2.1 — prefix of the top-level `function*` the parser hoists a
/// class generator method into (`__cm_gen_<Class>__<method>`). The class
/// name is embedded because `desugar_classes` has to claim these bodies
/// back when rewriting `super.<m>()`: they belong to the class even
/// though they no longer sit in its `methods`.
pub(crate) const GEN_METHOD_PREFIX: &str = "__cm_gen_";

/// P-SURF S2.1 — the parameter carrying the receiver into such a hoisted
/// body. Deliberately not `__this`, which is load-bearing as the first
/// parameter of every `__cm_*` method: the generator desugar turns this
/// parameter into a field of the `__Gen_*` state-machine class, and a
/// field named `__this` collides with the `__this` that class's own
/// `next()` receives.
pub(crate) const GEN_RECV_PARAM: &str = "__genrecv";

/// RFC 20260801-arguments-method-face knife 2b — the trailing param
/// carrying a generator METHOD's real argv into the hoisted
/// `function*`. The body's `arguments` idents are renamed to this
/// while the method parses (same reasoning as [`GEN_RECV_PARAM`]:
/// once the state machine owns the body, `arguments` would denote
/// the `next()` method's own object), and the class-side forwarder
/// passes `[...arguments]` — which the method static-argv face
/// expands to the exact call-site argv.
pub(crate) const GEN_ARGV_PARAM: &str = "__torajs_gen_argv";
