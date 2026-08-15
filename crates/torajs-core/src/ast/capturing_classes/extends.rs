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

/// The claim-time `extends` bookkeeping for a class this lane just
/// took (moved verbatim from the parent's claim path at the
/// file-size cap): where its `super(…)` lands — a ctor-less class
/// forwards, and skipping it transitively keeps every hop off the
/// rest-param promotion bar (the parent's own target is already
/// resolved; siblings lower top-down, so one lookup composes the
/// chain) — and, 405-01 face 2, the REAL-class parent record that
/// makes `desugar_classes` mint the `__ctorany_<P>` twin the
/// `super(…)` spelling calls. A ctor-less subclass forwards to the
/// parent, so the forward TARGET may be the real class even when
/// the direct parent is a lane sibling — whichever real name the
/// chain lands on is recorded.
pub(super) fn record_claim_tables(ast: &mut Ast, s: &Stmt, new: &str) {
    let Stmt::ClassDecl { ctor, parent, .. } = s else {
        return;
    };
    let target = match (ctor, parent) {
        (None, Some(p)) => ast
            .es5_ctor_forward
            .get(p)
            .cloned()
            .unwrap_or_else(|| p.clone()),
        _ => new.to_string(),
    };
    ast.es5_ctor_forward.insert(new.to_string(), target);
    if let Some(p) = parent {
        let landing = ast.es5_ctor_forward.get(p).cloned().unwrap_or(p.clone());
        for n in [p, &landing] {
            if ast.top_root_real_classes.contains(n) {
                ast.es5_real_parents.insert(n.clone());
            }
        }
    }
}

/// Does any statement in `body` say `super(…)` — the CALL form only?
/// Asked of static bodies (405-01 face 3): `super.m(…)` in a static
/// home object resolves through the parent CLASS (`P.m.call(this)`)
/// and lowers fine, but a bare `super(…)` outside a constructor is
/// not a shape §13.3.7.1 gives a meaning to, so it stays declined.
pub(super) fn body_says_super_call(ast: &Ast, body: &[Stmt]) -> bool {
    let mut ctor_sites = Vec::new();
    for s in body {
        collect_super_in_stmt(ast, s, &mut ctor_sites);
    }
    !ctor_sites.is_empty()
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
/// half reads through the home object's parent (§13.3.7.1): an
/// INSTANCE body's super base is `P.prototype`, a STATIC body's is
/// the parent class itself (405-01 face 3) — either way the runtime
/// lookup rides the chain the lane already links.
pub(super) fn rewrite_super_sites(
    ast: &mut Ast,
    body: &[Stmt],
    parent: &str,
    static_home: bool,
    _class_binding: &str,
) {
    let mut ctor_sites = Vec::new();
    let mut method_sites = Vec::new();
    for s in body {
        collect_super_in_stmt(ast, s, &mut ctor_sites);
        collect_supercall_in_stmt(ast, s, &mut method_sites);
    }
    let ctor_target = ctor_forward_target(ast, parent);
    for (eid, args) in ctor_sites {
        // A REAL-class target (405-01 face 2) calls its
        // `__ctorany_<P>` receiver-polymorphic twin directly — the
        // class's own `.call` correctly throws per §10.3.1, and the
        // mono ctor reads baked struct offsets a dynobj instance
        // never has. The twin's head is `(__this, __new_target,
        // …user)`; the newTarget slot rides `undefined` — the true
        // value is the subclass binding, but spelling that name in
        // its own init expression defeats the capture census
        // (recorded residue: `new.target` inside the parent ctor
        // reads undefined on this lane).
        if ast.es5_real_parents.contains(&ctor_target) {
            let callee = ast.add_expr(Expr::Ident(format!("__ctorany_{ctor_target}")));
            let this = ast.add_expr(Expr::This);
            let nt = ast.add_expr(Expr::Ident("undefined".to_string()));
            let mut full = vec![this, nt];
            full.extend(args);
            ast.exprs[eid.0 as usize] = Expr::Call { callee, args: full };
            continue;
        }
        let callee = call_member(ast, &ctor_target, &[]);
        ast.exprs[eid.0 as usize] = Expr::Call {
            callee,
            args: this_first(ast, args),
        };
    }
    for (eid, mname, args) in method_sites {
        let proto_path = ["prototype", mname.as_str()];
        let static_path = [mname.as_str()];
        let path: &[&str] = if static_home {
            &static_path
        } else {
            &proto_path
        };
        let callee = call_member(ast, parent, path);
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
pub(super) fn implicit_derived_ctor(
    ast: &mut Ast,
    parent: &str,
    _class_binding: &str,
) -> (Vec<Param>, Vec<Stmt>) {
    let target = ctor_forward_target(ast, parent);
    let call = if ast.es5_real_parents.contains(&target) {
        // Real-class target — the `__ctorany_<P>` direct call (see
        // `rewrite_super_sites`), spread-forwarding the rest param.
        let callee = ast.add_expr(Expr::Ident(format!("__ctorany_{target}")));
        let this = ast.add_expr(Expr::This);
        let nt = ast.add_expr(Expr::Ident("undefined".to_string()));
        let rest_ref = ast.add_expr(Expr::Ident("args".to_string()));
        let spread = ast.add_expr(Expr::Spread { expr: rest_ref });
        ast.add_expr(Expr::Call {
            callee,
            args: vec![this, nt, spread],
        })
    } else {
        let callee = call_member(ast, &target, &[]);
        let rest_ref = ast.add_expr(Expr::Ident("args".to_string()));
        let spread = ast.add_expr(Expr::Spread { expr: rest_ref });
        let args = this_first(ast, vec![spread]);
        ast.add_expr(Expr::Call { callee, args })
    };
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
