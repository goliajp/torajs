//! A bare read of a name that only an eval / Script declares —
//! `EvalVar` / `ScriptVar` in `Ast::script_global_var_names`, the
//! flavours whose property binds where the text runs (RFC
//! 20260912-global-var-cell knife 4) rather than in the prelude, and
//! the hidden slot of an `EvalFn` function of such a text, which every
//! read of the function's global binding names
//! (`desugar_eval::bound_fns`).
//!
//! Until that text has run the name has no declarative binding at all
//! (§16.1.7 / §19.2.1.3 instantiate it then), so §9.1.1.4
//! GetBindingValue resolves it on the OBJECT half of the global
//! record: `Object.defineProperty(this, "f", {value: "x"})` followed
//! by a Script that declares `f` makes every `f` between the two read
//! `"x"`, and with no such property the read is a ReferenceError
//! (`typeof` answers `"undefined"`). The compiled program keeps one
//! slot for the name from the start, and every read loads it; so
//! before the load the slot is refreshed from the object's own
//! property (`__torajs_global_var_sync` — an accessor's getter runs,
//! and may throw, and an absent property throws). Once the text has
//! bound the property it is a cell over this very slot and the kernel
//! answers at once.
//!
//! A written `var` or a program function is never here: their
//! property exists from instantiation over the slot itself. Nor is a
//! program without eval / Script text: these flavours come only from
//! one, so such a program emits nothing extra.

use crate::ast::ScriptGlobalVar;
use crate::ssa::{InstKind, Operand, Type, ValueId};
use crate::ssa_lower::LowerCtx;

pub(crate) fn emit_sync(
    ctx: &mut LowerCtx<'_>,
    eid: crate::ast::ExprId,
    name: &str,
    slot: ValueId,
) {
    let Some(key) = property_name(ctx, name) else {
        return;
    };
    // The `x = x` that creates the binding reads the slot before the
    // property exists (`var_hoist_eval_sites::mark_eval_var_sites`).
    if ctx.ast.eval_var_sites.contains_key(&eid) {
        return;
    }
    let key = ctx.intern_string_literal(key);
    let sync = ctx.intrinsics.global_var_sync;
    let cur_block = ctx.cur_block;
    ctx.f.append_void(
        cur_block,
        InstKind::Call(sync, vec![Operand::Value(key), Operand::Value(slot)]),
    );
    ctx.emit_throw_check(None);
}

/// `typeof` on one of these names: the object answers, absent or not
/// (§13.5.3 step 2.a), through the kernel an unresolvable name uses.
pub(crate) fn try_typeof(ctx: &mut LowerCtx<'_>, name: &str) -> Option<Operand> {
    if ctx.locals.contains_key(name) || !ctx.globals.contains_key(name) {
        return None;
    }
    let key = property_name(ctx, name)?;
    let key = ctx.intern_string_literal(key);
    let f = ctx.intrinsics.global_ident_typeof;
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Call(f, vec![Operand::Value(key)]),
        Type::Str,
        None,
    );
    ctx.emit_throw_check_owned(None, Operand::Value(v), Type::Str);
    Some(Operand::Value(v))
}

/// The global object's property a read of this slot resolves through,
/// for the names here.
fn property_name<'n>(ctx: &LowerCtx<'_>, name: &'n str) -> Option<&'n str> {
    match ctx.ast.script_global_var_names.get(name)? {
        ScriptGlobalVar::EvalVar | ScriptGlobalVar::ScriptVar => Some(name),
        ScriptGlobalVar::EvalFn => {
            name.strip_prefix(crate::ast::script_global_fns::SCRIPT_GLOBAL_FN_SLOT_PREFIX)
        }
        _ => None,
    }
}
