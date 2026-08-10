//! AAPCS64 lane bookkeeping for the linear-scan sweep — which register
//! each parameter arrives in, and whether the return value may be
//! parked in the lane it shares with the first parameter.

use std::collections::HashMap;

use torajs_core::ssa::{Function, Type};

use crate::liveness::Interval;
use crate::reg::{Reg, aapcs64};

/// One value's AAPCS64 §5.4.2 lane: an argument register, or a stack
/// slot when its register class is exhausted. Stack slots number in
/// declaration order at 8 bytes each, based at the call-moment SP —
/// both sides classify the same sequence with [`classify_lanes`], so
/// caller stores and callee loads agree by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgLane {
    Reg(Reg),
    /// Stack slot index `j` → byte offset `j * 8` from the call SP.
    Stack(u32),
}

/// Classify a parameter / argument sequence into AAPCS64 lanes:
/// ints/ptrs consume `ARG_RET` in order, floats `FP_ARG_RET`; each
/// value past its class's 8 registers takes the next stack slot
/// (stages C.14/C.17 — every stack arg is 8-byte-sized here, so the
/// two collapse to one slot counter).
pub(crate) fn classify_lanes(is_fp_seq: impl Iterator<Item = bool>) -> Vec<ArgLane> {
    let mut gpr_idx = 0usize;
    let mut fpr_idx = 0usize;
    let mut stack_slot = 0u32;
    is_fp_seq
        .map(|is_fp| {
            let (idx, exhausted) = if is_fp {
                (&mut fpr_idx, aapcs64::FP_ARG_RET.len())
            } else {
                (&mut gpr_idx, aapcs64::ARG_RET.len())
            };
            if *idx >= exhausted {
                let j = stack_slot;
                stack_slot += 1;
                return ArgLane::Stack(j);
            }
            let lane = if is_fp {
                ArgLane::Reg(Reg::Fpr(aapcs64::FP_ARG_RET[*idx]))
            } else {
                ArgLane::Reg(Reg::Gpr(aapcs64::ARG_RET[*idx]))
            };
            *idx += 1;
            lane
        })
        .collect()
}

/// AAPCS64 param lanes for the sweep — register params as
/// `vid → Reg`, stack params (past the 8-per-class registers) as
/// `vid → stack slot index`.
pub(crate) fn param_lanes(func: &Function) -> (HashMap<u32, Reg>, HashMap<u32, u32>) {
    let is_fp_seq = func.params.iter().map(|p| {
        let ty = func
            .values
            .get(p.0 as usize)
            .map(|vi| &vi.ty)
            .expect("ValueId(param) out of bounds");
        matches!(ty, Type::F64)
    });
    let mut regs: HashMap<u32, Reg> = HashMap::new();
    let mut stack: HashMap<u32, u32> = HashMap::new();
    for (lane, &param) in classify_lanes(is_fp_seq).into_iter().zip(&func.params) {
        match lane {
            ArgLane::Reg(r) => {
                regs.insert(param.0, r);
            }
            ArgLane::Stack(j) => {
                stack.insert(param.0, j);
            }
        }
    }
    (regs, stack)
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
