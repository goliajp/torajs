//! Fields a literal leaves out of the type it is stored as.
//!
//! An optional field the literal does not spell (`const p: { a?:
//! number; b: number } = { b: 2 }`) is not an own property of the
//! object (§10.1.11): `"a" in p` is false and `Object.keys(p)` lists
//! only `b`. The checker records such a literal with the declared field
//! list, so the cell takes the declared layout and every typed read of
//! `p.a` stays one load of the slot's undefined.
//!
//! The cell records which slots it lacks itself: the header bit
//! `FLAG_OBJ_ABSENT_FIELD` and the set in the high half of the `+8`
//! word, bit `i` for slot `i` — stores among the ones that build it.
//! The typed own faces read the set inline, a typed store clears its
//! slot's bit (`ssa_lower_assign_member_field::guard`), and the runtime
//! reads it beside the deletion records `delete` leaves (torajs-meta
//! `struct_absent`). A slot past the 32 the set can name takes that
//! record from the start.

use crate::ast::{ExprId, PropKey};
use crate::ssa::{BinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

mod raise;
pub(crate) use raise::{fits_set, raise_set, record_deleted};

/// The slots the absent set can name; the set rides the high half of
/// the `+8` word (bit `SET_SHIFT + i` for slot `i`).
const SET_SLOTS: usize = 32;
const SET_WORD_OFF: u64 = 8;
const SET_SHIFT: u32 = 32;
/// The header word at `+4` is `type_tag | flags << 16`.
const FLAG_SHIFT: u32 = 16;

/// The declared fields the literal at `eid` does not spell, in
/// declared order with their declared index — empty unless the checker
/// recorded the literal with more fields than it has.
pub(super) fn absent_fields(
    ctx: &LowerCtx<'_>,
    eid: ExprId,
    entries: &[(PropKey, ExprId)],
) -> Vec<(usize, PropKey)> {
    match ctx.expr_types.get(&eid) {
        Some(t) => crate::check_assignable::literal_absent_fields(t, entries),
        None => Vec::new(),
    }
}

/// Put each absent field in at its declared position as the operand an
/// `undefined` literal lowers to; the caller re-materializes it as the
/// slot's undefined once the layout is resolved.
pub(super) fn insert_absent(
    absent: &[(usize, PropKey)],
    field_tys: &mut Vec<(PropKey, Type)>,
    field_vals: &mut Vec<Operand>,
) {
    for (i, name) in absent {
        let at = (*i).min(field_tys.len());
        field_tys.insert(at, (name.clone(), Type::Ptr));
        field_vals.insert(at, Operand::ConstPtrNull);
    }
}

/// An Any slot's undefined (`ANY_UNDEF`, payload 0).
pub(crate) fn any_undef(ctx: &mut LowerCtx<'_>) -> Operand {
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BoxAny(5, Operand::ConstI64(0)),
        Type::Any,
        None,
    );
    Operand::Value(v)
}

/// Whether a cell can lack a declared field: some literal of the
/// program leaves a member out, or the program can delete one.
pub(crate) fn own_set_moves(ctx: &LowerCtx<'_>) -> bool {
    ctx.ast.mints_absent_set || ctx.ast.accessor_field_names.may_freeze_or_delete()
}

/// The own-property answer for the declared field `key` of the struct
/// `obj`, which the layout lists: true, unless a cell can lack it
/// ([`own_set_moves`]), when the cell decides.
pub(crate) fn declared_field_own(
    ctx: &mut LowerCtx<'_>,
    obj: Operand,
    key: &torajs_wtf8::Wtf8,
) -> Operand {
    if !own_set_moves(ctx) {
        return Operand::ConstBool(true);
    }
    let absent = field_absent(ctx, obj, key);
    let own = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, absent, Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(own)
}

