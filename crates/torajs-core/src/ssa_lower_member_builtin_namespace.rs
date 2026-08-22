//! `Math.<C>` / `Number.<C>` / `<Ctor>.{prototype,name,length}`
//! builtin-namespace Member-access constant + singleton-lookup
//! cluster pulled out of [`crate::ssa_lower::lower_expr_inner`]'s
//! `Expr::Member` god-arm as chunk-58 of the decomp (chunks 1-57 =
//! ... + process.* Member cluster).
//!
//! Three sub-arms tried in source order:
//!
//! - **Math constants** — `PI`, `E`, `LN2`, `LN10`, `LOG2E`,
//!   `LOG10E`, `SQRT2`, `SQRT1_2` → `Operand::ConstF64(...)` via
//!   `std::f64::consts::*`. An unknown name falls through to the
//!   generic any-member walk — `Math` is an ordinary extensible
//!   object, so the answer is its own-entry table, then undefined.
//! - **Number constants + prototype/name/length** — `NaN`,
//!   `POSITIVE_INFINITY`, `NEGATIVE_INFINITY`, `EPSILON`,
//!   `MAX_SAFE_INTEGER` / `MIN_SAFE_INTEGER` (2^53 - 1, V3-18
//!   m1.h.38: `MIN_VALUE = 5e-324` smallest subnormal, not
//!   `f64::MIN_POSITIVE`), `MAX_VALUE`, `prototype` (builtin-proto
//!   tag 0, singleton via `get_builtin_prototype` + `any_box(4,
//!   proto)`), `name` (intern `"Number"`), `length` (ctor arity
//!   `1`).
//! - **Constructor namespace prototype/name/length** — for
//!   `{Object, Array, String, Boolean, Symbol, BigInt, RegExp,
//!   Date, Error, Promise, Map, Set, Function, Iterator, WeakMap,
//!   WeakSet, WeakRef}` (see [`builtin_proto_tag`] for the tags,
//!   order locked to `torajs-rc::builtin_proto::NUM_BUILTIN_PROTOS`
//!   — never reorder): `.prototype` builtin-proto singleton +
//!   any_box; `.name` interns the namespace string; `.length`
//!   reads the ctor clause's arity off the same table. Other
//!   Member names fall through.
//! - **`<Ctor>.prototype.<m>` method value** (RFC
//!   20260711-closure-reflection chunk A) — the THREE-level static
//!   form (`String.prototype.anchor`) routes to
//!   `__torajs_builtin_proto_method_value(tag, key)`: singleton
//!   dynobj own-entry probe first (user monkey-patch wins), then
//!   the interned reified method cell for names the receiver
//!   shape's dispatch arm supports, else undefined. Tried before
//!   the two-level arms because its `obj` is a Member, not an
//!   Ident.
//!
//! All sub-arms return `Some(op)` on hit. Falls through (`None`)
//! only when `obj` isn't an `Expr::Ident` matching one of the
//! three namespaces (or the proto-method Member form); missing
//! constants within `Math.*` or `Number.*` panic (typechecker
//! contract).

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    name: &str,
) -> Option<Operand> {
    // `<Ctor>.prototype.<m>` — obj is itself a Member on a builtin
    // namespace ident. Handled here (before the Ident gate) so the
    // outer member never falls to the generic any-member walk.
    let proto_inner = match ctx.ast.get_expr(obj) {
        Expr::Member {
            obj: inner,
            name: pname,
        } if pname == "prototype" => Some(*inner),
        _ => None,
    };
    if let Some(inner) = proto_inner
        && let Expr::Ident(ns) = ctx.ast.get_expr(inner)
        && !ctx.ast.class_parents.contains_key(ns)
        && let Some(tag) = proto_method_tag(ns)
    {
        return Some(lower_proto_method_value(ctx, tag, name));
    }
    let Expr::Ident(ns_name) = ctx.ast.get_expr(obj) else {
        return None;
    };
    // A DECLARED class (user or injected — the Error family) owns a
    // real synthesized prototype (`__proto_<C>`, carrying its own
    // entries like Error.prototype's dedicated `toString`); the
    // class-value chain must resolve it, not the rc lazy singleton —
    // the two are different objects.
    if ctx.ast.class_parents.contains_key(ns_name) {
        return None;
    }
    // RFC 20260719-ns-static-value-reify — a namespace STATIC read
    // as a value (`Math.max` outside a call position) resolves the
    // interned dispatcher cell; the constant arms below keep every
    // non-static name. Gated on the checker's own truth (obj typed
    // `Type::Object`, member typed `Function`) so a user binding
    // shadowing `Math` never mints.
    if let Some(op) = try_lower_ns_static_value(ctx, eid, obj, ns_name.as_str(), name) {
        return Some(op);
    }
    // §23.2.5.1 table 71 — `<T>.BYTES_PER_ELEMENT` off the
    // CONSTRUCTOR is decided by the name, so it folds at compile
    // time (RFC 20260823-typedarray-substrate 刀 2).
    if name == "BYTES_PER_ELEMENT"
        && let Some(n) = crate::ssa_lower_call_typedarray::bytes_per_element(ns_name.as_str())
    {
        return Some(Operand::ConstI64(n));
    }
    match ns_name.as_str() {
        "Math" => lower_math_const(name),
        "Number" => lower_number(ctx, name),
        other => {
            let tag = builtin_proto_tag(other)?;
            lower_ctor_namespace(ctx, other, tag, name)
        }
    }
}

