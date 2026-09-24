//! Which callback-slot expressions are argv-face callees — the
//! callables whose signature leads with the synthetic `__torajs_argv`
//! instead of positional slots, so a higher-order lane must reach
//! them through the boxed variadic dispatch (the dual-entry adapter
//! feeds the true argc + argv) rather than a direct or devirt call.
//!
//! Two shapes, both named by the argv-face collectors in
//! `arguments_object`: the inline fn-expr the HOF-anon arm admitted
//! (`closure_argv_fns`), and a binding the safe-chain walk routed to
//! the slot (`closure_argv_locals`). Every array / Map / Set / sort /
//! `Array.from` lane and the checker's HOF wedge ask this one
//! question; it used to be spelled out at each of them.

use super::{Ast, Expr, ExprId};

pub(crate) fn is_argv_face_callback(ast: &Ast, cb: ExprId) -> bool {
    match ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => ast.closure_argv_fns.contains(fn_name),
        Expr::Ident(n) => ast.closure_argv_locals.contains(n),
        _ => false,
    }
}

