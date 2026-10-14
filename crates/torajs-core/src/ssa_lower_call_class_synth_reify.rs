//! Reification arms (`__torajs_static_method_reify` /
//! `__torajs_static_field_reify` / `__torajs_class_accessor_reify` /
//! `__torajs_class_static_accessor_reify`) split out of
//! [`crate::ssa_lower_call_class_synth`] (file-size limit): each
//! resolves a class-synthesis sentinel into the runtime define call
//! that lands a real own entry on the class object / prototype.

use crate::ast::{Expr, ExprId, mangle_key};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

pub(crate) use crate::ssa_lower_call_class_synth_writeback::emit_class_binding_writeback;

/// Knife B cut 2 (RFC 20260717-class-first-class-value) —
/// `__torajs_static_method_reify("<C>", "<M>")`: resolve
/// `__sm_<C>__<M>`'s boxed adapter and hand the runtime the
/// `(tag, name-Str, adapter-vaddr)` triple so the class object gets
/// its own `<M>` function entry. An adapter-synthesis dropout
/// (unboxable signature) skips the define — the member read keeps
/// its current answer instead of minting an uncallable cell.
pub(super) fn try_lower_static_method_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
    let Expr::String(mname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let cname = cname.clone();
    let mname = mname.clone();
    let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() else {
        return Some(Operand::ConstI64(0));
    };
    // 402-01 — a GENERIC static method's original never lowers; its
    // all-`any` mono instance (`$$anywv`, pre-seeded by
    // `monomorphize_and_check`) is the value-lane dispatch body the
    // class object's cell reifies instead.
    // The symbol is the key's mangled spelling (557-02 C 组) — a lone
    // surrogate in the name is `__u_XXXX` there, never U+FFFD.
    let mut body = format!("__sm_{cname}__{}", mangle_key(&mname));
    if !ctx.fn_table.contains_key(body.as_str()) {
        body.push_str("$$anywv");
    }
    let Some(&body_fid) = ctx.fn_table.get(body.as_str()) else {
        return Some(Operand::ConstI64(0));
    };
    let Some(&(adapter_fid, adapter_sig)) = ctx.boxed_entries.get(&body_fid) else {
        return Some(Operand::ConstI64(0));
    };
    let this_free = static_this_free(ctx, &body, body_fid);
    let name_op = ctx.lower_expr(args[1]);
    let cur_block = ctx.cur_block;
    let adapter = ctx.f.append_inst(
        cur_block,
        InstKind::FnAddr(adapter_fid),
        Type::FnSig(adapter_sig),
        None,
    );
    // RFC 20260804-fn-this-channel knife 3b — the receiver-polymorphic
    // static twin's boxed adapter (knife 3a's `__smany_` mint), or 0
    // when no twin exists (this-free body / mint residue): the face
    // cell carries it and `invoke_with_this` routes a `.call/.apply`
    // rebind through it (a static face encodes as tag 0 + twin ≠ 0).
    let twin_name = format!("__smany_{cname}__{mname}");
    let twin_op = ctx
        .fn_table
        .get(twin_name.as_str())
        .copied()
        .and_then(|fid| ctx.boxed_entries.get(&fid).copied())
        .map(|(twin_fid, twin_sig)| {
            Operand::Value(ctx.f.append_inst(
                cur_block,
                InstKind::FnAddr(twin_fid),
                Type::FnSig(twin_sig),
                None,
            ))
        })
        .unwrap_or(Operand::ConstI64(0));
    let define = ctx.intrinsics.static_method_define;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            define,
            vec![
                Operand::ConstI64(tag as i64),
                name_op,
                Operand::Value(adapter),
                Operand::ConstI64(i64::from(this_free)),
                twin_op,
            ],
        ),
    );
    // The lowered Str literal is a caller-owned temp — the runtime
    // key copy is the define's own (dynobj_define rc_incs the key).
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    emit_class_binding_writeback(ctx, &cname, tag, false);
    Some(Operand::ConstI64(0))
}

