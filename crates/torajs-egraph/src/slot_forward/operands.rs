//! Operand substitution over every instruction kind.

use crate::id_hash::HashMap;
use torajs_core::ssa::{InstKind, Operand, ValueId};

use super::resolve;

/// Rewrite every value operand of `kind` through the replacement map.
/// Mirrors `rc_peephole::visit_value_operands`'s coverage but mutably;
/// kept exhaustive over operand-carrying variants via the catch-all
/// helpers below. Shared with `mem2reg`.
pub(crate) fn rewrite_operands(kind: &mut InstKind, replace: &HashMap<ValueId, Operand>) {
    let r = |op: &mut Operand| *op = resolve(op, replace);
    match kind {
        InstKind::BinOp(_, a, b) | InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) => {
            r(a);
            r(b);
        }
        InstKind::Call(_, args) => args.iter_mut().for_each(r),
        InstKind::CallIndirect(_, p, args) => {
            r(p);
            args.iter_mut().for_each(r);
        }
        InstKind::Load(_, p, _)
        | InstKind::RcInc(p)
        | InstKind::RcIncAny(p)
        | InstKind::RcDecAny(p)
        | InstKind::BoxAny(_, p)
        | InstKind::AnyIsNumber(p)
        | InstKind::AnyIsInt32(p)
        | InstKind::AnyToInt32(p)
        | InstKind::BoxInt32(p)
        | InstKind::BoxF64(p) => r(p),
        InstKind::AnyToF64(v, f) => {
            r(v);
            r(f);
        }
        InstKind::Store(v, p, _) => {
            r(v);
            r(p);
        }
        InstKind::LoadDyn(_, p, o) => {
            r(p);
            r(o);
        }
        InstKind::StoreDyn(v, p, o) => {
            r(v);
            r(p);
            r(o);
        }
        InstKind::LoadDynScaled8(_, p, i) => {
            r(p);
            r(i);
        }
        InstKind::LoadU8Dyn(p, i) | InstKind::LoadU16Dyn(p, i) => {
            r(p);
            r(i);
        }
        InstKind::StoreDynScaled8(v, p, i) => {
            r(v);
            r(p);
            r(i);
        }
        InstKind::Select(_, c, t, e) => {
            r(c);
            r(t);
            r(e);
        }
        InstKind::CtpopRangeSum(s, b, a) => {
            r(s);
            r(b);
            r(a);
        }
        InstKind::Vec(vi) => vi.operands_mut().into_iter().flatten().for_each(r),
        InstKind::SiToFp(a)
        | InstKind::FpToSi(a)
        | InstKind::FpToInt32(a)
        | InstKind::ZExtBoolToI64(a)
        | InstKind::ZExtI32ToI64(a)
        | InstKind::BitCastF64ToI64(a)
        | InstKind::BitCastI64ToF64(a)
        | InstKind::IntToPtr(a)
        | InstKind::PtrToInt(a)
        | InstKind::TruncI64ToBool(a)
        | InstKind::Identity(a)
        | InstKind::Neg(a)
        | InstKind::Ctpop(a)
        | InstKind::Cttz(a)
        | InstKind::Copy(_, a)
        | InstKind::PinSet(_, a) => r(a),
        InstKind::Suspend(_, _, a, held) => {
            r(a);
            for h in held.iter_mut() {
                r(h);
            }
        }
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
