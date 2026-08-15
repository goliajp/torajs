//! The per-target adapter synthesis of
//! [`super`]/`ssa_lower_boxed_entry` — split out when RFC 20260808
//! knife 2's recv-slot window shift pushed the parent past the
//! 500-line cap (same sibling shape as `unbox`).

use std::collections::HashMap;

use crate::ssa::{self, FuncId, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower::intern_fn_sig;

use super::unbox::{BoxedCoerceIntrinsics, drop_obj_temps, drop_str_temps, unbox_args};
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
    feeds_env: bool,
    hidden_argc: bool,
    dflt_lits: &[Option<DfltLit>],
    rest: bool,
    rest_kind: Option<i64>,
    arr_any_to_typed: FuncId,
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
    // A static-method body (`__sm_`) has no env slot — the adapter
    // drops its env argument (knife B cut 2).
    if feeds_env {
        args.push(Operand::Value(env));
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
        if recv_slot {
            args.push(Operand::Value(shifted_argv(&mut f, entry, argv)));
        } else {
            args.push(Operand::Value(argv));
        }
    }
    // Rest split — the fixed prefix unboxes positionally; the rest
    // slot itself is fed below from the trailing argv window.
    let n_fixed = if rest {
        user_tys.len() - 1
    } else {
        user_tys.len()
    };
    unbox_args(
        &mut f,
        module,
        entry,
        argv,
        &user_tys[..n_fixed],
        intr,
        anyv_to_str,
        &coerce,
        &mut args,
        &mut str_temps,
        &mut obj_temps,
        &dflt_lits[..n_fixed.min(dflt_lits.len())],
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
            _ => f.append_inst(
                entry,
                InstKind::Call(intr.any_box, vec![Operand::ConstI64(4), Operand::Value(r)]),
                Type::Any,
                None,
            ),
        }
    };
    f.set_term(entry, Terminator::Ret(Some(Operand::Value(boxed))));
    module.funcs.push(f);
    (fid, own_sig)
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

/// The rest-slot half of the adapter (刀 2 extraction, function-size
/// line): collect argv[fixed..argc] into an Arr<Any>, convert to the
/// typed repr when the rest is typed (NULL-gating the catchable
/// mismatch TypeError into an early undefined return), and record
/// the value's box as a post-call temp. Answers the slot operand and
/// the (possibly new) current block.
#[allow(clippy::too_many_arguments)]
fn emit_rest_slot(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    coerce: &BoxedCoerceIntrinsics<'_>,
    arr_any_to_typed: FuncId,
    argv: ssa::ValueId,
    argc: ssa::ValueId,
    n_fixed: usize,
    arr_ty: Type,
    rest_kind: Option<i64>,
    obj_temps: &mut Vec<ssa::ValueId>,
) -> (ssa::ValueId, ssa::BlockId) {
    let mut entry = entry;
    let any_arr = emit_rest_collect(f, entry, intr, argv, argc, n_fixed, arr_ty);
    // A TYPED rest converts the Arr<Any> collection through the
    // assign-boundary kernel (per-slot coerce + kind stamp). A
    // mismatched element arms the catchable TypeError and answers
    // NULL; the gate below short-circuits before the body could
    // read the NULL block, and the pending throw surfaces at the
    // caller's check point.
    let arr = match rest_kind {
        Some(kind) => {
            let typed = f.append_inst(
                entry,
                InstKind::Call(
                    arr_any_to_typed,
                    vec![Operand::Value(any_arr), Operand::ConstI64(kind)],
                ),
                arr_ty,
                None,
            );
            // Release the Any collection now that the typed copy
            // owns the elements' new stakes.
            let boxed_src = f.append_inst(
                entry,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(4), Operand::Value(any_arr)],
                ),
                Type::Any,
                None,
            );
            f.append_void(
                entry,
                InstKind::Call(coerce.anyv_rc_dec, vec![Operand::Value(boxed_src)]),
            );
            let pi = f.append_inst(
                entry,
                InstKind::PtrToInt(Operand::Value(typed)),
                Type::I64,
                None,
            );
            let isnull = f.append_inst(
                entry,
                InstKind::ICmp(ssa::IPred::Eq, Operand::Value(pi), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let throw_blk = f.add_block();
            let cont_blk = f.add_block();
            f.set_term(
                entry,
                Terminator::CondBr {
                    cond: Operand::Value(isnull),
                    then_blk: throw_blk,
                    else_blk: cont_blk,
                },
            );
            let undef = f.append_inst(
                throw_blk,
                InstKind::Call(
                    intr.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            f.set_term(throw_blk, Terminator::Ret(Some(Operand::Value(undef))));
            entry = cont_blk;
            typed
        }
        None => any_arr,
    };
    let boxed_arr = f.append_inst(
        entry,
        InstKind::Call(
            intr.any_box,
            vec![Operand::ConstI64(4), Operand::Value(arr)],
        ),
        Type::Any,
        None,
    );
    obj_temps.push(boxed_arr);
    (arr, entry)
}

fn emit_rest_collect(
    f: &mut ssa::Function,
    entry: ssa::BlockId,
    intr: &BoxedEntryIntrinsics,
    argv: ssa::ValueId,
    argc: ssa::ValueId,
    n_fixed: usize,
    arr_ty: Type,
) -> ssa::ValueId {
    let n_rest = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::Sub,
            Operand::Value(argc),
            Operand::ConstI64(n_fixed as i64),
        ),
        Type::I64,
        None,
    );
    let sign = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::AShr,
            Operand::Value(n_rest),
            Operand::ConstI64(63),
        ),
        Type::I64,
        None,
    );
    let not_sign = f.append_inst(
        entry,
        InstKind::BinOp(ssa::BinOp::Xor, Operand::Value(sign), Operand::ConstI64(-1)),
        Type::I64,
        None,
    );
    let count = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::And,
            Operand::Value(n_rest),
            Operand::Value(not_sign),
        ),
        Type::I64,
        None,
    );
    let arr = f.append_inst(
        entry,
        InstKind::Call(intr.arr_alloc_any, vec![Operand::Value(count)]),
        arr_ty,
        None,
    );
    let pi = f.append_inst(
        entry,
        InstKind::PtrToInt(Operand::Value(argv)),
        Type::I64,
        None,
    );
    let off = f.append_inst(
        entry,
        InstKind::BinOp(
            ssa::BinOp::Add,
            Operand::Value(pi),
            Operand::ConstI64(8 * n_fixed as i64),
        ),
        Type::I64,
        None,
    );
    let pv = f.append_inst(
        entry,
        InstKind::IntToPtr(Operand::Value(off)),
        Type::Ptr,
        None,
    );
    f.append_void(
        entry,
        InstKind::Call(
            intr.arr_any_push,
            vec![
                Operand::Value(arr),
                Operand::Value(pv),
                Operand::Value(count),
                Operand::ConstPtrNull,
            ],
        ),
    );
    arr
}
