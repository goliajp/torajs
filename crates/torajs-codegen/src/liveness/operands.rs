//! The SSA operands an instruction or a terminator reads, as liveness
//! walks them.

use torajs_core::ssa::{InstKind, Operand, Terminator, ValueId};

/// Invoke `f` on every `ValueId` referenced as an operand of `kind`.
/// Mirrors the InstKind dispatch in `compile::emit_inst` so adding a
/// new variant there forces this match to be updated too.
pub(crate) fn visit_inst_operands(kind: &InstKind, mut f: impl FnMut(ValueId)) {
    let mut visit = |op: &Operand| {
        if let Operand::Value(v) = op {
            f(*v);
        }
    };
    match kind {
        InstKind::BinOp(_, a, b) => {
            visit(a);
            visit(b);
        }
        InstKind::ICmp(_, a, b) | InstKind::FCmp(_, a, b) => {
            visit(a);
            visit(b);
        }
        InstKind::BitCastF64ToI64(a) | InstKind::BitCastI64ToF64(a) => visit(a),
        InstKind::SiToFp(a) | InstKind::FpToSi(a) | InstKind::FpToInt32(a) => visit(a),
        InstKind::ZExtBoolToI64(a) | InstKind::ZExtI32ToI64(a) | InstKind::TruncI64ToBool(a) => {
            visit(a)
        }
        InstKind::PtrToInt(a) | InstKind::IntToPtr(a) => visit(a),
        InstKind::Neg(a) => visit(a),
        InstKind::Ctpop(a) => visit(a),
        InstKind::Cttz(a) => visit(a),
        InstKind::Copy(_, a) => visit(a),
        InstKind::Suspend(_, _, a, held) => {
            visit(a);
            for h in held {
                visit(h);
            }
        }
        InstKind::Select(_, c, t, e) => {
            visit(c);
            visit(t);
            visit(e);
        }
        InstKind::CtpopRangeSum(s, b, a) => {
            visit(s);
            visit(b);
            visit(a);
        }
        InstKind::Vec(vi) => vi.operands().into_iter().flatten().for_each(visit),
        InstKind::Alloca(_) | InstKind::AllocaBytes(_) => {}
        InstKind::Load(_, ptr, _)
        | InstKind::RcInc(ptr)
        | InstKind::RcIncAny(ptr)
        | InstKind::RcDecAny(ptr)
        | InstKind::BoxAny(_, ptr)
        | InstKind::AnyIsNumber(ptr)
        | InstKind::AnyIsInt32(ptr)
        | InstKind::AnyToInt32(ptr)
        | InstKind::BoxInt32(ptr)
        | InstKind::BoxF64(ptr) => visit(ptr),
        InstKind::AnyToF64(val, flag) => {
            visit(val);
            visit(flag);
        }
        InstKind::Store(val, ptr, _) => {
            visit(val);
            visit(ptr);
        }
        InstKind::LoadDyn(_, base, off) => {
            visit(base);
            visit(off);
        }
        InstKind::StoreDyn(val, base, off) => {
            visit(val);
            visit(base);
            visit(off);
        }
        InstKind::LoadDynScaled8(_, base, idx) => {
            visit(base);
            visit(idx);
        }
        InstKind::LoadU8Dyn(base, idx) | InstKind::LoadU16Dyn(base, idx) => {
            visit(base);
            visit(idx);
        }
        InstKind::StoreDynScaled8(val, base, idx) => {
            visit(val);
            visit(base);
            visit(idx);
        }
        InstKind::Call(_, args) => {
            for a in args {
                visit(a);
            }
        }
        InstKind::CallIndirect(_, fp, args) => {
            visit(fp);
            for a in args {
                visit(a);
            }
        }
        // Leaf "address materialize" ops carry no Operand inputs.
        InstKind::GlobalRef(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_)
        | InstKind::Pinned(_) => {}
        InstKind::PinSet(_, v) => visit(v),
        InstKind::ThrowSignal => {}
        // P-OPT Phase 1 — `Identity` is dropped by egraph elaborate
        // before liveness ever runs. If it survives, liveness would
        // see a "result with no real def site" and produce nonsense;
        // signal the bug instead of silently accepting it.
        InstKind::Identity(_) => unreachable!(
            "InstKind::Identity reached liveness — egraph elaborate must \
             drop it via set_opt_value before this point"
        ),
    }
}

/// Invoke `f` on every `ValueId` referenced by a terminator's operands.
pub(crate) fn visit_terminator_operands(term: &Terminator, mut f: impl FnMut(ValueId)) {
    let mut visit = |op: &Operand| {
        if let Operand::Value(v) = op {
            f(*v);
        }
    };
    match term {
        Terminator::Ret(Some(op)) => visit(op),
        Terminator::Ret(None) => {}
        Terminator::Br(_) => {}
        Terminator::CondBr { cond, .. } => visit(cond),
        Terminator::Unreachable => {}
    }
}
