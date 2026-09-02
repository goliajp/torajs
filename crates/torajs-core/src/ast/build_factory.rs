//! `__new_<C>` factory-body synthesis — split from
//! `lift_arrow_fns.rs` when the ctor relay's rest-param spread
//! forward pushed that file past the 500-line limit (verbatim move).
//!
//! Called from `ast::desugar_classes_pass3` (the class factory) and
//! mirrored under its own prefix by `ast::fn_constructor`.

use super::{Ast, ClassCtor, Expr, ExprId, Stmt};
use crate::ast::PropKey;

pub(crate) fn build_factory_body(
    ast: &mut Ast,
    cname: &str,
    type_params: &[String],
    field_inits: &[(String, ExprId)],
    prelude: Vec<Stmt>,
    ctor: Option<&ClassCtor>,
) -> Vec<Stmt> {
    let exotic_root =
        super::desugar_classes_builtin_heritage::exotic_root_parent(ast, cname).map(str::to_owned);
    let exotic = exotic_root.is_some();
    let obj_lit = if let Some(root) = exotic_root {
        // RFC 20260730 blades 1-2 — an exotic-parent class mints a
        // REAL builtin cell, not an ObjectLit; a user DESCENDANT of
        // one mints the same root cell (rotation 451 — `class CP2
        // extends CP` instances must be real Promise cells). The
        // zero-arg magic call is resolved by the lowerer from the
        // enclosing `__new_<C>` fn name (same channel
        // write_class_tag uses); `super(...)` in the ctor applies
        // the builtin's semantics afterwards.
        if !field_inits.is_empty() {
            // Same loud bucket as the strip's direct-class check —
            // exotic cells have no fixed field region.
            panic!(
                "M5.N: `{cname}` — declared fields on an exotic builtin \
                 subclass chain are not yet supported"
            );
        }
        let magic = super::desugar_classes_builtin_heritage::exotic_alloc_self_magic(&root);
        let callee = ast.add_expr(Expr::Ident(magic.to_string()));
        // The buffer-family mint is one kernel shared by eleven
        // kinds — the kind discriminant rides as a literal argument
        // (every other magic stays zero-arg).
        let magic_args = match torajs_buffer::typedarray::Kind::from_name(&root) {
            Some(k) => vec![ast.add_expr(Expr::Number(k as u8 as f64))],
            None => Vec::new(),
        };
        ast.add_expr(Expr::Call {
            callee,
            args: magic_args,
        })
    } else {
        ast.add_expr(Expr::ObjectLit {
            fields: field_inits
                .iter()
                .map(|(n, e)| (PropKey::from(n), *e))
                .collect(),
        })
    };
    let this_ann = if exotic {
        // Exotic instances live in the any world — typing __this as
        // the class name would send member access down the Obj-layout
        // typed tier against an Arr cell.
        "any".to_string()
    } else if type_params.is_empty() {
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
        let cname_str = ast.add_expr(Expr::String(cname.to_string().into()));
        let new_target_id = ast.add_expr(Expr::Call {
            callee: class_ref_callee,
            args: vec![cname_str],
        });
        let mut args: Vec<ExprId> = Vec::with_capacity(c.params.len() + 2);
        args.push(this_id);
        args.push(new_target_id);
        for p in &c.params {
            let pid = ast.add_expr(Expr::Ident(p.name.clone()));
            // A rest param forwards as a SPREAD: a bare `args` ident
            // would get re-packed by apply_rest_args (the ctor fn's
            // own rest tail) into `[args]` — the double-wrap that
            // answered `new C(1,2,3)` an args.length of 1. The
            // spread rides that pass's single-spread arm
            // (Array.from — the forwarders_object_synth precedent).
            args.push(if p.is_rest {
                ast.add_expr(Expr::Spread { expr: pid })
            } else {
                pid
            });
        }
        let call = ast.add_expr(Expr::Call { callee, args });
        // RFC 20260820-ctor-return-override blade 3 — for a class on a
        // chain that touches a value-returning ctor, what the ctor
        // answers may BE the instance (§10.2.2 step 13). Bind the
        // answer, then let the step-13 pick choose between it and what
        // we minted.
        //
        // The mint above stays typed on purpose. An `any`-annotated
        // `let __this` would send the whole object literal down the
        // dynobj lane (`ssa_lower_object_lit`'s first gate), and the
        // instance would lose its class tag and vtable — `instanceof`
        // and typed method dispatch with it. Only the ctor's parameter
        // and this factory's ANSWER widen; a typed argument arriving
        // at an `any` parameter is just a box.
        //
        // The pick's answer takes a slot of its own and is ASSIGNED
        // into it, never bound by the `let` directly. The SSA retains
        // what an assignment stores and does not retain what a `let`
        // takes over, so the borrow-shaped pick needs the assignment
        // to acquire a stake of its own; declared-then-assigned is
        // also exactly the shape the super site is forced into, which
        // is what lets one kernel serve both.
        //
        // Returning the pick inline instead handed back a view that
        // the scope walk then dropped on the way out, so the caller
        // received freed memory. A churn probe cannot see that — an
        // over-release costs nothing. What saw it was a constructor
        // answering a PRIMITIVE, where the mint is the only stake
        // there is: `new D2()` read its instance back as a Str.
        if super::desugar_classes_ctor_return::factory_relays_answer(ast, cname, true) {
            let answer = ast.add_expr(Expr::Ident("__ctor_answer".into()));
            body.push(Stmt::LetDecl {
                mutable: false,
                name: "__ctor_answer".into(),
                type_ann: Some("any".into()),
                init: call,
                is_var: false,
            });
            let minted = ast.add_expr(Expr::Ident("__this".into()));
            let derived = super::desugar_classes_ctor_return::is_derived(ast, cname);
            let picked =
                super::desugar_classes_ctor_return::pick_call(ast, minted, answer, derived);
            let undef = ast.add_expr(Expr::Ident("undefined".into()));
            body.push(Stmt::LetDecl {
                mutable: true,
                name: "__picked".into(),
                type_ann: Some("any".into()),
                init: undef,
                is_var: false,
            });
            let slot = ast.add_expr(Expr::Ident("__picked".into()));
            let assign = ast.add_expr(Expr::Assign {
                target: slot,
                value: picked,
            });
            body.push(Stmt::Expr(assign));
            let ret = ast.add_expr(Expr::Ident("__picked".into()));
            body.push(Stmt::Return(Some(ret)));
            return body;
        }
        body.push(Stmt::Expr(call));
    }
    let ret_id = ast.add_expr(Expr::Ident("__this".into()));
    body.push(Stmt::Return(Some(ret_id)));
    body
}
