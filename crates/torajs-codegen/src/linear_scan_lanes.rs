//! AAPCS64 lane bookkeeping for the linear-scan sweep — which register
//! each parameter arrives in, and whether the return value may be
//! parked in the lane it shares with the first parameter.

use std::collections::HashMap;

use torajs_core::ssa::{Function, Type};

use crate::liveness::Interval;
use crate::reg::{Reg, aapcs64};

/// AAPCS64 §5.4.2 param lanes — each param's entry register. Integer
/// and pointer params consume `ARG_RET` in order, floats `FP_ARG_RET`.
pub(crate) fn param_arg_regs(func: &Function) -> HashMap<u32, Reg> {
    let mut param_arg_reg: HashMap<u32, Reg> = HashMap::new();
    let mut gpr_arg_idx = 0usize;
    let mut fpr_arg_idx = 0usize;
    for &param in &func.params {
        let ty = func
            .values
            .get(param.0 as usize)
            .map(|vi| &vi.ty)
            .expect("ValueId(param) out of bounds");
        let is_fp = matches!(ty, Type::F64);
        let reg = if is_fp {
            let v = aapcs64::FP_ARG_RET[fpr_arg_idx];
            fpr_arg_idx += 1;
            Reg::Fpr(v)
        } else {
            let x = aapcs64::ARG_RET[gpr_arg_idx];
            gpr_arg_idx += 1;
            Reg::Gpr(x)
        };
        param_arg_reg.insert(param.0, reg);
    }
    param_arg_reg
}

/// Whether the return value may be parked directly in `reg` — X0 or
/// V0, the ABI sink `emit_terminator::Ret` reads from.
///
/// That lane is also the FIRST PARAM's. A param that doesn't cross a
/// call stays in its arg register and is never pushed onto `active`,
/// and neither is a return value parked here, so the sweep cannot see
/// the clash on its own: X0 would hold both the incoming argument and
/// the outgoing result. A callee-free body that reads its parameter
/// after the result is first written then reads the result instead —
/// `leaf_count(parents)` over a widened `number[]` put `count = 0` on
/// top of `parents`, and the next element read dereferenced 0.
///
/// `false` sends the value through the general allocator instead;
/// `emit_terminator::Ret` moves it into X0/V0 at the actual ret, and
/// its `if src != X0/V0 { mov }` is idempotent, so a value the
/// allocator happens to place there still costs nothing.
pub(crate) fn ret_lane_free(
    func: &Function,
    reg: Reg,
    ret_interval: Interval,
    intervals: &HashMap<u32, Interval>,
    by_value: &HashMap<u32, Reg>,
) -> bool {
    !func.params.iter().any(|p| {
        by_value.get(&p.0) == Some(&reg)
            && intervals
                .get(&p.0)
                .is_some_and(|pi| pi.start <= ret_interval.end && ret_interval.start <= pi.end)
    })
}
