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
    has_real_argc: bool,
    has_argv: bool,
    recv_slot: bool,
    feeds_env: bool,
    first_is_env: bool,
    dflt_lits: &[Option<DfltLit>],
    rest: bool,
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
    let entry = f.add_block();

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
    // RFC 20260810-indirect-argc-abi S1 — an `__env`-first body's
    // sig carries the hidden I64 `__torajs_argc` at position 1;
    // forward the adapter's real argc there (any-lane calls thereby
    // get the true-argc channel for free).
    if first_is_env {
        args.push(user_argc);
    }
    // Feed the real argc into the injected `__torajs_real_argc`
    // slot (head-less / this-first faces only since S3.4 —
    // env-first bodies read the hidden slot fed above).
    if has_real_argc {
        args.push(user_argc);
    }
    if has_argv {
        if recv_slot {
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
            let pv = f.append_inst(
                entry,
                InstKind::IntToPtr(Operand::Value(off)),
                Type::Ptr,
                None,
            );
            args.push(Operand::Value(pv));
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
        let arr = emit_rest_collect(&mut f, entry, intr, argv, argc, n_fixed, user_tys[n_fixed]);
        args.push(Operand::Value(arr));
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
