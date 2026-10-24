//! Generic SSA operand visitors — walk the `Operand::Value` uses of
//! one instruction. Lives beside `InstKind` so the exhaustive match
//! breaks loudly when a new variant grows an operand (the egraph
//! passes and codegen both count uses through this single walker).

use super::{BinOp, InstKind, Operand, ValueId};

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
        InstKind::Load(_, ptr, _)
        | InstKind::RcInc(ptr)
        | InstKind::RcIncAny(ptr)
        | InstKind::RcDecAny(ptr)
        | InstKind::BoxAny(_, ptr)
        | InstKind::AnyIsNumber(ptr)
        | InstKind::AnyIsInt32(ptr)
        | InstKind::AnyToInt32(ptr)
        | InstKind::BoxInt32(ptr)
        | InstKind::BoxF64(ptr) => v(ptr),
        InstKind::AnyToF64(val, flag) => {
            v(val);
            v(flag);
        }
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
        InstKind::LoadU8Dyn(ptr, idx) | InstKind::LoadU16Dyn(ptr, idx) => {
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
        | InstKind::FpToInt32(o)
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
        | InstKind::Cttz(o)
        | InstKind::Copy(_, o)
        | InstKind::PinSet(_, o) => v(o),
        InstKind::Suspend(_, _, o, held) => std::iter::once(o).chain(held).for_each(v),
        InstKind::Vec(vi) => vi.operands().into_iter().flatten().for_each(v),
        InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_)
        | InstKind::Pinned(_)
        | InstKind::ThrowSignal => {}
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
        InstKind::Load(_, ptr, _)
        | InstKind::RcInc(ptr)
        | InstKind::RcIncAny(ptr)
        | InstKind::RcDecAny(ptr)
        | InstKind::BoxAny(_, ptr)
        | InstKind::AnyIsNumber(ptr)
        | InstKind::AnyIsInt32(ptr)
        | InstKind::AnyToInt32(ptr)
        | InstKind::BoxInt32(ptr)
        | InstKind::BoxF64(ptr) => v(ptr),
        InstKind::AnyToF64(val, flag) => {
            v(val);
            v(flag);
        }
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
        InstKind::LoadU8Dyn(ptr, idx) | InstKind::LoadU16Dyn(ptr, idx) => {
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
        | InstKind::FpToInt32(o)
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
        | InstKind::Cttz(o)
        | InstKind::Copy(_, o)
        | InstKind::PinSet(_, o) => v(o),
        InstKind::Suspend(_, _, o, held) => std::iter::once(o).chain(held).for_each(v),
        InstKind::Vec(vi) => vi.operands_mut().into_iter().flatten().for_each(v),
        InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_)
        | InstKind::Pinned(_)
        | InstKind::ThrowSignal => {}
    }
}

/// `true` iff codegen lowers the instruction to a `BL`, or to a canned
/// loop clobbering a subset of the caller-saved set a call clobbers:
///   - `Call` / `CallIndirect`,
///   - `BinOp(FRem)`, a libm `_fmod` call,
///   - `RcDecAny`, whose slow arm calls `__torajs_value_drop_heap`,
///   - `CtpopRangeSum`, no `BL`, but its SIMD loop clobbers V0-V7,
///     V16-V18 and X9-X12,
///   - `Suspend`, a call until the split pass replaces it.
///
/// The allocator moves values live across one to callee-saved homes
/// and the prologue saves the link register; the mid-end reads it to
/// keep a value from living across one when that costs more than
/// recomputing it.
pub fn lowers_to_call(kind: &InstKind) -> bool {
    matches!(
        kind,
        InstKind::Call(..)
            | InstKind::CallIndirect(..)
            | InstKind::BinOp(BinOp::FRem, ..)
            | InstKind::CtpopRangeSum(..)
            | InstKind::RcDecAny(_)
            | InstKind::Suspend(..)
    )
}
