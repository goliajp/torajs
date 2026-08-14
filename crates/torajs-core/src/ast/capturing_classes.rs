//! A nested `class` declaration that reads something from the scope
//! around it.
//!
//! ```text
//! { let a = 7; class K { m() { return a } }; new K().m() }   // bun: 7
//! ```
//!
//! [`super::hoist_nested_classes`] gets a nested class into the class
//! machinery by lifting it to the top level, and a top-level body sees
//! no outer local — so it lifts only the capture-free ones and leaves
//! the rest loud. This module is the other half: the ones it leaves.
//!
//! ## Why not an `__env` channel on the methods
//!
//! The obvious mirror of [`super::nested_fns_capture`] is to give
//! method bodies an env. But a class method is dispatched statically
//! (`__cm_<C>__<m>(__this, …)` through a vtable), so an env channel
//! means touching the ABI, the vtable, `class_layouts` and
//! `method_owners` at once — and it still would not answer the real
//! question, which is IDENTITY: `function outer(){ class K {…} }`
//! mints a FRESH class per call of `outer`, closed over that call's
//! environment. tr models a class as a static entity — one vtable, one
//! layout, one tag — and that is the thing per-call identity
//! contradicts.
//!
//! So a class whose identity varies per evaluation belongs on the
//! runtime-value lane, which tr already has (`__torajs_construct`,
//! rotation 250). The textbook way onto it is the ES5 constructor
//! pattern — exactly what Babel and `tsc --target es5` emit:
//!
//! ```text
//! const K: any = function (p) { this.x = p };
//! K.prototype.m = function () { return a + this.x };
//! ```
//!
//! Everything that shape needs already works (RFC
//! 20260814-capturing-nested-class records the probe table): the
//! function expressions get their env from [`super::lift_arrow_fns`],
//! `new K()` routes through the runtime construct, and `instanceof`
//! answers off the prototype link.
//!
//! ## What routes (blades 1-2)
//!
//! Only a shape this lane reproduces faithfully, and only one that is
//! REJECTED today — a whitelist, so no program that currently answers
//! correctly can be pulled in. Constructor, plain public instance
//! methods, and plain public static methods that do not say `this`;
//! no `extends`, no static fields or static blocks, no accessors, no
//! type params, no computed-key fields, and no compiler-minted free
//! name in a body (`__cm_gen_*` forwarders to a hoisted generator
//! method, `__supercall__*`). Everything else keeps today's loud
//! abort.
//!
//! Recorded deviations are in the RFC: the binding is `const`, `.name`
//! is empty, a declare-only field does not materialize, and the class
//! name is no longer a type name.

use super::desugar_with::walk::{expr_children, stmt_children_ref, stmt_exprs};
use super::free_vars::free_vars_of_body;
use super::{Ast, Expr, ExprId, Stmt, Visibility};

/// Rewrite the class at `stmts[idx]` when this lane covers it.
/// Returns whether it did.
///
/// The binding is α-renamed first, over the statement list that holds
/// it. Receiver promotion — the thing that gives the constructor's
/// `this` somewhere to come from — pairs a binding to its uses by
/// NAME, and refuses when the name is declared more than once in the
/// program: two functions each declaring `class K` would mint two
/// `let K` and take each other down. A minted name is unique by
/// construction, so the question never arises.
pub(super) fn try_rewrite_capturing_class(
    ast: &mut Ast,
    stmts: &mut [Stmt],
    idx: usize,
    counter: &mut u32,
) -> bool {
    if !routes(ast, &stmts[idx]) {
        return false;
    }
    let Stmt::ClassDecl { name, .. } = &stmts[idx] else {
        unreachable!("routes() matched a ClassDecl");
    };
    let old = name.clone();
    let new = format!("__cc{}_{}", *counter, old);
    *counter += 1;
    super::hoist_nested_classes_rename::rename_in_stmts(ast, stmts, &old, &new);
    let taken = std::mem::replace(&mut stmts[idx], Stmt::Multi(Vec::new()));
    stmts[idx] = lower_to_es5(ast, taken);
    true
}