/// 1 when the declared field `key` of the struct `obj` is no own
/// property, else 0 (I64): its slot's bit in the absent set, or — on a
/// cell whose declared fields were redefined or deleted, which never
/// carries a set — the deletion record the runtime keeps.
pub(crate) fn field_absent(
    ctx: &mut LowerCtx<'_>,
    obj: Operand,
    key: &torajs_wtf8::Wtf8,
) -> Operand {
    let hdr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I32, obj, 4),
        Type::I32,
        None,
    );
    let hdr = Operand::Value(hdr);
    let in_set = match slot_of(ctx, obj, key).filter(|i| *i < SET_SLOTS) {
        Some(i) => {
            let raised = flag_raised(ctx, hdr, torajs_rc::flags::FLAG_OBJ_ABSENT_FIELD);
            let word = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, obj, SET_WORD_OFF),
                Type::I64,
                None,
            );
            let at = Operand::ConstI64((SET_SHIFT + i as u32) as i64);
            let shifted = bin(ctx, BinOp::LShr, Operand::Value(word), at);
            let lacks = bin(ctx, BinOp::And, shifted, Operand::ConstI64(1));
            bin(ctx, BinOp::And, raised, lacks)
        }
        None => Operand::ConstI64(0),
    };
    let exotic = flag_raised(ctx, hdr, torajs_rc::FLAG_OBJ_EXOTIC_FIELD);
    let hit = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, exotic, Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    let slot = ctx.alloca_in_entry(Type::I64, Some("__field_absent"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(in_set, Operand::Value(slot), 0),
    );
    let (ask, cont) = (ctx.f.add_block(), ctx.f.add_block());
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: ask,
            else_blk: cont,
        },
    );
    ctx.cur_block = ask;
    // the record lives in the expando dict; asking the dict rather than
    // the cell keeps the cell out of every call
    let k = ctx.intern_string_literal(key);
    let props = ctx.f.append_inst(
        ask,
        InstKind::Load(Type::Ptr, obj, crate::ssa_lower::OBJ_PROPS_OFF),
        Type::Ptr,
        None,
    );
    let hole = ctx.f.append_inst(
        ask,
        InstKind::Call(
            ctx.intrinsics.struct_expando.entry_is_hole,
            vec![Operand::Value(props), Operand::Value(k)],
        ),
        Type::I32,
        None,
    );
    let deleted = ctx.f.append_inst(
        ask,
        InstKind::ZExtI32ToI64(Operand::Value(hole)),
        Type::I64,
        None,
    );
    ctx.f.append_void(
        ask,
        InstKind::Store(Operand::Value(deleted), Operand::Value(slot), 0),
    );
    ctx.f.set_term(ask, Terminator::Br(cont));
    ctx.cur_block = cont;
    let v = ctx.f.append_inst(
        cont,
        InstKind::Load(Type::I64, Operand::Value(slot), 0),
        Type::I64,
        None,
    );
    Operand::Value(v)
}

/// The layout slot of the declared field `key` in the struct `obj`.
fn slot_of(ctx: &LowerCtx<'_>, obj: Operand, key: &torajs_wtf8::Wtf8) -> Option<usize> {
    let Type::Obj(sid) = ctx.operand_ty(&obj) else {
        return None;
    };
    ctx.struct_layouts[sid.0 as usize]
        .iter()
        .position(|(n, _)| n.as_wtf8() == key)
}

/// The header flag `flag` of the I32 header word `hdr`, as an I64 1 / 0.
fn flag_raised(ctx: &mut LowerCtx<'_>, hdr: Operand, flag: u16) -> Operand {
    let at = (FLAG_SHIFT + u32::from(flag).trailing_zeros()) as i32;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::LShr, hdr, Operand::ConstI32(at)),
        Type::I32,
        None,
    );
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::And, Operand::Value(v), Operand::ConstI32(1)),
        Type::I32,
        None,
    );
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ZExtI32ToI64(Operand::Value(v)),
        Type::I64,
        None,
    );
    Operand::Value(v)
}

fn bin(ctx: &mut LowerCtx<'_>, op: BinOp, a: Operand, b: Operand) -> Operand {
    let v = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::BinOp(op, a, b), Type::I64, None);
    Operand::Value(v)
}