/// K4-1 (RFC 20260917-class-eval-unit) —
/// `__torajs_unit_static_method_reify("<C>", "<M>", __unit)`: resolve
/// `__sm_<C>__<M>`'s boxed adapter and define one MORTAL face holding
/// this evaluation's class object onto that object. The same adapter
/// dropout as the template reify skips the define. No writeback: the
/// class object is the evaluation's local, not a registered binding.
pub(super) fn try_lower_unit_static_method_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    let [cname_id, mname_id, unit_id] = args else {
        return None;
    };
    let Expr::String(cname) = ctx.ast.get_expr(*cname_id) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
    let Expr::String(mname) = ctx.ast.get_expr(*mname_id) else {
        return None;
    };
    let mname = mname.clone();
    let mut body = format!("__sm_{cname}__{}", mangle_key(&mname));
    if !ctx.fn_table.contains_key(body.as_str()) {
        body.push_str("$$anywv");
    }
    let Some(&body_fid) = ctx.fn_table.get(body.as_str()) else {
        return Some(Operand::ConstI64(0));
    };
    let Some(&(adapter_fid, adapter_sig)) = ctx.boxed_entries.get(&body_fid) else {
        return Some(Operand::ConstI64(0));
    };
    // The row's `MethodMeta` word: bit 3 when the body reaches a
    // private element (K4-4's call-boundary brand guard reads it). The
    // AST pass answered by name before any `$$` suffix existed.
    let base = body.split_once("$$").map_or(body.as_str(), |(b, _)| b);
    let flags = if ctx.ast.private_touch_fns.contains(base) {
        8
    } else {
        0
    };
    let unit_op = ctx.lower_expr(*unit_id);
    let name_op = ctx.lower_expr(*mname_id);
    let cur_block = ctx.cur_block;
    let adapter = ctx.f.append_inst(
        cur_block,
        InstKind::FnAddr(adapter_fid),
        Type::FnSig(adapter_sig),
        None,
    );
    let define = ctx.intrinsics.class_unit_static_method_define;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            define,
            vec![
                unit_op,
                name_op,
                Operand::Value(adapter),
                Operand::ConstI64(flags),
            ],
        ),
    );
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}

/// K4-3 — `__torajs_unit_static_accessor_reify("<C>", "<p>", __unit)`:
/// the static twin of `try_lower_class_accessor_reify` for ONE
/// evaluation's class object — both faces' boxed adapters (either may
/// be absent) and the object itself, to `__torajs_class_unit_static_
/// accessor_define`. Both-adapters dropout skips the define.
pub(super) fn try_lower_unit_static_accessor_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    let [cname_id, pname_id, unit_id] = args else {
        return None;
    };
    let Expr::String(cname) = ctx.ast.get_expr(*cname_id) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
    let Expr::String(pname) = ctx.ast.get_expr(*pname_id) else {
        return None;
    };
    let pname = pname.clone();
    let face = |suffix: &str| -> Option<(crate::ssa::FuncId, crate::ssa::SigId)> {
        let body = format!("__sm_{cname}__{}{suffix}", mangle_key(&pname));
        let body_fid = ctx.fn_table.get(body.as_str()).copied()?;
        ctx.boxed_entries.get(&body_fid).copied()
    };
    let get = face("_get");
    let set = face("_set");
    if get.is_none() && set.is_none() {
        return Some(Operand::ConstI64(0));
    }
    let unit_op = ctx.lower_expr(*unit_id);
    let name_op = ctx.lower_expr(*pname_id);
    let cur_block = ctx.cur_block;
    let mut vaddr = |f: Option<(crate::ssa::FuncId, crate::ssa::SigId)>| match f {
        Some((fid, sig)) => Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::FnAddr(fid),
            Type::FnSig(sig),
            None,
        )),
        None => Operand::ConstI64(0),
    };
    let get_op = vaddr(get);
    let set_op = vaddr(set);
    let define = ctx.intrinsics.class_unit_static_accessor_define;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(define, vec![unit_op, name_op, get_op, set_op]),
    );
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}

/// K4-2 — `__torajs_unit_static_field_define(__unit, "<f>", <init>)`:
/// one static field of ONE evaluation, defined (not written) onto its
/// class object. Evaluation order is the spec's: the initializer runs,
/// then the define.
pub(super) fn try_lower_unit_static_field_define(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    let [unit_id, name_id, init_id] = args else {
        return None;
    };
    // `static f;` initializes to `undefined` (DefineField: no
    // initializer means undefined), which the bare pair encoder spells
    // as null.
    let (vtag, vvalue) = ctx.lower_to_tag_value(*init_id);
    let unit_op = ctx.lower_expr(*unit_id);
    let name_op = ctx.lower_expr(*name_id);
    let define = ctx.intrinsics.class_unit_static_field_define;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(define, vec![unit_op, name_op, vtag, vvalue]),
    );
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}

