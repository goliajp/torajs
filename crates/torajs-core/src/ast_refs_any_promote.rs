//! S2.35 — the shared any-promotion verdict for un-annotated
//! top-level lets a named-fn body reads.
//!
//! The K.3b whitelist promoted literal shapes, lifted closures,
//! degraded/all-literal object literals — and nothing else, so a
//! call-result init (`let it = (function*(){…})()`, the test262
//! iterator idiom) or a method-carrying literal (`let iter = {
//! next() {…} }`) left the binding main-local and every named-fn
//! read died with "unknown identifier" (rotation 238 census: 1335
//! declared-but-unregistered cases, asyncIter/values/iter on top).
//!
//! The call-init shape promotes as an `Any` slot: the value's
//! static type may be exact, but Any is the one repr every reader
//! dispatches on uniformly (the chunk-809 Any-global machinery
//! boxes the init and settles ownership; reads ride the
//! any-member/any-call lanes). The objlit half is deferred — see
//! the verdict fn's doc.
//!
//! Consumed by BOTH the checker (`check_pipeline::pass_2` register +
//! `check_stmt_let_decl` main-binding widen via the recorded eid
//! set) and the lowerer (`ssa_lower_toplevel_globals::
//! inferred_slot_ty` promote) — one verdict fn is the no-drift
//! contract. Callers gate on `is_var: false` and the named-fn-refs
//! set themselves; this fn judges the init SHAPE only.

use crate::ast::{Ast, Expr, ExprId};

/// True iff `init` is a shape the toplevel whitelist promotes as an
/// `Any` slot — a call result, or an EMPTY object literal.
/// Method-carrying ObjectLits (`let iter = { next() {…} }`) are NOT
/// admitted: the Any slot's init lane lowers them through the dynobj
/// lane, whose method `this`-home doesn't yet round-trip (probe tb2:
/// `next()` answered box bits as a number and the main-side `count`
/// read a stale 0 — a silent wrong, worse than the loud
/// unknown-identifier this arm would trade it for). That half stays
/// on the L3b ledger until the objlit-anylane method substrate
/// carries it.
///
/// The empty literal (`var obj = {};` — the dominant test262 shared-
/// fixture idiom) has no fields at all, so the tb2 hazard cannot
/// arise: the init lane mints an empty dynobj (the rotation-204
/// `Any × ObjectLit` arm), reads ride the any-member lanes, and
/// expando writes land on the dynobj cell like any degraded binding.
/// Without this arm the binding fell into a gap — not degraded (no
/// define/expando trigger), not `__inlobj` (empty is excluded), not
/// shaped — and every named-fn read died loud with
/// "unknown identifier".
pub(crate) fn any_promote_init(ast: &Ast, init: ExprId) -> bool {
    if let Expr::ObjectLit { fields } = ast.get_expr(init) {
        return fields.is_empty();
    }
    let Expr::Call { callee, .. } = ast.get_expr(init) else {
        return false;
    };
    // `new C()` desugars to a `__new_<C>` factory call — the binding
    // has nominal class identity the typed lanes dispatch on, and
    // boxing it away demotes every main-side method call to the
    // any-lane (rotation 238: the destr-param method fixture walked
    // straight into the any×destr silent hole). Class instances are
    // the named-fn-visibility problem of a different wall; never
    // promote them here.
    !matches!(ast.get_expr(*callee), Expr::Ident(n) if n.starts_with("__new_"))
}
