//! `__new_<C>` factory-body synthesis — split from
//! `lift_arrow_fns.rs` when the ctor relay's rest-param spread
//! forward pushed that file past the 500-line limit (verbatim move).
//!
//! Called from `ast::desugar_classes_pass3` (the class factory) and
//! mirrored under its own prefix by `ast::fn_constructor`.

use super::{Ast, ClassCtor, Expr, ExprId, Stmt};

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
        ast.add_expr(Expr::Call {
            callee,
            args: Vec::new(),
        })
    } else {
        ast.add_expr(Expr::ObjectLit {
            fields: field_inits.to_vec(),
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
        let cname_str = ast.add_expr(Expr::String(cname.to_string()));
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
        body.push(Stmt::Expr(call));
    }
    let ret_id = ast.add_expr(Expr::Ident("__this".into()));
    body.push(Stmt::Return(Some(ret_id)));
    body
}
