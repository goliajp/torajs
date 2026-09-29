//! The ToString half of the callable-coercion dispatch — split from
//! `ssa_lower_call_coercion.rs` when the file reached the 500-line
//! limit. The sibling keeps `try_lower`'s routing and the ToNumber
//! family (§7.1.4); everything that turns a value into a string
//! (§7.1.17, and the §13.2.8.6 template-substitution variant that
//! differs only on Symbol) lives here.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// Spec §7.1.17 ToString dispatch by arg SSA type. Str/Substr pass
/// through; I64/F64/Bool → matching `*_to_str` intrinsic; null →
/// null_to_str; Any → coerce_to_str (tag-dispatched); Arr → join(",")
/// (same dispatch as `arr.toString()`); Obj → "[object Object]" per
/// §20.1.4.4 generic Object toString.
///
/// `implicit_tostring` — true for a parser-synthesized template
/// wrapper (§13.2.8.6 substitution: a Symbol throws TypeError);
/// false for the explicit `String(...)` display face (a Symbol
/// answers its SymbolDescriptiveString per §22.1.1 step 1.a). The
/// two only diverge on Symbol.
pub(crate) fn emit_to_string(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    arg_ty: Type,
    implicit_tostring: bool,
) -> Operand {
    // A pointer-shaped `T | null` arrives here spelled as bare T: the
    // slot is the same slot and its null is the in-band 0 (the two
    // scalars ride `any` instead and reach the Any arm below, which
    // has always answered "null"). Every arm past this point reads
    // that 0 as a live value of its own type — the Str arm handed it
    // straight back, so `String(s)` answered a null POINTER that
    // printed as `null` and had no `.length`; the join kernel read it
    // as a cell and produced nothing at all.
    //
    // §7.1.17 asks the question this branch asks, in the order it
    // asks it: null first, then ToString of whatever is there. The
    // live arm is the same arm the non-nullable spelling takes, so a
    // typed array keeps its join kernel.
    if crate::ssa_lower_call_coercion_nullable::is_nullable_ptr(ctx, arg_eid, &arg_ty) {
        return crate::ssa_lower_call_coercion_nullable::emit_nullable_to_string(
            ctx,
            arg_eid,
            arg_op,
            arg_ty,
            implicit_tostring,
        );
    }
    emit_to_string_inner(ctx, arg_eid, arg_op, arg_ty, implicit_tostring)
}

