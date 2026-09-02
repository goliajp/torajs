//! `Type::Obj` arm of [`super::lower`] — split from
//! `composite.rs` when the 666 null gate pushed it over the
//! 500-line file limit. Field recursion goes back through
//! `super::lower`; the shared null gate lives in
//! [`super::composite`].

use crate::ast::PropKey;
use crate::ssa::{InstKind, Operand, StructId, Terminator, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};
use torajs_wtf8::{Wtf8, Wtf8Buf};

use super::composite::with_null_gate;

/// `{"k":v,...}` — compile-time field unfold from
/// `struct_layouts[sid]`; primitive-only layouts take the `__torajs_jsb_*`
/// builder fast path, everything else the str_concat chain. The
/// unfold sits behind the shared [`with_null_gate`] — a NULL Obj
/// slot (`Nullable<Obj>` holding JS null) answers `"null"` instead
/// of dereferencing NULL on the first field load (655 arr-arm
/// mirror; the parse-reject that used to shield this lane is gone).
pub(super) fn lower_obj(
    ctx: &mut LowerCtx,
    val_op: Operand,
    sid: StructId,
    fe_fields: Option<Vec<(PropKey, crate::check::Type)>>,
    gap: Option<Operand>,
    depth: u32,
    is_error: bool,
) -> Operand {
    with_null_gate(ctx, &val_op.clone(), "__json_obj_out", |ctx| {
        // §20.5 — an error instance's own `message` / `stack` are
        // `E:false` and `message` may not be own at all, which is
        // runtime state a layout unfold cannot read. Hand the receiver
        // to the any-lane walk (the `Type::BigInt` arm's posture, throw
        // check included); `box_to_any` is rc-neutral, so the walk
        // borrows and there is nothing extra to drop.
        if is_error {
            let boxed = ctx.box_to_any(val_op.clone());
            let v = super::emit_any_walk(ctx, boxed, gap.clone(), depth);
            ctx.emit_throw_check(None);
            return v;
        }
        let layout = ctx.struct_layouts[sid.0 as usize].clone();
        let obj_ptr = match val_op {
            Operand::Value(v) => v,
            _ => unreachable!(),
        };
        let primitive_only = layout
            .iter()
            .all(|(_, fty)| matches!(fty, Type::I64 | Type::Bool | Type::Str));
        if primitive_only && gap.is_none() {
            // A frontend `undefined` field is pointer-shaped at SSA,
            // so a layout this gate accepts never carries one — the
            // builder path needs no frontend types. It also writes
            // its punctuation straight into one buffer, which leaves
            // nowhere to splice indentation, so an indenting call
            // takes the concat lane instead.
            super::composite_obj_jsb::lower_obj_jsb(ctx, obj_ptr, &layout)
        } else {
            lower_obj_concat(ctx, obj_ptr, &layout, fe_fields.as_deref(), gap, depth)
        }
    })
}

/// str_concat slow lane (mixed layouts): slot-held accumulator,
/// runtime `json_obj_sep` separator, per-Str-field sentinel branch
/// (undefined → the whole `<sep>"key":<val>` segment is skipped).
fn lower_obj_concat(
    ctx: &mut LowerCtx,
    obj_ptr: crate::ssa::ValueId,
    layout: &[(PropKey, Type)],
    fe_fields: Option<&[(PropKey, crate::check::Type)]>,
    gap: Option<Operand>,
    depth: u32,
) -> Operand {
    // Chunk 658 — the accumulator lives in a slot and the `,`
    // separator is a runtime decision (`json_obj_sep`: emitted-any-
    // field ⇔ acc longer than the opening `{`): an undefined Str
    // field skips key + value + separator per §25.5.2.4 step 8.b,
    // which a compile-time `i > 0` comma cannot express.
    let lbrace = ctx.intern_string_literal("{");
    let rbrace = ctx.intern_string_literal("}");
    let colon = ctx.intern_string_literal(":");
    let acc_slot = ctx.alloca_in_entry(Type::Str, Some("__json_obj_acc"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(lbrace), Operand::Value(acc_slot), 0),
    );
    for (i, (fname, slot_ty)) in layout.iter().enumerate() {
        let fe =
            fe_fields.and_then(|fs| fs.iter().find(|(n, _)| n == fname).map(|(_, t)| t.clone()));
        emit_field(
            ctx, obj_ptr, acc_slot, colon, i, fname, slot_ty, fe, &gap, depth,
        );
    }
    let acc_final = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc_slot), 0),
        Type::Str,
        None,
    );
    let acc_final = match &gap {
        Some(g) => super::composite::concat_close_indent(ctx, Operand::Value(acc_final), g, depth),
        None => Operand::Value(acc_final),
    };
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![acc_final.clone(), Operand::Value(rbrace)],
        ),
        Type::Str,
        None,
    );
    // 642 ledger — release the final accumulator (an all-skipped
    // object's acc is still the "{" literal: no-op drop).
    ctx.emit_drop_value(acc_final, Type::Str);
    Operand::Value(result)
}

