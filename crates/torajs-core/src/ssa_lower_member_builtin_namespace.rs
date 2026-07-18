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
//!   `std::f64::consts::*`. Unknown name panics (typechecker
//!   upstream filters).
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
//!   Date, Error, Promise, Map, Set, Function}` (tags 1..14,
//!   order locked to `torajs-rc::builtin_proto::NUM_BUILTIN_PROTOS`
//!   — never reorder): `.prototype` builtin-proto singleton +
//!   any_box; `.name` interns the namespace string; `.length`
//!   ConstI64(1). Other Member names fall through.
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

pub(crate) fn try_lower(ctx: &mut LowerCtx<'_>, obj: ExprId, name: &str) -> Option<Operand> {
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
        && let Some(tag) = proto_method_tag(ns)
    {
        return Some(lower_proto_method_value(ctx, tag, name));
    }
    let Expr::Ident(ns_name) = ctx.ast.get_expr(obj) else {
        return None;
    };
    match ns_name.as_str() {
        "Math" => Some(lower_math_const(name)),
        "Number" => Some(lower_number(ctx, name)),
        other => {
            let tag = builtin_proto_tag(other)?;
            lower_ctor_namespace(ctx, other, tag, name)
        }
    }
}

/// Builtin-proto tag for the proto-method form — `Number` (tag 0)
/// plus the [`builtin_proto_tag`] ctor set (tags 1..14).
fn proto_method_tag(ns: &str) -> Option<i64> {
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

fn lower_math_const(name: &str) -> Operand {
    match name {
        "PI" => Operand::ConstF64(std::f64::consts::PI),
        "E" => Operand::ConstF64(std::f64::consts::E),
        "LN2" => Operand::ConstF64(std::f64::consts::LN_2),
        "LN10" => Operand::ConstF64(std::f64::consts::LN_10),
        "LOG2E" => Operand::ConstF64(std::f64::consts::LOG2_E),
        "LOG10E" => Operand::ConstF64(std::f64::consts::LOG10_E),
        "SQRT2" => Operand::ConstF64(std::f64::consts::SQRT_2),
        "SQRT1_2" => Operand::ConstF64(std::f64::consts::FRAC_1_SQRT_2),
        other => panic!("ssa-lower: unknown Math constant `{other}`"),
    }
}

fn lower_number(ctx: &mut LowerCtx<'_>, name: &str) -> Operand {
    match name {
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
        other => panic!("ssa-lower: unknown Number constant `{other}`"),
    }
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
        _ => None,
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
        "length" => Some(Operand::ConstI64(1)),
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
