//! The `extends` half of the capturing-class lane (RFC 20260814
//! blade 5).
//!
//! A routed subclass lowers to the ES5 inheritance pattern — the same
//! textbook shape Babel and `tsc --target es5` emit:
//!
//! ```text
//! const D: any = function (p) { P.call(this, p); … };
//! D.prototype = Object.create(P.prototype);
//! Object.defineProperty(D.prototype, "constructor",
//!                       { value: D, writable: true, configurable: true });
//! ```
//!
//! Every piece is a probed runtime fact (RFC blade-5 probe table
//! e1-e11): `P.call(this, …)` reaches the parent constructor through
//! the any-lane receiver channel, `Object.create` links the prototype
//! chain `instanceof` walks, a chain of these composes to
//! grandparents, and an accessor installed on the parent's prototype
//! answers through the link.
//!
//! The parent must be a binding THIS lane minted for a static-free
//! sibling class (`Ast::es5_parent_classes`) — that is the whole
//! admit, and `decline` enforces it. A parent with static members
//! would need a class-side prototype link (`Object.setPrototypeOf(D,
//! P)`), which the any lane does not answer member reads through
//! (probe e5), so that face stays declined rather than silently
//! dropping inherited statics.

use super::super::super_collect::{collect_super_in_stmt, collect_supercall_in_stmt};
use super::super::{Ast, Expr, ExprId, Param, Stmt};
use super::install::define_member;

/// Does any statement in `body` say `super(…)` or `super.m(…)`?
/// Asked of static bodies, which have no lowering here: a static
/// method runs with `this` bound to the class FUNCTION, and neither
/// `P.call` nor `P.prototype.m.call` is what §13.3.7.1 resolves super
/// to in that home object. They decline instead.
pub(super) fn body_says_super(ast: &Ast, body: &[Stmt]) -> bool {
    let mut ctor_sites = Vec::new();
    let mut method_sites = Vec::new();
    for s in body {
        collect_super_in_stmt(ast, s, &mut ctor_sites);
        collect_supercall_in_stmt(ast, s, &mut method_sites);
    }
    !ctor_sites.is_empty() || !method_sites.is_empty()
}

/// Rewrite every super site in `body` against the parent BINDING.
/// `super(args)` becomes `P.call(this, args…)` and `super.m(args)`
/// becomes `P.prototype.m.call(this, args…)` — the exact spellings the
/// probe table proves (e1 / e2). Collection walks arrows, so a super
/// inside one rides its lexical `this` the same way the source did.
///
/// The `super(…)` half calls the parent's CTOR-FORWARD target
/// (`es5_ctor_forward`, 405-01) rather than the parent itself: a
/// ctor-less middle class only forwards, and its synthesized
/// forwarder is the rest-param `this`-reader the promotion ABI bar
/// refuses once a subclass consumes it through `.call`. The method
/// half stays on the parent — `P.prototype.m` resolves through the
/// `Object.create` chain at run time either way.
pub(super) fn rewrite_super_sites(ast: &mut Ast, body: &[Stmt], parent: &str) {
    let mut ctor_sites = Vec::new();
    let mut method_sites = Vec::new();
    for s in body {
        collect_super_in_stmt(ast, s, &mut ctor_sites);
        collect_supercall_in_stmt(ast, s, &mut method_sites);
    }
    let ctor_target = ctor_forward_target(ast, parent);
    for (eid, args) in ctor_sites {
        let callee = call_member(ast, &ctor_target, &[]);
        ast.exprs[eid.0 as usize] = Expr::Call {
            callee,
            args: this_first(ast, args),
        };
    }
    for (eid, mname, args) in method_sites {
        let callee = call_member(ast, parent, &["prototype", &mname]);
        ast.exprs[eid.0 as usize] = Expr::Call {
            callee,
            args: this_first(ast, args),
        };
    }
}

/// Where `super(…)` against `parent` actually lands (405-01) — the
/// nearest ancestor with an explicit ctor, itself included; an
/// unrecorded parent answers itself (defensive identity, not a
/// fallback: every admitted parent is lane-claimed and recorded).
fn ctor_forward_target(ast: &Ast, parent: &str) -> String {
    ast.es5_ctor_forward
        .get(parent)
        .cloned()
        .unwrap_or_else(|| parent.to_string())
}

/// `<parent>.<path…>.call` as a member chain.
fn call_member(ast: &mut Ast, parent: &str, path: &[&str]) -> ExprId {
    let mut obj = ast.add_expr(Expr::Ident(parent.to_string()));
    for name in path {
        obj = ast.add_expr(Expr::Member {
            obj,
            name: name.to_string(),
        });
    }
    ast.add_expr(Expr::Member {
        obj,
        name: "call".to_string(),
    })
}

