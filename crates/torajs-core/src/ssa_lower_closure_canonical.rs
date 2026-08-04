//! Canonical `__fncell_*` singleton mint for `__forward_*` closures —
//! the RFC 20260717-namedfn-canonical-cell arm carved out of
//! [`crate::ssa_lower_closure::lower`] when the RFC
//! 20260804-fnprops-canonical-cell bind wiring pushed that file past
//! the 500-line cap. Verbatim move; the fresh-mint path (plain arrow /
//! fn-expr evaluations — each a NEW object per spec) stays in the
//! parent.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_closure::{alloc_env, init_env_header};

/// RFC 20260717-namedfn-canonical-cell chunk 1 — a `__forward_*`
/// Closure expr denotes a top-level named fn used as a value, and
/// the ES fn object is a SINGLETON (declaration instantiation
/// creates it once): mint THE cell lazily into the hidden
/// `__fncell_*` slot and answer it from every site, +1 per use
/// (the slot keeps a permanent stake so the cell never hits
/// rc 0). Pre-fix each site minted a fresh env, so `t === u` on
/// two reads of the same fn name answered false and expando
/// writes landed on throwaway cells.
///
/// Answers `None` for every non-canonical shape (not a forwarder, or
/// it carries captures) — the caller keeps its fresh-mint path.
pub(crate) fn try_lower_canonical_cell(
    ctx: &mut LowerCtx<'_>,
    fn_name: &str,
    fid: crate::ssa::FuncId,
    closure_ty: Type,
    has_captures: bool,
) -> Option<Operand> {
    if !fn_name.starts_with("__forward_") || has_captures {
        return None;
    }
    let slot_name = format!("__fncell_{fn_name}");
    let gref = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::GlobalRef(slot_name),
        Type::Ptr,
        None,
    );
    let cached = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(closure_ty, Operand::Value(gref), 0),
        closure_ty,
        None,
    );
    let res_slot = ctx.alloca(closure_ty, Some("__fncell_res"));
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(cached), Operand::Value(res_slot), 0),
    );
    let is_null = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::ICmp(IPred::Eq, Operand::Value(cached), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let mint_blk = ctx.f.add_block();
    let join_blk = ctx.f.add_block();
    ctx.f.set_term(
        ctx.cur_block,
        Terminator::CondBr {
            cond: Operand::Value(is_null),
            then_blk: mint_blk,
            else_blk: join_blk,
        },
    );
    ctx.cur_block = mint_blk;
    let env_v = alloc_env(ctx, closure_ty, 0, fn_name);
    init_env_header(ctx, env_v, fid, fn_name);
    // RFC 20260804-fnprops-canonical-cell — from this mint on,
    // THE cell's props slot is the fn's one property bag: bind
    // the target's raw fn ptr so the FnSig static spelling
    // (fnprops table) delegates here, and a bag it already
    // filled migrates in. Before the split, `PROTO.type = v`
    // (FnSig spelling → fnprops) and a proto-chain / any-lane
    // read through the cell answered different storage.
    if let Some(target) = fn_name.strip_prefix("__forward_")
        && let Some(&target_fid) = ctx.fn_table.get(target)
    {
        let target_addr =
            ctx.f
                .append_inst(ctx.cur_block, InstKind::FnAddr(target_fid), Type::Ptr, None);
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.fnprops_bind_cell,
                vec![Operand::Value(target_addr), Operand::Value(env_v)],
            ),
        );
    }
    // G2 (rotation 178) — a generator factory's `.prototype` IS
    // its `__Gen_<name>` class proto (what getPrototypeOf(g())
    // answers); define it into the fresh cell's props so every
    // member-get channel hits the ordinary props probe.
    if let Some(tag) = fn_name
        .strip_prefix("__forward_")
        .and_then(|n| ctx.ast.generator_factory_classes.get(n))
        .and_then(|cls| ctx.class_name_to_tag.get(cls))
        .copied()
    {
        let proto_v = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.proto_get,
                vec![Operand::ConstI64(tag as i64)],
            ),
            Type::Any,
            None,
        );
        ctx.f.append_void(
            ctx.cur_block,
            InstKind::Call(
                ctx.intrinsics.closure_install_gen_proto,
                vec![Operand::Value(env_v), Operand::Value(proto_v)],
            ),
        );
    }
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(env_v), Operand::Value(gref), 0),
    );
    ctx.f.append_void(
        ctx.cur_block,
        InstKind::Store(Operand::Value(env_v), Operand::Value(res_slot), 0),
    );
    ctx.f.set_term(ctx.cur_block, Terminator::Br(join_blk));
    ctx.cur_block = join_blk;
    let v = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::Load(closure_ty, Operand::Value(res_slot), 0),
        closure_ty,
        None,
    );
    ctx.emit_rc_inc(Operand::Value(v));
    Some(Operand::Value(v))
}