/// The `[[Get]]` half of [`emit_field`]: answers the key, the value
/// operand, its type and whether that operand is a fresh stake —
/// or `None` for a slot whose whole segment §25.5.2.4 omits before
/// any value exists. Bodies verbatim from the pre-split loop
/// (`continue` becomes `None`).
fn resolve_field(
    ctx: &mut LowerCtx,
    obj_ptr: crate::ssa::ValueId,
    field_off: u64,
    fname: &Wtf8,
    slot_ty: &Type,
    fe: Option<&crate::check::Type>,
) -> Option<(Wtf8Buf, Operand, Type, bool)> {
    // RFC 20260714-objlit-accessor — §25.5.2.4 serializes through
    // [[Get]], so an accessor contributes its GETTER'S RESULT under
    // the plain property name, never the closure sitting in its
    // synthetic slot (which used to reach the value recursion as a
    // `Type::Closure` and panic "not yet supported"). A setter is
    // skipped outright: paired with a getter its key is already
    // emitted, and alone its [[Get]] is undefined — step 8.b then
    // omits the whole segment.
    let accessor = crate::check_type_of_object_lit::accessor_slot(fname);
    if matches!(accessor, Some(("__setter_", _))) {
        return None;
    }
    // §25.5.2.4 step 8.b again, for the shape SSA cannot express: an
    // `undefined` slot is pointer-shaped, indistinguishable from a
    // slot holding JS null, whose key DOES print. The checker keeps
    // the two apart, so the frontend type settles it before the load.
    if matches!(fe, Some(crate::check::Type::Undefined)) {
        return None;
    }
    // §25.5.2.4 — a property that serializes to nothing has its
    // whole segment omitted by step 8.b. Two static shapes always
    // do: a callable (step 11) and a Symbol. (`toJSON` never
    // reaches here — the hook consumed the value before the
    // unfold.) Accessor slots are excluded from the callable half:
    // their Closure is synthetic and the arm below calls it for
    // its [[Get]].
    if matches!(slot_ty, Type::Symbol)
        || (accessor.is_none() && matches!(slot_ty, Type::Closure(_)))
    {
        return None;
    }
    match accessor {
        Some((_, prop)) => {
            let Type::Closure(sig_id) = *slot_ty else {
                return None;
            };
            // A body with no `return` types Void — its [[Get]] is
            // `undefined`, so step 8.b omits the key. (Calling it
            // and handing a Void operand to the value recursion is
            // the shape that read x0 garbage in the
            // RFC 20260713-accessor-void-kind incident.)
            if matches!(ctx.fn_sigs[sig_id.0 as usize].1, Type::Void) {
                return None;
            }
            let got = crate::ssa_lower_call_struct_method_dispatch::emit_receiver_closure_call(
                ctx,
                Operand::Value(obj_ptr),
                field_off,
                sig_id,
                &[],
            );
            let ret_ty = ctx.operand_ty(&got);
            Some((prop.to_owned(), got, ret_ty, true))
        }
        None => {
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(*slot_ty, Operand::Value(obj_ptr), field_off),
                *slot_ty,
                None,
            );
            Some((fname.to_owned(), Operand::Value(v), *slot_ty, false))
        }
    }
}