fn routes(ast: &Ast, s: &Stmt) -> bool {
    decline_reason(ast, s).is_none()
}

/// What to say about a `ClassDecl` that reached the checker.
///
/// Exactly one shape gets there, and it is ordinary code: a class
/// nested inside a block or a function body that reads a binding from
/// the scope around it. Such a class cannot be lifted to the top level
/// (nothing up there resolves what it reads), and this lane covers
/// only part of the class surface. Calling that "internal" and asking
/// "desugar didn't run?" reads as a compiler bug report to someone who
/// wrote perfectly good TypeScript; name what is actually missing.
pub(crate) fn unclaimed_class_message(ast: &Ast, s: &Stmt) -> String {
    let name = match s {
        Stmt::ClassDecl { name, .. } => name.as_str(),
        _ => "?",
    };
    let why = decline_reason(ast, s).unwrap_or("the class desugar did not claim it");
    format!(
        "class `{name}` is declared inside a block or a function body and reads a \
         binding from around it, which is not supported yet because {why}"
    )
}

/// Why this class stays off the lane, phrased for the person who wrote
/// it. `None` means it routes.
///
/// A nested class only reaches the checker when the hoist declined it,
/// and the hoist declines exactly the ones that read something from
/// around them — so whoever asks this question already knows the class
/// captures, and what is missing is the SECOND half of the sentence.
fn decline_reason(ast: &Ast, s: &Stmt) -> Option<&'static str> {
    let Stmt::ClassDecl {
        name,
        type_params,
        parent,
        is_abstract,
        fields,
        static_init,
        ctor,
        methods,
        static_methods,
    } = s
    else {
        return Some("it is not a class declaration");
    };
    if parent.is_some() {
        return Some("it extends another class");
    }
    if *is_abstract {
        return Some("it is abstract");
    }
    if !type_params.is_empty() {
        return Some("it has type parameters");
    }
    // Static fields and static blocks both run at class-evaluation
    // time with `this` bound to the class object, and an initializer
    // saying `this` would read the ENCLOSING `this` once inlined at
    // the store. A static METHOD has no such problem: `K.s =
    // function () { … this … }` called as `K.s()` binds `this` to K,
    // which is what §10.2.1.2 wants.
    if !static_init.is_empty() {
        return Some("it has a static field or a static block");
    }
    // A computed member name parses into a `__ccm_<n>__` sentinel,
    // and where the sentinel then LIVES depends on what it named: a
    // method keeps it as the member name, an instance field turns it
    // into a keyed write inside the constructor, a static field goes
    // to its own side table. Asking the side table the parser filled
    // answers all three at once — reading the sentinel back out of
    // whichever place it landed would answer each of them differently,
    // which is how two of these shapes used to report as "a generator
    // or otherwise compiler-rewritten method" and "a body names
    // something the compiler minted".
    if ast.class_computed_keys.keys().any(|(c, _)| c == name)
        || ast
            .class_computed_static_fields
            .iter()
            .any(|(c, _, _)| c == name)
        || fields.iter().any(|(f, _)| f.starts_with("__ccm_"))
    {
        return Some("it has a computed member name");
    }
    if let Some(m) = methods.iter().chain(static_methods.iter()).find(|m| {
        m.is_abstract
            || m.accessor_kind.is_some()
            || m.visibility != Visibility::Public
            || m.name.starts_with("__")
    }) {
        return Some(if m.accessor_kind.is_some() {
            "it has a getter or a setter"
        } else if m.is_abstract {
            "it has an abstract method"
        } else if m.visibility != Visibility::Public {
            "it has a private or protected member"
        } else {
            "it has a generator or otherwise compiler-rewritten method"
        });
    }
    // A static method saying `this` means the class object, and
    // reaching it through `K.s = function () { … this … }` needs the
    // receiver channel that only an object receiver arranges today —
    // the store promotes, but the binding on the left is a function
    // value, and the body's `this` stays an unbound capture.
    if static_methods.iter().any(|m| body_says_this(ast, &m.body)) {
        return Some("a static method in it uses `this`");
    }
    // A body naming a compiler-minted global is not ordinary user
    // nesting: `__cm_gen_<C>__<m>` is the top-level generator method
    // the parser hoisted out (it cannot capture either), and
    // `__supercall__*` belongs to a parent link rejected above. The
    // caller already decided this class captures; the walk here only
    // screens for those names, so the prebound set need cover no more
    // than what would otherwise report a `__` name spuriously.
    let prebound = vec![name.clone(), "arguments".to_string()];
    let synthetic_free = |params: &[super::Param], body: &[Stmt]| -> bool {
        let mut bound = prebound.clone();
        bound.extend(params.iter().map(|p| p.name.clone()));
        free_vars_of_body(ast, &bound, body)
            .iter()
            .any(|n| n.starts_with("__"))
    };
    let ctor_synthetic = ctor
        .as_ref()
        .is_some_and(|c| synthetic_free(&c.params, &c.body));
    if ctor_synthetic
        || methods
            .iter()
            .chain(static_methods.iter())
            .any(|m| synthetic_free(&m.params, &m.body))
    {
        return Some("a body in it names something the compiler minted");
    }
    None
}

