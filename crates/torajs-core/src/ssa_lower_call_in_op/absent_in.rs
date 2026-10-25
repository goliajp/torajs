//! `"k" in o` for a member the struct layout of `o` declares, in a
//! program where a cell can lack it (a literal left it out, a delete
//! removed it): the cell's own answer first, then the rest of the
//! prototype chain. Moved out of [`super`].

use crate::ssa::{BinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// The members `Object.prototype` has before user code adds any.
const OBJECT_PROTO_MEMBERS: [&str; 12] = [
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
    "__proto__",
];

/// `"k" in o` for a member `k` the layout of the struct `obj` declares,
/// in a program where a cell can lack it: true when the cell owns it
/// (one inline test), else the prototype chain answers.
pub(super) fn absent_member_in(
    ctx: &mut LowerCtx<'_>,
    obj: Operand,
    key: &torajs_wtf8::Wtf8,
) -> Operand {
    let own = crate::ssa_lower_object_lit::absent::declared_field_own(ctx, obj, key);
    let slot = ctx.alloca_in_entry(Type::Bool, Some("__in_own"));
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(own, Operand::Value(slot), 0));
    let (chain, cont) = (ctx.f.add_block(), ctx.f.add_block());
    ctx.f.set_term(
        ctx.cur_block,
        crate::ssa::Terminator::CondBr {
            cond: own,
            then_blk: cont,
            else_blk: chain,
        },
    );
    ctx.cur_block = chain;
    let k = Operand::Value(ctx.intern_string_literal(key));
    let r = match OBJECT_PROTO_MEMBERS.iter().any(|m| key == *m) {
        // the builtin members answer through the interned prototype tables
        true => {
            let boxed = ctx.box_to_any(obj);
            let call = InstKind::Call(ctx.intrinsics.in_op_any_str, vec![boxed, k]);
            Operand::Value(ctx.f.append_inst(chain, call, Type::Bool, None))
        }
        false => lacking_member_chain(ctx, obj, k, cont),
    };
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(r, Operand::Value(slot), 0));
    ctx.f.set_term(blk, crate::ssa::Terminator::Br(cont));
    ctx.cur_block = cont;
    let v = ctx.f.append_inst(
        cont,
        InstKind::Load(Type::Bool, Operand::Value(slot), 0),
        Type::Bool,
        None,
    );
    Operand::Value(v)
}

/// The C symbol of torajs-rc's builtin prototype slots, one pointer per
/// tag, 0 until the singleton is minted.
pub(crate) const BUILTIN_PROTO_SLOTS_SYM: &str = "___torajs_builtin_proto_slots";

/// HasProperty of `k` past the own face of the struct cell `obj`, which
/// lacks the declared member: the rest of the chain. Leaves `cur_block`
/// on the block holding the answer; `skip` is where a `false` answer
/// already waits (the caller stored the own answer there).
///
/// A cell that is no class instance and carries no user [[Prototype]]
/// inherits from %Object.prototype% alone, and while that object holds
/// only what its mint installed the answer is false without the walk
/// ([`object_proto_as_minted`]) — three cells in four on
/// objlit-optional-field-1m, 165 instructions each in the walk. The
/// cell's own words, never the cell, cross into the walk, so a frame
/// cell stays put.
fn lacking_member_chain(
    ctx: &mut LowerCtx<'_>,
    obj: Operand,
    k: Operand,
    skip: crate::ssa::BlockId,
) -> Operand {
    let load = |ctx: &mut LowerCtx<'_>, ty, off| {
        let v = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::Load(ty, obj, off), ty, None);
        Operand::Value(v)
    };
    let hdr = load(ctx, Type::I32, 4);
    let flags = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::ZExtI32ToI64(hdr), Type::I64, None);
    let flags = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::LShr, Operand::Value(flags), Operand::ConstI64(16)),
        Type::I64,
        None,
    );
    if !is_class_layout(ctx, obj) {
        let user_proto = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(
                BinOp::And,
                Operand::Value(flags),
                Operand::ConstI64(i64::from(torajs_rc::FLAG_OBJ_USER_PROTO)),
            ),
            Type::I64,
            None,
        );
        let own = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(user_proto), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let (ask, root) = (ctx.f.add_block(), ctx.f.add_block());
        ctx.f.set_term(
            ctx.cur_block,
            crate::ssa::Terminator::CondBr {
                cond: Operand::Value(own),
                then_blk: ask,
                else_blk: root,
            },
        );
        ctx.cur_block = root;
        object_proto_as_minted(ctx, ask, skip);
        ctx.cur_block = ask;
    }
    let tag = load(ctx, Type::I64, 8);
    let props = load(ctx, Type::Ptr, crate::ssa_lower::OBJ_PROPS_OFF);
    let args = vec![Operand::Value(flags), tag, props, k];
    let call = InstKind::Call(ctx.intrinsics.struct_expando.struct_lacks_in, args);
    let r = ctx.f.append_inst(ctx.cur_block, call, Type::I64, None);
    let cmp = InstKind::ICmp(IPred::Ne, Operand::Value(r), Operand::ConstI64(0));
    Operand::Value(ctx.f.append_inst(ctx.cur_block, cmp, Type::Bool, None))
}

