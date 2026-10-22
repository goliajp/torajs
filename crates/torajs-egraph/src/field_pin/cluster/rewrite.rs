//! The rewrites that give a cluster its registers: the entry kept
//! under a member's name, the word loads read from the registers, and
//! the calls inside the cluster sent to the bodies.

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BlockId, FuncId, Function, IPred, InstKind, Operand, PinWord, THROW_ACTIVE_SYM, Terminator,
    Type, ValueId, rewrite_value_operands,
};

use super::{aliases, words};

/// The entry kept under `member`'s name: call `body` with the same
/// arguments, probe the throw flag as a lowered call does, and return
/// what `body` did.
pub(super) fn entry(member: &Function, body: FuncId) -> Function {
    let mut f = Function::new(member.name.clone(), member.ret);
    let params: Vec<Operand> = (member.params.iter())
        .map(|p| {
            let name = member.values[p.0 as usize].name.clone();
            Operand::Value(f.add_param(member.value_type(*p), name.as_deref().unwrap_or("p")))
        })
        .collect();
    let b0 = f.add_block();
    let ok = f.add_block();
    let thrown = f.add_block();
    let call = InstKind::Call(body, params);
    let result = match f.ret {
        Type::Void => {
            f.append_void(b0, call);
            None
        }
        ty => Some(Operand::Value(f.append_inst(b0, call, ty, None))),
    };
    let flag = f.append_inst(
        b0,
        InstKind::GlobalRef(THROW_ACTIVE_SYM.into()),
        Type::Ptr,
        None,
    );
    let word = f.append_inst(
        b0,
        InstKind::Load(Type::I64, Operand::Value(flag), 0),
        Type::I64,
        None,
    );
    let cond = InstKind::ICmp(IPred::Ne, Operand::Value(word), Operand::ConstI64(0));
    let cond = f.append_inst(b0, cond, Type::Bool, None);
    f.set_term(
        b0,
        Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: thrown,
            else_blk: ok,
        },
    );
    f.set_term(ok, Terminator::Ret(result));
    f.append_void(thrown, InstKind::ThrowSignal);
    let sentinel = match f.ret {
        Type::Void => None,
        Type::F64 => Some(Operand::ConstF64(0.0)),
        Type::I32 => Some(Operand::ConstI32(0)),
        Type::Bool => Some(Operand::ConstBool(false)),
        _ => Some(Operand::ConstI64(0)),
    };
    f.set_term(thrown, Terminator::Ret(sentinel));
    f
}

/// Replace each load of the receiver's field's length or data pointer
/// with the `Pinned` value at the head of the entry block; drop the
/// field loads nothing reads any longer. The number of loads replaced.
pub(super) fn pin_words(f: &mut Function, field_off: u64) -> u32 {
    let words = words(f, field_off);
    if words.is_empty() {
        return 0;
    }
    let len = f.insert_inst(BlockId(0), 0, InstKind::Pinned(PinWord::Len), Type::I64);
    let data = f.insert_inst(BlockId(0), 1, InstKind::Pinned(PinWord::Data), Type::Ptr);
    let to = |v: ValueId| match words.get(&v) {
        Some(PinWord::Len) => len,
        Some(PinWord::Data) => data,
        None => v,
    };
    for block in &mut f.blocks {
        block
            .insts
            .retain(|i| !i.result.is_some_and(|r| words.contains_key(&r)));
        for inst in &mut block.insts {
            rewrite_value_operands(&mut inst.kind, to);
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = &mut block.term
        {
            *v = to(*v);
        }
    }
    drop_unread_field_loads(f, field_off);
    words.len() as u32
}

/// Remove the loads of the receiver's field whose result nothing reads.
fn drop_unread_field_loads(f: &mut Function, field_off: u64) {
    let mut read: HashSet<ValueId> = HashSet::default();
    for block in &f.blocks {
        for inst in &block.insts {
            torajs_core::ssa::visit_value_operands(&inst.kind, |v| {
                read.insert(v);
            });
        }
        if let Terminator::CondBr {
            cond: Operand::Value(v),
            ..
        }
        | Terminator::Ret(Some(Operand::Value(v))) = &block.term
        {
            read.insert(*v);
        }
    }
    let this = f.params[0];
    let alias = aliases(f);
    for block in &mut f.blocks {
        block.insts.retain(|i| match (&i.kind, i.result) {
            (InstKind::Load(_, Operand::Value(b), off), Some(r)) => {
                let base = *alias.get(b).unwrap_or(b);
                !(base == this && *off == field_off && !read.contains(&r))
            }
            _ => true,
        });
    }
}

/// Send each call to a member that has an entry, with the caller's own
/// receiver, to that member's body.
pub(super) fn retarget(f: &mut Function, set: &HashSet<FuncId>, body_of: &HashMap<FuncId, FuncId>) {
    let alias = aliases(f);
    let this = f.params[0];
    for inst in f.blocks.iter_mut().flat_map(|b| &mut b.insts) {
        if let InstKind::Call(t, args) = &mut inst.kind
            && set.contains(t)
            && let Some(&body) = body_of.get(t)
            && matches!(args.first(), Some(Operand::Value(a)) if *alias.get(a).unwrap_or(a) == this)
        {
            *t = body;
        }
    }
}
