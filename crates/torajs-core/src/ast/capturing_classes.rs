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
//! ## What routes (blade 1)
//!
//! Only a shape this lane reproduces faithfully, and only one that is
//! REJECTED today — a whitelist, so no program that currently answers
//! correctly can be pulled in. Constructor plus plain public instance
//! methods; no `extends`, no statics, no accessors, no type params, no
//! computed-key fields, and no compiler-minted free name in a body
//! (`__cm_gen_*` forwarders to a hoisted generator method,
//! `__supercall__*`). Everything else keeps today's loud abort.
//!
//! Recorded deviations are in the RFC: the binding is `const`, `.name`
//! is empty, a declare-only field does not materialize, and the class
//! name is no longer a type name.

use super::free_vars::free_vars_of_body;
use super::{Ast, Expr, Stmt, Visibility};

/// Rewrite `slot` in place when it is a capturing nested class this
/// lane covers. Returns whether it did.
pub(super) fn try_rewrite_capturing_class(ast: &mut Ast, slot: &mut Stmt) -> bool {
    if !routes(ast, slot) {
        return false;
    }
    let taken = std::mem::replace(slot, Stmt::Multi(Vec::new()));
    *slot = lower_to_es5(ast, taken);
    true
}

fn routes(ast: &Ast, s: &Stmt) -> bool {
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
        return false;
    };
    if parent.is_some()
        || *is_abstract
        || !type_params.is_empty()
        || !static_init.is_empty()
        || !static_methods.is_empty()
    {
        return false;
    }
    // A computed member name parses into a `__ccm_` sentinel field
    // whose initializer is a keyed write into an expando dict — a
    // shape this lane does not reproduce.
    if fields.iter().any(|(f, _)| f.starts_with("__ccm_")) {
        return false;
    }
    if methods.iter().any(|m| {
        m.is_abstract
            || m.accessor_kind.is_some()
            || m.visibility != Visibility::Public
            || m.name.starts_with("__")
    }) {
        return false;
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
    if let Some(c) = ctor
        && synthetic_free(&c.params, &c.body)
    {
        return false;
    }
    !methods.iter().any(|m| synthetic_free(&m.params, &m.body))
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
    for m in methods {
        let eid = ast.add_expr(Expr::ArrowFn {
            params: m.params,
            return_type: m.return_type,
            body: m.body,
        });
        ast.set_expr_span(eid, m.span);
        ast.fn_expr_exprs.insert(eid);
        let obj = ast.add_expr(Expr::Ident(name.clone()));
        let proto = ast.add_expr(Expr::Member {
            obj,
            name: "prototype".to_string(),
        });
        let target = ast.add_expr(Expr::Member {
            obj: proto,
            name: m.name.clone(),
        });
        let assign = ast.add_expr(Expr::Assign { target, value: eid });
        out.push(Stmt::Expr(assign));
    }
    Stmt::Multi(out)
}