/// Branch from `cur_block` to `skip` while %Object.prototype% holds only
/// what its mint installed, else to `walk`: it is not minted yet, or it
/// holds the mint's entry count and never lost a key (torajs-dynobj's
/// `KEY_DELETED` bit), so every key it holds is the mint's. Every member
/// the mint installs answers through the interned tables before this
/// point (`OBJECT_PROTO_MEMBERS`).
fn object_proto_as_minted(
    ctx: &mut LowerCtx<'_>,
    walk: crate::ssa::BlockId,
    skip: crate::ssa::BlockId,
) {
    use torajs_dynobj::delete::{DYNOBJ_HDR_FLAG_KEY_DELETED, OBJECT_PROTO_MINT_ENTRIES};
    use torajs_dynobj::layout::DYNOBJ_COUNT_OFF;
    use torajs_rc::builtin_proto::OBJECT_PROTO_TAG;
    let inst = |ctx: &mut LowerCtx<'_>, kind, ty| {
        Operand::Value(ctx.f.append_inst(ctx.cur_block, kind, ty, None))
    };
    let slots = inst(
        ctx,
        InstKind::GlobalRef(BUILTIN_PROTO_SLOTS_SYM.to_string()),
        Type::Ptr,
    );
    let slot_off = (OBJECT_PROTO_TAG * 8) as u64;
    let proto = inst(ctx, InstKind::Load(Type::Ptr, slots, slot_off), Type::Ptr);
    let bits = inst(ctx, InstKind::PtrToInt(proto), Type::I64);
    let minted = inst(
        ctx,
        InstKind::ICmp(IPred::Ne, bits, Operand::ConstI64(0)),
        Type::Bool,
    );
    let read = ctx.f.add_block();
    let term = |cond, then_blk, else_blk| crate::ssa::Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    };
    ctx.f.set_term(ctx.cur_block, term(minted, read, skip));
    ctx.cur_block = read;
    let hdr = inst(ctx, InstKind::Load(Type::I32, proto, 4), Type::I32);
    let hdr = inst(ctx, InstKind::ZExtI32ToI64(hdr), Type::I64);
    let lost = i64::from(DYNOBJ_HDR_FLAG_KEY_DELETED) << 16;
    let lost = inst(
        ctx,
        InstKind::BinOp(BinOp::And, hdr, Operand::ConstI64(lost)),
        Type::I64,
    );
    let count = inst(
        ctx,
        InstKind::Load(Type::I32, proto, DYNOBJ_COUNT_OFF as u64),
        Type::I32,
    );
    let count = inst(ctx, InstKind::ZExtI32ToI64(count), Type::I64);
    let grown = inst(
        ctx,
        InstKind::BinOp(
            BinOp::Xor,
            count,
            Operand::ConstI64(i64::from(OBJECT_PROTO_MINT_ENTRIES)),
        ),
        Type::I64,
    );
    let moved = inst(ctx, InstKind::BinOp(BinOp::Or, lost, grown), Type::I64);
    let moved = inst(
        ctx,
        InstKind::ICmp(IPred::Ne, moved, Operand::ConstI64(0)),
        Type::Bool,
    );
    ctx.f.set_term(ctx.cur_block, term(moved, walk, skip));
}

/// Whether the struct `obj` is an instance of a declared class, whose
/// prototype answers for members the cell lacks (`struct_proto_has`).
/// An object literal's layout registers no member table.
fn is_class_layout(ctx: &LowerCtx<'_>, obj: Operand) -> bool {
    let Type::Obj(sid) = ctx.operand_ty(&obj) else {
        return true;
    };
    let named = ctx
        .ast
        .class_parents
        .keys()
        .any(|c| matches!(ctx.aliases.get(c), Some(Type::Obj(s)) if *s == sid));
    // a generic class instantiation lives under its spelled key
    let generic = ctx.inst_memo.iter().any(|(key, s)| {
        *s == sid
            && key
                .split_once('<')
                .is_some_and(|(head, _)| ctx.ast.class_parents.contains_key(head))
    });
    named || generic
}
