//! Generic SSA operand visitors — walk the `Operand::Value` uses of
//! one instruction. Lives beside `InstKind` so the exhaustive match
//! breaks loudly when a new variant grows an operand (the egraph
//! passes and codegen both count uses through this single walker).

use super::{InstKind, Operand, ValueId};

/// Call `f` with every `Operand::Value` the instruction reads.
pub fn visit_value_operands(kind: &InstKind, mut f: impl FnMut(ValueId)) {
    let mut v = |op: &Operand| {
        if let Operand::Value(x) = op {
            f(*x);
        }
    };
    match kind {
        InstKind::BinOp(_, a, b) | InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) => {
            v(a);
            v(b);
        }
        InstKind::Call(_, args) => args.iter().for_each(v),
        InstKind::CallIndirect(_, ptr, args) => {
            v(ptr);
            args.iter().for_each(v);
        }
        InstKind::Load(_, ptr, _) => v(ptr),
        InstKind::Store(val, ptr, _) => {
            v(val);
            v(ptr);
        }
        InstKind::LoadDyn(_, ptr, off) => {
            v(ptr);
            v(off);
        }
        InstKind::StoreDyn(val, ptr, off) => {
            v(val);
            v(ptr);
            v(off);
        }
        InstKind::LoadDynScaled8(_, ptr, idx) => {
            v(ptr);
            v(idx);
        }
        InstKind::LoadU8Dyn(ptr, idx) => {
            v(ptr);
            v(idx);
        }
        InstKind::StoreDynScaled8(val, ptr, idx) => {
            v(val);
            v(ptr);
            v(idx);
        }
        InstKind::Select(_, cond, t, e) => {
            v(cond);
            v(t);
            v(e);
        }
        InstKind::CtpopRangeSum(start, bound, acc) => {
            v(start);
            v(bound);
            v(acc);
        }
        InstKind::SiToFp(o)
        | InstKind::FpToSi(o)
        | InstKind::ZExtBoolToI64(o)
        | InstKind::ZExtI32ToI64(o)
        | InstKind::BitCastF64ToI64(o)
        | InstKind::BitCastI64ToF64(o)
        | InstKind::IntToPtr(o)
        | InstKind::PtrToInt(o)
        | InstKind::TruncI64ToBool(o)
        | InstKind::Identity(o)
        | InstKind::Neg(o)
        | InstKind::Ctpop(o)
        | InstKind::Copy(_, o) => v(o),
        InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_) => {}
    }
}

/// Rewrite every `Operand::Value` the instruction reads through `f`
/// in place — the mutable twin of `visit_value_operands` (same
/// exhaustive-match contract: a new variant with an operand breaks
/// both walkers loudly).
pub fn rewrite_value_operands(kind: &mut InstKind, mut f: impl FnMut(ValueId) -> ValueId) {
    let mut v = |op: &mut Operand| {
        if let Operand::Value(x) = op {
            *x = f(*x);
        }
    };
    match kind {
        InstKind::BinOp(_, a, b) | InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) => {
            v(a);
            v(b);
        }
        InstKind::Call(_, args) => args.iter_mut().for_each(v),
        InstKind::CallIndirect(_, ptr, args) => {
            v(ptr);
            args.iter_mut().for_each(v);
        }
        InstKind::Load(_, ptr, _) => v(ptr),
        InstKind::Store(val, ptr, _) => {
            v(val);
            v(ptr);
        }
        InstKind::LoadDyn(_, ptr, off) => {
            v(ptr);
            v(off);
        }
        InstKind::StoreDyn(val, ptr, off) => {
            v(val);
            v(ptr);
            v(off);
        }
        InstKind::LoadDynScaled8(_, ptr, idx) => {
            v(ptr);
            v(idx);
        }
        InstKind::LoadU8Dyn(ptr, idx) => {
            v(ptr);
            v(idx);
        }
        InstKind::StoreDynScaled8(val, ptr, idx) => {
            v(val);
            v(ptr);
            v(idx);
        }
        InstKind::Select(_, cond, t, e) => {
            v(cond);
            v(t);
            v(e);
        }
        InstKind::CtpopRangeSum(start, bound, acc) => {
            v(start);
            v(bound);
            v(acc);
        }
        InstKind::SiToFp(o)
        | InstKind::FpToSi(o)
        | InstKind::ZExtBoolToI64(o)
        | InstKind::ZExtI32ToI64(o)
        | InstKind::BitCastF64ToI64(o)
        | InstKind::BitCastI64ToF64(o)
        | InstKind::IntToPtr(o)
        | InstKind::PtrToInt(o)
        | InstKind::TruncI64ToBool(o)
        | InstKind::Identity(o)
        | InstKind::Neg(o)
        | InstKind::Ctpop(o)
        | InstKind::Copy(_, o) => v(o),
        InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_) => {}
    }
}
