//! A bare read of a name that only a LATER eval / Script declares —
//! `EvalVar` / `ScriptVar` in `Ast::script_global_var_names`, the
//! flavours whose property binds where the text runs (RFC
//! 20260912-global-var-cell knife 4) rather than in the prelude.
//!
//! Until that text has run the name has no declarative binding at all
//! (§16.1.7 / §19.2.1.3 instantiate it then), so §9.1.1.4
//! GetBindingValue resolves it on the OBJECT half of the global
//! record: `Object.defineProperty(this, "f", {value: "x"})` followed
//! by a Script that declares `f` makes every `f` between the two read
//! `"x"`. The compiled program keeps one slot for the name from the
//! start, and every read loads it; so before the load the slot is
//! refreshed from the object's own property (`__torajs_global_var_sync`
//! — an accessor's getter runs, and may throw). Once the text has
//! bound the property it is a cell over this very slot and the kernel
//! answers at once; an absent property leaves the slot as it is.
//!
//! A written `var` or a function binding is never here: their
//! property exists from instantiation over the slot itself.

use crate::ssa::{InstKind, Operand, ValueId};
use crate::ssa_lower::LowerCtx;

pub(crate) fn emit_sync(ctx: &mut LowerCtx<'_>, name: &str, slot: ValueId) {
    if !matches!(
        ctx.ast.script_global_var_names.get(name),
        Some(crate::ast::ScriptGlobalVar::EvalVar | crate::ast::ScriptGlobalVar::ScriptVar)
    ) {
        return;
    }
    let key = ctx.intern_string_literal(name);
    let sync = ctx.intrinsics.global_var_sync;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(sync, vec![Operand::Value(key), Operand::Value(slot)]),
    );
    ctx.emit_throw_check(None);
}
