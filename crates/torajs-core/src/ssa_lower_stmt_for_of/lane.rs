//! The non-array sources of a `for…of`, each to its own lane.

use super::source_class::src_class_name;
use crate::ast::Stmt;
use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// Whether a `for await` steps its source itself, as an index loop over
/// an array, rather than through the any lane, whose GetIterator (hint
/// async) and Async-from-Sync steps are the runtime's: the one
/// criterion the lowering and the width analysis (`num_width::walk`)
/// read. `is_array`: the source is an array; `iteration_shadowed`
/// (asked of an array only): the module can replace array iteration
/// (`builtin_proto_shadow`).
pub(crate) fn for_await_steps_array(
    is_array: bool,
    iteration_shadowed: impl FnOnce() -> bool,
) -> bool {
    is_array && !iteration_shadowed()
}

/// The non-array sources of [`super::lower`], one lane each — map-likes,
/// user-class iterables, strings, `any`, and every remaining shape
/// boxed into the any lane. Answers `true` when the loop was lowered.
/// Carved out of `lower` when its true extent passed 200 lines (the
/// audit had been reading the `{cname}` / `{src_ty:?}` placeholders in
/// its panic messages as braces — file-size lesson 19).
pub(super) fn lower_non_array_source(
    ctx: &mut LowerCtx,
    src_ref_eid: crate::ast::ExprId,
    src_ptr_op: crate::ssa::Operand,
    src_ty: Type,
    names: (&str, &str),
    body: &Stmt,
    is_await: bool,
) -> bool {
    let (var_name, i_ident) = names;
    if matches!(
        src_ty,
        Type::Map | Type::Set | Type::MapIter | Type::ArrIter
    ) {
        let f64_values = crate::ssa_lower_map_num_lane::is_lane_iter(ctx, src_ref_eid);
        crate::ssa_lower_for_of_map_like::lower_for_of_map_like(
            ctx, src_ptr_op, src_ty, f64_values, var_name, body,
        );
        return true;
    }
    if let Type::Obj(sid) = src_ty {
        let cname = src_class_name(ctx, src_ref_eid);
        if let Some(cname) = cname {
            // RFC 20260920-struct-user-proto K4 — in a program that can
            // re-parent, the instance's @@iterator may come from its user
            // chain; the any lane's GetIterator asks the chain.
            if ctx.ast.program_reparents {
                let boxed = ctx.box_to_any(src_ptr_op);
                crate::ssa_lower_for_of_any_iter::lower(ctx, boxed, var_name, body, is_await);
                return true;
            }
            let iter_fn = format!("__cm_{cname}____sym_Symbol_iterator__");
            if let Some(&iter_fid) = ctx.fn_table.get(&iter_fn) {
                crate::ssa_lower_for_of_iter_protocol::lower_for_of_iter_protocol(
                    ctx, src_ptr_op, iter_fid, var_name, body, &cname,
                );
                return true;
            }
            panic!(
                "ssa-lower: for-of on class `{cname}` requires a `[Symbol.iterator](): SomeIter` method (P5.2 syntax, P5.3 Phase B dispatch) — fn `{iter_fn}` not registered"
            );
        }
        // An inline struct (an object literal that reached a typed
        // slot — a specialized `src: any` param bound to `{
        // [Symbol.iterator]() {…} }`) has no class to look a method
        // up on, but the any lane's runtime GetIterator reads its
        // symbol-keyed field like any other own property, and answers
        // §7.4.3's TypeError for a literal that has none. Boxing is
        // the whole route; before rotation 625 this was a compile
        // refusal ("inline-struct iteration not yet supported").
        let _ = sid;
        let boxed = ctx.box_to_any(src_ptr_op);
        crate::ssa_lower_for_of_any_iter::lower(ctx, boxed, var_name, body, is_await);
        return true;
    }
    if src_ty == Type::Str {
        // The string lane binds a Substr view, which a body write
        // cannot replace; a written loop variable takes the any lane,
        // whose slot owns each character string.
        if crate::ssa_lower_for_of_written::body_writes_var(ctx, body, var_name) {
            let boxed = ctx.box_to_any(src_ptr_op);
            crate::ssa_lower_for_of_any_iter::lower(ctx, boxed, var_name, body, is_await);
        } else {
            crate::ssa_lower_for_of_str::lower(ctx, src_ptr_op, i_ident, var_name, body);
        }
        return true;
    }
    // Any-dynamic-access RFC (20260704) S5+ — `for (x of recv)` where
    // recv is an `any` value routes through the unified runtime
    // iteration protocol (`__torajs_any_iter_next` tag-dispatch over
    // strings / arrays / MapIter / ArrIter; catchable TypeError for
    // non-iterable receivers). See sibling ssa_lower_for_of_any_iter.
    if src_ty == Type::Any {
        crate::ssa_lower_for_of_any_iter::lower(ctx, src_ptr_op, var_name, body, is_await);
        return true;
    }
    // Every other shape the compiler can name boxes into the any lane,
    // which already answers it: GetIterator on a non-iterable is a
    // TypeError (§7.4.3), and the lane raises exactly that for a
    // number, a boolean, a Date, a function … Before this the typed
    // spelling was a COMPILE refusal — a specialized body's
    // `for (const q of x)` with `x` bound to `1` said "for-of source
    // type I64 not yet supported" while `const n: any = 1` and the
    // same loop threw. Same posture as the incr ladder (fa111f46e):
    // knowing the shape is not a licence to skip §7.4.3.
    if matches!(
        src_ty,
        Type::I64
            | Type::I32
            | Type::F64
            | Type::Bool
            | Type::FnSig(_)
            | Type::Closure(_)
            | Type::RegExp
            | Type::Date
            | Type::Symbol
            | Type::Promise
            | Type::BigInt
            | Type::WeakRef
            | Type::WeakMap
            | Type::WeakSet
    ) {
        let boxed = ctx.box_to_any(src_ptr_op);
        crate::ssa_lower_for_of_any_iter::lower(ctx, boxed, var_name, body, is_await);
        return true;
    }
    false
}