/// 563-03 — `__torajs_class_static_own_move_to_end("<C>", "<M>")`:
/// move the class object's own `<M>` entry behind everything defined
/// so far. §15.7.14 defines every static element in ONE ordered
/// pass, but a COMPUTED static member's key exists only at the
/// class-decl position — long after the prologue's registration walk
/// defined the plain members — and an own entry can only be
/// APPENDED. So the members declared after the computed one move
/// behind it, one call each. The instance side reads its element
/// order back off the class's method table (562-07); a class object
/// has no such table, its members arrive one emitted reify at a
/// time, so the repair is emitted too.
///
/// A tag miss (generator-factory class) skips the move — the
/// member's own order is whatever the walk gave it.
pub(super) fn try_lower_static_own_move_to_end(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
    let Some(tag) = ctx.class_name_to_tag.get(cname.as_str()).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let name_op = ctx.lower_expr(args[1]);
    let cur_block = ctx.cur_block;
    let mv = ctx.intrinsics.class_static_own_move_to_end;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(mv, vec![Operand::ConstI64(tag as i64), name_op]),
    );
    // The lowered Str literal is a caller-owned temp — the move
    // carries the entry's OWN key cell across, never this one.
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    Some(Operand::ConstI64(0))
}

/// L3b static-field-reflect (2026-07-22) —
/// `__torajs_static_field_reify("<C>", "<f>", __sf_<C>__<f>)`: box
/// the global slot's current value to its `(tag, value)` pair (the
/// heap arm's rc_inc is the entry's stake — the define takes it)
/// and hand runtime the quad for a real own data entry on the class
/// object. A tag-miss (generator-factory class) skips the define.
pub(super) fn try_lower_static_field_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
) -> Option<Operand> {
    if args.len() != 3 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
    let cname = cname.clone();
    let Some(tag) = ctx.class_name_to_tag.get(cname.as_str()).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let name_op = ctx.lower_expr(args[1]);
    let val = ctx.lower_expr(args[2]);
    let (vtag, vvalue) = ctx.box_to_tag_value(val);
    let define = ctx.intrinsics.static_field_define;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(
            define,
            vec![Operand::ConstI64(tag as i64), name_op, vtag, vvalue],
        ),
    );
    // The lowered Str literal is a caller-owned temp — the runtime
    // key copy is the define's own (dynobj_define rc_incs the key).
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    emit_class_binding_writeback(ctx, &cname, tag, false);
    Some(Operand::ConstI64(0))
}

/// RFC 20260802-class-computed-member 刀 2 —
/// `__torajs_class_computed_reify("<C>", "<sentinel>", <key expr>,
/// kind, is_static)`: the class-decl-position patch for one runtime
/// computed member. `lower_key(DefineKey::Expr)` evaluates the key
/// at the spec ClassDefinitionEvaluation point (Str coerce / Symbol
/// pass-through / any-lane ToPropertyKey with throw check), the
/// sentinel resolves the ordinary `__cm_` / `__sm_` body's boxed
/// adapter (accessor faces under `_get` / `_set`), and the runtime
/// define lands the reified face on `__proto_<C>` / `__class_<C>`.
/// kind: 0 = method, 1 = getter, 2 = setter. Adapter-dropout skips
/// the define (same posture as the sibling reifies) — but the key
/// STILL evaluates: its side effects and throws are spec-observable
/// (accessor-name computed-err-* family).
/// S2.38 — a static body never reads a runtime receiver (its `this`
/// resolves to the class object at parse time), so its face may run a
/// detached bare call IF the adapter-visible argument surface is
/// lossless: every undefaulted param `Any`, `F64` or `Str` (the
/// adapter keeps an undefined argv box as the slot's own undefined
/// sentinel; an I64 / Bool slot would make it 0 / false) and no
/// caller-side-injected default a runtime call would bypass.
///
/// Mirror of the instance-side verdict (S2.38/S2.39, see
/// `ssa_lower_module_metadata`): an adapter-substituted literal
/// default (Number/Bool) admits any scalar slot; an expression
/// default refuses.
///
/// Shared by the named-static reify and the computed-key one. The
/// computed path used to hardcode `this_free = 0`, so a
/// symbol-keyed static that met every criterion still refused a
/// detached call — `const f = C[Symbol.hasInstance]; f(4)` threw
/// "class method called without a receiver" where the identically
/// shaped `C.named` ran fine.
pub(super) fn static_this_free(
    ctx: &LowerCtx<'_>,
    body: &str,
    body_fid: crate::ssa::FuncId,
) -> bool {
    let ast_params = ctx.ast.stmts.iter().find_map(|s| match s {
        crate::ast::Stmt::FnDecl { name, params, .. } if name == body => Some(params),
        _ => None,
    });
    // A body that reads `this` (the class object) keeps its face on
    // the twin for every call: a rebind must see the new receiver.
    if ctx.ast.smany_reads_this.contains(body) {
        return false;
    }
    // A boolean / integer slot is lossless once a twin with an
    // adapter stands behind it (the mono adapter hands an undefined
    // one over).
    let twin_backed = crate::ssa_lower_boxed_entry::twin_adapter_of(
        ctx.ast,
        ctx.fn_table,
        ctx.boxed_entries,
        body,
    )
    .is_some();
    ctx.fn_sig_ids
        .get(&body_fid)
        .zip(ast_params)
        .is_some_and(|(sid, ps)| {
            let tys = &ctx.fn_sigs[sid.0 as usize].0;
            tys.len() == ps.len()
                && tys.iter().zip(ps.iter()).all(|(ty, p)| match &p.default {
                    None => {
                        matches!(ty, Type::Any | Type::F64 | Type::Str)
                            || (twin_backed && matches!(ty, Type::Bool | Type::I64 | Type::I32))
                    }
                    Some(d) => {
                        matches!(ctx.ast.get_expr(*d), Expr::Number(_) | Expr::Bool(_))
                            && matches!(
                                ty,
                                Type::Any | Type::I64 | Type::I32 | Type::F64 | Type::Bool
                            )
                    }
                })
        })
}

