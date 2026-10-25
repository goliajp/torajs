//! Expression shapes that raise whatever their operands' types say,
//! moved out of [`super`] whole.

use std::collections::HashMap;

use crate::ast::{Ast, Expr, ExprId, PropKey};

/// A property STORE target (`o.k = v`, `o.k++`, `xs[i] = v`, `xs[i]++`)
/// — the shape whose store can raise, whatever the receiver is.
///
/// This used to be an inclusion whitelist of receiver types, grown
/// one entry at a time: `xs.length` (rotation 270, the §10.4.2.5
/// resize helper), a `ClassRef`/`Struct` field (rotation 373, the
/// §10.1.9 writable gate), and `Any` by way of the `Expr::Member`
/// read arm below. Three arms, one reason — and the whitelist shape
/// was structurally wrong, because the store ladder it is meant to
/// mirror ends in a lane that takes EVERYTHING ELSE: a receiver with
/// no arm of its own is boxed and routed through
/// `__torajs_any_member_set`, which raises. So the receivers nobody
/// had thought to name were exactly the ones whose throw went
/// missing, and the one that surfaced it is a receiver the ladder
/// never even had to reach for: a specialized body's parameter.
///
/// `function f(n) { n.nope = 1 }` called as `f(1)` types `n` as
/// `Number` — not `Any`, because specialization already resolved it —
/// so no arm matched, the caller pruned its check under M4.3.b, and
/// the strict-mode TypeError §10.1.8.1 owes a primitive receiver was
/// dropped on the floor. The callee did stop (its own throw check
/// fired); the caller then printed the ret sentinel, so
/// `console.log(f(1))` answered `null` and a `try`/`catch` around it
/// caught nothing. The SAME program with `n: any`, or routed through
/// a `const m: any = n`, threw correctly all along.
///
/// Nothing statically rules a store out: a frozen instance, a
/// `defineProperty`-demoted field, an accessor-only property and a
/// primitive receiver all raise, and none of them is visible in the
/// receiver's type. So the predicate is the position, not the shape.
/// The KEYED spelling is on the same rule for the same reason, and it
/// is not an exotic shape either: `function w(xs) { xs[0] = 99 }` over
/// a FROZEN array raised in the callee, which correctly left the
/// element alone — and then the caller, having pruned its check,
/// printed the ret sentinel. `console.log(w(a))` answered `null` where
/// every engine throws.
pub(super) fn is_property_store(ast: &Ast, eid: ExprId) -> bool {
    matches!(ast.get_expr(eid), Expr::Member { .. } | Expr::Index { .. })
}

/// What makes a bare identifier read RAISE instead of answering: an
/// unresolvable Reference (RFC 20260730-undeclared-ident, §6.2.5.5), a
/// binding still in its temporal dead zone (`ast::mark_tdz_reads`,
/// §8.1.1.1), and a name only an eval / Script text binds, read before
/// that text ran (`ssa_lower_ident_site_bound`, which skips the
/// binding's own self-read). Either way the enclosing fn must enter
/// `may_throw`, or callers prune the check and the throw is silently
/// swallowed across the fn boundary.
pub(crate) fn ident_read_raises(ast: &Ast, eid: ExprId) -> bool {
    use crate::ast::ScriptGlobalVar::{EvalFn, EvalVar, ScriptVar};
    ast.undeclared_reads.contains_key(&eid)
        || ast.tdz_reads.contains(&eid)
        || matches!(ast.get_expr(eid), Expr::Ident(n)
            if matches!(ast.script_global_var_names.get(n), Some(EvalVar | ScriptVar | EvalFn))
                && !ast.eval_var_sites.contains_key(&eid))
}

/// An object literal's spread of a class instance or struct (`{ ...c }`)
/// reads every field through [[Get]] (§7.3.25), which runs a getter
/// once the program can install one over a declared field
/// (`Ast::accessor_field_names`).
pub(super) fn spread_runs_getters(
    ast: &Ast,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    key: &PropKey,
    src: ExprId,
) -> bool {
    crate::check_type_of_object_lit::spread_omit_set(key.as_wtf8()).is_some()
        && ast.accessor_field_names.reaches_any()
        && super::obj_cast::object_ty(expr_types.get(&src))
}

/// Whether `new C(…)` raises (`super(…)` is the parent's own body). A user class's `new` is a factory call by
/// the time the scan runs (`desugar_classes`), so what is left is a
/// builtin constructor the lowering intercepts, and every one of those
/// checks the pending throw after its kernel (`ssa_lower_new`): an
/// `Iterator` always raises, `Array(n)` / `ArrayBuffer` / a typed array
/// on a bad length, `RegExp` on a bad pattern, `Proxy` on a non-object,
/// a collection on an iterable that raises. The ones that cannot are a
/// collection made empty, `Boolean` (ToBoolean never raises), and
/// `Number` / `String` of a primitive, whose conversion runs no code.
pub(crate) fn new_raises(
    ast: &Ast,
    types: &HashMap<ExprId, crate::check::Type>,
    eid: ExprId,
) -> bool {
    use crate::check::Type;
    let Expr::New {
        class_name, args, ..
    } = ast.get_expr(eid)
    else {
        return false;
    };
    match class_name.as_str() {
        "Map" | "Set" | "WeakMap" | "WeakSet" => !args.is_empty(),
        "Boolean" => false,
        "Number" | "String" => !args.first().is_some_and(|a| {
            matches!(
                types.get(a),
                Some(Type::Number | Type::String | Type::Boolean)
            )
        }),
        _ => true,
    }
}

/// Whether `x instanceof rhs` can raise: §13.10.2 throws when the
/// target is not an object or its `@@hasInstance` handler throws, and
/// the lowering asks the operator for every target but a declared
/// class or builtin constructor it folds by name
/// (`ssa_lower_instanceof::name_runs_operator`). A local binding of the
/// name is a value the operator is asked of. A folded builtin walks the
/// prototype chain of an object `x`, which runs a Proxy's trap.
pub(super) fn instanceof_raises(sc: &super::Scan<'_>, x: ExprId, rhs: ExprId) -> bool {
    let Expr::Ident(n) = sc.ast.get_expr(rhs) else {
        return true;
    };
    if sc.fn_values.contains(n) {
        return true;
    }
    let name = n.strip_prefix("__class_").unwrap_or(n);
    if sc.fn_values.contains(name) {
        return true;
    }
    let name = (sc.ast.generator_factory_classes.get(name)).map_or(name, String::as_str);
    crate::ssa_lower_instanceof::name_runs_operator(sc.ast, name, sc.has_instance_hook)
        || (crate::ssa_lower_instanceof::name_walks_proto_chain(name)
            && !sc.expr_types.get(&x).is_some_and(primitive))
}

/// A type all of whose values are primitives ToNumeric converts
/// without running code or throwing.
fn primitive(t: &crate::check::Type) -> bool {
    use crate::check::Type;
    match t {
        Type::Number
        | Type::String
        | Type::Boolean
        | Type::BigInt
        | Type::Null
        | Type::Undefined
        | Type::Void
        | Type::Never
        | Type::Literal(_) => true,
        Type::Nullable(t) => primitive(t),
        Type::Union(ts) => ts.iter().all(primitive),
        _ => false,
    }
}