/// `Call(__torajs_ns_static_cell, [id])` typed as the checker
/// signature's Closure repr — the cell is an immortal borrow (rc
/// no-ops), calls route through the variadic boxed lane (the
/// let-decl shape recorder registers ns-static-init bindings in
/// `variadic_locals`), and every reflection face resolves off the
/// carried id (print / `.name` / `.length` / toString source /
/// `.call`+`.apply` / any-lane calls).
fn try_lower_ns_static_value(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    ns_name: &str,
    name: &str,
) -> Option<Operand> {
    let id = torajs_rc::ns_static_id(ns_name, name);
    if id == torajs_rc::NS_STATIC_UNKNOWN {
        return None;
    }
    if !matches!(
        ctx.expr_types.get(&obj),
        Some(crate::check::Type::Object(_))
    ) {
        return None;
    }
    let Some(crate::check::Type::Function(ps, ret)) = ctx.expr_types.get(&eid) else {
        return None;
    };
    let params: Vec<Type> = ps.iter().map(check_ty_to_ssa).collect();
    let ret = check_ty_to_ssa(ret.as_ref());
    let sig = crate::ssa_lower::intern_fn_sig(ctx.fn_sigs, params, ret);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(ctx.intrinsics.ns_static_cell, vec![Operand::ConstI64(id)]),
        Type::Closure(sig),
        None,
    );
    Some(Operand::Value(v))
}

/// Checker-type → SSA repr for an ns-static signature. `Number`
/// maps F64 (the boxed lane re-coerces returns through
/// `any_to_number`, so width never routes a call); unknown shapes
/// collapse to Any — the boxed lane boxes every arg anyway.
pub(crate) fn check_ty_to_ssa(t: &crate::check::Type) -> Type {
    match t {
        crate::check::Type::Number => Type::F64,
        crate::check::Type::String => Type::Str,
        crate::check::Type::Boolean => Type::Bool,
        crate::check::Type::Void => Type::Void,
        _ => Type::Any,
    }
}

/// Builtin-proto tag for the proto-method form — `Number` (tag 0)
/// plus the [`builtin_proto_tag`] ctor set.
pub(crate) fn proto_method_tag(ns: &str) -> Option<i64> {
    if ns == "Number" {
        return Some(0);
    }
    builtin_proto_tag(ns)
}

