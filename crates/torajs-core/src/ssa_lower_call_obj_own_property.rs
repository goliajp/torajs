//! `hasOwnProperty` / `propertyIsEnumerable` on a `Type::Obj`
//! receiver — split out of `ssa_lower_call_universal_methods` when the
//! expando deferral pushed that file past the 500-line limit.
//!
//! Two questions the compile-time field list alone cannot answer live
//! here: an Error-derived receiver's `message` / `name` / `stack`
//! carry runtime own-state (§20.5.6.1.1 / §20.5.3.2), and ANY class
//! instance can carry expando entries the layout never mentions. The
//! fold stays for layout hits; everything else defers to a kernel.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;
use torajs_wtf8::Wtf8;

/// Emit `obj.hasOwnProperty(k)` / `obj.propertyIsEnumerable(k)` on a
/// `Type::Obj` receiver. Literal key → compile-time fold against
/// struct_layouts. Runtime key (V3-18 m2.g) → inline str_eq chain over
/// the struct's field names (zero-alloc — each name interned as
/// literal Str).
///
/// The runtime own-property probe a layout MISS defers to. A class
/// instance's `+24` expando dict holds own properties the
/// compile-time field list cannot see (`err.cause` is one the
/// injected ctor installs), so answering `false` off the layout alone
/// was wrong for every one of them. Layout hits never come here — the
/// fold keeps them.
fn emit_expando_probe(
    ctx: &mut LowerCtx<'_>,
    recv_op: &Operand,
    key: Operand,
    is_enumerable_probe: bool,
) -> Operand {
    let boxed = ctx.box_to_any(recv_op.clone());
    let target = if is_enumerable_probe {
        ctx.intrinsics.any_prop_enumerable
    } else {
        ctx.intrinsics.any_prop_has
    };
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(target, vec![boxed, key]),
        Type::I64,
        None,
    );
    let nz = ctx.f.append_inst(
        cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(nz)
}

