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
//! any-member/any-call lanes). Data-only literal ObjectLits promote
//! too; the method-carrying half is deferred — see the verdict fn's
//! doc.
//!
//! Consumed by BOTH the checker (`check_pipeline::pass_2` register +
//! `check_stmt_let_decl` main-binding widen via the recorded eid
//! set) and the lowerer (`ssa_lower_toplevel_globals::
//! inferred_slot_ty` promote) — one verdict fn is the no-drift
//! contract. Callers gate on `is_var: false` and the named-fn-refs
//! set themselves; this fn judges the init SHAPE only.

use crate::ast::{Ast, Expr, ExprId};

/// True iff `init` is a shape the toplevel whitelist promotes as an
/// `Any` slot — a call result, or a DATA-ONLY literal object literal.
/// Method-carrying ObjectLits (`let iter = { next() {…} }`) are NOT
/// admitted: the Any slot's init lane lowers them through the dynobj
/// lane, whose method `this`-home doesn't yet round-trip (probe tb2:
/// `next()` answered box bits as a number and the main-side `count`
/// read a stale 0 — a silent wrong, worse than the loud
/// unknown-identifier this arm would trade it for). That half stays
/// on the L3b ledger until the objlit-anylane method substrate
/// carries it.
///
/// The data-only literal (`var obj = {};` / `{ a: null }` /
/// `{ p: { q: 1 } }` / `{ xs: [1, 2] }` — the test262 shared-fixture
/// idioms) carries no callable and no accessor, so the tb2 hazard
/// cannot arise: the init lane mints a dynobj (the rotation-204
/// `Any × ObjectLit` arm), reads ride the any-member lanes, and
/// expando writes land on the dynobj cell like any degraded binding.
/// Non-empty ALL-PRIMITIVE literals never reach this fallback — the
/// `__inlobj` arm sits earlier in both consumers and keeps them on
/// the typed-struct lane; only the shapes `__inlobj` refuses (empty,
/// null / nested-literal / array-literal fields) land here. Without
/// this arm those bindings fell into a gap — not degraded (no
/// define/expando trigger), not `__inlobj`, not shaped — and every
/// named-fn read died loud with "unknown identifier". Field VALUES
/// stay literal-only (no Ident / Call / BinOp): a runtime expression
/// could evaluate to a closure and re-open the tb2 hole through an
/// any-member call.
pub(crate) fn any_promote_init(ast: &Ast, init: ExprId) -> bool {
    if let Expr::ObjectLit { .. } = ast.get_expr(init) {
        return data_literal_value(ast, init);
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

/// True iff `e` is a pure data literal: a primitive literal, or an
/// ObjectLit / Array whose members are all pure data literals
/// themselves. Dunder field names reject the whole literal — they
/// are the parser's sentinels for exactly the non-data members
/// (`__getter_` / `__setter_` accessors, `__computed_<n>__` keys,
/// `__spread__`, `__proto__` side-channel), mirroring the
/// `objlit_literal_inlobj_ann` name gate.
fn data_literal_value(ast: &Ast, e: ExprId) -> bool {
    match ast.get_expr(e) {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::BigInt { .. } | Expr::Null => {
            true
        }
        Expr::ObjectLit { fields } => fields.iter().all(|(name, val)| {
            let mut chars = name.chars();
            let head_ok = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$');
            head_ok
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
                && !name.starts_with("__")
                && data_literal_value(ast, *val)
        }),
        Expr::Array(els) => els.iter().all(|el| data_literal_value(ast, *el)),
        _ => false,
    }
}