/// `Call(__torajs_builtin_proto_method_value, [tag, key])` — the
/// runtime resolves monkey-patch entries / interned method cells /
/// undefined; the result is an owned `Type::Any`.
fn lower_proto_method_value(ctx: &mut LowerCtx<'_>, tag: i64, name: &str) -> Operand {
    let key = ctx.intern_string_literal(name);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.builtin_proto_method_value,
            vec![Operand::ConstI64(tag), Operand::Value(key)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(v)
}

/// The eight §21.3.1 value properties. A name outside the set is
/// NOT an error: `Math` is an ordinary extensible object, so
/// `Math.nope` is a runtime [[Get]] answering undefined and
/// `Math.prop = 7` plants a real entry. `None` hands the read to the
/// generic any-member walk, which reads the singleton's own-entry
/// table. (Pre-r464 this panicked, on the contract that the
/// typechecker filtered unknown names upstream — it no longer does.)
fn lower_math_const(name: &str) -> Option<Operand> {
    Some(match name {
        "PI" => Operand::ConstF64(std::f64::consts::PI),
        "E" => Operand::ConstF64(std::f64::consts::E),
        "LN2" => Operand::ConstF64(std::f64::consts::LN_2),
        "LN10" => Operand::ConstF64(std::f64::consts::LN_10),
        "LOG2E" => Operand::ConstF64(std::f64::consts::LOG2_E),
        "LOG10E" => Operand::ConstF64(std::f64::consts::LOG10_E),
        "SQRT2" => Operand::ConstF64(std::f64::consts::SQRT_2),
        "SQRT1_2" => Operand::ConstF64(std::f64::consts::FRAC_1_SQRT_2),
        _ => return None,
    })
}

/// The §21.1.2 value properties plus the ctor's own reflection
/// faces. Unknown names fall through for the same reason as
/// [`lower_math_const`] — `Number` is extensible too.
fn lower_number(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    Some(match name {
        "NaN" => Operand::ConstF64(f64::NAN),
        "POSITIVE_INFINITY" => Operand::ConstF64(f64::INFINITY),
        "NEGATIVE_INFINITY" => Operand::ConstF64(f64::NEG_INFINITY),
        "EPSILON" => Operand::ConstF64(f64::EPSILON),
        "MAX_SAFE_INTEGER" => Operand::ConstI64(9007199254740991),
        "MIN_SAFE_INTEGER" => Operand::ConstI64(-9007199254740991),
        "MAX_VALUE" => Operand::ConstF64(f64::MAX),
        "MIN_VALUE" => Operand::ConstF64(5e-324),
        "prototype" => any_boxed_builtin_proto(ctx, 0),
        "name" => Operand::Value(ctx.intern_string_literal("Number")),
        "length" => Operand::ConstI64(1),
        _ => return None,
    })
}

fn builtin_proto_tag(ns_name: &str) -> Option<i64> {
    match ns_name {
        "Object" => Some(1),
        "Array" => Some(2),
        "String" => Some(3),
        "Boolean" => Some(4),
        "Symbol" => Some(5),
        "BigInt" => Some(6),
        "RegExp" => Some(7),
        "Date" => Some(8),
        "Error" => Some(9),
        "Promise" => Some(10),
        "Map" => Some(11),
        "Set" => Some(12),
        "Function" => Some(13),
        "Iterator" => Some(15),
        // The weak pair joined last (rotation 314). Their instances
        // and any-lane method dispatch predate this by a long way —
        // what was missing is the constructor read as a VALUE, which
        // is what every `WeakMap.prototype.<m>.call(…)` brand-check
        // test needs to even reach its assertion.
        "WeakMap" => Some(16),
        "WeakSet" => Some(17),
        "WeakRef" => Some(18),
        // RFC 20260823-typedarray-substrate 刀 4 — the buffer family
        // at 19, then the eleven typed arrays in element-kind order.
        // Reading one of these as a VALUE is what
        // `[Float64Array, Float32Array]` needs, and that is the
        // shape testTypedArray.js is built out of.
        "ArrayBuffer" => Some(19),
        n => crate::ssa_lower_call_typedarray::kind_of_name(n).map(|k| 20 + k),
    }
}

fn lower_ctor_namespace(
    ctx: &mut LowerCtx<'_>,
    ns_name: &str,
    tag: i64,
    name: &str,
) -> Option<Operand> {
    match name {
        "prototype" => Some(any_boxed_builtin_proto(ctx, tag)),
        "name" => Some(Operand::Value(ctx.intern_string_literal(ns_name))),
        // Ctor-clause length off the torajs-rc single source (RFC
        // 20260720 刀 3) — the previous constant 1 was wrong for
        // Date (7) / RegExp (2) / Symbol / Map / Set (0).
        "length" => {
            let (_, l) = torajs_rc::builtin_proto::builtin_ctor_meta(tag)?;
            Some(Operand::ConstI64(l as i64))
        }
        _ => None,
    }
}

fn any_boxed_builtin_proto(ctx: &mut LowerCtx<'_>, tag: i64) -> Operand {
    let cur_block = ctx.cur_block;
    let proto = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.get_builtin_prototype,
            vec![Operand::ConstI64(tag)],
        ),
        Type::Ptr,
        None,
    );
    // The singleton slot's pointer is a BORROW — box it out under
    // the owned Any convention (+1 here, the consumer's drop
    // balances). Pre-fix the borrow box leaked into owned drops and
    // bled the singleton's refcount until it freed; a later `{}`
    // reused the address and misclassified as the prototype (RFC
    // 20260718-accessor-reify 刀 1 diag).
    ctx.emit_rc_inc(Operand::Value(proto));
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(4), Operand::Value(proto)],
        ),
        Type::Any,
        None,
    );
    Operand::Value(v)
}
