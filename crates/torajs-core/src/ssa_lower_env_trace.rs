//! RFC 20260717 closure-env-cycle knife 2 — synthesize the
//! per-closure `__env_trace_<fn>` body: the drop_fn twin the cycle
//! collector enumerates capture-slot children through (CPython
//! `tp_traverse` shape). The compiler's `cap_meta` is the single
//! source of truth for slot kinds, exactly as it is for
//! `synthesize_env_drop` — no second layout ledger.
//!
//! ABI: `__env_trace_<fn>(env: ptr, visit: ptr, ctx: ptr) -> void`
//! where `visit` is `fn(idx: i64, child: ptr, slot_addr: ptr,
//! ctx: ptr) -> void`. For every capture slot that can sit on a
//! reference cycle the body calls `visit` with:
//!
//! - `idx` — the capture's declaration-order index (diagnostics).
//! - `child` — the raw 8-byte slot payload. For an `Any` slot this
//!   is the NaN-box bit pattern; the COLLECTOR's visit callback owns
//!   the cell-like gate (same contract as `rc_inc` / `value_drop_
//!   heap`), the trace body never filters.
//! - `slot_addr` — the address holding `child`, so collect_white's
//!   edge-break can zero it in place. For a capture-box slot this is
//!   the box's value-slot pointer (the env slot itself holds the box,
//!   an interior edge the collector must not treat as a cell).
//!
//! Which slots trace ([`cap_is_traceable`]):
//!
//! - byref Copy box — scalar payload, no heap edge: skip.
//! - byref non-Copy / Any box (RFC 20260710 promoted) — the box owns
//!   heap content: trace THROUGH the box (child = box payload,
//!   slot_addr = box value-slot).
//! - by-value `Substr` — view cell without a universal header whose
//!   parent chain is Str-only (leaves): a cycle can never pass
//!   through it, and the collector could not read its header. Skip.
//! - by-value any other non-Copy — direct cell edge: trace.
//!
//! The `+24` props dynobj is NOT traced here — it sits at a fixed
//! offset every closure shares, so the collector's Closure arm reads
//! it directly (knife 3), keeping trace bodies capture-only.
//!
//! Closures where no capture traces keep `trace_fn = 0` in the env
//! (the construction site decides — [`crate::ssa_lower_closure`]);
//! the collector treats 0 as a leaf without the indirect call. The
//! pre-registered FuncId still gets this (empty) body so the module
//! stays well-formed.

use crate::ssa::{self, BinOp, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::CLOSURE_CAP_BASE_OFF;

/// True iff a capture slot of this shape can hold a heap edge the
/// cycle collector must see. Single predicate shared by the body
/// synthesis and the construction site's store-real-vs-0 decision.
pub(crate) fn cap_is_traceable(ty: &Type, is_byref: bool) -> bool {
    if is_byref {
        !ty.is_copy()
    } else {
        !ty.is_copy() && *ty != Type::Substr
    }
}

/// Build the per-closure env-trace SSA fn. `visit_sig` is the
/// interned `(i64, ptr, ptr, ptr) -> void` callback signature the
/// `CallIndirect`s dispatch through.
pub(crate) fn synthesize_env_trace(
    name: &str,
    cap_meta: &[(Type, bool)],
    visit_sig: ssa::SigId,
) -> ssa::Function {
    let mut f = ssa::Function::new(name, Type::Void);
    let env_pid = f.add_param(Type::Ptr, "env");
    let visit_pid = f.add_param(Type::Ptr, "visit");
    let ctx_pid = f.add_param(Type::Ptr, "ctx");
    let entry = f.add_block();
    let env_op = Operand::Value(env_pid);
    for (i, (cap_ty, is_byref)) in cap_meta.iter().enumerate() {
        if !cap_is_traceable(cap_ty, *is_byref) {
            continue;
        }
        let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
        let (child_v, slot_addr_v) = if *is_byref {
            // The env slot holds the box's value-slot pointer
            // (= box_base + 8): it doubles as the child slot addr.
            let box_ptr = f.append_inst(
                entry,
                InstKind::Load(Type::Ptr, env_op, offset),
                Type::Ptr,
                None,
            );
            let child = f.append_inst(
                entry,
                InstKind::Load(Type::Ptr, Operand::Value(box_ptr), 0),
                Type::Ptr,
                None,
            );
            (child, box_ptr)
        } else {
            let env_i = f.append_inst(entry, InstKind::PtrToInt(env_op), Type::I64, None);
            let addr_i = f.append_inst(
                entry,
                InstKind::BinOp(
                    BinOp::Add,
                    Operand::Value(env_i),
                    Operand::ConstI64(offset as i64),
                ),
                Type::I64,
                None,
            );
            let addr = f.append_inst(
                entry,
                InstKind::IntToPtr(Operand::Value(addr_i)),
                Type::Ptr,
                None,
            );
            let child = f.append_inst(
                entry,
                InstKind::Load(Type::Ptr, env_op, offset),
                Type::Ptr,
                None,
            );
            (child, addr)
        };
        f.append_void(
            entry,
            InstKind::CallIndirect(
                visit_sig,
                Operand::Value(visit_pid),
                vec![
                    Operand::ConstI64(i as i64),
                    Operand::Value(child_v),
                    Operand::Value(slot_addr_v),
                    Operand::Value(ctx_pid),
                ],
            ),
        );
    }
    f.set_term(entry, Terminator::Ret(None));
    f
}