/// One `<sep>"key":<value>` segment of [`lower_obj_concat`] — split
/// out when the step 8.b runtime verdict pushed the loop body past
/// the 200-line function limit. Body verbatim from the pre-split
/// loop apart from that probe.
fn emit_field(
    ctx: &mut LowerCtx,
    obj_ptr: crate::ssa::ValueId,
    acc_slot: crate::ssa::ValueId,
    colon: crate::ssa::ValueId,
    i: usize,
    fname: &Wtf8,
    slot_ty: &Type,
    fe: Option<crate::check::Type>,
    gap: &Option<Operand>,
    depth: u32,
) {
    let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
    let Some((key, field_v, fty, getter_owned)) =
        resolve_field(ctx, obj_ptr, field_off, fname, slot_ty, fe.as_ref())
    else {
        return;
    };
    let fty = &fty;
    // Undefined-only probe (NULL keeps the key, prints null):
    // §25.5.2.4 step 8.b skips the whole `<sep>"key":<val>`
    // segment. Str slots probe the Str sentinel; refcounted
    // pointer slots (RFC 20260710 C2b) probe the generic
    // Tag::Undefined cell — without it the field recursion
    // would walk the bare oddball header as a live cell.
    let undef_probe = match fty {
        Type::Str => Some(ctx.intrinsics.str_is_undef),
        t if t.spells_undef_with_generic_cell() => Some(ctx.intrinsics.is_undef_cell),
        _ => None,
    };
    let merge_blk = if let Some(probe_fid) = undef_probe {
        let is_undef = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(probe_fid, vec![field_v.clone()]),
            Type::Bool,
            None,
        );
        let emit_blk = ctx.f.add_block();
        let merge = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(is_undef),
                then_blk: merge,
                else_blk: emit_blk,
            },
        );
        ctx.cur_block = emit_blk;
        Some(merge)
    } else {
        None
    };
    // §25.5.2 step 2.b — the key this value sits under is the
    // property name, which this lane knows statically. The value is
    // serialized BEFORE the separator / key are appended: step 8.b
    // omits the whole segment when SerializeJSONProperty answers
    // nothing, a verdict only the finished walk can report, and
    // spec order runs [[Get]] then SerializeJSONProperty then the
    // emit decision anyway (the separator / key concat carries no
    // user-observable effect, so moving it after is free).
    let hook_key = Operand::Value(ctx.intern_string_literal(&key));
    let field_str = super::lower_keyed(
        ctx,
        field_v.clone(),
        *fty,
        Some(hook_key),
        fe,
        gap.clone(),
        depth + 1,
    );
    // A field Load borrows the struct's slot; a getter call answers
    // a fresh +1 that nothing else owns.
    if getter_owned {
        ctx.emit_drop_value(field_v.clone(), *fty);
    }
    // The static shapes that always serialize to nothing were
    // skipped above; an `any` slot decides at runtime (undefined, a
    // callable, a Symbol) and the any-lane walk reports all three as
    // the undefined-Str sentinel. Every typed lane answers real
    // text, so the probe sits behind that static gate.
    let merge_blk = if matches!(fty, Type::Any) {
        let merge = merge_blk.unwrap_or_else(|| ctx.f.add_block());
        let is_nothing = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.str_is_undef, vec![field_str.clone()]),
            Type::Bool,
            None,
        );
        let skip_blk = ctx.f.add_block();
        let write_blk = ctx.f.add_block();
        ctx.f.set_term(
            ctx.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(is_nothing),
                then_blk: skip_blk,
                else_blk: write_blk,
            },
        );
        ctx.cur_block = skip_blk;
        // 642 ledger — the segment is dropped, so is the walk's
        // answer (a static sentinel block: the drop is a no-op).
        ctx.emit_drop_value(field_str.clone(), Type::Str);
        ctx.f.set_term(ctx.cur_block, Terminator::Br(merge));
        ctx.cur_block = write_blk;
        Some(merge)
    } else {
        merge_blk
    };
    let acc_now = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::Str, Operand::Value(acc_slot), 0),
        Type::Str,
        None,
    );
    let acc_sep = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.json_obj_sep, vec![Operand::Value(acc_now)]),
        Type::Str,
        None,
    );
    let acc_sep = match gap {
        Some(g) => super::composite::concat_indent(ctx, Operand::Value(acc_sep), g, depth + 1),
        None => Operand::Value(acc_sep),
    };
    let key_str = ctx.intern_string_literal(&key);
    let key_quoted = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.json_quote_str, vec![Operand::Value(key_str)]),
        Type::Str,
        None,
    );
    let v1 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![acc_sep.clone(), Operand::Value(key_quoted)],
        ),
        Type::Str,
        None,
    );
    // 642 ledger — every str_concat answers a fresh Str, so each
    // consumed link releases right after its concat (json_obj_sep
    // is owned-in/owned-out and already settled acc_now; interned
    // literals no-op through the same drop).
    ctx.emit_drop_value(acc_sep, Type::Str);
    ctx.emit_drop_value(Operand::Value(key_quoted), Type::Str);
    // Under a gap the colon carries a trailing space, but only if
    // that gap is non-empty — a runtime fact, so the spelling comes
    // from the runtime too (§25.5.2.4 step 9.b.iii).
    let colon_op = match gap {
        Some(g) => {
            let c = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.json_colon, vec![g.clone()]),
                Type::Str,
                None,
            );
            Operand::Value(c)
        }
        None => Operand::Value(colon),
    };
    let v2 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(v1), colon_op.clone()],
        ),
        Type::Str,
        None,
    );
    ctx.emit_drop_value(Operand::Value(v1), Type::Str);
    if gap.is_some() {
        // The interned `:` literal is static (its drop is a no-op);
        // the runtime spelling is a fresh stake this segment owns.
        ctx.emit_drop_value(colon_op, Type::Str);
    }
    let v3 = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.str_concat,
            vec![Operand::Value(v2), field_str],
        ),
        Type::Str,
        None,
    );
    ctx.emit_drop_value(Operand::Value(v2), Type::Str);
    ctx.emit_drop_value(field_str.clone(), Type::Str);
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(v3), Operand::Value(acc_slot), 0),
    );
    if let Some(mb) = merge_blk {
        ctx.f.set_term(ctx.cur_block, Terminator::Br(mb));
        ctx.cur_block = mb;
    }
}