/// RFC 20260718-accessor-reify 刀 2+3 —
/// `__torajs_class_accessor_reify("<C>", "<p>")` (instance,
/// `__cm_` faces onto the prototype) and its static twin
/// (`__sm_` faces onto the class object): resolves the face
/// bodies' boxed adapters (either may be absent for a get-/set-only
/// accessor) and hands runtime the `(tag, name-Str, get-vaddr,
/// set-vaddr)` quad. Both-adapters dropout skips the define.
pub(super) fn try_lower_class_accessor_reify(
    ctx: &mut LowerCtx<'_>,
    args: &[ExprId],
    is_static: bool,
) -> Option<Operand> {
    if args.len() != 2 {
        return None;
    }
    let Expr::String(cname) = ctx.ast.get_expr(args[0]) else {
        return None;
    };
    let cname = cname.to_string_lossy_owned();
    let Expr::String(pname) = ctx.ast.get_expr(args[1]) else {
        return None;
    };
    let cname = cname.clone();
    let pname = pname.clone();
    let Some(tag) = ctx.class_name_to_tag.get(&cname).copied() else {
        return Some(Operand::ConstI64(0));
    };
    let body_prefix = if is_static { "__sm_" } else { "__cm_" };
    let face = |suffix: &str| -> Option<(crate::ssa::FuncId, crate::ssa::SigId)> {
        let body = format!("{body_prefix}{cname}__{}{suffix}", mangle_key(&pname));
        let body_fid = ctx.fn_table.get(body.as_str()).copied()?;
        ctx.boxed_entries.get(&body_fid).copied()
    };
    let get = face("_get");
    let set = face("_set");
    if get.is_none() && set.is_none() {
        return Some(Operand::ConstI64(0));
    }
    let cur_block = ctx.cur_block;
    let mut vaddr = |f: Option<(crate::ssa::FuncId, crate::ssa::SigId)>| match f {
        Some((fid, sig)) => Operand::Value(ctx.f.append_inst(
            cur_block,
            InstKind::FnAddr(fid),
            Type::FnSig(sig),
            None,
        )),
        None => Operand::ConstI64(0),
    };
    let get_op = vaddr(get);
    let set_op = vaddr(set);
    let name_op = ctx.lower_expr(args[1]);
    let define = if is_static {
        ctx.intrinsics.class_static_accessor_define
    } else {
        ctx.intrinsics.class_accessor_define
    };
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Call(
            define,
            vec![Operand::ConstI64(tag as i64), name_op, get_op, set_op],
        ),
    );
    // The lowered Str literal is a caller-owned temp — the runtime
    // key copy is the define's own (dynobj_define rc_incs the key).
    let ty = ctx.operand_ty(&name_op);
    ctx.emit_drop_value(name_op, ty);
    emit_class_binding_writeback(ctx, &cname, tag, !is_static);
    Some(Operand::ConstI64(0))
}