pub(crate) fn emit_to_string_inner(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    arg_ty: Type,
    implicit_tostring: bool,
) -> Operand {
    // A pointer-shaped slot the checker typed `T | undefined` holds
    // the generic immortal cell for its undefined, and every arm
    // below reads the slot as a live value of its own type: the Obj
    // arm handed the cell to the ToPrimitive ladder, which looked for
    // `toString` on a cell that has none and threw "value is not a
    // function"; the Closure arm asked the source registry and got
    // `function () { [native code] }`. `const o: { a: number } |
    // undefined = undefined; String(o)` — a program every engine
    // answers "undefined" for.
    //
    // The nullish pre-check in front of this function only ever
    // tested NULL, which is `null`'s in-band spelling, not
    // undefined's. So ask the question the lowering already has a
    // predicate for and hand the value to the any boundary, where
    // `box_to_any_from_expr` re-encodes the cell as ANY_UNDEF and the
    // §7.1.17 kernel answers "undefined". Self-gating: a slot the
    // checker did not type nullable never reaches it, so no typed
    // spelling moves.
    if crate::ssa_lower_nullable_guard::holds_undef_cell(ctx, arg_eid, arg_ty) {
        return emit_boxed_to_string(ctx, arg_eid, arg_op, implicit_tostring);
    }
    match arg_ty {
        Type::Str | Type::Substr => {
            // Identity pass-through: the result IS the arg value, and
            // the owned-result invariant makes the consumer release it.
            // A borrow-shaped arg (Ident / Member) therefore shares —
            // +1 here so the source binding keeps its own stake; the
            // old consume path stole the source's single stake (UAF
            // once the result's owner dropped it, reuse-window probe).
            // Owned temps (concat results) transfer their fresh ref.
            if !ctx.expr_transfers_ownership(arg_eid) {
                ctx.emit_rc_inc(arg_op.clone());
            }
            arg_op
        }
        Type::I64 => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.i64_to_str, vec![arg_op]),
            Type::Str,
            None,
        )),
        Type::F64 => {
            // RFC 20260708-typed-arr-oob-read chunk 3 — an
            // undefined-infected F64 source (number[] OOB read /
            // its aliases) stringifies the sentinel as "undefined",
            // not "NaN" (same branch the concat lane takes; the
            // template String(...) wrap routes substitutions here).
            if crate::ssa_lower_undef_f64_source::is_undef_f64_source(ctx, arg_eid) {
                return crate::ssa_lower_binop_inner::add_str::coerce_undefable_f64(ctx, arg_op).0;
            }
            Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.f64_to_str, vec![arg_op]),
                Type::Str,
                None,
            ))
        }
        Type::Bool => Operand::Value(ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.bool_to_str, vec![arg_op]),
            Type::Str,
            None,
        )),
        // RFC 20260716 刀 6 — undef vs null distinction (same
        // rationale as emit_to_number above): ToString(undef) is
        // "undefined" per §7.1.17 step 3, ToString(null) is "null"
        // per §7.1.17 step 2.
        // The gate here used to be `arg_op == ConstPtrNull`, which only
        // ever holds for the LITERAL: bind the same value and the read
        // is a Load, so `const s = null; String(s)` fell off the end
        // of this match and panicked while `String(null)` answered
        // "null". A bare `Type::Ptr` is null or undefined and nothing
        // else here, and the kernel pick one line down already reads
        // the checker's record to tell them apart.
        Type::Ptr => {
            let fid = if matches!(
                ctx.expr_types.get(&arg_eid),
                Some(crate::check::Type::Undefined)
            ) {
                ctx.intrinsics.undefined_to_str
            } else {
                ctx.intrinsics.null_to_str
            };
            Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(fid, vec![]),
                Type::Str,
                None,
            ))
        }
        // S133-2 — `String(Any)`: tag-dispatched ToString via the
        // display variant (§22.1.1 step 1.a — the explicit String()
        // call answers a Symbol's SymbolDescriptiveString instead of
        // the §7.1.17 implicit-coercion TypeError; every other tag
        // matches anyv_to_str). A parser-synthesized TEMPLATE
        // wrapper (`ast.template_str_calls`) takes the implicit
        // kernel instead — §13.2.8.6 substitution ToString throws on
        // a Symbol; the two kernels agree on every other tag (both
        // hint-string). Borrows the Any box; release an owned temp
        // arg after the read.
        Type::Any => {
            let fid = if implicit_tostring {
                ctx.intrinsics.any_to_str_box
            } else {
                ctx.intrinsics.any_to_display_str
            };
            let v = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(fid, vec![arg_op.clone()]),
                Type::Str,
                None,
            );
            ctx.release_owned_temp(arg_eid, &arg_op);
            // Both kernels can record a pending throw: the implicit
            // one on ToString(Symbol) (§13.2.8.6), and BOTH on an
            // OrdinaryToPrimitive receiver whose toString/valueOf
            // each answer objects (§7.1.17 step 3 TypeError) or whose
            // user hook throws — the display variant only differs on
            // the Symbol arm (test262 String S8.12.8_A1 leaked the
            // placeholder without this).
            ctx.emit_throw_check(None);
            Operand::Value(v)
        }
        // rotation 141 — `String(symbol)` typed spelling: §22.1.1
        // step 1.a SymbolDescriptiveString (the same kernel
        // `sym.toString()` rides; the Any lane's display variant
        // answers this shape already — this is its typed twin). A
        // template wrapper over a statically-typed Symbol takes the
        // implicit kernel through a tag-4 box instead — §13.2.8.6
        // ToString(Symbol) throws (the box is a pure encode; the
        // kernel only reads the borrow).
        Type::Symbol => emit_symbol_to_string(ctx, arg_eid, arg_op, implicit_tostring),
        Type::Arr(elem_arr_id) => crate::ssa_lower_coerce_arr::emit_to_string(
            ctx,
            arg_eid,
            arg_op,
            elem_arr_id,
            implicit_tostring,
        ),
        Type::Obj(_) => emit_struct_to_string(ctx, arg_eid, arg_op),
        // A FnSig slot is a bare code address; the kernel finds the
        // function object (its canonical cell) and runs the same
        // ToPrimitive walk the Closure arm below gets from the any
        // kernel — the registry's source row only when no cell exists.
        Type::FnSig(_) => emit_fnsig_to_str(ctx, arg_op, 1),
        // §7.1.17 step 9 — ToString(fn) is ToPrimitive(fn, string),
        // i.e. OrdinaryToPrimitive over the function OBJECT: its own
        // `toString` / `valueOf` decide before the source text does.
        // This arm used to call the registry's source kernel
        // directly, so `f.toString = () => "x"; String(f)` answered
        // the source while `String(f as any)` answered "x" — the any
        // kernel has always run the ladder, and its closure arm falls
        // back to the same registry row when no hook is installed.
        Type::Closure(_) => emit_boxed_to_string(ctx, arg_eid, arg_op, implicit_tostring),
        // §7.1.17 ToString takes every value. A typed spelling whose
        // repr is a heap cell simply has no dedicated kernel here, so
        // it boxes at the any boundary and takes the very kernel the
        // `any` spelling already takes — the Symbol arm above is the
        // same move, one type earlier. Without it `String(10n)` and
        // `` `${new Map()}` `` were compile errors while
        // `String(x as any)` answered "10" and "[object Map]" off
        // this kernel, the receiver's own spelling deciding whether
        // the program builds.
        // The kernel reads the header tag, so the rest of the heap
        // cells answer too. Adding four of them and stopping is how a
        // list stays half-written: `String(new Date(0))` was still a
        // compile error while `String(d as any)` was not.
        Type::BigInt
        | Type::Map
        | Type::Set
        | Type::RegExp
        | Type::Date
        | Type::WeakMap
        | Type::WeakSet
        | Type::WeakRef
        | Type::MapIter
        | Type::ArrIter
        | Type::Promise => emit_boxed_to_string(ctx, arg_eid, arg_op, implicit_tostring),
        _ => panic!("ssa-lower: String() with arg type {arg_ty:?} not yet supported"),
    }
}

