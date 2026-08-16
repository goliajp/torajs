//! RFC 20260816-headless-argv-face — the head-less callee's
//! synthetic leading slots, split out of `ssa_lower_call_terminal`
//! when the argv packer pushed that file past the 500-line limit
//! (verbatim move).
//!
//! A head-less body (no `__env` / `__this` head) carries what the
//! env-first and this-first tiers carry behind their head param: the
//! H1 hidden I64 argc, and — since the argv face — the raw argv
//! pointer after it. Neither is visible in the AST arg list, so the
//! terminal shifts every sig-aligned read past them and prepends
//! their operands last. This module owns that whole channel: which
//! slots a callee wants, what count they describe, and how the
//! caller fills the pointer.

use crate::ast::ExprId;
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

/// H1 — 1 when the callee is a head-less T-31 body (hidden argc at
/// sig position 0), 0 otherwise. Keyed by name — the mono retarget
/// name when the checker recorded one (clones are mirrored into the
/// side table), the direct Ident otherwise. Non-Ident callees can't
/// reach a head-less body (value escapes ride the `__forward_` relay,
/// itself an env-first closure).
/// `__forward_<f>` is a TRANSPARENT relay minted for a value escape
/// of `f`; when its body's forwarding call reaches a head-less
/// callee, the hidden argc slot must carry the relay's OWN runtime
/// argc (the S1 hidden param every env-first closure receives), not
/// the relay's declared arity — `(f as any)(1, 2, 3)` reaches `f`
/// through the relay and `arguments.length` must answer 3. Any
/// other caller keeps the static count (`args.len()`).
pub(crate) fn forwarded_argc(ctx: &mut LowerCtx<'_>, callee: ExprId) -> Option<Operand> {
    let crate::ast::Expr::Ident(callee_name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if ctx.f.name.strip_prefix("__forward_") != Some(callee_name.as_str()) {
        return None;
    }
    let info = ctx.locals.get("__torajs_argc")?;
    let (slot, ty) = (info.slot, info.ty);
    let cur_block = ctx.cur_block;
    let v = ctx.f.append_inst(
        cur_block,
        InstKind::Load(ty, Operand::Value(slot), 0),
        ty,
        None,
    );
    Some(Operand::Value(v))
}

/// The head-less callee's synthetic leading slots: the H1 hidden
/// argc, and — since RFC 20260816-headless-argv-face — the raw argv
/// pointer that follows it. Neither is visible in the AST arg list,
/// so both shift every sig-aligned read and both get prepended at
/// the very end of the emit.
#[derive(Clone, Copy)]
pub(crate) struct HeadlessSlots {
    pub(crate) argc: bool,
    pub(crate) argv: bool,
}

impl HeadlessSlots {
    pub(crate) fn width(self) -> usize {
        usize::from(self.argc) + usize::from(self.argv)
    }
}

pub(crate) fn headless_slots(ctx: &LowerCtx<'_>, eid: ExprId, callee: ExprId) -> HeadlessSlots {
    let name = if let Some(n) = ctx.call_retargets.get(&eid) {
        n.as_str()
    } else if let crate::ast::Expr::Ident(n) = ctx.ast.get_expr(callee) {
        n.as_str()
    } else {
        return HeadlessSlots {
            argc: false,
            argv: false,
        };
    };
    HeadlessSlots {
        argc: ctx.ast.headless_argc_fns.contains(name),
        argv: ctx.ast.headless_argv_fns.contains(name),
    }
}

/// The count of arguments the SOURCE wrote at this call, which is
/// what `arguments` describes — `apply_default_args` may have
/// appended slots holding the callee's own declared defaults.
pub(crate) fn user_argc(ctx: &LowerCtx<'_>, eid: ExprId, lowered: usize) -> usize {
    ctx.ast
        .default_padded_argc
        .get(&eid)
        .copied()
        .unwrap_or(lowered)
}

/// RFC 20260816-headless-argv-face — box the call's argument
/// operands into a stack `AnyValue[]` and answer its pointer, the
/// value that rides the callee's synthetic `__torajs_argv` param.
///
/// The slots are BORROWS, matching the boxed adapter's ledger: the
/// body's `__torajs_arguments_materialize` expansion incs every heap
/// cell it stores into the array, and each operand outlives the call
/// (owned temps release only after it). So nothing is inc'd here and
/// nothing needs releasing.
///
/// The buffer allocas in the entry block (mirroring `pack_any_argv`)
/// so a call inside a loop reuses one slab instead of growing the
/// frame per iteration.
pub(crate) fn pack_headless_argv(
    ctx: &mut LowerCtx<'_>,
    eid: ExprId,
    args: &[ExprId],
    ops: &[Operand],
) -> Operand {
    let n = user_argc(ctx, eid, args.len()).min(ops.len());
    let buf = ctx.f.append_inst(
        crate::ssa::BlockId(0),
        InstKind::AllocaBytes((n.max(1) * 8) as u64),
        Type::Ptr,
        Some("__hl_argv"),
    );
    for i in 0..n {
        let op = ops[i].clone();
        let slot = if ctx.operand_ty(&op) == Type::Any {
            op
        } else {
            ctx.box_to_any_from_expr(args[i], op)
        };
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Store(slot, Operand::Value(buf), (i * 8) as u64),
        );
    }
    Operand::Value(buf)
}
