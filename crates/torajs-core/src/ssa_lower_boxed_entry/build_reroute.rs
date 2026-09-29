//! The typed mono adapter's hand-off to its receiver-polymorphic
//! twin when a boolean / integer slot arrives undefined.
//!
//! Those slots have no in-band undefined: `any_to_bool` would make
//! the missing argument `false` and `any_to_number` + `FpToSi` would
//! make it `0`, and `typeof b` / `b === undefined` in the body would
//! then answer for a value nobody passed. The twin's every parameter
//! is `any`, so it hosts the under-supplied call exactly; with every
//! such argument present the typed body runs as before. The test is
//! one tag compare per lossy slot, ahead of every unbox.

use crate::ssa::{self, FuncId, IPred, InstKind, Operand, Terminator, Type};

use super::BoxedEntryIntrinsics;

/// The twin adapter a mono adapter hands an under-supplied call to.
#[derive(Clone, Copy)]
pub(super) struct TwinReroute {
    pub(super) adapter: FuncId,
    pub(super) sig: ssa::SigId,
    /// `__torajs_anyv_invoke_recv_first(env, entry, recv, argv, argc)`.
    pub(super) invoke: FuncId,
}

/// A slot a missing argument cannot cross without loss: no literal
/// default to substitute, and a type with no undefined of its own.
pub(super) fn lossy_slot(ty: Type, dflt: Option<&super::DfltLit>) -> bool {
    dflt.is_none() && matches!(ty, Type::Bool | Type::I64 | Type::I32)
}

/// Test every lossy slot of the window at `base`; on the first
/// undefined one, call the twin adapter and return its answer. Returns
/// the block the adapter continues in.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_twin_reroute(
    f: &mut ssa::Function,
    mut entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    rr: TwinReroute,
    env: ssa::ValueId,
    argv: ssa::ValueId,
    argc: ssa::ValueId,
    base: ssa::ValueId,
    user_tys: &[Type],
    dflt_lits: &[Option<super::DfltLit>],
    recv_slot: bool,
    feeds_env: bool,
) -> ssa::BlockId {
    let lossy: Vec<usize> = (0..user_tys.len())
        .filter(|&i| lossy_slot(user_tys[i], dflt_lits.get(i).and_then(|d| d.as_ref())))
        .collect();
    if lossy.is_empty() {
        return entry;
    }
    let reroute = f.add_block();
    for i in lossy {
        let av = f.append_inst(
            entry,
            InstKind::Load(Type::Any, Operand::Value(base), (i as u64) * 8),
            Type::Any,
            None,
        );
        let tag = f.append_inst(
            entry,
            InstKind::Call(intr.any_unbox_tag, vec![Operand::Value(av)]),
            Type::I64,
            None,
        );
        let is_undef = f.append_inst(
            entry,
            InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(5)),
            Type::Bool,
            None,
        );
        let next = f.add_block();
        f.set_term(
            entry,
            Terminator::CondBr {
                cond: Operand::Value(is_undef),
                then_blk: reroute,
                else_blk: next,
            },
        );
        entry = next;
    }
    // A recv-slot mono already carries its receiver in argv[0], the
    // twin adapter's own shape; otherwise the receiver is the env
    // pointer of an instance body (boxed as a heap cell, null on the
    // this-free path) or nothing at all for a static body.
    let answer = if recv_slot {
        f.append_inst(
            reroute,
            InstKind::Call(
                rr.adapter,
                vec![
                    Operand::Value(env),
                    Operand::Value(argv),
                    Operand::Value(argc),
                ],
            ),
            Type::Any,
            None,
        )
    } else {
        let recv = if feeds_env {
            let bits = f.append_inst(
                reroute,
                InstKind::PtrToInt(Operand::Value(env)),
                Type::I64,
                None,
            );
            f.append_inst(
                reroute,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(4), Operand::Value(bits)],
                ),
                Type::Any,
                None,
            )
        } else {
            f.append_inst(
                reroute,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            )
        };
        let addr = f.append_inst(
            reroute,
            InstKind::BoxedEntryAddr(rr.adapter),
            Type::FnSig(rr.sig),
            None,
        );
        let addr_bits = f.append_inst(
            reroute,
            InstKind::PtrToInt(Operand::Value(addr)),
            Type::I64,
            None,
        );
        f.append_inst(
            reroute,
            InstKind::Call(
                rr.invoke,
                vec![
                    Operand::Value(env),
                    Operand::Value(addr_bits),
                    Operand::Value(recv),
                    Operand::Value(argv),
                    Operand::Value(argc),
                ],
            ),
            Type::Any,
            None,
        )
    };
    f.set_term(reroute, Terminator::Ret(Some(Operand::Value(answer))));
    entry
}
