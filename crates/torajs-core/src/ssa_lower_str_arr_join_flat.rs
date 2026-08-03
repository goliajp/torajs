//! Array-receiver `<Arr>.<method>(args)` dispatch for `join` /
//! `toString` / `toLocaleString` / `flat` — fifth sub-split carved
//! out of [`ssa_lower_str::try_lower_method_call`]. These three
//! method shapes share the array-element-type-aware helper dispatch
//! pattern (element-typed `arr_join_*` intrinsics for join family,
//! depth-unrolled `arr_flat_any` / direct `arr_slice` clone for
//! `flat`) and so live together in this sibling.
//!
//! Returns `None` when the receiver is not `Type::Arr` or the method
//! isn't in this dispatch's responsibility so the caller can keep
//! trying the remaining branches.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

/// Try to lower `<Arr>.join | toString | toLocaleString | flat (...)`
/// through the array-receiver join / flat dispatch. Returns
/// `Some(value)` when handled; `None` otherwise.
pub(crate) fn try_dispatch(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    try_join(ctx, method, args, recv_op, recv_ty)
        .or_else(|| try_flat(ctx, method, args, recv_op, recv_ty))
}

/// `<Arr>.join(sep)` / `toString()` / `toLocaleString()` — the
/// element-typed `arr_join_*` intrinsic family.
fn try_join(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    // Array<string>.join(sep) — receiver is Type::Arr,
    // method == "join". The check.rs guard ensures
    // element type is String, so we don't re-validate
    // here.
    // V3-18 wedge — Array.toString routes to the same
    // join intrinsic with sep="," per JS spec
    // §22.1.3.30. Same element-type constraint as
    // join itself.
    if let Type::Arr(elem_arr_id) = recv_ty
        && (method == "join" || method == "toString" || method == "toLocaleString")
    {
        let elem_ty = ctx.arr_layouts[elem_arr_id.0 as usize];
        // V3-18 m1.h.43 — element-type dispatch for
        // join. Number / Bool elements use dedicated
        // runtime helpers that ToString each element
        // inline; Str / Substr take the existing
        // pointer-walking helpers.
        // ES §22.1.3.32 step 5.b — toLocaleString routes
        // numeric elements through Number.toLocaleString
        // (en-US group separator). String / Substr / Bool
        // / Any keep the ToString-equivalent helpers
        // because their .toLocaleString returns the same
        // value (no Intl substrate engaged).
        let is_locale = method == "toLocaleString";
        // §23.1.3.32 — an Arr<Any> receiver invokes each element's
        // OWN toLocaleString (observable hook; the join helper only
        // stringifies). Separate lane: unary, fresh owned Str, may
        // leave a pending element-hook throw.
        if is_locale && matches!(elem_ty, Type::Any) {
            let s = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_any_to_locale_string, vec![recv_op]),
                Type::Str,
                None,
            );
            ctx.emit_throw_check(None);
            return Some(Operand::Value(s));
        }
        // RFC 20260721 刀 11 G13 — the primitive-element lanes take
        // a runtime patch gate: a builtin-prototype monkey-patch on
        // the element family's Invoke face must fire per element
        // (§23.1.3.32 step 4.b), while the no-patch program keeps
        // the direct join kernel behind one bitmap load. Trailing
        // args lower-and-drop (S287 idiom — toLocaleString is
        // 0-useful).
        if is_locale {
            let prim = match elem_ty {
                Type::Bool => Some(0i64),
                Type::Str => Some(1),
                Type::Substr => Some(2),
                Type::I64 => Some(3),
                Type::F64 => Some(4),
                _ => None,
            };
            if let Some(p) = prim {
                ctx.emit_arr_mark_kind(&recv_op);
                for &a in args.iter() {
                    let _ = ctx.lower_expr(a);
                }
                let s = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(
                        ctx.intrinsics.arr_typed_to_locale_string,
                        vec![recv_op, Operand::ConstI64(p)],
                    ),
                    Type::Str,
                    None,
                );
                ctx.emit_throw_check(None);
                return Some(Operand::Value(s));
            }
        }
        let join_fid = match elem_ty {
            Type::Substr => ctx.intrinsics.arr_join_substr,
            Type::I64 => {
                if is_locale {
                    ctx.intrinsics.arr_join_i64_locale
                } else {
                    ctx.intrinsics.arr_join_i64
                }
            }
            Type::F64 => {
                if is_locale {
                    ctx.intrinsics.arr_join_f64_locale
                } else {
                    ctx.intrinsics.arr_join_f64
                }
            }
            Type::Bool => ctx.intrinsics.arr_join_bool,
            Type::Any => ctx.intrinsics.arr_join_any, // S126-4
            _ => ctx.intrinsics.arr_join,
        };
        let mut argv = Vec::with_capacity(2);
        argv.push(recv_op);
        // V3-18 m1.h.42 — default separator ","
        // when join() is called with no arg.
        //
        // S206 — explicit `undefined` sep follows the same
        // default-undefined rule per spec §23.1.3.16 step 1:
        // if sep is undefined → sep = ",". Detect the
        // typed-Undefined arg shape and skip the operand
        // lower (the literal has no side effects to drop).
        // S242 widens the 1-arg detection to 2-arg so the trailing-arg
        // shape `xs.join(undef, trailing)` still folds to "," without
        // lowering the undef operand into the helper's Str slot.
        // S299 — widen to any args.len() >= 1 so `xs.join(undef, t1, t2, ...)`
        // 3+-arg trailing shape stays on the undef-sep fold path; trailing
        // args[1..] lower-and-drop via the S287 useful=1 skip loop below.
        let undef_sep = !args.is_empty()
            && matches!(
                ctx.expr_types.get(&args[0]),
                Some(crate::check::Type::Undefined)
            );
        // S287 — toString / toLocaleString are 0-useful (sep is
        // hard-coded ","); join is 1-useful (sep:Str|Undef).
        // Trailing args past the useful slots get lower-and-dropped
        // here so step()-style side-effect exprs fire per ES
        // eval-then-discard (S272 idiom). Mirrors check.rs S287.
        let useful = if method == "join" { 1 } else { 0 };
        let sep = if useful == 0 || args.is_empty() || undef_sep {
            let s = ctx.intern_string_literal(",");
            Operand::Value(s)
        } else {
            ctx.lower_expr(args[0])
        };
        for &a in args.iter().skip(useful) {
            let _ = ctx.lower_expr(a);
        }
        argv.push(sep);
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(join_fid, argv),
            Type::Str,
            None,
        );
        return Some(Operand::Value(v));
    }
    None
}

