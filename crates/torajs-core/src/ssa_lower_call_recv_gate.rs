//! Receiver-aware indirect call — 398-06 / 398-11 修法 ① substrate.
//!
//! A promoted fn-expr body's native entry carries a `__this: any`
//! param between the hidden argc and the user params
//! (`fnexpr_this_faces::promote_recv_any`), and every any-lane call
//! path honors `FLAG_CLOSURE_RECV_FIRST` at runtime. The TYPED
//! indirect lanes did not: a call site whose static answer for
//! "is this binding promoted?" is no (the name is not in
//! `fnexpr_recv_locals` — a field read, an alias, a slot the value
//! flowed through) passed argv unshifted, and a promoted callee then
//! read its receiver slot off the first real argument (measured:
//! `typeof this + x` answered `undefined16384`).
//!
//! The gate mirrors `ssa_lower_struct_exotic_gate`: one masked test
//! on the header word the closure cell carries anyway (flags bit 12
//! at bit 60 of the I64 header load — same cache line as the fn_addr
//! the call loads regardless). The plain arm is the native call
//! exactly as before; a never-promoted callee pays one not-taken
//! branch.
//!
//! The taken arm does NOT call the native entry under a widened
//! signature — a promoted body's native param widths are its own
//! (num_width narrows them against the body), while the call site's
//! `user_params` come from the SLOT's annotation, and the two split
//! (measured: caller `[F64, I64]` vs callee `(i64, i64)`, an ABI
//! register mismatch that read `x` off the wrong register file).
//! Instead it re-boxes the already-evaluated args and dispatches
//! through `__torajs_closure_call_with_this`, whose
//! `invoke_with_this` owns the shift/dispatch story exactly as the
//! any lane always has — uniform ABI, no width question. The Any
//! result coerces back to the native return type
//! (`coerce_any_result`, the boxed-variadic contract).
//!
//! Static call sites that already KNOW the callee is promoted (the
//! `fnexpr_recv_locals` name test) keep their single-path emit —
//! this gate is only for sites where the answer is a runtime fact.

use crate::ssa::{BlockId, IPred, InstKind, Operand, SigId, Terminator, Type};
use crate::ssa_lower::LowerCtx;

/// The universal heap header is `[refcount u32 | tag u16 | flags u16]`,
/// so flags bit N sits at bit 48+N of the I64 load.
const FLAGS_SHIFT: u32 = 48;

/// `Bool` — true when `cell`'s header says the closure's native entry
/// expects a receiver box ahead of its user params. `cell` must be a
/// live, non-NULL closure cell operand.
pub(crate) fn emit_recv_first_test(ctx: &mut LowerCtx<'_>, cell: &Operand) -> Operand {
    let mask = (torajs_rc::FLAG_CLOSURE_RECV_FIRST as i64) << FLAGS_SHIFT;
    let hdr = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, cell.clone(), 0),
        Type::I64,
        None,
    );
    let bits = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(
            crate::ssa::BinOp::And,
            Operand::Value(hdr),
            Operand::ConstI64(mask),
        ),
        Type::I64,
        None,
    );
    let recv = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Ne, Operand::Value(bits), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    Operand::Value(recv)
}

/// What the recv arm seeds into the promoted callee's `__this` slot.
pub(crate) enum RecvSeed {
    /// Already-boxed Any — the explicit `.call`/`.apply` thisArg,
    /// evaluated unconditionally by the caller (§20.2.3.3 step order).
    Boxed(Operand),
    /// Box this operand inside the recv arm — the §13.3.6.2 method
    /// receiver; boxing only on the taken arm keeps the plain path
    /// free of the encode.
    BoxOf(Operand),
    /// Strict-mode `undefined` (§10.2.1.2), minted inside the arm.
    Undef,
}

/// Emit the gated indirect call. `argv` is the plain-ABI argument
/// list, `[env, argc, user args…, pads…]` — the plain arm calls the
/// native entry with it verbatim; the recv arm re-boxes the user
/// slots and dispatches through the runtime kernel (module doc).
///
/// Returns `None` for a `Void` return type. The caller runs its
/// throw-check and owned-temp releases after this returns — both arms
/// have joined by then.
pub(crate) fn emit_indirect_call_recv_gated(
    ctx: &mut LowerCtx<'_>,
    cell: &Operand,
    fn_ptr: Operand,
    plain_sig: SigId,
    argv: Vec<Operand>,
    recv_seed: RecvSeed,
    ret_ty: Type,
) -> Option<Operand> {
    let is_recv = emit_recv_first_test(ctx, cell);
    let recv_blk = ctx.f.add_block();
    let plain_blk = ctx.f.add_block();
    let done_blk = ctx.f.add_block();
    let out_slot = if ret_ty == Type::Void {
        None
    } else {
        Some(ctx.alloca(ret_ty.clone(), None))
    };
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: is_recv,
            then_blk: recv_blk,
            else_blk: plain_blk,
        },
    );

    // Promoted callee — box the receiver and the already-evaluated
    // user args, dispatch through the uniform-ABI kernel.
    ctx.cur_block = recv_blk;
    let boxed_seed = match recv_seed {
        RecvSeed::Boxed(b) => b,
        RecvSeed::BoxOf(v) => ctx.box_to_any(v),
        RecvSeed::Undef => {
            let undef = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            Operand::Value(undef)
        }
    };
    let user_ops: Vec<Operand> = argv[2..].to_vec();
    let n = user_ops.len();
    // Entry-block stack space (pack_any_argv shape): allocating
    // unconditionally costs bytes, not work — the plain arm never
    // stores into it.
    let buf = ctx.f.append_inst(
        BlockId(0),
        InstKind::AllocaBytes((n.max(1) * 8) as u64),
        Type::Ptr,
        Some("__recv_gate_argv"),
    );
    for (i, op) in user_ops.into_iter().enumerate() {
        // box_to_any is RC-NEUTRAL (a pure encode); ownership of the
        // underlying temps stays with the caller's release ledger.
        let b = ctx.box_to_any(op);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(b, Operand::Value(buf), (i * 8) as u64),
        );
    }
    // argc rides argv[1] — the REAL user argument count (pads stay
    // behind it; the kernel's copy pre-fills undefined, so uncopied
    // pad slots decode identically).
    let result = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.closure_call_with_this,
            vec![
                argv[0].clone(),
                boxed_seed,
                Operand::Value(buf),
                argv[1].clone(),
            ],
        ),
        Type::Any,
        None,
    );
    let conv = crate::ssa_lower_call_closure_local::coerce_any_result(ctx, result, ret_ty.clone());
    if let Some(slot) = out_slot {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(conv, Operand::Value(slot), 0),
        );
    }
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));

    // Ordinary callee — the native call exactly as before this gate
    // existed.
    ctx.cur_block = plain_blk;
    if ret_ty == Type::Void {
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::CallIndirect(plain_sig, fn_ptr, argv),
        );
    } else {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::CallIndirect(plain_sig, fn_ptr, argv),
            ret_ty.clone(),
            None,
        );
        if let Some(slot) = out_slot {
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
            );
        }
    }
    ctx.f.set_term(ctx.cur_block, Terminator::Br(done_blk));

    ctx.cur_block = done_blk;
    let slot = out_slot?;
    let v = ctx.f.append_inst(
        done_blk,
        InstKind::Load(ret_ty.clone(), Operand::Value(slot), 0),
        ret_ty,
        None,
    );
    Some(Operand::Value(v))
}
