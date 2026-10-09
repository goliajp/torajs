//! The ident reads that go through the GLOBAL OBJECT rather than
//! through a binding or a compile-time fold — [`crate::ssa_lower_ident`]'s
//! two object-backed arms, split out when that file reached its size
//! limit.
//!
//! Both end in the same `__torajs_global_ident_get` kernel and differ
//! only in why they are reached: one is a name no declaration binds,
//! the other a name whose fold this module's own write invalidated.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// RFC 20260730-undeclared-ident — §6.2.5.5 GetValue on an
/// unresolvable Reference. The checker marked this read
/// (`ast.undeclared_reads`), so evaluating it raises a catchable
/// ReferenceError (`<name> is not defined`). The value lane past the
/// throw-check is unreachable; `undefined`'s shape stands in so the
/// enclosing expression still types out.
pub(crate) fn try_undeclared_read_throw(
    ctx: &mut LowerCtx<'_>,
    eid: crate::ast::ExprId,
    name: &str,
) -> Option<Operand> {
    if !ctx.ast.undeclared_reads.contains_key(&eid) {
        return None;
    }
    let name_str = ctx.intern_string_literal(name);
    // §9.1.1.4 GetBindingValue with `S` = false — the global
    // environment record's OBJECT half answers first, and only a name
    // the global object does not carry reaches the ReferenceError.
    // `this.p = 1` in global code writes through that object
    // (`ast::toplevel_this` binds top-level `this` to the singleton
    // under the sloppy goal), so `p` reads it back; the kernel records
    // the throw rather than raising it, which is what lets one call
    // site serve both outcomes.
    let getter = ctx.intrinsics.global_ident_get;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(getter, vec![Operand::Value(name_str)]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Some(Operand::Value(v))
}

/// §9.1.1.4 through the global environment record's OBJECT half,
/// for a name whose fold this module's own write invalidated. Shares
/// the kernel with [`try_undeclared_read_throw`]; the throw it can
/// record is unreachable here, since the write put the name there.
pub(crate) fn emit_global_ident_read(ctx: &mut LowerCtx<'_>, name: &str) -> Operand {
    let name_str = ctx.intern_string_literal(name);
    let getter = ctx.intrinsics.global_ident_get;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(getter, vec![Operand::Value(name_str)]),
        Type::Any,
        None,
    );
    ctx.emit_throw_check(None);
    Operand::Value(v)
}

/// §9.1.1.1 HasBinding — a declarative record between this site and
/// the global one (a local, a parameter, a capture) or the global
/// record's own declarative half (a top-level function or binding)
/// answers before the global object does. A builtin-name lane that
/// folds the global object's property stands down whenever this
/// answers true.
pub(crate) fn user_binds(ctx: &LowerCtx<'_>, name: &str) -> bool {
    ctx.locals.contains_key(name)
        || ctx.fn_table.contains_key(name)
        || ctx.globals.contains_key(name)
        || crate::ast::toplevel_binds(ctx.ast, name)
}
