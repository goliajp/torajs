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
use super::super::{Ast, ClassCtor, Expr, ExprId, Param, Stmt};
use super::install::define_member;
use super::new_target::{self, SuperArgs};

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
pub(super) fn record_claim_tables(ast: &mut Ast, s: &Stmt, new: &str, self_target: &str) {
    let Stmt::ClassDecl { ctor, parent, .. } = s else {
        return;
    };
    // Admission (`decline_reason`) already required a bare-Ident
    // heritage, so the name read-back cannot miss here.
    let parent: Option<String> = ast.parent_ident_name(*parent).map(str::to_string);
    let target = match (ctor, &parent) {
        (None, Some(p)) => ast
            .es5_ctor_forward
            .get(p)
            .cloned()
            .unwrap_or_else(|| p.clone()),
        // A class carrying its own constructor forwards to itself —
        // to the CLASS-SCOPE binding (or its construct twin), so a
        // later write through the container's cannot redirect a
        // `super(…)`.
        _ => self_target.to_string(),
    };
    ast.es5_ctor_forward.insert(new.to_string(), target.clone());
    // A sibling with its own ctor and a parent is a derived
    // constructor: a subclass's `super(…)` lands on `target` and runs
    // this class's step 13 there.
    if ctor.is_some() && parent.is_some() {
        ast.es5_derived_parents.insert(target);
    }
    if let Some(p) = &parent {
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
    let value_parent = ast.es5_value_parents.contains(parent);
    for (eid, args) in ctor_sites {
        // RFC 20260815 knife 2b — a VALUE-shaped parent dispatches at
        // run time: `__torajs_super_value(P, this, [args…])` (a
        // synthetic the checker and lowering intercept by name). The
        // args pack into an array literal so one kernel shape serves
        // the explicit and the implicit (rest-forwarding) ctor alike.
        if value_parent {
            let callee = ast.add_expr(Expr::Ident("__torajs_super_value".to_string()));
            let p = ast.add_expr(Expr::Ident(parent.to_string()));
            let this = ast.add_expr(Expr::This);
            let pack = ast.add_expr(Expr::Array(args));
            // §13.3.7.1 step 5 — SuperCall constructs the parent with
            // the current new.target.
            let nt = ast.add_expr(Expr::NewTarget);
            let call = Expr::Call {
                callee,
                args: vec![p, this, pack, nt],
            };
            // A value-shaped parent's kind is a runtime fact this
            // desugar cannot read (recorded residue: its step 13.c
            // TypeError is missed).
            ast.exprs[eid.0 as usize] = adopt_super_answer(ast, call, false);
            continue;
        }
        // A REAL-class target (405-01 face 2) calls its
        // `__ctorany_<P>` receiver-polymorphic twin directly — the
        // class's own `.call` correctly throws per §10.3.1, and the
        // mono ctor reads baked struct offsets a dynobj instance
        // never has. The twin's head is `(__this, __new_target,
        // …user)`, and the slot takes this constructor's own
        // new.target (§13.3.7.1 step 5).
        if ast.es5_real_parents.contains(&ctor_target) {
            let callee = ast.add_expr(Expr::Ident(format!("__ctorany_{ctor_target}")));
            let this = ast.add_expr(Expr::This);
            let nt = ast.add_expr(Expr::NewTarget);
            let mut full = vec![this, nt];
            full.extend(args);
            let call = Expr::Call { callee, args: full };
            let derived = ast.es5_derived_parents.contains(&ctor_target);
            ast.exprs[eid.0 as usize] = adopt_super_answer(ast, call, derived);
            continue;
        }
        let call = new_target::sibling_super(ast, &ctor_target, SuperArgs::List(args));
        let derived = ast.es5_derived_parents.contains(&ctor_target);
        ast.exprs[eid.0 as usize] = adopt_super_answer(ast, call, derived);
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

/// The SuperProperty sites of one body on this lane (rotation 628) —
/// `super.x` / `super[k]` / `super.x = v` / `super.x++` / `super.m?.()`
/// — through the class desugar's own rewrite in its runtime-base
/// form: `Object.getPrototypeOf(<K>.prototype)` (instance) or
/// `Object.getPrototypeOf(<K>)` (static) is the parent this lane
/// linked with `proto_chain_stmts`, so a function-valued parent
/// (`function Parent() {}`) reads exactly like a class one, and every
/// kernel carries the body's `this` as the §13.3.7 receiver.
pub(super) fn rewrite_super_prop_sites(
    ast: &mut Ast,
    body: &[Stmt],
    parent: &str,
    static_home: bool,
    class_binding: &str,
) {
    let mut sites = super::super::super_collect_prop::SuperPropSites::default();
    for s in body {
        super::super::super_collect_prop::collect_superprop_in_stmt(ast, s, &mut sites);
    }
    super::super::desugar_classes_super_prop::rewrite_sites(
        ast,
        &[],
        super::super::desugar_classes_super_prop_mint::SiteCtx {
            parent_name: Some(parent),
            cname: class_binding,
            is_static: static_home,
            dynamic: true,
            this_receiver: true,
        },
        sites,
    );
}

/// §10.2.2 step 13 at a `super(…)` site whose call answers a value (a
/// runtime-value parent, or another class of this lane through
/// `.call`): an object the parent handed back becomes `this`. The
/// `this` nodes here are the ones `return_override::reshape` turns
/// into the constructor's local; the pick's answer is owned, for the
/// reason `ctor_return.rs` records.
///
/// `derived` names the PARENT whose constructor the call runs: step
/// 13.c refuses a non-object, non-undefined answer only for a derived
/// one (`Ast::es5_derived_parents`).
fn adopt_super_answer(ast: &mut Ast, call: Expr, derived: bool) -> Expr {
    let call_id = ast.add_expr(call);
    let slot = ast.add_expr(Expr::Ident(super::return_override::SUP.to_string()));
    let store = ast.add_expr(Expr::Assign {
        target: slot,
        value: call_id,
    });
    let answer = ast.add_expr(Expr::Ident(super::return_override::SUP.to_string()));
    let incumbent = ast.add_expr(Expr::This);
    let picked =
        super::super::desugar_classes_ctor_return::pick_call(ast, incumbent, answer, derived);
    let target = ast.add_expr(Expr::This);
    let adopt = ast.add_expr(Expr::Assign {
        target,
        value: picked,
    });
    let seq = ast.add_expr(Expr::Sequence {
        left: store,
        right: adopt,
    });
    let value = ast.add_expr(Expr::This);
    Expr::Sequence {
        left: seq,
        right: value,
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
/// The implicit forwarding constructor's rest parameter. Not a name a
/// program can spell: field initializers now run in that constructor's
/// scope (`ctor_parts`), and `w = args.length` must keep reading the
/// OUTER `args`.
pub(super) const REST_ARGS: &str = "__super_args";

/// The constructor's params and body as the lane lowers them.
///
/// A class that wrote no constructor but has field initializers
/// arrives with one anyway: the parser hangs the initializer statements
/// on a synthesized, EMPTY-headed ctor (`field_init_synth_ctors`). For
/// a derived class that is not a constructor the program wrote — it
/// holds no `super(…)` — so taking it as explicit skipped the parent
/// entirely: `class extends Base { w = 2 }` built instances with `w`
/// and none of the parent's fields. It is the implicit forwarding
/// constructor with the initializers run after `super(...args)`
/// returns (§10.2.1.3 step 11 → §7.3.34), the same repair
/// `desugar_classes_default_ctor` makes for the real-class lane.
pub(super) fn ctor_parts(
    ast: &mut Ast,
    ctor: Option<ClassCtor>,
    parent: Option<&str>,
    src_name: &str,
    class_binding: &str,
) -> (Vec<Param>, Vec<Stmt>) {
    match (ctor, parent) {
        (Some(c), Some(p)) if ast.field_init_synth_ctors.contains(src_name) => {
            let (params, mut body) = implicit_derived_ctor(ast, p, class_binding);
            body.extend(c.body);
            (params, body)
        }
        (Some(c), _) => (c.params, c.body),
        (None, Some(p)) => implicit_derived_ctor(ast, p, class_binding),
        (None, None) => (Vec::new(), Vec::new()),
    }
}

pub(super) fn implicit_derived_ctor(
    ast: &mut Ast,
    parent: &str,
    _class_binding: &str,
) -> (Vec<Param>, Vec<Stmt>) {
    let target = ctor_forward_target(ast, parent);
    let call = if ast.es5_value_parents.contains(parent) {
        // RFC 20260815 knife 2b — value-shaped parent: the implicit
        // forwarding ctor hands its rest binding (already a dense
        // any[] array) straight to the runtime dispatch kernel.
        let callee = ast.add_expr(Expr::Ident("__torajs_super_value".to_string()));
        let p = ast.add_expr(Expr::Ident(parent.to_string()));
        let this = ast.add_expr(Expr::This);
        let rest_ref = ast.add_expr(Expr::Ident(REST_ARGS.to_string()));
        let nt = ast.add_expr(Expr::NewTarget);
        let call = Expr::Call {
            callee,
            args: vec![p, this, rest_ref, nt],
        };
        let adopted = adopt_super_answer(ast, call, false);
        ast.add_expr(adopted)
    } else if ast.es5_real_parents.contains(&target) {
        // Real-class target — the `__ctorany_<P>` direct call (see
        // `rewrite_super_sites`), spread-forwarding the rest param.
        let callee = ast.add_expr(Expr::Ident(format!("__ctorany_{target}")));
        let this = ast.add_expr(Expr::This);
        let nt = ast.add_expr(Expr::NewTarget);
        let rest_ref = ast.add_expr(Expr::Ident(REST_ARGS.to_string()));
        let spread = ast.add_expr(Expr::Spread { expr: rest_ref });
        let call = Expr::Call {
            callee,
            args: vec![this, nt, spread],
        };
        let derived = ast.es5_derived_parents.contains(&target);
        let adopted = adopt_super_answer(ast, call, derived);
        ast.add_expr(adopted)
    } else {
        let call = new_target::sibling_super(ast, &target, SuperArgs::Rest);
        let derived = ast.es5_derived_parents.contains(&target);
        let adopted = adopt_super_answer(ast, call, derived);
        ast.add_expr(adopted)
    };
    // The annotation is load-bearing: an unannotated rest lowered as
    // scalar `any` (the packing built `__empty_arr__any` instead of
    // `__empty_arr__any__` and the callee crashed reading it as an
    // array) — `any[]` is what the parser's own `(...args)` infers to
    // by the closure-param pass.
    let params = vec![Param {
        name: REST_ARGS.to_string(),
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
    // `extends null` (rotation 410): the prototype's [[Prototype]] is
    // null itself — reading `P.prototype` off the null binding would
    // throw at class-definition time, which §15.7.14 does not.
    let null_parent = ast.es5_null_parents.contains(parent);
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
    let create_arg = if null_parent {
        ast.add_expr(Expr::Null)
    } else {
        let p = ast.add_expr(Expr::Ident(parent.to_string()));
        ast.add_expr(Expr::Member {
            obj: p,
            name: "prototype".to_string(),
        })
    };
    let value = ast.add_expr(Expr::Call {
        callee: create,
        args: vec![create_arg],
    });
    out.push(Stmt::Expr(ast.add_expr(Expr::Assign { target, value })));

    // Class-side static inheritance (§15.7.14 ClassDefinitionEvaluation
    // step 7: a derived class's own [[Prototype]] is its parent) —
    // 405-01. `D.s()` resolves inherited statics through the function
    // value's user [[Prototype]] chain, and a static added to `P`
    // after this line still flows down. Both argument positions are
    // receiver-safe use shapes (`define_property_target_idents`).
    // A null heritage skips the link: step 7's protoParent-null arm
    // keeps constructorParent = %Function.prototype%.
    if !null_parent {
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
    }

    let d = ast.add_expr(Expr::Ident(class_binding.to_string()));
    let recv = ast.add_expr(Expr::Member {
        obj: d,
        name: "prototype".to_string(),
    });
    let key = ast.add_expr(Expr::String("constructor".to_string().into()));
    let value = ast.add_expr(Expr::Ident(class_binding.to_string()));
    let writable = ast.add_expr(Expr::Bool(true));
    let configurable = ast.add_expr(Expr::Bool(true));
    let fields = vec![
        ("value".into(), value),
        ("writable".into(), writable),
        ("configurable".into(), configurable),
    ];
    out.push(Stmt::Expr(define_member(ast, recv, key, fields)));
}
