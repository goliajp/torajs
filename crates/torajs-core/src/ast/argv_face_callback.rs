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

/// What the higher-order LANES route through the boxed variadic
/// dispatch: the two argv-face shapes above, plus a `__forward_` /
/// `__fwdrecv_` relay whose target reads argument values.
///
/// A declaration passed by value to a callback slot reaches the lane
/// as its relay closure. The relay's typed forwarding call packs
/// exactly its declared parameters — that is all its signature
/// holds — so a direct call through it hands the target a list
/// truncated to the declared arity: `function cb(v, i) { arguments
/// [2] }` answered `undefined` under `[11].map(cb)` and
/// `arguments.length` answered 2 where §23.1.3.21 passes «kValue, k,
/// O». The relay's BOXED adapter enters the target directly with its
/// own argc and argv (`ssa_lower_boxed_entry::relay`), which is the
/// true list, so the lane only has to take that entry.
///
/// The checker's wedge keeps the narrower question: a relay's public
/// type is its declared signature, which the method table already
/// admits.
pub(crate) fn lane_takes_argv_face(ast: &Ast, cb: ExprId) -> bool {
    is_argv_face_callback(ast, cb) || relays_to_argv_body(ast, cb)
}

fn relays_to_argv_body(ast: &Ast, cb: ExprId) -> bool {
    let name = match ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => fn_name,
        Expr::Ident(n) => n,
        _ => return false,
    };
    let Some(target) = name
        .strip_prefix("__forward_")
        .or_else(|| name.strip_prefix("__fwdrecv_"))
    else {
        return false;
    };
    ast.headless_argv_fns.contains(target) || ast.this_first_argv_fns.contains_key(target)
}
