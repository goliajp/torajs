//! The typed mono adapter's hand-off to its all-`any` twin when a
//! slot's argument cannot cross the typed face as it is.
//!
//! A boolean / integer slot has no in-band undefined: `any_to_bool`
//! would make the missing argument `false` and `any_to_number` +
//! `FpToSi` would make it `0`, and `typeof b` / `b === undefined` in
//! the body would then answer for a value nobody passed. A struct
//! slot reads its argument through the layout, which only a cell of
//! that layout (or one extending it) has: an array, a Proxy, a
//! getter-bearing literal, a `3` handed over through `any` means a
//! generic [[Get]] for every `p.x` (§13.3.2.1), not an offset read.
//! The twin's every parameter is `any`, so it hosts such a call
//! exactly; with every argument fitting, the typed body runs as
//! before. The test is one tag compare per lossy scalar slot and one
//! `__torajs_anyv_struct_fits` call per struct slot, ahead of every
//! unbox.

use crate::ssa::{self, FuncId, IPred, InstKind, Operand, Terminator, Type};

use super::BoxedEntryIntrinsics;
use super::unbox::BoxedCoerceIntrinsics;

/// The twin adapter a mono adapter hands a non-fitting call to.
#[derive(Clone, Copy)]
pub(super) struct TwinReroute {
    pub(super) adapter: FuncId,
    pub(super) sig: ssa::SigId,
    /// `__torajs_anyv_invoke_recv_first(env, entry, recv, argv, argc)`.
    pub(super) invoke: FuncId,
    /// The twin adapter wears the mono adapter's own `(env, argv,
    /// argc)` face (the all-`any` relay shell behind a value escape,
    /// `check_monomorph_any_widen::struct_escape`): it is called
    /// directly with the same env. A receiver-polymorphic twin takes
    /// its receiver in argv[0] and goes through `invoke`.
    pub(super) same_face: bool,
}

/// A slot an argument cannot cross without loss: no literal default
/// to substitute, and a type with no undefined of its own, or a
/// struct layout the argument may not have.
pub(super) fn lossy_slot(ty: Type, dflt: Option<&super::DfltLit>) -> bool {
    dflt.is_none() && matches!(ty, Type::Bool | Type::I64 | Type::I32 | Type::Obj(_))
}

/// Test every lossy slot of the window at `base`; on the first
/// undefined scalar or non-fitting struct argument, call the twin
/// adapter and return its answer. Returns the block the adapter
/// continues in.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_twin_reroute(
    f: &mut ssa::Function,
    mut entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    coerce: &BoxedCoerceIntrinsics<'_>,
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
        let miss = match user_tys[i] {
            Type::Obj(_) => {
                let tag = coerce.obj_tags[i].expect("obj_tags aligns with user_tys");
                let fits = f.append_inst(
                    entry,
                    InstKind::Call(
                        coerce.struct_fits,
                        vec![Operand::Value(av), Operand::ConstI64(tag as i64)],
                    ),
                    Type::I64,
                    None,
                );
                f.append_inst(
                    entry,
                    InstKind::ICmp(IPred::Eq, Operand::Value(fits), Operand::ConstI64(0)),
                    Type::Bool,
                    None,
                )
            }
            _ => {
                let tag = f.append_inst(
                    entry,
                    InstKind::Call(intr.any_unbox_tag, vec![Operand::Value(av)]),
                    Type::I64,
                    None,
                );
                f.append_inst(
                    entry,
                    InstKind::ICmp(IPred::Eq, Operand::Value(tag), Operand::ConstI64(5)),
                    Type::Bool,
                    None,
                )
            }
        };
        let next = f.add_block();
        f.set_term(
            entry,
            Terminator::CondBr {
                cond: Operand::Value(miss),
                then_blk: reroute,
                else_blk: next,
            },
        );
        entry = next;
    }
    // A recv-slot mono already carries its receiver in argv[0], the
    // twin adapter's own shape, and a same-face twin takes the mono's
    // env as it is; otherwise the receiver is the env pointer of an
    // instance body (boxed as a heap cell, null on the this-free
    // path) or nothing at all for a static body.
    let answer = if recv_slot || rr.same_face {
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
