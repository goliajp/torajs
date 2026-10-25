//! Array-receiver `<Arr>.sort | toSorted (cmp?)` dispatch — sixth
//! sub-split carved out of [`ssa_lower_str::try_lower_method_call`].
//! Lowers in-place insertion-sort (`sort`) and clone-then-sort
//! (`toSorted`) over typed elements. The body emits the comparator
//! inline: when a user cmp is supplied it's invoked through the
//! callee Operand and the return widened to a Bool predicate;
//! without one, an element-type-aware `prev > cur` pred is built
//! directly (ICmp/FCmp for primitive layouts, the runtime helper
//! `__torajs_str_locale_compare` for Str/Substr, branchless XOR for
//! Bool). The pred drives the inner-shift loop that walks the
//! growing prefix backward to find the insertion point.
//!
//! Split: [`helper`] holds the `__torajs_arr_sort_cb` O(n log n)
//! fast-path gate + emission; [`pred`] holds the inline predicate
//! (user-cmp call / default ToString compare) emission.
//!
//! Returns `None` when the receiver is not `Type::Arr` or the
//! method is neither `sort` nor `toSorted` so the caller can keep
//! trying the remaining branches.

use crate::ast::{Expr, ExprId};
use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

mod helper;
mod insertion;
mod pred;
mod shift;

/// Try to lower `<Arr>.sort | toSorted(cmp?)`. Returns
/// `Some(value)` when handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // `arr.sort(cmp)` — in-place insertion sort calling
    // `cmp` for each compare. Returns the same array. The
    // comparator's return is treated as an i64 (or
    // implicitly-promoted-to-i64); ssa-lower picks ICmp/
    // FCmp(>0) based on its actual SSA type. Insertion
    // sort is O(n²) but works for moderate array sizes
    // and avoids needing closure-aware C runtime.
    if let Type::Arr(arr_id) = recv_ty
        && (method == "sort" || method == "toSorted")
    {
        // S276 — widen from `args.len() <= 1` to any arg count per ES
        // §23.1.3.{30,33} trailing-arg ignore. SSA-emit reads only
        // args[0] (cmp); trailing args eval-and-drop below so side-
        // effect exprs fire.
        let mut elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        // toSorted clones the receiver via arr_slice
        // before sorting so the source stays intact.
        // arr_slice does the alloc + memcpy in one
        // runtime call; the rest of the body operates on
        // the clone.
        let recv_op = if method == "toSorted" {
            clone_for_to_sorted(ctx, recv_op, arr_id, &mut elem_ty)
        } else {
            recv_op
        };
        let arr_ptr = match recv_op {
            Operand::Value(v) => v,
            _ => unreachable!(),
        };
        // toSorted owns its clone until it answers it: a comparator
        // that throws leaves through every check below
        let clone_tok = (method == "toSorted").then(|| {
            let ty = ctx.f.value_type(arr_ptr);
            ctx.push_throw_temp(Operand::Value(arr_ptr), ty)
        });
        // V3-18 wedge — sort/toSorted with no comparator
        // emits inline element-type-aware `prev > cur`
        // instead of calling a user fn. cmp_val + cmp_ty
        // stay None for the no-arg path so the
        // pred-computation block can branch later.
        //
        // ES §23.1.3.{29,31} step 1 also accepts `undefined` for
        // the comparator literal; route a 1-arg call with arg type
        // Undefined to the same default-compare path (check.rs
        // mirror at the sort/toSorted arity special-case).
        let (cmp_val, cmp_ty) = if let Some(arg0) = args.first() {
            let arg_static_ty = ctx.expr_types.get(arg0).cloned();
            if matches!(arg_static_ty, Some(crate::check::Type::Undefined)) {
                // Drop the operand without lowering its side
                // effects — `undefined` is a literal keyword with
                // none.
                (None, None)
            } else {
                let v = ctx.lower_expr(*arg0);
                let t = ctx.operand_ty(&v);
                (Some(v), Some(t))
            }
        } else {
            (None, None)
        };
        // S276 — eval-and-drop trailing args past cmp so step()-style
        // side-effect exprs fire per ES §23.1.3.{30,33} trailing-arg
        // ignore. SSA-emit reads only args[0]; args[1..] discarded.
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        // Rotation 364 — an argv-face comparator (body reads
        // `arguments` values, or typed slots fed `any` elements) takes
        // the boxed variadic dispatch (`emit_argv_face_call`) with the
        // §23.1.3.30.2 «x, y» pair, never the direct call. The helper
        // fast path self-excludes (its gate requires the params to
        // equal the element type; neither face has them).
        let argv_face = matches!((args.first(), cmp_ty), (Some(&a0), Some(t))
            if crate::ssa_lower_call_arr_ho::cb_face::cb_takes_argv_face(ctx, a0, t, recv_ty));
        // RFC 20260905 residue 1 — a promoted comparator (the same
        // static test the HOF lane makes) owns a leading `__this`;
        // §23.1.3.30.2 binds it undefined. The helper fast path reads
        // the cell's flag at runtime instead.
        let promoted = cmp_val.is_some()
            && args.first().is_some_and(|&a0| {
                matches!(ctx.ast.get_expr(a0),
                    Expr::Closure { fn_name, .. } if ctx.ast.fnexpr_recv_fns.contains(fn_name))
                    || ctx.ast.is_fnexpr_recv_use(a0)
            });
        let route = pred::CmpRoute {
            argv_face,
            this_arg: promoted.then(|| {
                Operand::Value(crate::ssa_lower_call_arr_ho_loop::emit_undef_any_box(ctx))
            }),
        };
        // Perf Round 5 attack #1 (RFC 20260703-perf-arr-sort-nlogn):
        // a user comparator whose signature matches the element layout
        // exactly routes through `__torajs_arr_sort_cb` — a stable
        // O(n log n) merge sort in torajs-arr that calls the comparator
        // back through the closure ABI. The inline insertion sort below
        // is O(n²) (28× the comparison count at n=1000, measured vs
        // JSC) and stays only as the fallback for shapes the helper
        // doesn't cover (default no-comparator ToString compare,
        // Substr→Str materialization, non-{I64,F64,Str} elements).
        // Comparator evaluated; §23.1.3.30's failing Set is next, on
        // either path — the insertion path writes inline.
        let len_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let writes = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Sge, Operand::Value(len_v), Operand::ConstI64(2)),
            Type::Bool,
            None,
        );
        crate::ssa_lower_str_arr_inplace::emit_arr_frozen_guard(
            ctx,
            Operand::Value(arr_ptr),
            Operand::Value(writes),
        );
        let cmp_tok = match (&cmp_val, args.first()) {
            (Some(cv), Some(a0)) => ctx.park_owned_temp(*a0, cv),
            _ => None,
        };
        let divert_join = helper::emit_exotic_divert(ctx, arr_ptr, &cmp_val, &cmp_ty, elem_ty);
        if !helper::try_emit_sort_helper(ctx, &cmp_val, &cmp_ty, args, elem_ty, arr_ptr) {
            insertion::emit_insertion_sort(ctx, arr_ptr, elem_ty, &cmp_val, &cmp_ty, &route);
        }
        if let Some(join) = divert_join {
            ctx.f.set_term(ctx.cur_block, Terminator::Br(join));
            ctx.cur_block = join;
        }
        // RFC 20260705 owned-result invariant: in-place sort answers
        // the receiver — the chaining result carries its own ref.
        // toSorted answers the fresh clone which is already owned.
        if method == "sort" {
            ctx.emit_rc_inc(Operand::Value(arr_ptr));
        }
        ctx.unpark_owned_temp(cmp_tok);
        ctx.unpark_owned_temp(clone_tok);
        release_cmp_temp(ctx, args, &cmp_val);
        return Some(Operand::Value(arr_ptr));
    }
    None
}