/// `arr.flat()` / `arr.flat(N)` — N-level deep flatten.
/// Default depth = 1. Literal depth N is statically
/// unrolled into N calls to the depth-1 runtime
/// helper, peeling one Array<> layer per iter and
/// stopping early if a layer is non-Array. depth=0 is
/// a shallow clone via arr_slice.
fn try_flat(
    ctx: &mut LowerCtx<'_>,
    method: &str,
    args: &[ExprId],
    recv_op: Operand,
    recv_ty: Type,
) -> Option<Operand> {
    if let Type::Arr(_) = recv_ty
        && method == "flat"
    {
        // §23.1.3.13 step 3 ArraySpeciesCreate — constructor-face
        // guard before the derive (RFC 20260713 blade 3).
        ctx.emit_arr_species_guard(recv_op.clone());
        // S289 — accept any trailing operands past `depth` per ES
        // §23.1.3.10 trailing-arg ignore; lower-and-drop so step()-
        // style side-effect exprs fire (S272 idiom). depth detection
        // below still inspects only args.first().
        for &a in args.iter().skip(1) {
            let _ = ctx.lower_expr(a);
        }
        let literal_depth = args.is_empty()
            || match ctx.ast.get_expr(args[0]) {
                Expr::Number(_) => true,
                Expr::Ident(n) => n == "Infinity" || n == "undefined",
                _ => false,
            };
        let depth: i64 = if args.is_empty() {
            1
        } else if !literal_depth {
            // Non-literal depth (checker mirror: the wedge admits it
            // as Array<Any>) — ToIntegerOrInfinity runs at runtime
            // inside the kernel (§23.1.3.13 step 2: NaN → 0, a
            // Symbol/BigInt operand leaves a pending TypeError).
            // NULL out = pending throw; the check unwinds before the
            // value is consumed.
            let d = ctx.lower_expr(args[0]);
            let d_any = ctx.box_to_any(d);
            ctx.emit_arr_mark_kind(&recv_op);
            let any_id = crate::ssa_lower::intern_arr_layout(ctx.arr_layouts, Type::Any);
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_flat_runtime_depth, vec![recv_op, d_any]),
                Type::Arr(any_id),
                None,
            );
            ctx.emit_throw_check(None);
            return Some(Operand::Value(v));
        } else if let Expr::Number(d) = ctx.ast.get_expr(args[0]) {
            *d as i64
        } else if let Expr::Ident(name) = ctx.ast.get_expr(args[0])
            && name == "Infinity"
        {
            // S129-5 `xs.flat(Infinity)` — ES §23.1.3.13 spec form
            // for full-depth flatten. ssa-lower unrolls flat-1
            // calls; the typed branch early-breaks once cur_ty
            // stops being Arr<Arr<T>>, and the Array<Any> branch's
            // arr_flat_any is a no-op shallow clone when no slot
            // wraps an inner Array<Any>. Using i64::MAX would
            // explode loop bookkeeping at lower-time — 64 unrolls
            // are plenty for realistic nesting (matches V8 / JSC
            // arbitrary limits in spec-test fixtures).
            64
        } else if let Expr::Ident(name) = ctx.ast.get_expr(args[0])
            && name == "undefined"
        {
            // S220 — `xs.flat(undefined)` per ES §23.1.3.10 step 1:
            // `If depth is undefined, depthNum = 1`. Equivalent to the
            // 0-arg `xs.flat()` default.
            1
        } else {
            panic!("ssa-lower: flat depth must be a number literal");
        };
        if depth == 0 {
            return Some(emit_shallow_clone(ctx, recv_op, recv_ty));
        }
        let mut cur = recv_op;
        let mut cur_ty = recv_ty;
        for _ in 0..depth {
            let Type::Arr(outer_id) = cur_ty else {
                break;
            };
            let outer_elem = ctx.arr_layouts[outer_id.0 as usize];
            // S129-3 Array<Any>.flat — outer_elem is Type::Any
            // (NaN-box AnyValue per slot). Routes to the Any-aware
            // runtime helper which decodes each slot's tag and
            // extends inner Array<Any> via arr_extend_any (rc-safe)
            // / passes other slots through as a single push. Result
            // type stays Array<Any> (depth=N unrolls N flat-1 calls,
            // mirror of the typed branch below).
            if matches!(outer_elem, Type::Any) {
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Call(ctx.intrinsics.arr_flat_any, vec![cur]),
                    cur_ty,
                    None,
                );
                cur = Operand::Value(v);
                continue;
            }
            let Type::Arr(_) = outer_elem else {
                // ES §23.1.3.11 — non-Array slot: receiver is already
                // "flat" at this depth, but flat() must still return a
                // fresh array. Emit a shallow clone (arr_slice + rc_inc
                // range on refcounted elem) — same shape as depth=0.
                cur = emit_shallow_clone(ctx, cur, cur_ty);
                break;
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_flat, vec![cur]),
                outer_elem,
                None,
            );
            cur = Operand::Value(v);
            cur_ty = outer_elem;
        }
        return Some(cur);
    }
    None
}

/// Shallow clone of one array layer — `arr_slice(op, 0, len)` plus a
/// per-element rc_inc range on refcounted layouts. Shared by the
/// `flat(0)` fold and the non-Array-slot early stop.
fn emit_shallow_clone(ctx: &mut LowerCtx<'_>, op: Operand, ty: Type) -> Operand {
    let len = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(Type::I64, op, ARR_LEN_OFF),
        Type::I64,
        None,
    );
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.arr_slice,
            vec![op, Operand::ConstI64(0), Operand::Value(len)],
        ),
        ty,
        None,
    );
    if let Type::Arr(arr_id) = ty {
        let elem_ty = ctx.arr_layouts[arr_id.0 as usize];
        if elem_ty.is_refcounted() {
            let len2 = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                Type::I64,
                None,
            );
            ctx.emit_arr_rc_inc_range(
                Operand::Value(v),
                Operand::ConstI64(0),
                Operand::Value(len2),
            );
        }
    }
    Operand::Value(v)
}
