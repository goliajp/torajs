//! The head's own bindings around the initializer: which of them get
//! per-iteration copies, and which a closure the initializer makes
//! keeps in its box.

use crate::ast::{ExprId, Stmt};
use crate::ssa::{InstKind, Operand};
use crate::ssa_lower::{LocalInfo, LowerCtx};
use crate::ssa_lower_closure_captures::captures_any;

/// The head names a closure in the loop captures, and the lexical ones
/// (`let` or `const`) a closure the initializer makes captures, which
/// keep their box through the initializer.
pub(in crate::ssa_lower_stmt_for) fn per_iter_candidates(
    ctx: &LowerCtx,
    init: Option<&Stmt>,
) -> (Vec<String>, Vec<String>) {
    fn decls<'a>(s: &'a Stmt, out: &mut Vec<(bool, &'a String, ExprId)>) {
        match s {
            Stmt::LetDecl {
                is_var, name, init, ..
            } => out.push((!*is_var, name, *init)),
            Stmt::Multi(v) => v.iter().for_each(|s| decls(s, out)),
            _ => {}
        }
    }
    let mut head = Vec::new();
    init.into_iter().for_each(|s| decls(s, &mut head));
    let captured: Vec<String> = head
        .iter()
        .filter(|d| ctx.escape_captured_lets.contains(d.1))
        .map(|d| d.1.clone())
        .collect();
    let captured_by_init = |name: &str| head.iter().any(|e| captures_any(ctx.ast, e.2, &[name]));
    let in_init = head
        .iter()
        .filter(|d| d.0 && captured_by_init(d.1))
        .map(|d| d.1.clone())
        .collect();
    (captured, in_init)
}

/// After the initializer ran: a binding the initializer's closures
/// captured lives in their box, which stays theirs. The loop starts
/// from a plain slot holding its value.
pub(in crate::ssa_lower_stmt_for) fn detach_init_boxes(ctx: &mut LowerCtx, in_init: &[String]) {
    for name in in_init {
        let Some(info) = ctx.locals.get(name).copied() else {
            continue;
        };
        if !info.boxed {
            continue;
        }
        let v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Load(info.ty, Operand::Value(info.slot), 0),
            info.ty,
            None,
        );
        if !info.ty.is_copy() {
            ctx.emit_owned_result_inc(Operand::Value(v), info.ty);
        }
        let slot = ctx.binding_slot_alloca(info.ty, name);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
        );
        let drop_fid = ctx.capture_box_drop_fid(info.ty);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(drop_fid, vec![Operand::Value(info.slot)]),
        );
        if let Some(ann) = ctx.let_anns.get(&info.slot).cloned() {
            ctx.let_anns.insert(slot, ann);
        }
        ctx.boxed_noncopy_lets.remove(name);
        ctx.locals.insert(
            name.clone(),
            LocalInfo {
                slot,
                ty: info.ty,
                moved: false,
                borrowed: false,
                boxed: false,
                scope_depth: info.scope_depth,
            },
        );
    }
}
