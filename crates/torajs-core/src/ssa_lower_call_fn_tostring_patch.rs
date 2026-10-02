//! When the compile-time `f.toString()` fold is sound — the
//! whole-program half of `ssa_lower_call_fn_tostring`.
//!
//! Folding a function's `toString()` into its erased source is only
//! right while nothing can have replaced the method that call reaches:
//! the function's own `toString` (`f.toString = …`) or
//! `Function.prototype.toString`. The compiler sees the whole program,
//! so it can answer that from the tree: the fold stays unless some
//! expression could install a `toString` — a string literal spelling
//! it (the `defineProperty` / `Reflect.set` / computed-key forms), a
//! `.toString` read that is not being called (a store target, a saved
//! reference), an object-literal or class member of that name, or a
//! dynamic-key store into an object or function. Anything else only
//! calls `toString`, which cannot change it.
//!
//! When the scan cannot rule a patch out, the call goes through the
//! any-lane method dispatch instead, which reads the live property.
//! A key built at run time out of other strings is the one spelling
//! this cannot see.

use std::collections::HashSet;

use crate::ast::{Ast, Expr, ExprId, Stmt};
use crate::ssa::{Operand, Type};
use crate::ssa_lower::LowerCtx;

const NAME: &str = "toString";

/// Whether any expression in the program could install a `toString`.
pub(crate) fn program_may_patch_to_string(
    ast: &Ast,
    types: &std::collections::HashMap<ExprId, crate::check::Type>,
) -> bool {
    let mut callees: HashSet<u32> = HashSet::new();
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } | Expr::OptCall { callee, .. } = e {
            callees.insert(callee.0);
        }
    }
    let dynamic_target = |obj: &ExprId| {
        matches!(
            types.get(obj),
            None | Some(
                crate::check::Type::Any
                    | crate::check::Type::Object(_)
                    | crate::check::Type::Function(..)
            )
        )
    };
    let patches = ast.exprs.iter().enumerate().any(|(i, e)| match e {
        Expr::String(s) => *s == *NAME,
        Expr::Member { name, .. } | Expr::OptChain { name, .. } => {
            name == NAME && !callees.contains(&(i as u32))
        }
        Expr::ObjectLit { fields } => fields.iter().any(|(k, _)| k.lossy() == NAME),
        Expr::Assign { target, .. } => matches!(
            ast.get_expr(*target),
            Expr::Index { obj, .. } if dynamic_target(obj)
        ),
        _ => false,
    });
    patches
        || ast.stmts.iter().any(|s| match s {
            Stmt::FnDecl { name, .. } => name.ends_with(NAME) && name.len() > NAME.len(),
            _ => false,
        })
}

/// Whether `obj` is the name of a top-level function declaration —
/// its value is a bare code address (`Type::FnSig`), and there is no
/// function OBJECT to hand a method dispatch (`__torajs_fnprops_cell`
/// answers one only once something bound it). Those keep the fold;
/// a patched `toString` on them is the registered remainder.
pub(crate) fn names_top_level_fn(ctx: &LowerCtx<'_>, obj: ExprId) -> bool {
    let Expr::Ident(name) = ctx.ast.get_expr(obj) else {
        return false;
    };
    ctx.ast
        .stmts
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name: n, .. } if n == name))
}

/// `obj.toString()` through the any-lane dispatch: the receiver boxed
/// (rc-neutral), the live method called, its answer made a string
/// for the checker's `string` result.
pub(crate) fn dispatch(ctx: &mut LowerCtx<'_>, obj: ExprId) -> Operand {
    let recv = ctx.lower_expr(obj);
    let boxed = if ctx.operand_ty(&recv) == Type::Any {
        recv
    } else {
        ctx.box_to_any(recv)
    };
    let out = crate::ssa_lower_any_method_call::emit_any_method_call(
        ctx,
        boxed,
        NAME,
        Operand::ConstPtrNull,
        &[],
    );
    ctx.release_owned_temp(obj, &recv);
    ctx.emit_throw_check_owned(None, Operand::Value(out), Type::Any);
    let s = ctx.coerce_to_str(Operand::Value(out), Type::Any);
    ctx.emit_drop_value(Operand::Value(out), Type::Any);
    s
}