fn this_first(ast: &mut Ast, args: Vec<ExprId>) -> Vec<ExprId> {
    let this = ast.add_expr(Expr::This);
    let mut full = vec![this];
    full.extend(args);
    full
}

/// The implicit ctor of a derived class: §15.7.14 gives a subclass
/// with no constructor `constructor(...args) { super(...args) }`, and
/// on this lane that spells `function (...args) { P.call(this,
/// ...args) }` (probe e8; `arguments` does not resolve in a minted
/// fn-expr, probe e7, so the rest form is the one that works). Minted
/// before `apply_rest_args` / `apply_spread_args` run, which is what
/// packs the call sites. Calls the ctor-forward target, not the
/// parent — see `rewrite_super_sites`.
pub(super) fn implicit_derived_ctor(ast: &mut Ast, parent: &str) -> (Vec<Param>, Vec<Stmt>) {
    let target = ctor_forward_target(ast, parent);
    let callee = call_member(ast, &target, &[]);
    let rest_ref = ast.add_expr(Expr::Ident("args".to_string()));
    let spread = ast.add_expr(Expr::Spread { expr: rest_ref });
    let args = this_first(ast, vec![spread]);
    let call = ast.add_expr(Expr::Call { callee, args });
    // The annotation is load-bearing: an unannotated rest lowered as
    // scalar `any` (the packing built `__empty_arr__any` instead of
    // `__empty_arr__any__` and the callee crashed reading it as an
    // array) — `any[]` is what the parser's own `(...args)` infers to
    // by the closure-param pass.
    let params = vec![Param {
        name: "args".to_string(),
        type_ann: Some("any[]".to_string()),
        default: None,
        is_rest: true,
    }];
    (params, vec![Stmt::Expr(call)])
}

/// The two statements that link `class_binding` under `parent`,
/// emitted between the constructor binding and the member installs —
/// members must land on the LINKED prototype, not the one the
/// function was born with.
///
/// `Object.create` leaves `constructor` answering the parent's, so it
/// is reinstalled with the attributes §15.7.14 gives it (writable,
/// non-enumerable, configurable — probe e4).
pub(super) fn proto_chain_stmts(
    ast: &mut Ast,
    class_binding: &str,
    parent: &str,
    out: &mut Vec<Stmt>,
) {
    let d = ast.add_expr(Expr::Ident(class_binding.to_string()));
    let target = ast.add_expr(Expr::Member {
        obj: d,
        name: "prototype".to_string(),
    });
    let object = ast.add_expr(Expr::Ident("Object".to_string()));
    let create = ast.add_expr(Expr::Member {
        obj: object,
        name: "create".to_string(),
    });
    let p = ast.add_expr(Expr::Ident(parent.to_string()));
    let pproto = ast.add_expr(Expr::Member {
        obj: p,
        name: "prototype".to_string(),
    });
    let value = ast.add_expr(Expr::Call {
        callee: create,
        args: vec![pproto],
    });
    out.push(Stmt::Expr(ast.add_expr(Expr::Assign { target, value })));

    // Class-side static inheritance (§15.7.14 ClassDefinitionEvaluation
    // step 7: a derived class's own [[Prototype]] is its parent) —
    // 405-01. `D.s()` resolves inherited statics through the function
    // value's user [[Prototype]] chain, and a static added to `P`
    // after this line still flows down. Both argument positions are
    // receiver-safe use shapes (`define_property_target_idents`).
    let object = ast.add_expr(Expr::Ident("Object".to_string()));
    let spo = ast.add_expr(Expr::Member {
        obj: object,
        name: "setPrototypeOf".to_string(),
    });
    let d = ast.add_expr(Expr::Ident(class_binding.to_string()));
    let p = ast.add_expr(Expr::Ident(parent.to_string()));
    out.push(Stmt::Expr(ast.add_expr(Expr::Call {
        callee: spo,
        args: vec![d, p],
    })));

    let d = ast.add_expr(Expr::Ident(class_binding.to_string()));
    let recv = ast.add_expr(Expr::Member {
        obj: d,
        name: "prototype".to_string(),
    });
    let key = ast.add_expr(Expr::String("constructor".to_string()));
    let value = ast.add_expr(Expr::Ident(class_binding.to_string()));
    let writable = ast.add_expr(Expr::Bool(true));
    let configurable = ast.add_expr(Expr::Bool(true));
    let fields = vec![
        ("value".to_string(), value),
        ("writable".to_string(), writable),
        ("configurable".to_string(), configurable),
    ];
    out.push(Stmt::Expr(define_member(ast, recv, key, fields)));
}
