//! The per-argument loop of [`super::general_call`], moved out of it
//! whole: each argument against its parameter, with the route the
//! call takes when it is admitted.

use super::arg_admitted::arg_admitted;
use crate::ast::{Ast, ExprId};
use crate::check::{Checker, Type};
use crate::check_stmt_let_decl::held_any::face as held_face;

/// Checks every argument against its parameter. `Some(boxed)` says
/// whether the call rides the callee's boxed entry; `None` means a
/// callback slot recorded its runtime throw, and the call answers its
/// return type as is.
pub(super) fn check(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    params: &[Type],
    args: &[ExprId],
) -> Result<Option<bool>, String> {
    // M5.1 class-method dispatch flag derived in
    // [`crate::check_type_of_call_dispatch_flags`] (chunk 306;
    // M6.1 String borrow flag pruned in chunk 311).
    let is_class_method = crate::check_type_of_call_dispatch_flags::derive(ast, callee);
    let mut boxed_entry = false;
    for (i, (param_ty, arg_id)) in params.iter().zip(args.iter()).enumerate() {
        let arg_ty = checker.type_of(ast, *arg_id)?;
        let arg_ty =
            crate::check_assignable::in_declared_order(checker, ast, param_ty, *arg_id, arg_ty);
        let arg_ty = crate::check_type_of_call::source_array_param::face_view(
            checker, ast, *callee, param_ty, &arg_ty,
        )
        .unwrap_or(arg_ty);
        held_face::check_arg(checker, ast, (i, *arg_id), param_ty)?;
        let Some(boxed) = arg_route(
            checker,
            ast,
            eid,
            callee,
            is_class_method,
            i,
            param_ty,
            &arg_ty,
            arg_id,
        ) else {
            // §7.2 middle tier — a callback slot handed a value no
            // type of which is callable. The spec has a runtime
            // answer for that (throw, after the arguments evaluate),
            // so refusing the program here would be the static-subset
            // answer. See `crate::check_type_of_call::callback_slot_throw`.
            if let Some(msg) = crate::check_type_of_call::callback_slot_throw::message_for(
                checker, ast, *callee, param_ty, &arg_ty,
            ) {
                for &rest in &args[i + 1..] {
                    checker.type_of(ast, rest)?;
                }
                checker.spec_throws.insert(eid, msg);
                return Ok(None);
            }
            return Err(format!(
                "argument {i}: expected {param_ty:?}, got {arg_ty:?}"
            ));
        };
        boxed_entry |= boxed;
        crate::iface_witness::note_flow(param_ty, &arg_ty, true);
        // TS-shape: function parameters SHARE non-Copy args —
        // calling `f(x)` never marks `x` moved. The consuming-
        // params bitmap retired in chunk 568: every store lane
        // takes its own +1 (chunks 564-567), so the historical
        // caller-side consume double-counted into a leak.
        //
        // Rotation 412 — TS contextual typing for a literal Array
        // ARGUMENT: once admitted, its recorded type is the param's
        // (the chunk-702 let-decl story, same helper). Without this
        // a uniform-kind literal against an `any[]` param (the ctor
        // rest relay's `[true]`) minted the typed flavor and the
        // callee's Arr<Any> readers mis-decoded the slots.
        crate::check_stmt_let_decl::apply_contextual_array_ann(checker, ast, *arg_id, param_ty);
    }
    Ok(Some(boxed_entry))
}

/// Is the argument admitted, and does the call then ride the callee's
/// boxed entry? None refuses it. [`arg_admitted`], then an `any`
/// argument into an object parameter of a function value
/// ([`crate::check_type_of_call::any_struct_slot`]).
#[allow(clippy::too_many_arguments)]
fn arg_route(
    checker: &mut Checker,
    ast: &Ast,
    eid: ExprId,
    callee: &ExprId,
    is_class_method: bool,
    i: usize,
    param_ty: &Type,
    arg_ty: &Type,
    arg_id: &ExprId,
) -> Option<bool> {
    use crate::check_type_of_call::any_struct_slot::{Route, admit};
    if arg_admitted(
        checker,
        ast,
        eid,
        callee,
        is_class_method,
        i,
        param_ty,
        arg_ty,
        arg_id,
    ) {
        return Some(false);
    }
    if is_class_method {
        return None;
    }
    admit(checker, ast, eid, *callee, i, arg_ty, param_ty).map(|r| matches!(r, Route::Boxed))
}
