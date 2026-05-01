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
pub fn desugar_classes(ast: &mut Ast) {
    // Pass 1 — extract every ClassDecl. After this loop the original
    // ClassDecl stmts are replaced by their generated TypeDecl in-place;
    // ctor / methods / factory FnDecls accumulate in `appended`.
    let mut method_to_class: std::collections::HashMap<String, String> =
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
                parent,
                fields,
                ctor,
                methods,
            } => Some((
                i,
                name.clone(),
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
    for (_, cname, parent, _, _, _) in &class_index {
        parent_map.insert(cname.clone(), parent.clone());
    }
    // Detect missing-parent and cycle errors. We don't allow forward
    // references to classes that come later in source order — every
    // ancestor must be declared before its descendants. This keeps
    // field-flattening + factory-emission order trivially correct.
    let mut declared_so_far: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (_, cname, parent, _, _, _) in &class_index {
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
    for (_, cname, parent, fields, _, _) in &class_index {
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

    // Build the method dispatch table. Each method name resolves to the
    // class that declares it; subclass redeclaration of an inherited
    // method (override) is rejected for M5.2.a — virtual dispatch is M5.3.
    for (_, cname, _, _, _, methods) in &class_index {
        // Walk up the chain to detect overrides.
        for m in methods {
            // Check ancestors.
            let mut cur = parent_map.get(cname).cloned().flatten();
            while let Some(anc) = cur {
                if let Some(owner) = method_to_class.get(&m.name)
                    && (owner == &anc || method_owner_is_in_chain(&parent_map, owner, &anc))
                {
                    panic!(
                        "M5.2.a: subclass `{cname}` overrides parent method `{}` \
                         (declared on `{owner}`) — virtual dispatch is M5.3, \
                         rename for now",
                        m.name
                    );
                }
                cur = parent_map.get(&anc).cloned().flatten();
            }
            // Check siblings (any other class).
            if let Some(prev) = method_to_class.get(&m.name) {
                panic!(
                    "M5.2: method name `{}` is declared on both `{prev}` and `{cname}` — \
                     no virtual dispatch yet (rename one, or wait for M5.3)",
                    m.name
                );
            }
            method_to_class.insert(m.name.clone(), cname.clone());
        }
    }
    // Inherited methods: subclass instances should resolve a parent's
    // method too. Walk each class's chain and add (parent_method →
    // declaring_class) entries as additional resolution hints.
    // Implementation: when handling `c.method()` rewriting, the dispatch
    // table is keyed only by method name. If a method is declared on
    // class A and class B extends A, then `b.someParentMethod()` works
    // because `someParentMethod` is in `method_to_class` mapped to A.
    // No extra work needed here.

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
    for (_, cname, _, _, _, _) in &class_index {
        let combined = full_fields.get(cname).unwrap();
        let mut init_pairs: Vec<(String, ExprId)> = Vec::with_capacity(combined.len());
        let mut prelude: Vec<Stmt> = Vec::new();
        for (fname, fty) in combined {
            if fty.ends_with("[]") {
                let local = format!("__def_arr_{cname}_{fname}");
                let arr_lit = ast.add_expr(Expr::Array(Vec::new()));
                prelude.push(Stmt::LetDecl {
                    mutable: false,
                    name: local.clone(),
                    type_ann: Some(fty.clone()),
                    init: arr_lit,
                });
                let ident_id = ast.add_expr(Expr::Ident(local));
                init_pairs.push((fname.clone(), ident_id));
            } else {
                let init_expr = default_init_for_type(fty);
                let id = ast.add_expr(init_expr);
                init_pairs.push((fname.clone(), id));
            }
        }
        class_field_inits.insert(cname.clone(), init_pairs);
        class_field_preludes.insert(cname.clone(), prelude);
    }

    // Pass 1.5 — rewrite `super(args)` inside each subclass's ctor body
    // into a Call to `__cm_<Parent>__ctor(__this, args)`. Must run before
    // pass 2 (which rewrites `Expr::This` and method-call shapes).
    for (_, cname, parent, _, ctor, _) in &class_index {
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
                    if let Some(cname) = method_to_class.get(&m_name) {
                        // `__cm_` prefix signals to check.rs / ssa_lower
                        // that the first arg is the receiver and must be
                        // passed by borrow (not consumed). See
                        // `is_class_method_name` in check.rs.
                        let mangled = format!("__cm_{cname}__{m_name}");
                        let new_callee = ast.add_expr(Expr::Ident(mangled));
                        let mut new_args = Vec::with_capacity(args_clone.len() + 1);
                        new_args.push(obj_id);
                        new_args.extend(args_clone);
                        ast.exprs[i] = Expr::Call {
                            callee: new_callee,
                            args: new_args,
                        };
                    }
                }
            }
            _ => {}
        }
    }

    // Pass 3 — rewrite the stmt list. Replace each ClassDecl in-place
    // with its TypeDecl (using the flattened field list so subclasses
    // carry parent fields too), and accumulate the generated FnDecls.
    for (idx, cname, _parent, _own_fields, ctor, methods) in class_index {
        let type_decl = Stmt::TypeDecl {
            name: cname.clone(),
            type_params: Vec::new(),
            fields: full_fields[&cname].clone(),
        };
        ast.stmts[idx] = type_decl;

        // Constructor → C__ctor(__this: C, params...): void { body }
        let mut ctor_params_for_factory: Vec<Param> = Vec::new();
        if let Some(c) = &ctor {
            ctor_params_for_factory = c.params.clone();
            let mut params: Vec<Param> = Vec::with_capacity(c.params.len() + 1);
            params.push(Param {
                name: "__this".into(),
                type_ann: Some(cname.clone()),
                default: None,
                is_rest: false,
            });
            params.extend(c.params.iter().cloned());
            appended.push(Stmt::FnDecl {
                name: format!("__cm_{cname}__ctor"),
                type_params: Vec::new(),
                params,
                return_type: Some("void".into()),
                body: c.body.clone(),
            });
        }

        // Methods → __cm_C__m(__this: C, params...): R { body }
        for m in &methods {
            let mut params: Vec<Param> = Vec::with_capacity(m.params.len() + 1);
            params.push(Param {
                name: "__this".into(),
                type_ann: Some(cname.clone()),
                default: None,
                is_rest: false,
            });
            params.extend(m.params.iter().cloned());
            appended.push(Stmt::FnDecl {
                name: format!("__cm_{cname}__{}", m.name),
                type_params: Vec::new(),
                params,
                return_type: m.return_type.clone(),
                body: m.body.clone(),
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
            &class_field_inits[&cname],
            class_field_preludes
                .get(&cname)
                .cloned()
                .unwrap_or_default(),
            ctor.as_ref(),
        );
        appended.push(Stmt::FnDecl {
            name: format!("__new_{cname}"),
            type_params: Vec::new(),
            params: ctor_params_for_factory,
            return_type: Some(cname.clone()),
            body: factory_body,
        });
    }

    ast.stmts.extend(appended);
}

/// Build a default-initializer Expr for a type annotation string. Used by
/// `desugar_classes` to seed the factory's object-literal at the top of
/// `__new_C`. The constructor (if any) is responsible for overwriting
/// these defaults with caller-provided values; the defaults exist so the
/// object is well-typed even on fields a buggy constructor forgets to
/// touch.
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
        _ => Expr::Number(0.0),
    }
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
        Stmt::Expr(eid) | Stmt::Throw(eid) => collect_super_in_expr(ast, *eid, out),
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
        Expr::TypeOf { expr } | Expr::Spread { expr } => collect_super_in_expr(ast, *expr, out),
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
    field_inits: &[(String, ExprId)],
    prelude: Vec<Stmt>,
    ctor: Option<&ClassCtor>,
) -> Vec<Stmt> {
    let obj_lit = ast.add_expr(Expr::ObjectLit {
        fields: field_inits.to_vec(),
    });
    let let_this = Stmt::LetDecl {
        mutable: true,
        name: "__this".into(),
        type_ann: Some(cname.to_string()),
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
    if fn_defaults.is_empty() {
        return;
    }
    let n = ast.exprs.len();
    for i in 0..n {
        if let Expr::Call { callee, args } = &ast.exprs[i] {
            let callee = *callee;
            let args_len = args.len();
            // Look up callee's name. Direct Ident only.
            let name = match ast.get_expr(callee) {
                Expr::Ident(n) => n.clone(),
                _ => continue,
            };
            let Some(defaults) = fn_defaults.get(&name) else {
                continue;
            };
            if args_len >= defaults.len() {
                continue;
            }
            // Append defaults for missing positions.
            let mut new_args = match &ast.exprs[i] {
                Expr::Call { args, .. } => args.clone(),
                _ => unreachable!(),
            };
            let mut ok = true;
            for j in args_len..defaults.len() {
                if let Some(default_eid) = defaults[j] {
                    new_args.push(default_eid);
                } else {
                    // Missing required arg — leave alone, typecheck
                    // will report.
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
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) => walk_expr(ast, *eid, bound, out),
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
        Stmt::Expr(eid) | Stmt::Return(Some(eid)) => {
            scan_expr_for_calls(ast, *eid, called)
        }
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
        Expr::TypeOf { expr } | Expr::Spread { expr } => scan_expr_for_calls(ast, *expr, out),
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
        Expr::TypeOf { expr } | Expr::Spread { expr } => walk_expr(ast, *expr, bound, out),
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
