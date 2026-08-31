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
//! any-member/any-call lanes). Any-slot-safe ObjectLits promote
//! too — including closure/method-carrying shapes since rotation
//! 546; see the verdict fn's doc.
//!
//! Consumed by BOTH the checker (`check_pipeline::pass_2` register +
//! `check_stmt_let_decl` main-binding widen via the recorded eid
//! set) and the lowerer (`ssa_lower_toplevel_globals::
//! inferred_slot_ty` promote) — one verdict fn is the no-drift
//! contract. Callers gate on `is_var: false` and the named-fn-refs
//! set themselves; this fn judges the init SHAPE only.

use crate::ast::{Ast, Expr, ExprId};
use crate::ast_refs::lifted_closure_fn_canon;

/// True iff `init` is a shape the toplevel whitelist promotes as an
/// `Any` slot — a call result, or an any-slot-safe object literal.
/// The tb2 deferral (rotation 238: method-carrying literals whose
/// `this`-home didn't round-trip) is mostly retired as of rotation
/// 546: the all-shaped method literal rides the `__inlobj` + `__mth`
/// typed lane, and receiver-less closure members admit here. What
/// remains out is a METHOD member in an `__inlobj`-refused mix —
/// see the closure arm's comment for the proven `this`-chain hole.
///
/// The admitted literal (`var obj = {};` / `{ a: null }` /
/// `{ p: { q: 1 + 1 } }` / `{ v: mk() }` — the test262
/// shared-fixture idioms) lowers through the init lane: it mints a
/// dynobj (the rotation-204 `Any × ObjectLit` arm), reads ride the
/// any-member lanes, and expando writes land on the dynobj cell
/// like any degraded binding. Non-empty ALL-PRIMITIVE literals
/// never reach this fallback — the `__inlobj` arm sits earlier in
/// both consumers and keeps them on the typed-struct lane; only the
/// shapes `__inlobj` refuses (empty, null / undefined /
/// nested-literal / array-literal / call-valued fields) land here.
/// Without this arm those bindings fell into a gap — not degraded
/// (no define/expando trigger), not `__inlobj`, not shaped — and
/// every named-fn read died loud with "unknown identifier".
pub(crate) fn any_promote_init(ast: &Ast, init: ExprId) -> bool {
    if let Expr::ObjectLit { .. } = ast.get_expr(init) {
        return any_slot_safe_value(ast, init);
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

/// True iff `e` is safe to evaluate and box on the Any-slot init
/// lane: a primitive literal, a statically-shaped expression, a
/// non-`new` call result, or an ObjectLit / Array whose members all
/// are. Dunder field names reject the whole literal — they are the
/// parser's sentinels for exactly the non-data members
/// (`__getter_` / `__setter_` accessors, `__computed_<n>__` keys,
/// `__spread__`, `__proto__` side-channel), mirroring the
/// `objlit_literal_inlobj_ann` name gate.
///
/// The 468-01 remainder (rotation 546) widened this from pure DATA
/// literals: the old refusal of any runtime expression guarded the
/// tb2 method hole, and that hole is closed — probes against the
/// explicitly-`any` spelling of every widened shape (call-valued
/// field, runtime closure field called through any-member, `this`
/// through the any-call lane, call-valued `any[]` element) are
/// bun-equal. `new C(...)` (the `__new_` factory spelling) stays
/// out, mirroring `any_promote_init`'s class refusal: instances
/// carry nominal identity the typed lanes dispatch on, and the
/// boxed spelling has no probe coverage. Closure-valued members
/// admit since the same rotation's face ② — see that arm's comment.
pub(crate) fn any_slot_safe_value(ast: &Ast, e: ExprId) -> bool {
    match ast.get_expr(e) {
        Expr::Number(_) | Expr::String(_) | Expr::Bool(_) | Expr::BigInt { .. } | Expr::Null => {
            true
        }
        // `{ a: undefined }` — the dynobj init lane stores the
        // ANY_UNDEF slot pair for this exact spelling (its own
        // shadow-guarded fast arm), and a TOP-LEVEL `var undefined`
        // shadow is a silent no-op in sloppy mode, so the spelling is
        // unambiguous at this position.
        Expr::Ident(n) if n == "undefined" => true,
        Expr::ObjectLit { fields } => fields.iter().all(|(name, val)| {
            let mut chars = name.chars();
            let head_ok = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$');
            head_ok
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
                && !name.starts_with("__")
                && any_slot_safe_value(ast, *val)
        }),
        Expr::Array(els) => els.iter().all(|el| any_slot_safe_value(ast, *el)),
        // A call result boxes like any other value; only the
        // `__new_` factory spelling (class instances) stays out —
        // see the fn doc.
        Expr::Call { callee, .. } => {
            !matches!(ast.get_expr(*callee), Expr::Ident(n) if n.starts_with("__new_"))
        }
        // Rotation 546 face ② — a closure-valued member admits when
        // it carries NO receiver (`fnexpr_recv_fns` = the lifted fn
        // took a hidden `__this`): probes cover the receiver-less
        // face end to end (closure+null mix, top-level captures with
        // mutation observed across named fns). A METHOD member stays
        // out of this fallback: the promoted dynobj's `this`-chain
        // read has a proven hole (`{ box: { d: 1 }, read() { return
        // this.box.d + 10 } }` answered the I64 box BITS through a
        // named fn — while the explicit-`any` spelling of the same
        // program is bun-equal), so admitting it would trade today's
        // loud unknown-identifier for a silent wrong. All-shaped
        // method literals never reach here — the `__inlobj` + `__mth`
        // arm carries them. Variadic sigs stay out — their boxed-dual
        // routing is a fn-local table.
        Expr::Closure { fn_name, .. } if !ast.fnexpr_recv_fns.contains(fn_name) => {
            lifted_closure_fn_canon(ast, fn_name).is_some_and(|c| !c.contains("__rest("))
        }
        // Statically-shaped expressions (operator results, top-level
        // aliases, `Symbol()`) are certainly non-callable — the
        // shape table only answers primitives.
        _ => crate::ast_refs::infer_toplevel_slot_shape(ast, e).is_some(),
    }
}
