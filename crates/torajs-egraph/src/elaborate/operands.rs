//! Operand canonicalisation: every operand mapped to its e-class
//! representative.

use crate::egraph::Egraph;
use torajs_core::ssa::{InstKind, Operand};

/// Rewrite every `Operand::Value(v)` operand of an `InstKind` through
/// `egraph.opt_value(v)`. Non-Value operands (constants) pass through.
///
/// This is the inverse-of-clone path: every `InstKind` variant carrying
/// an SSA value is enumerated explicitly so newly added variants don't
/// silently miss canonicalisation. Variants with no SSA-value payload
/// fall to the catch-all clone arm.
pub(super) fn canonicalize_operands(kind: &InstKind, egraph: &mut Egraph) -> InstKind {
    match kind {
        InstKind::BinOp(op, l, r) => {
            let l = map_operand(l, egraph);
            let r = map_operand(r, egraph);
            InstKind::BinOp(*op, l, r)
        }
        InstKind::ICmp(p, l, r) => {
            let l = map_operand(l, egraph);
            let r = map_operand(r, egraph);
            InstKind::ICmp(*p, l, r)
        }
        InstKind::FCmp(p, l, r) => {
            let l = map_operand(l, egraph);
            let r = map_operand(r, egraph);
            InstKind::FCmp(*p, l, r)
        }
        InstKind::Call(fid, args) => {
            let new_args: Vec<Operand> = args.iter().map(|a| map_operand(a, egraph)).collect();
            InstKind::Call(*fid, new_args)
        }
        InstKind::CallIndirect(sig, ptr, args) => {
            let new_ptr = map_operand(ptr, egraph);
            let new_args: Vec<Operand> = args.iter().map(|a| map_operand(a, egraph)).collect();
            InstKind::CallIndirect(*sig, new_ptr, new_args)
        }
        InstKind::Load(ty, addr, off) => InstKind::Load(*ty, map_operand(addr, egraph), *off),
        InstKind::RcInc(p) => InstKind::RcInc(map_operand(p, egraph)),
        InstKind::RcIncAny(v) => InstKind::RcIncAny(map_operand(v, egraph)),
        InstKind::BoxAny(t, v) => InstKind::BoxAny(*t, map_operand(v, egraph)),
        InstKind::AnyIsNumber(v) => InstKind::AnyIsNumber(map_operand(v, egraph)),
        InstKind::AnyIsInt32(v) => InstKind::AnyIsInt32(map_operand(v, egraph)),
        InstKind::AnyToInt32(v) => InstKind::AnyToInt32(map_operand(v, egraph)),
        InstKind::AnyToF64(v, f) => {
            InstKind::AnyToF64(map_operand(v, egraph), map_operand(f, egraph))
        }
        InstKind::BoxInt32(v) => InstKind::BoxInt32(map_operand(v, egraph)),
        InstKind::BoxF64(v) => InstKind::BoxF64(map_operand(v, egraph)),
        InstKind::RcDecAny(v) => InstKind::RcDecAny(map_operand(v, egraph)),
        InstKind::Store(val, addr, off) => {
            InstKind::Store(map_operand(val, egraph), map_operand(addr, egraph), *off)
        }
        InstKind::LoadDyn(ty, addr, off) => {
            InstKind::LoadDyn(*ty, map_operand(addr, egraph), map_operand(off, egraph))
        }
        InstKind::StoreDyn(val, addr, off) => InstKind::StoreDyn(
            map_operand(val, egraph),
            map_operand(addr, egraph),
            map_operand(off, egraph),
        ),
        InstKind::LoadDynScaled8(ty, addr, idx) => {
            InstKind::LoadDynScaled8(*ty, map_operand(addr, egraph), map_operand(idx, egraph))
        }
        InstKind::LoadU8Dyn(addr, idx) => {
            InstKind::LoadU8Dyn(map_operand(addr, egraph), map_operand(idx, egraph))
        }
        InstKind::LoadU16Dyn(addr, idx) => {
            InstKind::LoadU16Dyn(map_operand(addr, egraph), map_operand(idx, egraph))
        }
        InstKind::StoreDynScaled8(val, addr, idx) => InstKind::StoreDynScaled8(
            map_operand(val, egraph),
            map_operand(addr, egraph),
            map_operand(idx, egraph),
        ),
        InstKind::SiToFp(v) => InstKind::SiToFp(map_operand(v, egraph)),
        InstKind::FpToSi(v) => InstKind::FpToSi(map_operand(v, egraph)),
        InstKind::FpToInt32(v) => InstKind::FpToInt32(map_operand(v, egraph)),
        InstKind::ZExtBoolToI64(v) => InstKind::ZExtBoolToI64(map_operand(v, egraph)),
        InstKind::ZExtI32ToI64(v) => InstKind::ZExtI32ToI64(map_operand(v, egraph)),
        InstKind::Identity(v) => InstKind::Identity(map_operand(v, egraph)),
        InstKind::Neg(v) => InstKind::Neg(map_operand(v, egraph)),
        InstKind::Ctpop(v) => InstKind::Ctpop(map_operand(v, egraph)),
        InstKind::Cttz(v) => InstKind::Cttz(map_operand(v, egraph)),
        InstKind::Copy(_, _) => unreachable!(
            "InstKind::Copy reached the egraph elaborator — mem2reg's φ \
             destruction introduces Copy only after the egraph pass"
        ),
        InstKind::Select(_, _, _, _) => unreachable!(
            "InstKind::Select reached the egraph elaborator — select \
             formation introduces Select only after the egraph pass"
        ),
        InstKind::CtpopRangeSum(_, _, _) => unreachable!(
            "InstKind::CtpopRangeSum reached the egraph elaborator — \
             ctpop-range-sum formation introduces it only after the \
             egraph pass"
        ),
        InstKind::Vec(_) => unreachable!(
            "InstKind::Vec reached the egraph elaborator — loop widening \
             introduces it only after the egraph pass"
        ),
        InstKind::BitCastF64ToI64(v) => InstKind::BitCastF64ToI64(map_operand(v, egraph)),
        InstKind::BitCastI64ToF64(v) => InstKind::BitCastI64ToF64(map_operand(v, egraph)),
        InstKind::IntToPtr(v) => InstKind::IntToPtr(map_operand(v, egraph)),
        InstKind::PtrToInt(v) => InstKind::PtrToInt(map_operand(v, egraph)),
        InstKind::TruncI64ToBool(v) => InstKind::TruncI64ToBool(map_operand(v, egraph)),
        InstKind::PinSet(w, v) => InstKind::PinSet(*w, map_operand(v, egraph)),
        InstKind::Suspend(k, lane, v, held) => InstKind::Suspend(
            *k,
            *lane,
            map_operand(v, egraph),
            held.iter().map(|h| map_operand(h, egraph)).collect(),
        ),
        // No SSA Value payload — just clone.
        InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_)
        | InstKind::Pinned(_)
        | InstKind::ThrowSignal => kind.clone(),
    }
}

#[inline]
pub(super) fn map_operand(op: &Operand, egraph: &mut Egraph) -> Operand {
    match op {
        Operand::Value(v) => {
            // P-OPT Phase 1 chunk 9a-3 — const-folded values
            // propagate as literal operands. Falls back to the
            // canonical Value when no rule has wired a constant.
            let canon = egraph.opt_value(*v);
            if let Some(const_op) = egraph.const_of(canon) {
                const_op
            } else {
                Operand::Value(canon)
            }
        }
        other => *other,
    }
}
