//! The checked cast of an argument into a class-typed parameter of a
//! typed body entered through `any` (RFC 20260922-any-to-typed-checked-cast
//! §2, row C3): the argument crosses the way an `any` value crosses
//! into any other class slot (`ssa_lower_checked_cast::obj`) — an
//! instance of the class as it is, an object of the class's field types
//! as a fresh instance, and anything else throws, before the body runs.
//!
//! A mono adapter whose all-`any` twin hosts a non-fitting call hands
//! it there first (`build_reroute`); a class factory's twin keeps its
//! class-typed parameters (`check_monomorph_any_widen::struct_escape::
//! factory`), so its own adapter casts them.

use crate::ssa::{self, BlockId, InstKind, Operand, StringId, StringLiteral, Type};
use crate::ssa_lower_checked_cast::plan::{self, PlanTables};
use crate::ssa_lower_checked_cast::{CastSlot, emit_plan_operands, site_line};

use super::BoxedEntryIntrinsics;
use super::targets::BoxedEntryTarget;
use super::unbox::{BoxedCoerceIntrinsics, drop_obj_temps, drop_settle_temps, drop_str_temps};

/// A parameter's cast: its plan and message, registered as strings.
#[derive(Clone)]
pub(super) struct ParamCast {
    plan: StringId,
    vtables: Vec<Option<String>>,
    site: StringId,
    members: bool,
    proto_keys: bool,
}

/// Per user slot of `t`, the cast of a class-typed parameter; `None`
/// for every other slot.
pub(super) fn param_casts(
    t: &BoxedEntryTarget,
    tables: &PlanTables<'_>,
    strings: &mut Vec<StringLiteral>,
) -> Vec<Option<ParamCast>> {
    (0..t.user_tys.len())
        .map(|i| {
            let (Type::Obj(sid), Some(ann)) = (t.user_tys[i], t.type_anns.get(i)?.as_deref())
            else {
                return None;
            };
            let base = crate::check_type_ann::strip_nullable(ann.trim()).trim();
            if !tables.slot_class(base, sid) || !tables.casts_into(sid, ann) {
                return None;
            }
            let compiled = plan::compile(tables, t.user_tys[i], ann);
            let plan = StringId(strings.len() as u32);
            strings.push(StringLiteral::from_latin1_bytes(compiled.bytes));
            tables.anon_stamp_pool.borrow_mut().note_cast_plan(plan);
            let shown = if base.len() != ann.trim().len() {
                format!("{base} | undefined")
            } else {
                base.to_string()
            };
            let site = StringId(strings.len() as u32);
            strings.push(StringLiteral::encode_from_str(&site_line(
                CastSlot::Pass,
                &shown,
            )));
            Some(ParamCast {
                plan,
                vtables: compiled.vtables,
                site,
                members: compiled.members,
                proto_keys: compiled.proto_keys,
            })
        })
        .collect()
}

/// The cast of the argument `av` in `entry`: the parameter's cell (a
/// stake of its own, NULL under a pending throw, which
/// [`emit_cast_check`] tests before the body runs). The answer, boxed,
/// joins `obj_temps` and is released after the call like every other.
pub(super) fn emit_param_cast(
    f: &mut ssa::Function,
    entry: BlockId,
    cast: &ParamCast,
    coerce: &BoxedCoerceIntrinsics<'_>,
    intr: &BoxedEntryIntrinsics,
    av: Operand,
    obj_temps: &mut Vec<ssa::ValueId>,
) -> Operand {
    let (plan_bits, vtables) = emit_plan_operands(f, entry, cast.plan, &cast.vtables);
    let site =
        Operand::Value(f.append_inst(entry, InstKind::StaticStrRef(cast.site), Type::Str, None));
    let args = vec![av, plan_bits, vtables, site];
    let kernel =
        (coerce.ids.cast_obj_entries).pick(coerce.ids.cast_obj, cast.members, cast.proto_keys);
    let p = f.append_inst(entry, InstKind::Call(kernel, args), Type::Ptr, None);
    let bits = f.append_inst(
        entry,
        InstKind::PtrToInt(Operand::Value(p)),
        Type::I64,
        None,
    );
    let held = f.append_inst(
        entry,
        InstKind::Call(
            intr.any_box,
            vec![Operand::ConstI64(4), Operand::Value(bits)],
        ),
        Type::Any,
        None,
    );
    obj_temps.push(held);
    Operand::Value(p)
}

/// After the unboxes, when a parameter was cast: on a pending throw,
/// release the temps (Str, obj, settle) and return `undefined` without
/// running the body (the caller's throw check unwinds). Answers the
/// block the body call goes in.
pub(super) fn emit_cast_check(
    f: &mut ssa::Function,
    entry: BlockId,
    intr: &BoxedEntryIntrinsics,
    coerce: &BoxedCoerceIntrinsics<'_>,
    temps: (
        &[ssa::ValueId],
        &[ssa::ValueId],
        &[(ssa::ValueId, ssa::ValueId)],
    ),
) -> BlockId {
    if coerce.casts.iter().all(Option::is_none) {
        return entry;
    }
    let pending = f.append_inst(
        entry,
        InstKind::Call(coerce.ids.throw_check, vec![]),
        Type::I64,
        None,
    );
    let thrown = f.append_inst(
        entry,
        InstKind::ICmp(
            ssa::IPred::Ne,
            Operand::Value(pending),
            Operand::ConstI64(0),
        ),
        Type::Bool,
        None,
    );
    let bail = f.add_block();
    let body = f.add_block();
    f.set_term(
        entry,
        ssa::Terminator::CondBr {
            cond: Operand::Value(thrown),
            then_blk: bail,
            else_blk: body,
        },
    );
    let (str_temps, obj_temps, settle_temps) = temps;
    drop_str_temps(f, bail, intr, str_temps);
    drop_obj_temps(f, bail, coerce.ids.anyv_rc_dec, obj_temps);
    drop_settle_temps(f, bail, intr.any_unbox_settle, settle_temps);
    let undef = f.append_inst(
        bail,
        InstKind::Call(
            intr.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    );
    f.set_term(bail, ssa::Terminator::Ret(Some(Operand::Value(undef))));
    body
}
