//! `desugar_classes` Phase H.3.b dispatcher synthesis (chunk 181,
//! 2026-06-28).
//!
//! Extracted from `ast/desugar_classes.rs` (Pass 1 sub-section after
//! method_owners + chain_methods build, before factory body assembly).
//! Mutates `ast.exprs` via `add_expr` (typecheck-clean stub bodies)
//! and pushes a `Stmt::FnDecl` per chain method into `appended`. SSA
//! intercepts the stub (`__dispatch_<M>` interception in
//! `lower_expr`'s Call arm) so the body never runs at runtime — but
//! the typecheck-clean shape keeps `check.rs` happy + falls back
//! cleanly if the SSA fast path is ever bypassed.
//!
//! Phase H.3.b rules emitted here:
//!   * one `__dispatch_<M>(__this: Base, ...method_params) -> R`
//!     FnDecl per multi-owner method where owners[0] is ancestor of
//!     every other owner.
//!   * single-owner methods stay on the static `__cm_<Owner>__M`
//!     path — no dispatcher fn, no extra indirection.
//!
//! Same mutate-ast-via-add_expr pattern as `desugar_classes_super`.

use super::desugar_classes_super::ClassIndexEntry;
use super::*;
use std::collections::{HashMap, HashSet};

pub(super) fn emit_dispatch_method_stubs(
    ast: &mut Ast,
    class_index: &[ClassIndexEntry],
    method_owners: &HashMap<String, Vec<String>>,
    chain_methods: &HashSet<String>,
    appended: &mut Vec<Stmt>,
) {
    // Phase H.3.b — emit `__dispatch_<method>(__this, args...)` for every
    // method whose name has multiple owners (the override case). Body is
    // an instanceof-chain checking subclasses deepest-first, falling
    // through to the base owner's `__cm_<Base>__<method>`. Single-owner
    // methods stay on the static `__cm_<Owner>__M` path — no dispatcher
    // fn, no extra indirection.
    for (m_name, owners) in method_owners {
        if !chain_methods.contains(m_name) {
            continue;
        }
        // Locate the base owner's method to copy its signature.
        let base_owner = &owners[0];
        let (_, _, base_tp, _, _, _, _, base_methods, _) = class_index
            .iter()
            .find(|(_, n, ..)| n == base_owner)
            .expect("base owner must exist in class_index");
        let base_method = base_methods
            .iter()
            .find(|m| mangle_key(&m.name) == *m_name)
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
            // 398-01 — the stub copies the base signature, so its
            // generic list is the base class's plus the method's own.
            type_params: base_tp
                .iter()
                .chain(base_method.type_params.iter())
                .cloned()
                .collect(),
            params,
            return_type: base_method.return_type.clone(),
            body,
            is_generator: false,
            span: crate::lexer::Span { start: 0, end: 0 },
        });
    }
}
