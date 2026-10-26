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
        InstKind::BoxAny(5, Operand::ConstI64(0)),
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
        a.push(*t);
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
///
/// `sig_skip` — a promoted callback's leading boxed `__this` in
/// `args` (rotation 649): it is the RECEIVER, not an argument, so it
/// rides the kernel's explicit-`this` twin instead of the argv. The
/// receiver-less kernel prepends `undefined` for a flagged cell on
/// its own, and packing the receiver into the argv doubled the seed:
/// `[8].forEach(function (this, a) { arguments[1] }, o)` saw
/// `this === undefined`, `a` as the coerced thisArg and an
/// `arguments` list led by the receiver.
pub(crate) fn emit_argv_face_call(
    ctx: &mut LowerCtx<'_>,
    fn_val: &Operand,
    fn_ty: Type,
    args: Vec<Operand>,
    sig_skip: usize,
    spec_argc: i64,
) -> ValueId {
    let Type::Closure(user_sig_id) = fn_ty else {
        unreachable!("argv-face callee is always a lifted closure");
    };
    let mut args = args;
    let this_arg = (sig_skip == 1).then(|| args.remove(0));
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
        *fn_val,
        this_arg,
        crate::ssa_lower_any_argv::AnyArgv::borrowed(argv),
        spec_argc,
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
// one call's operands plus its lane flags
#[allow(clippy::too_many_arguments)]
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
        return emit_argv_face_call(ctx, fn_val, fn_ty, args, sig_skip, spec_argc);
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
    // an element handed to an `any` parameter crosses into the boxed
    // world here; a number slot holding the undefined sentinel, or a
    // pointer slot holding its undefined cell (a copied hole, an
    // out-of-range fill), has to arrive as undefined
    let params = match fn_ty {
        Type::Closure(s) | Type::FnSig(s) => ctx.fn_sigs[s.0 as usize].0.clone(),
        _ => Vec::new(),
    };
    let any_into_struct = needs_boxed_entry(ctx, known_fid, &args, &params, sig_skip);
    if any_into_struct && matches!(fn_ty, Type::Closure(_)) {
        let argc = (args.len() - sig_skip) as i64;
        return emit_argv_face_call(ctx, fn_val, fn_ty, args, sig_skip, argc);
    }
    let args = args
        .into_iter()
        .enumerate()
        .map(|(i, a)| {
            let to_any = i >= sig_skip && params.get(i - sig_skip) == Some(&Type::Any);
            let ty = ctx.operand_ty(&a);
            if to_any && ty == Type::F64 {
                ctx.box_f64_or_undef(a)
            } else if to_any && crate::ssa_lower_index_undef::has_undef_spelling(ty) {
                ctx.box_heap_slot_or_undef(a)
            } else {
                a
            }
        })
        .collect::<Vec<_>>();
    let r = match known_fid {
        Some(fid) => ctx.call_fn_value_devirt(fid, *fn_val, fn_ty, args, sig_skip, spec_argc),
        None => ctx.call_fn_value(*fn_val, fn_ty, args, sig_skip, spec_argc),
    };
    // §23.1.3.15 step 5.c ReturnIfAbrupt — a throwing callback ends
    // the iteration; without this the loop ran every remaining
    // element and swallowed the throw. Devirt'd callbacks ride the
    // may-throw gate (verified-non-throwing fns skip the check).
    ctx.emit_throw_check(known_fid);
    r
}

/// A Map / Set walk hands every argument as a box: a callback with a
/// boxed entry and a fn-typed parameter past `sig_skip` takes that
/// entry, which hands the call to the all-`any` twin.
pub(crate) fn boxed_fn_slot(
    ctx: &LowerCtx<'_>,
    known_fid: Option<FuncId>,
    fn_ty: Type,
    sig_skip: usize,
) -> bool {
    known_fid.is_some_and(|f| ctx.boxed_entries.contains_key(&f))
        && matches!(fn_ty, Type::Closure(_))
        && ctx.sig_param_tys(fn_ty).is_some_and(|ps| {
            ps.iter()
                .skip(sig_skip)
                .any(|p| matches!(p, Type::Closure(_) | Type::FnSig(_)))
        })
}

/// an `any` element meeting a struct parameter, or an `any[]` source
/// meeting an element-typed one, has no typed repr to pass: the
/// closure's boxed entry decides per value (a fitting cell passes
/// through, any other value takes the generic lane). A scalar
/// parameter would bind the element converted (§10.2.1.3 binds it
/// as it is: `typeof v` on "s" is "string"), so it takes the boxed
/// entry too when the callback has one: a fitting value runs the
/// typed body, any other the all-`any` twin.
/// an `any[]` source handed to an element-typed array param
fn needs_boxed_entry(
    ctx: &LowerCtx<'_>,
    known_fid: Option<FuncId>,
    args: &[Operand],
    params: &[Type],
    sig_skip: usize,
) -> bool {
    let boxed = known_fid.is_some_and(|f| ctx.boxed_entries.contains_key(&f));
    args.iter().enumerate().any(|(i, a)| {
        let (Some(p), actual) = (params.get(i.wrapping_sub(sig_skip)), ctx.operand_ty(a)) else {
            return false;
        };
        i >= sig_skip
            && match (actual, *p) {
                (Type::Any, Type::Obj(_)) => true,
                (Type::Any, Type::F64 | Type::I64 | Type::I32 | Type::Bool | Type::Str) => boxed,
                // no run-time test proves a value is a function of the
                // slot's signature, so the boxed entry hands every call
                // to the twin (`build_reroute`)
                (Type::Any, Type::Closure(_) | Type::FnSig(_)) => boxed,
                // an `any[]` source handed to an element-typed array param
                (Type::Arr(a), Type::Arr(p)) => {
                    ctx.arr_layouts[a.0 as usize] == Type::Any
                        && ctx.arr_layouts[p.0 as usize] != Type::Any
                }
                _ => false,
            }
    })
}
