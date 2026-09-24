//! The let-init axis of the fn-to-closure forwarders — moved out of
//! `forwarders_object.rs` whole when that file neared its 500-line
//! limit, with its admission split from its bookkeeping so a pass that
//! runs earlier can ask the same question
//! ([`crate::ast_closure_param_tag`] has to know which bindings end up
//! closure cells before the rewrite happens).

use super::{Ast, Expr, ExprId, Param, Stmt, is_fn_like_ann};
use std::collections::{HashMap, HashSet};

type FnSigSnapshot = HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>;

/// A top-level binding the axis wraps: the binding, its init, the
/// declaration the init names, and whether that one takes a rest
/// parameter.
pub(crate) struct LetInitWrap {
    pub(crate) binding: String,
    pub(crate) init: ExprId,
    pub(crate) target: String,
    pub(crate) is_rest: bool,
}

/// RFC 20260709-closure-global chunk 4 — the let-init axis:
/// `const f: (xs)=>n = top_fn` (or un-annotated) at the TOP level
/// where a named-fn body reads `f`. The wrap turns the init into a
/// lifted-arrow shape so the K.3b promote fires and the named fn
/// reaches the binding through the global-closure call lane. Gated
/// on the same ast_refs pair the promote uses — a main-only
/// binding keeps the direct-dispatch fn_addr_let home, and a
/// closure-captured one stays main-local (a slot would split the
/// binding into two homes).
pub(crate) fn let_init_axis_wrap(
    ast: &Ast,
    s: &Stmt,
    binding_refs: &crate::ast_refs::ToplevelBindingRefs,
    fn_sigs: &FnSigSnapshot,
) -> Option<LetInitWrap> {
    if let Stmt::LetDecl {
        name,
        init,
        type_ann,
        is_var: false,
        ..
    } = s
        // Chunk 737 — immutable closure-captured bindings promote
        // (the capture filter resolves them to the global), so
        // their named-fn inits wrap too. Chunk 740 — the
        // mutable+captured combination promotes the same way
        // (capture filter reads + Assign-Ident global writes =
        // one home), so its named-fn init wraps too.
        && let Expr::Ident(n) = ast.get_expr(*init)
        // 刀 3 (RFC 20260815-fn-value-rest-spread) — a rest-fn
        // target wraps past both narrowings below: its VALUE
        // binding must be a closure cell (the variadic call lane
        // dispatches through the boxed dual entry in the env; a
        // raw FnSig home has none), and a direct call on the
        // binding never sees apply_rest_args' name-keyed packing
        // anyway — pre-wrap it mis-fed the packed-array ABI one
        // argument per slot, silently.
        && let is_rest_target = fn_sigs
            .get(n)
            .is_some_and(|(ps, _, _)| ps.last().is_some_and(|p| p.is_rest))
        && type_ann
            .as_deref()
            .is_none_or(|a| is_fn_like_ann(a) && (!a.contains("__rest(") || is_rest_target))
        // r292 — a hoisted generator-expression init wraps without
        // the named-fn-refs gate: its top-level reads (harness
        // verifyProperty args — the fn-name-gen family) box into
        // any, which a raw FnSig can't, and the factory call is
        // alloc-heavy anyway so the closure-invoke detour is
        // free. Plain named-fn inits keep the read gate (their
        // direct calls are hot paths).
        && (binding_refs.named_fn_refs.contains(name)
            || is_rest_target
            || ast.generator_factory_classes.contains_key(n)
            || ast.async_generator_fns.contains(n))
        // Chunk 805 — no explicit-return-ann gate anymore: the
        // forwarder clones the target's `None` ret, and
        // `desugar_implicit_generics` (which runs AFTER this
        // pass) backfills it — its `__env` arm infers the
        // forwarder's `return target(...)` from the target's own
        // backfilled ret. A target with no value returns stays
        // `None` and the synthesized canon spells `void`, which
        // is exactly right for it. The other wrap axes (any /
        // assign / struct-field) have shipped this un-annotated
        // path all along.
        && fn_sigs.contains_key(n)
    {
        return Some(LetInitWrap {
            binding: name.clone(),
            init: *init,
            target: n.clone(),
            is_rest: is_rest_target,
        });
    }
    None
}

/// Apply [`let_init_axis_wrap`] over the top-level statements.
pub(super) fn collect_let_init_axis_rewrites(
    ast: &Ast,
    stmts_snapshot: &[Stmt],
    binding_refs: &crate::ast_refs::ToplevelBindingRefs,
    fn_sigs: &FnSigSnapshot,
    targets: &mut HashSet<String>,
    rewrites: &mut Vec<(ExprId, String)>,
    variadic_bindings: &mut Vec<String>,
) {
    for s in stmts_snapshot {
        if let Some(w) = let_init_axis_wrap(ast, s, binding_refs, fn_sigs) {
            targets.insert(w.target.clone());
            rewrites.push((w.init, w.target));
            if w.is_rest {
                variadic_bindings.push(w.binding);
            }
        }
    }
}
