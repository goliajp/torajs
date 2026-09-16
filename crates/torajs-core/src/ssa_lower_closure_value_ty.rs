//! The SSA type of an `Expr::Closure` VALUE — split from
//! `ssa_lower_closure.rs` at its size cap. That file mints the cell;
//! this one answers what the minted operand's type is, which the
//! let-decl self-reference path also needs before any cell exists.

use crate::ssa::Type;
use crate::ssa_lower::{LowerCtx, intern_fn_sig};

/// Does the lifted closure `fn_name` carry an object-literal method's
/// `__this` receiver (second param, after `__env`)?
fn fn_has_this_param(ctx: &LowerCtx<'_>, fn_name: &str) -> bool {
    ctx.ast.stmts.iter().any(|s| {
        matches!(s, crate::ast::Stmt::FnDecl { name, params, .. }
            if name == fn_name && params.get(1).is_some_and(|p| p.name == "__this"))
    })
}

/// The SSA type a `Expr::Closure` VALUE has: the lifted fn's interned
/// signature with the hidden leading params stripped.
///
/// Skip `__env`, and for an object-literal method (RFC
/// 20260714-objlit-accessor blade 1) also the `__this` receiver — the
/// closure VALUE's type is the user-facing signature. Keeping the
/// receiver out of every Type is what stops `__ObjLit_n`'s layout from
/// referring to itself: a method slot's sig would name the very struct
/// being interned, and `parse_struct` has no memo-before-fill to break
/// the cycle. The real `(__env, __this, ...user)` ABI is reassembled at
/// the one call arm that knows it (the field-call arm in
/// `ssa_lower_call_struct_method_dispatch`).
///
/// The mint below reads it, and so does the let-decl's self-reference
/// path — which has to size the binding's box BEFORE the closure that
/// fills it exists, and so cannot wait for the minted operand's type.
pub(crate) fn closure_value_ty(ctx: &mut LowerCtx<'_>, fn_name: &str) -> Type {
    let fid = ctx
        .fn_table
        .get(fn_name)
        .copied()
        .unwrap_or_else(|| panic!("ssa-lower: closure target `{fn_name}` not in fn table"));
    let own_sig = ctx
        .fn_sig_ids
        .get(&fid)
        .copied()
        .unwrap_or_else(|| panic!("ssa-lower: closure `{fn_name}` has no interned sig"));
    let (own_params, user_ret_ty) = ctx.fn_sigs[own_sig.0 as usize].clone();
    // RFC 20260810-indirect-argc-abi S1 — the hidden head is now two
    // slots (`__env` + the SSA-injected I64 `__torajs_argc`), plus
    // the object-literal method's `__this` when present.
    let hidden = 2 + usize::from(fn_has_this_param(ctx, fn_name));
    let user_param_tys: Vec<Type> = own_params.iter().skip(hidden).copied().collect();
    Type::Closure(intern_fn_sig(ctx.fn_sigs, user_param_tys, user_ret_ty))
}