/// All three of an Error-derived receiver's injected slots carry
/// runtime state a layout list cannot express, and each differently:
///
/// - `message` (§20.5.6.1.1) — propertyIsEnumerable is constantly
///   false ([[Enumerable]]: false); hasOwnProperty reads the
///   own-absence sentinel through `__torajs_error_message_present`.
/// - `name` (§20.5.3.2) — lives on `<C>.prototype`, so an own one
///   exists only where user code assigned it, and that assignment is
///   an ordinary enumerable data property. Both probes therefore ask
///   the same question and share `__torajs_error_name_present`.
/// - `stack` — same [[Enumerable]]: false as `message`, but written
///   by every construction, so its own-ness needs no runtime probe
///   and only the enumerable answer moves.
pub(crate) fn emit_obj_has_own_property(
    ctx: &mut LowerCtx<'_>,
    recv_id: ExprId,
    recv_op: &Operand,
    recv_ty: Type,
    m_name: &str,
    arg_eid: ExprId,
    args: &[ExprId],
) -> Operand {
    let Type::Obj(sid) = recv_ty else {
        unreachable!("emit_obj_has_own_property called with non-Obj receiver");
    };
    let is_err_recv = crate::ssa_lower_member_obj_field::class_name_of_expr(ctx, recv_id)
        .is_some_and(|c| ctx.class_is_error_derived(&c));
    let is_enumerable_probe = m_name == "propertyIsEnumerable";
    let emit_present = |ctx: &mut LowerCtx<'_>, key: &Wtf8| {
        let target = if key == "message" {
            ctx.intrinsics.error_message_present
        } else {
            ctx.intrinsics.error_name_present
        };
        let cur_block = ctx.cur_block;
        let v = ctx.f.append_inst(
            cur_block,
            InstKind::Call(target, vec![recv_op.clone()]),
            Type::Bool,
            None,
        );
        Operand::Value(v)
    };
    if let Expr::String(key) = ctx.ast.get_expr(arg_eid) {
        // Literal key — compile-time fold. The lowered literal is a
        // static cell (rc no-op); nothing to release.
        let key = key.to_string_lossy_owned();
        let layout = &ctx.struct_layouts[sid.0 as usize];
        // An accessor member rides the layout under its synthetic
        // `__getter_<k>` / `__setter_<k>` name — it IS the own
        // property `<k>` (test262 8.12.1-1_20: `{get foo(){}}`
        // hasOwnProperty("foo") is true).
        let getter = format!("__getter_{key}");
        let setter = format!("__setter_{key}");
        let result = layout
            .iter()
            .any(|(fname, _)| *fname == key || *fname == getter || *fname == setter);
        let arg_val = ctx.lower_expr(arg_eid);
        ctx.release_owned_temp(arg_eid, &arg_val);
        // S304 — lower-and-drop trailing args per S272 idiom
        // (hasOwnProperty / propertyIsEnumerable useful arity 1).
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        if result && is_err_recv {
            if key == "message" {
                if is_enumerable_probe {
                    return Operand::ConstBool(false);
                }
                return emit_present(ctx, Wtf8::new("message"));
            }
            // `name` answers the SAME probe either way: an own one
            // only exists because user code assigned it, and that
            // assignment is an ordinary enumerable data property.
            if key == "name" {
                return emit_present(ctx, Wtf8::new("name"));
            }
            // `stack` shares msgDesc's [[Enumerable]]: false but is
            // unconditionally own, so only the enumerable probe moves.
            if key == "stack" && is_enumerable_probe {
                return Operand::ConstBool(false);
            }
        }
        if result {
            return Operand::ConstBool(true);
        }
        let lit = ctx.intern_string_literal(&key);
        return emit_expando_probe(ctx, recv_op, Operand::Value(lit), is_enumerable_probe);
    }
    // Runtime key — emit inline str_eq chain.
    let layout = ctx.struct_layouts[sid.0 as usize].clone();
    let key_op = ctx.lower_expr(arg_eid);
    let key_ty = ctx.operand_ty(&key_op);
    let mut acc: Operand = Operand::ConstBool(false);
    for (fname, _) in &layout {
        // Accessor synthetic names compare under their public key.
        let public = fname
            .strip_prefix("__getter_")
            .or_else(|| fname.strip_prefix("__setter_"))
            .unwrap_or(fname.as_wtf8());
        // Error-derived `message` runtime attributes (see fn doc):
        // the enumerable probe never matches it; the hasOwnProperty
        // chain gates its eq on the own-presence probe.
        if is_err_recv && is_enumerable_probe && (public == "message" || public == "stack") {
            continue;
        }
        let lit = ctx.intern_string_literal(public);
        let cmp_target = if key_ty == Type::Substr {
            ctx.intrinsics.substr_eq_str
        } else {
            ctx.intrinsics.str_eq
        };
        let eq = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(cmp_target, vec![key_op, Operand::Value(lit)]),
            Type::Bool,
            None,
        );
        let mut eq_op = Operand::Value(eq);
        if is_err_recv && (public == "message" || public == "name") {
            let present = emit_present(ctx, public);
            let and = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::And, eq_op, present),
                Type::Bool,
                None,
            );
            eq_op = Operand::Value(and);
        }
        if matches!(acc, Operand::ConstBool(false)) {
            acc = eq_op;
        } else {
            let or = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Or, acc, eq_op),
                Type::Bool,
                None,
            );
            acc = Operand::Value(or);
        }
    }
    // A layout miss defers to the runtime probe here too, so the two
    // spellings of the same question agree: `e.hasOwnProperty("cause")`
    // and `e.hasOwnProperty(k)` take different lowering paths, and an
    // expando entry has to be visible through both. Only a Str key —
    // the probe wants a Str cell, which a Substr view is not.
    if matches!(key_ty, Type::Str) {
        let probe = emit_expando_probe(ctx, recv_op, key_op, is_enumerable_probe);
        acc = if matches!(acc, Operand::ConstBool(false)) {
            probe
        } else {
            let or = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::BinOp(SsaBinOp::Or, acc, probe),
                Type::Bool,
                None,
            );
            Operand::Value(or)
        };
    }
    // The str_eq chain only reads the key — an Ident key keeps its
    // stake and its scope drop; owned temp keys release here (the old
    // consume+drop pair destroyed the source's stake, reuse-window UAF).
    ctx.release_owned_temp(arg_eid, &key_op);
    // S304 — lower-and-drop trailing args (runtime-key path;
    // same useful=1).
    for &a in args.iter().skip(1) {
        let _ = ctx.lower_expr(a);
    }
    acc
}
