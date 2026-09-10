//! The per-target adapter synthesis of
//! [`super`]/`ssa_lower_boxed_entry` — split out when RFC 20260808
//! knife 2's recv-slot window shift pushed the parent past the
//! 500-line cap (same sibling shape as `unbox`).

use std::collections::HashMap;

use crate::ssa::{self, FuncId, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::intern_fn_sig;

use super::build_rest::emit_rest_slot;
use super::unbox::{
    BoxedCoerceIntrinsics, drop_obj_temps, drop_settle_temps, drop_str_temps, unbox_args,
};
use super::{BoxedEntryIntrinsics, DfltLit};

/// One adapter — see module doc for the ABI.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_boxed_entry(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    intr: &BoxedEntryIntrinsics,
    anyv_to_str: FuncId,
    coerce: BoxedCoerceIntrinsics<'_>,
    body_name: &str,
    body_fid: FuncId,
    user_tys: &[Type],
    ret_ty: Type,
    has_argv: bool,
    recv_slot: bool,
    recv_leads: bool,
    lead_undef: bool,
    feeds_env: bool,
    hidden_argc: bool,
    dflt_lits: &[Option<DfltLit>],
    rest: bool,
    rest_kind: Option<i64>,
    arr_any_to_typed: FuncId,
    ret_arr_kind: Option<i64>,
) -> (FuncId, ssa::SigId) {
    let name = format!("__boxed_{body_name}");
    let fid = FuncId(module.funcs.len() as u32);
    let own_sig = intern_fn_sig(fn_sigs, vec![Type::Ptr, Type::Ptr, Type::I64], Type::Any);
    fn_table.insert(name.clone(), fid);
    fn_sig_ids.insert(fid, own_sig);
    let mut f = ssa::Function::new(&name, Type::Any);
    let env = f.add_param(Type::Ptr, "__env");
    let argv = f.add_param(Type::Ptr, "argv");
    let argc = f.add_param(Type::I64, "argc");
    let mut entry = f.add_block();

    let mut args: Vec<Operand> = Vec::with_capacity(user_tys.len() + 2);
    // Str-param temps (owned via ToString — a ShortStr argument
    // materializes) released after the body call.
    let mut str_temps: Vec<ssa::ValueId> = Vec::new();
    // S2.36 — coerced struct-param boxes (the kernel answers OWNED:
    // a stake on pass-throughs, a fresh cell for materialized
    // dynobjs) released after the body call via anyv_rc_dec (no-op
    // on immediates).
    let mut obj_temps: Vec<ssa::ValueId> = Vec::new();
    // 546-02 Class B leg — (box, raw) pairs whose borrow-shaped
    // unbox may have materialized a ShortStr; settled after the
    // body call.
    let mut settle_temps: Vec<(ssa::ValueId, ssa::ValueId)> = Vec::new();
    // A static-method body (`__sm_`) has no env slot — the adapter
    // drops its env argument (knife B cut 2).
    if feeds_env {
        args.push(Operand::Value(env));
    }
    // A relay entering a one-slot this-first target directly: the
    // body's `__this` is the plain-call `undefined` (tag 5) the
    // relay's forwarding call would have seeded.
    if lead_undef {
        args.push(Operand::Value(f.append_inst(
            entry,
            InstKind::Call(
                intr.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        )));
    }
    let shifted = recv_slot.then(|| shifted_argv(&mut f, entry, argv));
    // A twin on the this-first argv lane: its `__this` is user param
    // 0 and sits BEFORE the hidden slots in the sig, so the receiver
    // in argv[0] is unboxed here and the rest of the user params
    // come off the shifted window below.
    let lead = usize::from(recv_leads);
    if recv_leads {
        unbox_args(
            &mut f,
            module,
            entry,
            argv,
            &user_tys[..1],
            intr,
            anyv_to_str,
            &coerce,
            &mut args,
            &mut str_temps,
            &mut obj_temps,
            &mut settle_temps,
            &dflt_lits[..1.min(dflt_lits.len())],
        );
    }
    // User-argument count the body-side slots receive: a recv-slot
    // body (RFC 20260808 knife 2) gets the window SHIFTED past
    // argv[0] — the receiver riding there feeds `__this`, not
    // `arguments` (§10.4.4) — so the count drops by one.
    let user_argc = if recv_slot {
        Operand::Value(f.append_inst(
            entry,
            InstKind::BinOp(ssa::BinOp::Sub, Operand::Value(argc), Operand::ConstI64(1)),
            Type::I64,
            None,
        ))
    } else {
        Operand::Value(argc)
    };
    // RFC 20260810-indirect-argc-abi S1 — the body's sig carries the
    // hidden I64 `__torajs_argc` at position 1 (`__env`-first, and
    // since S1-T1 the this-first method_argv family too); forward
    // the adapter's real argc there (any-lane calls thereby get the
    // true-argc channel for free). Sits BEFORE the injected
    // real_argc/argv slots — same order as the sig.
    if hidden_argc {
        args.push(user_argc);
    }
    if has_argv {
        args.push(Operand::Value(shifted.unwrap_or(argv)));
    }
    // Rest split — the fixed prefix unboxes positionally; the rest
    // slot itself is fed below from the trailing argv window.
    let n_fixed = if rest {
        user_tys.len() - 1
    } else {
        user_tys.len()
    };
    let base = if recv_leads {
        shifted.expect("recv_leads implies recv_slot, which minted the shifted window")
    } else {
        argv
    };
    unbox_args(
        &mut f,
        module,
        entry,
        base,
        &user_tys[lead..n_fixed],
        intr,
        anyv_to_str,
        &coerce,
        &mut args,
        &mut str_temps,
        &mut obj_temps,
        &mut settle_temps,
        &dflt_lits[lead.min(dflt_lits.len())..n_fixed.min(dflt_lits.len())],
    );
    if rest {
        let (arr, new_entry) = emit_rest_slot(
            &mut f,
            entry,
            intr,
            &coerce,
            arr_any_to_typed,
            argv,
            argc,
            n_fixed,
            user_tys[n_fixed],
            rest_kind,
            &mut obj_temps,
        );
        entry = new_entry;
        args.push(Operand::Value(arr));
    }

    let boxed = if ret_ty == Type::Void {
        f.append_void(entry, InstKind::Call(body_fid, args));
        drop_str_temps(&mut f, entry, intr, &str_temps);
        drop_obj_temps(&mut f, entry, coerce.anyv_rc_dec, &obj_temps);
        drop_settle_temps(&mut f, entry, intr.any_unbox_settle, &settle_temps);
        // undefined — tag 5.
        f.append_inst(
            entry,
            InstKind::Call(
                intr.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        )
    } else {
        let r = f.append_inst(entry, InstKind::Call(body_fid, args), ret_ty, None);
        drop_str_temps(&mut f, entry, intr, &str_temps);
        drop_obj_temps(&mut f, entry, coerce.anyv_rc_dec, &obj_temps);
        drop_settle_temps(&mut f, entry, intr.any_unbox_settle, &settle_temps);
        box_return(&mut f, entry, r, ret_ty, intr, ret_arr_kind)
    };
    f.set_term(entry, Terminator::Ret(Some(Operand::Value(boxed))));
    module.funcs.push(f);
    (fid, own_sig)
}

/// Box the body's typed return into the adapter's `AnyValue` result.
///
/// The heap arm is where a TYPED ARRAY has to declare its element
/// kind: the cell crosses into the any world here, and without the
/// stamp every slot an any-lane reader touches decodes as a NaN box
/// (RFC 20260823-proxy-substrate 刀 4b).
fn box_return(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    r: ssa::ValueId,
    ret_ty: Type,
    intr: &BoxedEntryIntrinsics,
    ret_arr_kind: Option<i64>,
) -> ssa::ValueId {
    match ret_ty {
        Type::Any => r,
        Type::F64 => {
            let bits = f.append_inst(
                entry,
                InstKind::BitCastF64ToI64(Operand::Value(r)),
                Type::I64,
                None,
            );
            f.append_inst(
                entry,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(3), Operand::Value(bits)],
                ),
                Type::Any,
                None,
            )
        }
        Type::Bool => {
            let z = f.append_inst(
                entry,
                InstKind::ZExtBoolToI64(Operand::Value(r)),
                Type::I64,
                None,
            );
            f.append_inst(
                entry,
                InstKind::Call(intr.any_box, vec![Operand::ConstI64(1), Operand::Value(z)]),
                Type::Any,
                None,
            )
        }
        Type::I64 | Type::I32 => f.append_inst(
            entry,
            InstKind::Call(intr.any_box, vec![Operand::ConstI64(2), Operand::Value(r)]),
            Type::Any,
            None,
        ),
        // Heap-typed returns arrive +1 owned; the box transfers
        // that reference to the returned AnyValue.
        _ => {
            // A TYPED array crossing into the any world carries
            // its element-kind chain, or every slot the any lane
            // reads decodes as a NaN box (RFC 20260823 刀 4b).
            if let Some(chain) = ret_arr_kind {
                f.append_void(
                    entry,
                    InstKind::Call(
                        intr.arr_mark_kind,
                        vec![Operand::Value(r), Operand::ConstI64(chain)],
                    ),
                );
            }
            f.append_inst(
                entry,
                InstKind::Call(intr.any_box, vec![Operand::ConstI64(4), Operand::Value(r)]),
                Type::Any,
                None,
            )
        }
    }
}

