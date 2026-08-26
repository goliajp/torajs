//! Operand / `InstKind` rewriter — shared between `splice` (Phase 1.0a
//! single-block leaf) and `splice2` (Phase 2.0a multi-block leaf).
//!
//! Both splice implementations build a `HashMap<ValueId, Operand>`
//! mapping callee values onto caller-side operands (callee parameters
//! → caller-supplied `args`, non-parameter callee values → fresh
//! caller `ValueId`s) and rewrite every cloned callee instruction
//! through it. The rewriter itself is operand-graph agnostic and lives
//! here so both files can share one exhaustive `match` against
//! `torajs-core::ssa::InstKind`. New SSA opcodes added upstream are a
//! compile-time signal in this single location.

use std::collections::HashMap;
use torajs_core::ssa::{InstKind, Operand, ValueId};

/// Rewrite a single `Operand` through the callee→caller mapping.
/// Value operands map either to a fresh caller `ValueId` (non-param
/// callee value) or to the caller-supplied arg (param callee value).
/// All non-Value operands (constants, etc.) pass through verbatim.
pub(super) fn rewrite_operand(op: &Operand, map: &HashMap<ValueId, Operand>) -> Operand {
    match op {
        Operand::Value(v) => map.get(v).cloned().unwrap_or_else(|| op.clone()),
        _ => op.clone(),
    }
}

/// Exhaustively rewrite every Operand inside an `InstKind`. The match
/// is intentionally non-`_` so adding a new `InstKind` variant in
/// `torajs-core::ssa` is a compile-time signal here — we want explicit
/// review of how operands are routed for every new SSA opcode rather
/// than silently no-oping it.
pub(super) fn rewrite_inst_kind(kind: &InstKind, map: &HashMap<ValueId, Operand>) -> InstKind {
    let r = |o: &Operand| rewrite_operand(o, map);
    match kind {
        InstKind::BinOp(op, a, b) => InstKind::BinOp(*op, r(a), r(b)),
        InstKind::ICmp(p, a, b) => InstKind::ICmp(*p, r(a), r(b)),
        InstKind::FCmp(p, a, b) => InstKind::FCmp(*p, r(a), r(b)),
        InstKind::Call(fid, args) => InstKind::Call(*fid, args.iter().map(r).collect()),
        InstKind::CallIndirect(sig, ptr, args) => {
            InstKind::CallIndirect(*sig, r(ptr), args.iter().map(r).collect())
        }
        InstKind::Alloca(ty) => InstKind::Alloca(ty.clone()),
        InstKind::AllocaBytes(n) => InstKind::AllocaBytes(*n),
        InstKind::Load(ty, ptr, off) => InstKind::Load(ty.clone(), r(ptr), *off),
        InstKind::Store(val, ptr, off) => InstKind::Store(r(val), r(ptr), *off),
        InstKind::LoadDyn(ty, ptr, off) => InstKind::LoadDyn(ty.clone(), r(ptr), r(off)),
        InstKind::StoreDyn(val, ptr, off) => InstKind::StoreDyn(r(val), r(ptr), r(off)),
        InstKind::LoadDynScaled8(ty, ptr, idx) => InstKind::LoadDynScaled8(*ty, r(ptr), r(idx)),
        InstKind::LoadU8Dyn(ptr, idx) => InstKind::LoadU8Dyn(r(ptr), r(idx)),
        InstKind::StoreDynScaled8(val, ptr, idx) => {
            InstKind::StoreDynScaled8(r(val), r(ptr), r(idx))
        }
        InstKind::SiToFp(o) => InstKind::SiToFp(r(o)),
        InstKind::FpToSi(o) => InstKind::FpToSi(r(o)),
        InstKind::ZExtBoolToI64(o) => InstKind::ZExtBoolToI64(r(o)),
        InstKind::ZExtI32ToI64(o) => InstKind::ZExtI32ToI64(r(o)),
        InstKind::BitCastF64ToI64(o) => InstKind::BitCastF64ToI64(r(o)),
        InstKind::BitCastI64ToF64(o) => InstKind::BitCastI64ToF64(r(o)),
        InstKind::IntToPtr(o) => InstKind::IntToPtr(r(o)),
        InstKind::PtrToInt(o) => InstKind::PtrToInt(r(o)),
        InstKind::TruncI64ToBool(o) => InstKind::TruncI64ToBool(r(o)),
        InstKind::StringRef(id) => InstKind::StringRef(*id),
        InstKind::StaticStrRef(id) => InstKind::StaticStrRef(*id),
        InstKind::GlobalRef(name) => InstKind::GlobalRef(name.clone()),
        InstKind::FnAddr(fid) => InstKind::FnAddr(*fid),
        InstKind::BoxedEntryAddr(fid) => InstKind::BoxedEntryAddr(*fid),
        InstKind::Identity(o) => InstKind::Identity(r(o)),
        InstKind::Neg(o) => InstKind::Neg(r(o)),
        InstKind::Ctpop(o) => InstKind::Ctpop(r(o)),
        InstKind::Copy(_, _) => unreachable!(
            "InstKind::Copy reached the inliner — mem2reg's φ destruction \
             introduces Copy only after the egraph pass, which runs after \
             all inline rounds"
        ),
        InstKind::Select(_, _, _, _) => unreachable!(
            "InstKind::Select reached the inliner — select formation \
             introduces Select only after the egraph pass, which runs \
             after all inline rounds"
        ),
        InstKind::CtpopRangeSum(_, _, _) => unreachable!(
            "InstKind::CtpopRangeSum reached the inliner — ctpop-range-sum \
             formation introduces it only after the egraph pass, which \
             runs after all inline rounds"
        ),
    }
}
