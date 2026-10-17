//! The typed access of an object-literal accessor member a delete of
//! the program can remove (`check_type_of_member_accessor::deleted_read`).
//!
//! §10.1.10 [[Delete]] records the removal in the member's sidecar
//! behind `FLAG_OBJ_EXOTIC_FIELD`; a later [[Get]] (§10.1.8.1) finds no
//! own property and goes on to the prototype chain, whose answer
//! (`undefined`, or what the prototype holds) the getter's declared
//! type cannot spell, so the checker typed the read `any`. A store
//! after the delete creates an ordinary data property there instead of
//! running the setter. An instance with the bit set asks what the
//! member is now ([`branch_gone`]); every other access calls the
//! accessor in the layout slot. The read here answers an owned `any`
//! on each arm; the setter store and the spread copy use the same legs.

use std::collections::HashMap;

use crate::ast::ExprId;
use crate::ast::named_prop_census::shadow_names::SlotKind;
use crate::ssa::{
    BlockId, FuncId, IPred, InstKind, Module, Operand, StructId, Terminator, Type, ValueId,
};
use crate::ssa_lower::{LowerCtx, declare_intrinsic};

/// `(cell, name Str) -> i64`: the member's state, 0 while it is an own
/// accessor, [`STATE_DELETED`] or 2 (a data property replaced it) otherwise.
const DELETED: &str = "__torajs_obj_member_deleted";
/// `(cell, name Str) -> owned any`: the replacing data property's
/// value, or the prototype chain's answer for a deleted member.
const PROTO_GET: &str = "__torajs_obj_deleted_member_get";

/// No own property is left under the name.
pub(crate) const STATE_DELETED: i64 = 1;

/// Declare the two legs [`read`] calls.
pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) {
    let args = [Type::Ptr, Type::Str];
    declare_intrinsic(module, fn_table, DELETED, &args, Type::I64);
    declare_intrinsic(module, fn_table, PROTO_GET, &args, Type::Any);
}

/// Whether a delete of the program can reach the accessor member
/// `name`, so that a typed access of it asks [`branch_gone`] first.
pub(crate) fn reaches(ctx: &LowerCtx<'_>, name: &str) -> bool {
    (ctx.ast.accessor_field_names).mistyped_read(name, SlotKind::Accessor)
}

/// The owned `any` the read `eid` of `obj.name` answers, or `None`
/// when the read is not one the checker retyped for a delete.
pub(super) fn read(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    obj: ExprId,
    obj_val: Operand,
    (sid, name): (StructId, &str),
) -> Option<Operand> {
    if !matches!(ctx.expr_types.get(&eid), Some(crate::check::Type::Any)) || !reaches(ctx, name) {
        return None;
    }
    let slot = format!("__getter_{name}");
    if !(ctx.struct_layouts[sid.0 as usize].iter())
        .any(|(fname, ty)| *fname == slot && matches!(ty, Type::Closure(_)))
    {
        return None;
    }
    let res = ctx.alloca_in_entry(Type::Any, Some("__accessor_read"));
    let gone = branch_gone(ctx, obj_val, name);
    let done = ctx.f.add_block();

    ctx.cur_block = gone.gone_blk;
    let got = own_value(ctx, obj_val, gone.name_str);
    store_and_join(ctx, got, res, done);

    ctx.cur_block = gone.own_blk;
    let v = super::try_objlit_getter(ctx, obj, obj_val, sid, name)
        .expect("ssa-lower: an accessor slot checked above lowers its getter call");
    let boxed = box_getter_answer(ctx, v);
    store_and_join(ctx, boxed, res, done);

    ctx.cur_block = done;
    let out = ctx.f.append_inst(
        done,
        InstKind::Load(Type::Any, Operand::Value(res), 0),
        Type::Any,
        None,
    );
    Some(Operand::Value(out))
}

/// The blocks [`branch_gone`] answers.
pub(crate) struct Gone {
    /// The member is no own accessor any more.
    pub gone_blk: BlockId,
    /// The member is the accessor the layout declares.
    pub own_blk: BlockId,
    /// The member's state, defined in `gone_blk`: [`STATE_DELETED`],
    /// or 2 for the data property a store after the delete created.
    pub state: Operand,
    /// The member's name, defined in `gone_blk`.
    pub name_str: Operand,
}

/// Branch on whether the declared accessor member `name` of `obj_val`
/// is still an own accessor. An instance without the exotic-field bit
/// takes `own_blk` after one header read. Terminates the current block.
pub(crate) fn branch_gone(ctx: &mut LowerCtx<'_>, obj_val: Operand, name: &str) -> Gone {
    let (hit_blk, plain_blk) = super::exotic_read::branch(ctx, obj_val);
    let gone_blk = ctx.f.add_block();
    let own_blk = ctx.f.add_block();
    ctx.cur_block = hit_blk;
    let name_str = Operand::Value(ctx.intern_string_literal(name));
    let state = call(ctx, hit_blk, DELETED, vec![obj_val, name_str], Type::I64);
    let gone = ctx.f.append_inst(
        hit_blk,
        InstKind::ICmp(IPred::Ne, state, Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    ctx.f.set_term(
        hit_blk,
        Terminator::CondBr {
            cond: Operand::Value(gone),
            then_blk: gone_blk,
            else_blk: own_blk,
        },
    );
    ctx.f.set_term(plain_blk, Terminator::Br(own_blk));
    Gone {
        gone_blk,
        own_blk,
        state,
        name_str,
    }
}

/// §10.1.8.1 [[Get]] of a member [`branch_gone`] found gone, owned:
/// the replacing data property's value, or the prototype's answer.
pub(crate) fn own_value(ctx: &mut LowerCtx<'_>, obj_val: Operand, name_str: Operand) -> Operand {
    let blk = ctx.cur_block;
    let got = call(ctx, blk, PROTO_GET, vec![obj_val, name_str], Type::Any);
    ctx.emit_throw_check(None);
    got
}

/// The owned `any` a getter's owned answer `v` boxes to.
pub(crate) fn box_getter_answer(ctx: &mut LowerCtx<'_>, v: Operand) -> Operand {
    match ctx.operand_ty(&v) {
        Type::Any => v,
        // boxing a cell moves its stake
        Type::F64 => ctx.box_f64_or_undef(v),
        _ => ctx.box_to_any(v),
    }
}

fn call(ctx: &mut LowerCtx<'_>, blk: BlockId, leg: &str, args: Vec<Operand>, ty: Type) -> Operand {
    let v = ctx
        .f
        .append_inst(blk, InstKind::Call(ctx.fn_table[leg], args), ty, None);
    Operand::Value(v)
}

fn store_and_join(ctx: &mut LowerCtx<'_>, v: Operand, res: ValueId, done: BlockId) {
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(v, Operand::Value(res), 0));
    ctx.f.set_term(blk, Terminator::Br(done));
}