/// Collect argv[n_fixed..argc] into a fresh Arr<Any> (§10.2.1.3 rest
/// binding — every runtime argument past the fixed prefix, in order,
/// `[]` when none). The count clamps at 0: a call may pass fewer than
/// the fixed arity, and arr_alloc_any's u64 cap would treat a
/// negative as huge; Select is egraph-elaborator-only territory, so
/// the clamp is the branch-free sign mask `n & ~(n >> 63)`.
/// Ownership: the caller boxes the returned arr (the pair box takes
/// the alloc's +1) and releases it after the body call — the
/// caller-owns convention direct call sites use for the packed
/// literal.
/// The recv-slot argv window shift (RFC 20260808 knife 2): the
/// receiver rides argv[0], so the body's argv face starts one slot
/// past it.
fn shifted_argv(f: &mut ssa::Function, entry: ssa::BlockId, argv: ssa::ValueId) -> ssa::ValueId {
    let pi = f.append_inst(
        entry,
        InstKind::PtrToInt(Operand::Value(argv)),
        Type::I64,
        None,
    );
    let off = f.append_inst(
        entry,
        InstKind::BinOp(ssa::BinOp::Add, Operand::Value(pi), Operand::ConstI64(8)),
        Type::I64,
        None,
    );
    f.append_inst(
        entry,
        InstKind::IntToPtr(Operand::Value(off)),
        Type::Ptr,
        None,
    )
}
