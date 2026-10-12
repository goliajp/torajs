//! The annotation an un-annotated top-level binding registers from
//! when a named function body reads it. Moved out of [`super`] whole.

use crate::ast::{Ast, Expr, ExprId};
use crate::check::Checker;

/// The annotation spelling an UN-annotated top-level binding registers
/// from, when a named function body reads it (named functions have no
/// capture machinery; a captured binding, mutable or not, takes the
/// same global home — the capture filter resolves it there and the
/// lifted body's writes take the global lane). `None` keeps the binding
/// main-local. The lowerer parses the recorded string for the slot, so
/// the checker's type and the slot's cannot drift. The flag says the
/// init was widened to `any` (a cross-type write, or a literal only the
/// `any` slot can hold): its main binding widens too.
///
/// The arms, in order:
/// - a dynobj-degraded object literal is `any` — the degrade IS the
///   any-lane routing decision (`crate::dynobj_degrade`);
/// - an init a cross-type write widened is `any` (`let_widen`);
/// - a lifted arrow spells the signature synthesized from its lifted
///   decl's annotations (a variadic one carries its rest sentinel: the
///   lowerer gives it a Closure slot from the same string and routes
///   its calls through the boxed dual entry);
/// - an alias of such a binding (`const c = k`) spells the same, and
///   so does a read of a closure field of a top-level object literal
///   (`const g = src.g`);
/// - an all-literal object literal spells its `__inlobj(...)`, an
///   all-literal array its `T[]`, a `new C()` its class or collection;
/// - a literal / operator / named-fn-call init spells its scalar
///   shape (`ast_refs::infer_toplevel_slot_shape`);
/// - a `parseInt` / `parseFloat` call on the builtin route is `f64`;
/// - a call result or literal no shape names, but the `any` slot can
///   hold (the test262 IIFE-iterator idiom), is `any`.
///   `var` stays main-local in every arm but the scalar shape (the hoist
///   escape hatch converts it first): the lowerer's promote loop only
///   takes `is_var: false`, and a checker-only registration would
///   typecheck programs whose lowering still aborts.
pub(super) fn synthesized_spelling(
    c: &Checker,
    ast: &Ast,
    name: &str,
    init: ExprId,
    is_var: bool,
    binding_refs: &crate::ast_refs::ToplevelBindingRefs,
) -> Option<(String, bool)> {
    if !binding_refs.named_fn_refs.contains(name) {
        return None;
    }
    if !is_var && c.dynobj_degraded.contains(&init) {
        return Some(("any".into(), false));
    }
    if !is_var && c.cross_type_widened.contains(&init) {
        return Some(("any".into(), true));
    }
    if let Expr::Closure { fn_name, .. } = ast.get_expr(init) {
        if is_var {
            return None;
        }
        return crate::ast_refs::lifted_closure_fn_canon(ast, fn_name).map(|s| (s, false));
    }
    if !is_var
        && let Some(ann) = crate::ast_refs::closure_alias_fn_canon(ast, init)
            .or_else(|| crate::ast_refs::objlit_literal_inlobj_ann(ast, init))
            .or_else(|| crate::ast_refs_arrlit::arrlit_literal_elem_ann(ast, init))
            .or_else(|| crate::ast_refs::new_class_ann(ast, init))
            .or_else(|| crate::ast_refs::collection_new_ann(ast, init))
            .or_else(|| {
                let (degraded, widened) = (&c.dynobj_degraded, &c.cross_type_widened);
                crate::ast_refs::closure_field_read_ann(ast, init, degraded, widened)
            })
    {
        return Some((ann, false));
    }
    if let Some(shape) = crate::ast_refs::infer_toplevel_slot_shape(ast, init) {
        use crate::ast_refs::GlobalSlotShape as S;
        let spelling = match shape {
            S::I64 => "number",
            S::F64 => "f64",
            S::Str => "string",
            S::Bool => "boolean",
            S::Symbol => "symbol",
            S::BigInt => "bigint",
            S::RegExp => "regex",
        };
        return Some((spelling.into(), false));
    }
    // §19.2.4 / §19.2.5 — the builtin parsers answer a Number on
    // every input and their route lowers to an f64; only a user
    // binding or a patched global of the name loses that certainty
    if let Expr::Call { callee, .. } = ast.get_expr(init)
        && let Expr::Ident(fname) = ast.get_expr(*callee)
        && matches!(fname.as_str(), "parseInt" | "parseFloat")
        && crate::check_type_of_call_bare_globals::takes_route(c, ast, fname)
    {
        return Some(("f64".into(), false));
    }
    (!is_var && crate::ast_refs_any_promote::any_promote_init(ast, init))
        .then(|| ("any".into(), true))
}
