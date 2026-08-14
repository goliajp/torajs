#![allow(dead_code)]
// mirror ssa_lower.rs — body_has_ident_return_to_global + stmt_has_ident_return live but are not yet wired into effective_ret_ty's mixed-return diagnostic

//! Two small independent helper families extracted from `ssa_lower.rs`
//! chunk 368.
//!
//! - `synthesize_env_drop` — build the per-closure env-drop SSA fn
//!   (T-27 conditional props-dynobj drop + T-15.g.5 Copy-capture
//!   capture_box_drop + `obj_drop_sized` on the env block). Wired into
//!   pass 2.5 via a pre-registered FuncId.
//! - `effective_ret_ty` + `body_has_ident_return_to_global` +
//!   `stmt_has_ident_return` — decide whether a parsed FnSig return-
//!   type annotation must be upgraded to a Closure to match the fn's
//!   actual body (used from `ssa_lower_pass_1` and `ssa_lower_fn`).

use crate::ast::{Ast, Expr, Param, Stmt};
use crate::ssa::{self, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{CLOSURE_CAP_BASE_OFF, CLOSURE_PROPS_OFF, Intrinsics};
use crate::ssa_lower_body_returns_closure::body_returns_closure;

/// Build the per-closure env-drop SSA fn:
///   - `T-27` — conditional drop of the props dynobj (skips the
///     cross-TU `value_drop_heap` call when NULL, the common case).
///   - `T-15.g.5` — Copy captures live in refcounted `capture_box`
///     slots; `capture_box_drop` deallocates when the last capturing
///     closure releases.
///   - RFC 20260705 closure-capture-ownership Phase 1 — non-Copy
///     captures are OWNED by the env (`write_captures` moves them in,
///     marking the outer binding moved): release each one. `Any`
///     slots hold a NaN-box value → `any_box_drop`; `Substr` is a
///     view cell with no universal Tag → `substr_drop` (parent-chain
///     aware); every other non-Copy type is a universal-header cell →
///     `value_drop_heap` (tag-dispatched, rc-aware, NULL/NaN-box
///     gated in the runtime).
///   - Free the env block itself with `obj_drop_sized(env,
///     CLOSURE_CAP_BASE_OFF + N_captures * 8)`.
///
/// All called intrinsics are runtime-provided. The fn signature is
/// `(env: ptr) -> void` and matches the FuncId pre-registered at
/// Pass 1.
pub(crate) fn synthesize_env_drop(
    name: &str,
    cap_meta: &[(Type, bool)],
    intrinsics: &Intrinsics,
) -> ssa::Function {
    let mut f = ssa::Function::new(name, Type::Void);
    let env_pid = f.add_param(Type::Ptr, "env");
    let entry = f.add_block();
    let env_op = Operand::Value(env_pid);
    // RFC 20260717 knife 3 — scrub the cycle root buffer before the
    // env goes away (a buffered candidate that then normal-drops
    // would leave a dangling entry; cheap no-op when FLAG_BUFFERED
    // is clear). Same position as the class-Obj drop scrub.
    f.append_void(
        entry,
        InstKind::Call(intrinsics.cycle_unbuffer, vec![env_op]),
    );
    // T-27 — drop the props dynobj if non-NULL. SSA-side NULL check
    // skips the value_drop_heap call entirely for closures that
    // never had a property write (the common case). Without this,
    // every closure construction pays an extra cross-TU call on
    // drop even when props_dynobj is NULL — measured 5-12% regression
    // on closure-heavy benches (promise-chain-1k, throw-catch-100k).
    let props_v = f.append_inst(
        entry,
        InstKind::Load(Type::Ptr, env_op, CLOSURE_PROPS_OFF),
        Type::Ptr,
        None,
    );
    let props_nonnull = f.append_inst(
        entry,
        InstKind::ICmp(IPred::Ne, Operand::Value(props_v), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let drop_blk = f.add_block();
    let after_props = f.add_block();
    f.set_term(
        entry,
        Terminator::CondBr {
            cond: Operand::Value(props_nonnull),
            then_blk: drop_blk,
            else_blk: after_props,
        },
    );
    f.append_void(
        drop_blk,
        InstKind::Call(intrinsics.value_drop_heap, vec![Operand::Value(props_v)]),
    );
    f.set_term(drop_blk, Terminator::Br(after_props));
    let entry = after_props;
    for (i, (cap_ty, is_byref)) in cap_meta.iter().enumerate() {
        let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
        if *is_byref {
            // T-15.g.5 — the env slot holds a capture-box value-slot
            // pointer (= alloc_base + 8); the box helper steps back
            // to dec the rc and frees on the last release. A Copy
            // box holds a plain value; a promoted non-Copy box (RFC
            // 20260710) owns its heap content — `capture_box_drop_
            // heap` releases the content through the universal drop
            // before freeing the box.
            let slot_ptr = f.append_inst(
                entry,
                InstKind::Load(Type::Ptr, env_op, offset),
                Type::Ptr,
                None,
            );
            let drop_fid = if cap_ty.is_copy() {
                intrinsics.capture_box_drop
            } else if *cap_ty == Type::Any {
                intrinsics.capture_box_drop_any
            } else {
                intrinsics.capture_box_drop_heap
            };
            f.append_void(
                entry,
                InstKind::Call(drop_fid, vec![Operand::Value(slot_ptr)]),
            );
        } else {
            // RFC 20260705 Phase 1 — the env owns its non-Copy
            // captures (construction moved them in; sibling captures
            // took their own +1 stake). Release one reference per
            // slot; shared captures free exactly once, on the last
            // release.
            let (load_ty, drop_fid) = match cap_ty {
                Type::Any => (Type::Any, intrinsics.any_box_drop),
                Type::Substr => (Type::Substr, intrinsics.substr_drop),
                other => (*other, intrinsics.value_drop_heap),
            };
            let cap_v = f.append_inst(
                entry,
                InstKind::Load(load_ty, env_op, offset),
                load_ty,
                None,
            );
            f.append_void(entry, InstKind::Call(drop_fid, vec![Operand::Value(cap_v)]));
        }
    }
    // Free the env block itself. Size = closure header
    // (`CLOSURE_CAP_BASE_OFF`) + N_captures * 8.
    let env_block_size = CLOSURE_CAP_BASE_OFF + (cap_meta.len() as u64) * 8;
    f.append_void(
        entry,
        InstKind::Call(
            intrinsics.obj_drop_sized,
            vec![env_op, Operand::ConstI64(env_block_size as i64)],
        ),
    );
    f.set_term(entry, Terminator::Ret(None));
    f
}

/// If the parsed return type is `Type::FnSig(sig)` and the function's
/// body returns a `Type::Closure` value, upgrade to `Type::Closure(sig)`.
/// Otherwise pass through. Both types share an 8-byte ABI so this is a
/// pure dispatch-discipline change.
///
/// When the body mixes Closure returns with FnSig-shaped returns
/// (bare top-level fn names or non-capturing arrows), we still
/// upgrade to Closure — the caller dispatches via the env's fn_addr —
/// and the Stmt::Return arm in `lower_stmt` wraps each FnSig return
/// in a synthesized forwarder closure (see `synthesize_forwarder` /
/// `wrap_fnsig_into_closure_via_forwarder`).
pub(crate) fn effective_ret_ty(parsed: Type, ast: &Ast, params: &[Param], body: &[Stmt]) -> Type {
    if let Type::FnSig(sig_id) = parsed
        && (body_returns_closure(ast, body)
            // 398-11 — a returned `any` binding is a closure cell
            // whenever it is callable at all (doc on the predicate).
            || crate::ssa_lower_body_returns_closure::body_returns_any_binding(ast, params, body))
    {
        return Type::Closure(sig_id);
    }
    parsed
}

/// True if any `Stmt::Return(Some(<expr>))` in `body` has an Ident
/// expression whose name resolves to a FnSig-shaped FnDecl (not a
/// capturing closure). The set of such "FnSig fns" is every top-level
/// FnDecl whose first parameter is NOT `__env`. Used to detect the
/// "mixed FnSig/Closure return" anti-pattern in `effective_ret_ty`:
/// if the body also returns a capturing arrow (Closure), the two
/// calling conventions clash and we panic with a clear workaround.
///
/// Includes non-capturing lifted closures (`__closure_N` whose lifted
/// FnDecl skips the __env param) — those produce FnSig at runtime
/// even though they originated from `(y) => ...` syntax.
fn body_has_ident_return_to_global(ast: &Ast, body: &[Stmt]) -> bool {
    let fnsig_fns: std::collections::HashSet<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, params, .. } => {
                let is_closure = params.first().is_some_and(|p| p.name == "__env");
                if is_closure { None } else { Some(name.clone()) }
            }
            _ => None,
        })
        .collect();
    body.iter()
        .any(|s| stmt_has_ident_return(ast, s, &fnsig_fns))
}

fn stmt_has_ident_return(ast: &Ast, s: &Stmt, globals: &std::collections::HashSet<String>) -> bool {
    match s {
        Stmt::Return(Some(eid)) => {
            matches!(ast.get_expr(*eid), Expr::Ident(n) if globals.contains(n))
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_has_ident_return(ast, then_branch, globals)
                || else_branch
                    .as_deref()
                    .is_some_and(|s| stmt_has_ident_return(ast, s, globals))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            stmt_has_ident_return(ast, body, globals)
        }
        Stmt::Labeled { body, .. } => stmt_has_ident_return(ast, body, globals),
        Stmt::For { body, .. } => stmt_has_ident_return(ast, body, globals),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_has_ident_return(ast, s, globals))
        }
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| {
                c.body
                    .iter()
                    .any(|s| stmt_has_ident_return(ast, s, globals))
            }) || default
                .as_ref()
                .is_some_and(|d| d.iter().any(|s| stmt_has_ident_return(ast, s, globals)))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_has_ident_return(ast, s, globals))
                || catch_body
                    .iter()
                    .any(|s| stmt_has_ident_return(ast, s, globals))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_has_ident_return(ast, s, globals)))
        }
        _ => false,
    }
}
