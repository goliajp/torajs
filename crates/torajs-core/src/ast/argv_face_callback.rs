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

/// The callback slot of a replace-family call — `s.replace(p, cb)`,
/// `s.replaceAll(p, cb)` and the protocol spelling
/// `re[Symbol.replace](s, cb)` — with any `as` suffix peeled.
/// §22.1.3.19 / §22.2.6.11 call that slot with «matched, captures…,
/// position, string[, groups]», a count no call site spells.
pub(crate) fn replace_callback_slot(ast: &Ast, callee: ExprId, args: &[ExprId]) -> Option<ExprId> {
    let named = match ast.get_expr(callee) {
        Expr::Member { name, .. } => name == "replace" || name == "replaceAll",
        Expr::Index { index, .. } => matches!(ast.get_expr(*index), Expr::Member { obj, name }
            if name == "replace" && matches!(ast.get_expr(*obj), Expr::Ident(s) if s == "Symbol")),
        _ => false,
    };
    let cb = args.get(1)?;
    named.then(|| super::fnexpr_this_names::peel_as(&ast.exprs, *cb))
}

/// A builtin callback slot whose every lane enters the callee through
/// its boxed entry with the true argument list: the replace family
/// above, and `getOrInsertComputed(key, cb)` on Map / WeakMap (the
/// upsert kernels box the callback and call it with «key»). `as`
/// peeled.
pub(crate) fn boxed_builtin_callback_slot(
    ast: &Ast,
    callee: ExprId,
    args: &[ExprId],
) -> Option<ExprId> {
    if let Expr::Member { name, .. } = ast.get_expr(callee)
        && name == "getOrInsertComputed"
    {
        let cb = args.get(1)?;
        return Some(super::fnexpr_this_names::peel_as(&ast.exprs, *cb));
    }
    replace_callback_slot(ast, callee, args)
}

/// Whether a replace callback has to be entered through its boxed
/// entry. The typed replace lanes (the inline walk and the kernel's
/// typed transmute) hand the callback the spec argument count but
/// only the slots its declaration names, which is all a body needs
/// unless it reads `arguments`. An argv-face callee reads values past
/// its declaration; a relay to a head-less or this-first target that
/// reads `arguments` forwards its own static count, so the target
/// would see the declared arity. The boxed entry carries the whole
/// list on both.
pub(crate) fn replace_callback_needs_boxed(ast: &Ast, cb: ExprId) -> bool {
    let cb = super::fnexpr_this_names::peel_as(&ast.exprs, cb);
    if lane_takes_argv_face(ast, cb) {
        return true;
    }
    let name = match ast.get_expr(cb) {
        Expr::Closure { fn_name, .. } => fn_name,
        Expr::Ident(n) => n,
        _ => return false,
    };
    name.strip_prefix("__forward_")
        .or_else(|| name.strip_prefix("__fwdrecv_"))
        .is_some_and(|t| {
            ast.headless_argc_fns.contains(t) || ast.this_first_argc_fns.contains_key(t)
        })
}