/// toSorted clones the receiver via arr_copy_get before sorting so the
/// source stays intact; the copy reads holes as undefined (§23.1.3.34
/// step 5 sorts `undefined`, not holes) and the rest of the body
/// operates on the clone.
/// A Substr element layout comes back as Str in `elem_ty`.
fn clone_for_to_sorted(
    ctx: &mut LowerCtx<'_>,
    recv_op: Operand,
    arr_id: crate::ssa::ArrId,
    elem_ty: &mut Type,
) -> Operand {
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
        Type::I64,
        None,
    );
    // An Any-elem receiver clones through the exotic-aware
    // `arr_any_slice` (RFC 20260721 刀 5 follow-up): an
    // accessor index reads through its getter per §23.1.3.34
    // [[Get]], and the kernel incs each slot itself. Typed
    // receivers keep the raw `arr_slice` memcpy + inc range.
    if *elem_ty == Type::Any {
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.arr_any_slice,
                vec![recv_op, Operand::ConstI64(0), Operand::Value(len)],
            ),
            Type::Arr(arr_id),
            None,
        );
        ctx.emit_throw_check(None);
        Operand::Value(v)
    } else {
        let out_id = ctx.copied_arr_layout(arr_id);
        let fill = crate::ssa_lower_str_arr_inplace::hole_fill_for(ctx, *elem_ty);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.arr_copy_get, vec![recv_op, fill]),
            // The clone of an `Arr<Substr>` is `Arr<Str>` (views
            // do not leave their split block — rotation 468);
            // the sort below then runs on owned strings.
            Type::Arr(out_id),
            None,
        );
        // Phase B refcount: arr_slice memcpys the slots without
        // touching element refcounts — inc each shared slot so the
        // clone owns its refs (mirrors arr.slice; without this the
        // clone's drop per-elem dec is unmatched = over-release).
        if elem_ty.is_refcounted() {
            ctx.emit_adopt_copied_range(
                Operand::Value(v),
                *elem_ty,
                Operand::ConstI64(0),
                Operand::Value(len),
            );
            if *elem_ty == Type::Substr {
                *elem_ty = Type::Str;
            }
        }
        Operand::Value(v)
    }
}

/// RFC 20260705 chunk 550 — a comparator that lowered to an
/// owned-shape temp (inline arrow's minted env) is released after
/// the sort consumed it; `let`-bound / named-fn comparators are
/// borrows and stay untouched.
fn release_cmp_temp(ctx: &mut LowerCtx<'_>, args: &[ExprId], cmp_val: &Option<Operand>) {
    if let (Some(cv), Some(a0)) = (cmp_val, args.first()) {
        ctx.release_owned_temp(*a0, cv);
    }
}
