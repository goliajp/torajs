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
        fields: Vec<(String, String)>,
        ctor: Option<ClassCtor>,
        methods: Vec<ClassMethod>,
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
    /// Phase L.2 — names of `async function` declarations recorded by
    /// the parser. desugar_async iterates ast.stmts and, for any
    /// FnDecl whose name is in this set, wraps the return value in a
    /// Promise and shifts the surface return type from T to Promise<T>.
    /// Avoids adding an `is_async: bool` to every FnDecl construction
    /// site.
    pub async_fns: std::collections::HashSet<String>,
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
            fields: class_fields,
            ctor: Some(ctor_with_params),
            methods: vec![next_method],
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
        // Same monomorphization-via-name dance the rest of the
        // codebase uses: type alias `Promise<number>` will get
        // resolved to the concrete `Promise_number` struct shape by
        // check.rs's generic-type machinery.

        // let __async_p = new Promise(<default T>)
        let default_init = default_init_for_type(&inner_ty);
        let default_id = ast.add_expr(default_init);
        let new_promise = ast.add_expr(Expr::New {
            class_name: "Promise".into(),
            args: vec![default_id],
        });
        let p_decl = Stmt::LetDecl {
            mutable: false,
            name: "__async_p".into(),
            type_ann: Some(promise_ty.clone()),
            init: new_promise,
        };

        // L.2 MVP — async fns must have a single tail return. Multi-
        // branch returns trigger a tr ownership bug (silent wrong
        // output: each branch ends up with its own moved Promise
        // instance, neither carrying the resolved value back to the
        // caller). Reject early so the user sees a clear error
        // instead of a mysterious 0.
        let return_count = count_returns(&body);
        if return_count > 1 {
            panic!(
                "async function {name}: {return_count} `return` statements detected. \
                 L.2 MVP only supports a single tail return — early returns hit a tr \
                 ownership tracker bug across branches (the Promise instance gets \
                 moved into one branch's helper call but later branches read the moved \
                 value). Refactor to a single tail return."
            );
        }

        // Rewrite returns inside body. Each `return e;` becomes
        // `__async_p.do_resolve(e); return __async_p;`. Returns with
        // no value get a default-init injected.
        let mut new_body: Vec<Stmt> = Vec::with_capacity(body.len() + 2);
        new_body.push(p_decl);
        for s in body {
            let mut s = s;
            rewrite_returns_for_async(ast, &mut s, &inner_ty);
            new_body.push(s);
        }
        // Tail safety: if control flow falls off the end without an
        // explicit return, hand back the (still-pending) Promise.
        // Skip emitting if the body trivially ends with a return —
        // tr's ownership tracker treats the second access as a
        // double-move even when the path is unreachable.
        if !body_ends_in_return(&new_body) {
            let p_ref = ast.add_expr(Expr::Ident("__async_p".into()));
            new_body.push(Stmt::Return(Some(p_ref)));
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

/// Count `Stmt::Return` occurrences inside `body`, walking into
/// control-flow constructs. Used by `desugar_async` to enforce its
/// "single tail return" MVP constraint.
fn count_returns(body: &[Stmt]) -> usize {
    body.iter().map(count_returns_stmt).sum()
}
fn count_returns_stmt(s: &Stmt) -> usize {
    match s {
        Stmt::Return(_) => 1,
        Stmt::If { then_branch, else_branch, .. } => {
            count_returns_stmt(then_branch)
                + else_branch.as_deref().map_or(0, count_returns_stmt)
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => count_returns_stmt(body),
        Stmt::For { init, body, .. } => {
            init.as_deref().map_or(0, count_returns_stmt) + count_returns_stmt(body)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => count_returns(stmts),
        Stmt::Switch { cases, default, .. } => {
            cases.iter().map(|c| count_returns(&c.body)).sum::<usize>()
                + default.as_ref().map_or(0, |d| count_returns(d))
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            count_returns(body)
                + count_returns(catch_body)
                + finally_body.as_ref().map_or(0, |fb| count_returns(fb))
        }
        _ => 0,
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

/// Recursively rewrite `Stmt::Return(Some(e))` (and `Stmt::Return(None)`)
/// inside `s` into the pair `__async_p.do_resolve(e); return __async_p;`.
/// The desugar guards against multi-branch returns at a higher level
/// (`count_returns > 1` panics with a clear error) so tr's ownership
/// tracker only sees one transfer of `__async_p` per body — the
/// straight-line tail return.
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
            let p_ref = ast.add_expr(Expr::Ident("__async_p".into()));
            let do_resolve_m = ast.add_expr(Expr::Member {
                obj: p_ref,
                name: "do_resolve".into(),
            });
            let call = ast.add_expr(Expr::Call {
                callee: do_resolve_m,
                args: vec![value],
            });
            let p_ret = ast.add_expr(Expr::Ident("__async_p".into()));
            *s = Stmt::Multi(vec![Stmt::Expr(call), Stmt::Return(Some(p_ret))]);
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

    // Snapshot the class metadata first (cloned out so we can mutate
    // ast.stmts in-place without aliasing). M5.2 adds `parent` to the
    // tuple — for inheritance flattening + super(args) rewriting.
    let class_index: Vec<(
        usize,
        String,
        Vec<String>,           // type_params
        Option<String>,
        Vec<(String, String)>,
        Option<ClassCtor>,
        Vec<ClassMethod>,
    )> = ast
        .stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match s {
            Stmt::ClassDecl {
                name,
                type_params,
                parent,
                fields,
                ctor,
                methods,
            } => Some((
                i,
                name.clone(),
                type_params.clone(),
                parent.clone(),
                fields.clone(),
                ctor.clone(),
                methods.clone(),
            )),
            _ => None,
        })
        .collect();

    if class_index.is_empty() {
        return;
    }

    // Build the parent map and validate the inheritance graph.
    let mut parent_map: std::collections::HashMap<String, Option<String>> =
        std::collections::HashMap::new();
    for (_, cname, _tp, parent, _, _, _) in &class_index {
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
    for (_, cname, _tp, parent, _, _, _) in &class_index {
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
    for (_, cname, _tp, parent, fields, _, _) in &class_index {
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
    for (_, cname, _tp, _, _, _, methods) in &class_index {
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
        let (_, _, base_tp, _, _, _, base_methods) = class_index
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
    for (_, cname, _tp, _, _, _, _) in &class_index {
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
    for (_, cname, _tp, parent, _, ctor, _) in &class_index {
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
                        //     `__cm_<C>__<M>`.
                        // (2) Multi-owner forming a single inheritance
                        //     chain (override case) — route through the
                        //     generated `__dispatch_<M>` runtime dispatcher.
                        // (3) Multi-owner across unrelated hierarchies
                        //     (sibling collision) — leave the Member-call
                        //     shape intact; ssa_lower's `Type::Obj`
                        //     Member-call arm picks the right per-class
                        //     `__cm_<C>__<M>` from obj's static type.
                        if owners.len() == 1 {
                            let mangled = format!("__cm_{}__{m_name}", owners[0]);
                            let new_callee = ast.add_expr(Expr::Ident(mangled));
                            let mut new_args = Vec::with_capacity(args_clone.len() + 1);
                            new_args.push(obj_id);
                            new_args.extend(args_clone);
                            ast.exprs[i] = Expr::Call {
                                callee: new_callee,
                                args: new_args,
                            };
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

    // Pass 3 — rewrite the stmt list. Replace each ClassDecl in-place
    // with its TypeDecl (using the flattened field list so subclasses
    // carry parent fields too), and accumulate the generated FnDecls.
    for (idx, cname, type_params, _parent, _own_fields, ctor, methods) in class_index {
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
    }

    ast.stmts.extend(appended);
    // Hand multi-owner method_owners to ssa_lower for the
    // `__dispatch_<M>` runtime-tag dispatch. Single-owner entries are
    // dropped since they don't need runtime resolution.
    ast.method_owners = method_owners
        .into_iter()
        .filter(|(_, owners)| owners.len() > 1)
        .collect();
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
        Stmt::FnDecl { .. } | Stmt::TypeDecl { .. } | Stmt::ClassDecl { .. } => {}
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
        | Expr::Null => {}
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
        Expr::Ident(_) | Expr::String(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null => {}
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
        Expr::String(_) | Expr::Number(_) | Expr::Bool(_) | Expr::Null => {}
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
        id
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
                fields,
                ctor,
                methods,
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
