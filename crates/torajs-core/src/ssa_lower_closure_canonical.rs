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
    // Rotation 599 — `__fwdrecv_` joins `__forward_` here. Both are
    // per-NAME shells (`namedfn_recv_cb::synthesize_one` checks for an
    // existing decl before minting), but only the plain one answered a
    // canonical cell, so two receiver-claimed sites of one function
    // minted two objects: `o.m = t; p.m = t; o.m === p.m` was false.
    // The shells still differ from each other — that cross-shell merge
    // is RFC §8 residue 1 and is NOT this — but a shell must at least
    // be one object per declaration (ES 10.2.11).
    if !crate::ast::is_shell(fn_name) || has_captures {
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
    if let Some(target) = crate::ast::shell_target(fn_name)
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
    if let Some(tag) = crate::ast::shell_target(fn_name)
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

/// The fn object of the top-level function `eid` names, when the bare
/// function (`val_ty`, a `FnSig`) is handed on as a value of the
/// closure lane: the canonical cell, +1 for the receiver. `None` for
/// any other value, or when no `__forward_*` shell was synthesized for
/// the function.
pub(crate) fn named_fn_owned_cell(
    ctx: &mut LowerCtx<'_>,
    eid: crate::ast::ExprId,
    val_ty: Type,
) -> Option<Operand> {
    let ast = ctx.ast;
    let crate::ast::Expr::Ident(name) = ast.get_expr(eid) else {
        return None;
    };
    if !matches!(val_ty, Type::FnSig(_)) || ctx.locals.contains_key(name) {
        return None;
    }
    let fwd = format!("__forward_{name}");
    let fid = ctx.fn_table.get(&fwd).copied()?;
    let closure_ty = crate::ssa_lower_closure::closure_value_ty(ctx, &fwd);
    try_lower_canonical_cell(ctx, &fwd, fid, closure_ty, false)
}

/// [`named_fn_owned_cell`] for a site that joins the bare function
/// into the any lane (`(flip && inc) || h`, `c ? 0 : f`): the cell
/// borrowed like the binding it stands for, since its hidden slot
/// keeps a permanent stake.
pub(crate) fn named_fn_cell(
    ctx: &mut LowerCtx<'_>,
    eid: crate::ast::ExprId,
    val_ty: Type,
) -> Option<Operand> {
    let cell = named_fn_owned_cell(ctx, eid, val_ty)?;
    let closure_ty = ctx.operand_ty(&cell);
    ctx.emit_drop_value(cell, closure_ty);
    Some(cell)
}

/// Lower the arm `eid` of a choice whose join slot is `slot_ty`. A
/// named function joining a closure slot (`slot ?? inc`, `slot ||
/// inc`) is its fn object there ([`named_fn_cell`]): the bare address
/// is not a cell.
pub(crate) fn lower_slot_arm(
    ctx: &mut LowerCtx<'_>,
    slot_ty: Type,
    eid: crate::ast::ExprId,
) -> Operand {
    let v = ctx.lower_expr(eid);
    if !matches!(slot_ty, Type::Closure(_)) {
        return v;
    }
    let ty = ctx.operand_ty(&v);
    named_fn_cell(ctx, eid, ty).unwrap_or(v)
}
