//! How ONE callback invocation is made, for the M6.2
//! `xs.{map,filter,reduce,reduceRight,forEach}` lowering — the arg
//! list, the argv-face downgrade, and the devirt/indirect dispatch
//! with its ReturnIfAbrupt check. Moved verbatim out of the parent
//! (file-size split, rotation 534; the parent had drifted to 473 and
//! the fast-push state was about to land in it).
//!
//! The seam is the parent's own: `ssa_lower_call_arr_ho_loop` shapes
//! the LOOP (cursor, header, reserve, step), this shapes the CALL.
//! Sibling to [`super::methods`], which is the third question — what
//! each method does with the answer.

use crate::ssa::{FuncId, InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

/// RC-1 — a boxed `undefined` AnyValue (`any_box(ANY_UNDEF=5, 0)`),
/// the JS result of calling a callback that never returns a value.
pub(crate) fn emit_undef_any_box(ctx: &mut LowerCtx<'_>) -> ValueId {
    ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_box,
            vec![Operand::ConstI64(5), Operand::ConstI64(0)],
        ),
        Type::Any,
        None,
    )
}

/// Callback arg list — a promoted receiver-first callback takes the
/// boxed thisArg (knife 4) as its leading `__this` arg; a plain
/// callback keeps `(elem, …)`. Spec §23.1.3 callbacks also receive
/// (index, sourceArray); those slots are appended only when the
/// callback's own sig declares them (`user_arity` 2 / 3), and
/// `materialize_call_args` aligns the reprs (I64 index → F64 / Any
/// box, array → Any box with the rc bookkeeping).
pub(super) fn cb_args(
    this_arg: Option<&Operand>,
    elem: ValueId,
    i_now: ValueId,
    src_arr: ValueId,
    user_arity: usize,
) -> Vec<Operand> {
    let mut a: Vec<Operand> = Vec::with_capacity(4);
    if let Some(t) = this_arg {
        a.push(t.clone());
    }
    a.push(Operand::Value(elem));
    if user_arity >= 2 {
        a.push(Operand::Value(i_now));
    }
    if user_arity >= 3 {
        a.push(Operand::Value(src_arr));
    }
    a
}

/// Rotation 363 — the argv-face downgrade: a callback the
/// argv-face collector reshaped (synthetic `__torajs_argv` head
/// param, body reads `arguments` values) cannot take the direct /
/// devirt lanes — their positional args would land in the argv
/// pointer slot. Route through the boxed variadic dispatch instead:
/// box each spec argument into a stack argv (the caller passes the
/// FULL spec list — cb_arity is forced to 3 upstream) and let the
/// dual-entry adapter feed real argc + argv into the synthetic
/// params. `box_to_any` is rc-neutral on every arm and an Any elem
/// is already a box, so the pack is pure encoding; the adapter's
/// materialize incs what it stores.
pub(crate) fn emit_argv_face_call(
    ctx: &mut LowerCtx<'_>,
    fn_val: &Operand,
    fn_ty: Type,
    args: Vec<Operand>,
    spec_argc: i64,
) -> ValueId {
    let Type::Closure(user_sig_id) = fn_ty else {
        unreachable!("argv-face callee is always a lifted closure");
    };
    let argv = ctx.f.append_inst(
        crate::ssa::BlockId(0),
        InstKind::AllocaBytes((args.len().max(1) * 8) as u64),
        Type::Ptr,
        Some("__hof_argv"),
    );
    for (i, a) in args.into_iter().enumerate() {
        let boxed = if ctx.operand_ty(&a) == Type::Any {
            a
        } else {
            ctx.box_to_any(a)
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(boxed, Operand::Value(argv), (i * 8) as u64),
        );
    }
    let r = crate::ssa_lower_call_closure_local::emit_variadic_call_conv(
        ctx,
        fn_val.clone(),
        argv,
        spec_argc,
        Vec::new(),
        user_sig_id,
    );
    match r {
        Operand::Value(v) => v,
        _ => unreachable!("variadic call conv answers an SSA value"),
    }
}

/// W4 / devirt dispatch — align arg widths with callback sig (f64-param
/// must not get raw i64 elem bits) then route through devirt direct call
/// when `known_fid` is set, else the indirect env+8 fn_ptr path.

pub(super) fn emit_do_call(
    ctx: &mut LowerCtx<'_>,
    known_fid: Option<FuncId>,
    fn_val: &Operand,
    fn_ty: Type,
    args: Vec<Operand>,
    sig_skip: usize,
    spec_argc: i64,
    argv_face: bool,
) -> ValueId {
    if argv_face {
        return emit_argv_face_call(ctx, fn_val, fn_ty, args, spec_argc);
    }
    // `sig_skip` — a promoted receiver-first callback's leading boxed
    // `__this` argv entry is not in the sig (knife 4); positional
    // alignment against the sig starts after it, and the call lanes
    // below apply the same skip.
    //
    // The I64 → F64 alignment that used to sit here is gone: it was
    // one direction of the shared argument contract, patched back in
    // on this side because `materialize_call_args` (which every lane
    // below reaches) did not carry the number lanes. It does now, so
    // converting here would only do the same work twice.
    let r = match known_fid {
        Some(fid) => {
            ctx.call_fn_value_devirt(fid, fn_val.clone(), fn_ty, args, sig_skip, spec_argc)
        }
        None => ctx.call_fn_value(fn_val.clone(), fn_ty, args, sig_skip, spec_argc),
    };
    // §23.1.3.15 step 5.c ReturnIfAbrupt — a throwing callback ends
    // the iteration; without this the loop ran every remaining
    // element and swallowed the throw. Devirt'd callbacks ride the
    // may-throw gate (verified-non-throwing fns skip the check).
    ctx.emit_throw_check(known_fid);
    r
}