/// Does anything in `body` say `this`? A nested `function` expression
/// binds its own, so descending into one over-answers — which is the
/// safe direction: over-answering keeps a shape on the lane it is
/// already on.
fn body_says_this(ast: &Ast, body: &[Stmt]) -> bool {
    let mut pending: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = pending.pop() {
        if stmt_exprs(s).into_iter().any(|e| expr_says_this(ast, e)) {
            return true;
        }
        pending.extend(stmt_children_ref(s));
    }
    false
}

fn expr_says_this(ast: &Ast, root: ExprId) -> bool {
    let mut pending = vec![root];
    while let Some(eid) = pending.pop() {
        match ast.get_expr(eid) {
            Expr::This => return true,
            // An arrow's body is a statement list, not a child expr.
            Expr::ArrowFn { body, .. } if body_says_this(ast, body) => return true,
            _ => {}
        }
        pending.extend(expr_children(ast, eid));
    }
    false
}

/// `class K { constructor(p){…} m(){…} }` →
/// `const K: any = function (p) {…}; K.prototype.m = function () {…};`
///
/// The function expressions register in `fn_expr_exprs` rather than
/// reading as arrows: that is what gives them a `.prototype` and a
/// dynamic `this`, both of which a class body assumes.
fn lower_to_es5(ast: &mut Ast, class: Stmt) -> Stmt {
    let Stmt::ClassDecl {
        name,
        ctor,
        methods,
        static_methods,
        ..
    } = class
    else {
        unreachable!("routes() matched a ClassDecl");
    };
    let (params, body) = match ctor {
        Some(c) => (c.params, c.body),
        None => (Vec::new(), Vec::new()),
    };
    let ctor_eid = ast.add_expr(Expr::ArrowFn {
        params,
        return_type: None,
        body,
    });
    ast.fn_expr_exprs.insert(ctor_eid);
    let mut out = vec![Stmt::LetDecl {
        mutable: false,
        name: name.clone(),
        type_ann: Some("any".to_string()),
        init: ctor_eid,
        is_var: false,
    }];
    for (m, on_prototype) in methods
        .into_iter()
        .map(|m| (m, true))
        .chain(static_methods.into_iter().map(|m| (m, false)))
    {
        let eid = ast.add_expr(Expr::ArrowFn {
            params: m.params,
            return_type: m.return_type,
            body: m.body,
        });
        ast.set_expr_span(eid, m.span);
        ast.fn_expr_exprs.insert(eid);
        // An instance method hangs off `.prototype`; a static one hangs
        // off the constructor itself, which is what makes `K.s()` bind
        // `this` to K.
        let mut recv = ast.add_expr(Expr::Ident(name.clone()));
        if on_prototype {
            recv = ast.add_expr(Expr::Member {
                obj: recv,
                name: "prototype".to_string(),
            });
        }
        let target = ast.add_expr(Expr::Member {
            obj: recv,
            name: m.name.clone(),
        });
        let assign = ast.add_expr(Expr::Assign { target, value: eid });
        out.push(Stmt::Expr(assign));
    }
    Stmt::Multi(out)
}
