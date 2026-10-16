//! The probe of the pending-throw flag that follows a call:
//! `%c = icmp ne (load i64 @throw_active), 0` and a branch on `%c`.
//! A sibling call (compile/sibcall.rs) looks through one.

use torajs_core::ssa::{IPred, Inst, InstKind, Operand, THROW_ACTIVE_SYM, Type, ValueId};

/// `%g = global_ref @throw_active; %l = load i64, %g; %c = icmp ne %l, 0`.
pub(super) fn is_throw_probe(g: &Inst, l: &Inst, test: &Inst, c: ValueId) -> bool {
    let (Some(gv), Some(lv)) = (g.result, l.result) else {
        return false;
    };
    matches!(&g.kind, InstKind::GlobalRef(name) if name == THROW_ACTIVE_SYM)
        && matches!(&l.kind, InstKind::Load(Type::I64, Operand::Value(p), 0) if *p == gv)
        && matches!(
            &test.kind,
            InstKind::ICmp(IPred::Ne, Operand::Value(x), Operand::ConstI64(0)) if *x == lv
        )
        && test.result == Some(c)
}