/// §7.1.17 step 9 over a FnSig slot — `hint_string` 1 for String() /
/// templates, 0 for the `+` position (§13.15.3 default hint). The
/// kernel can record a pending throw (a user hook that throws, or both
/// hooks answering objects).
pub(crate) fn emit_fnsig_to_str(
    ctx: &mut LowerCtx<'_>,
    fn_op: Operand,
    hint_string: i64,
) -> Operand {
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.fnsig_to_primitive_str,
            vec![fn_op, Operand::ConstI64(hint_string)],
        ),
        Type::Str,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// The one type the two faces disagree on. Split out of
/// [`emit_to_string_inner`] under the 200-line function discipline —
/// verbatim, both legs and the throw check.
fn emit_symbol_to_string(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    implicit_tostring: bool,
) -> Operand {
    if implicit_tostring {
        let boxed = ctx.box_to_any(arg_op.clone());
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(ctx.intrinsics.any_to_str_box, vec![boxed]),
            Type::Str,
            None,
        );
        ctx.release_owned_temp(arg_eid, &arg_op);
        ctx.emit_throw_check(None);
        return Operand::Value(v);
    }
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(ctx.intrinsics.symbol_to_str, vec![arg_op.clone()]),
        Type::Str,
        None,
    );
    ctx.release_owned_temp(arg_eid, &arg_op);
    Operand::Value(v)
}

/// The any-boundary route: box the cell and call the very kernel the
/// `any` spelling takes. Body of the arm above, lifted out so the arm
/// is its type list and nothing else.
fn emit_boxed_to_string(
    ctx: &mut LowerCtx<'_>,
    arg_eid: ExprId,
    arg_op: Operand,
    implicit_tostring: bool,
) -> Operand {
    // The expression-aware box, not the value-only one: a
    // pointer-shaped slot the checker typed `T | undefined` carries
    // the generic immortal cell for its undefined, and a plain
    // `box_to_any` encodes that cell as an ordinary heap object. The
    // kernel then ran the ToPrimitive ladder over a cell that has no
    // methods — `String(o)` on `const o: { a: number } | undefined =
    // undefined` threw "value is not a function" instead of
    // answering "undefined". The guarded box re-encodes the sentinel
    // as ANY_UNDEF, which is what every other any-boundary in the
    // lowering already hands the runtime.
    let boxed = ctx.box_to_any_from_expr(arg_eid, arg_op.clone());
    let fid = if implicit_tostring {
        ctx.intrinsics.any_to_str_box
    } else {
        ctx.intrinsics.any_to_display_str
    };
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(fid, vec![boxed]),
        Type::Str,
        None,
    );
    ctx.release_owned_temp(arg_eid, &arg_op);
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// S137 — `String(struct)` runs OrdinaryToPrimitive at runtime (RFC
/// 20260712-string-proto-cluster chunk C — `any_to_str` dispatches the
/// user hook and accepts any primitive result, undefined included, and
/// answers the §20.1.4.4 "[object Object]" through
/// Object.prototype.toString when the receiver has no hook of its own).
///
/// It used to shortcut a hook-free layout to a static literal, deciding
/// from the layout's FIELDS whether a hook existed. A class instance
/// has the same `Type::Obj` slot and keeps its methods on the
/// prototype, never in the layout — so that test answered no for every
/// class and would have printed "[object Object]" over a user
/// `toString`. Predicting the hook is what made it wrong; the runtime
/// is the one that knows, and it costs a call on a path nobody's hot
/// loop runs.
fn emit_struct_to_string(ctx: &mut LowerCtx<'_>, arg_eid: ExprId, arg_op: Operand) -> Operand {
    let raw = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::PtrToInt(arg_op.clone()),
        Type::I64,
        None,
    );
    let s = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Call(
            ctx.intrinsics.any_to_str,
            vec![Operand::ConstI64(4), Operand::Value(raw)],
        ),
        Type::Str,
        None,
    );
    ctx.emit_throw_check_releasing(arg_eid, &arg_op);
    Operand::Value(s)
}
