//! Register allocation.
//!
//! Per RFC D2 the target algorithm is Linear Scan (Poletto & Sarkar
//! 1999). Full LS lands in S2/S3 once arithmetic / mem / cast surface
//! broadens; S1 uses a trivial allocator that's just enough for the
//! `1 + 2` end-to-end fixture:
//!
//!   - Result ValueId of the *last* defining instruction with a
//!     ret-shaped Terminator goes to x0 (AAPCS64 ret register).
//!   - All other ValueIds get a scratch from the AAPCS64 caller-saved
//!     pool (x9, x10, ...), assigned in definition order.
//!
//! Trivial alloc is correct (every SSA value still lives in a distinct
//! register, no false aliasing) but not space-efficient — there's no
//! interval reuse, so we'd run out of regs around ~14 SSA values.
//! That's plenty for S1, fine for the S2 arithmetic surface, and gets
//! replaced by real LS in S3 once Load/Store/Alloca need spill support.

use std::collections::{HashMap, HashSet};
use torajs_core::ssa::{BinOp, Function, InstKind, Terminator, Type, ValueId};

use crate::reg::{Fpr, Gpr, Reg, aapcs64};

/// Per-function register assignment.
#[derive(Debug, Clone)]
pub struct Assignment {
    /// Map from SSA `ValueId` (debug-stable u32) to the register class
    /// + slot it lives in for its entire lifetime. Spilled values
    /// carry a `Reg::SpillGpr(off)` / `Reg::SpillFpr(off)` whose
    /// `off` is sp-relative (already includes the alloca region
    /// offset; see `linear_scan::allocate_linear_scan`).
    by_value: HashMap<u32, Reg>,
    /// Map from each `Alloca`/`AllocaBytes` result `ValueId` to its
    /// byte offset within the frame's alloca region. Slot 0 lives at
    /// `sp+0`, slot 1 at `sp+slot0_size`, etc.
    alloca_offsets: HashMap<u32, u32>,
    /// Total raw alloca bytes (sum of per-slot sizes).
    pub raw_alloca_bytes: u32,
    /// Total bytes carved for register spill slots (LS-3). 0 when no
    /// allocator-emitted spill happened.
    ///
    /// `FrameLayout::from_alloca_bytes` is built with
    /// `raw_alloca_bytes + total_spill_bytes` so a single sp
    /// adjustment in the prologue covers both regions; the alloca
    /// slots sit at the bottom (offsets `[0, raw_alloca_bytes)`) and
    /// the spill slots above them (offsets
    /// `[raw_alloca_bytes, raw_alloca_bytes + total_spill_bytes)`).
    pub total_spill_bytes: u32,
    /// `true` if any `Call` / `CallIndirect` inst is present —
    /// triggers FP/LR save in the prologue (`FrameLayout.uses_calls`).
    pub has_calls: bool,
    /// Bitmask (`Gpr::idx`) of callee-saved GPRs the allocator handed
    /// to call-crossing values — the frame must save/restore each.
    /// 0 for the trivial allocator and leaf / non-crossing functions.
    pub used_callee_gpr_mask: u32,
    /// Bitmask (`Fpr::idx`) of callee-saved FPRs used for call-crossing
    /// f64 values (mirrors `used_callee_gpr_mask`).
    pub used_callee_fpr_mask: u32,
    /// Entry moves for params whose live interval crosses a call: the
    /// AAPCS64 arg register is caller-saved and would be clobbered, so
    /// the allocator relocates the param to a callee-saved register
    /// (or a spill slot) and records `(arg_reg, dst)` here. The
    /// compiler emits these right after the prologue, before the body.
    pub param_entry_moves: Vec<(Reg, Reg)>,
    /// Entry loads for stack params (the 9th+ argument of a register
    /// class arrives in the caller's outgoing area, not a register):
    /// `(incoming slot j, dst)` — the compiler emits an
    /// `LDR [sp, #frame_size (+16 FP/LR) + j*8]` into `dst` right
    /// after `param_entry_moves`, so the body never touches the
    /// caller's frame again.
    pub stack_param_entry_loads: Vec<(u32, Reg)>,
}

impl Assignment {
    /// Build an `Assignment` from already-decided pieces. Used by
    /// alternate allocators (`linear_scan::allocate_linear_scan`)
    /// so they can produce the same opaque struct without exposing
    /// the private maps.
    pub(crate) fn from_parts(
        by_value: HashMap<u32, Reg>,
        alloca_offsets: HashMap<u32, u32>,
        raw_alloca_bytes: u32,
        total_spill_bytes: u32,
        has_calls: bool,
    ) -> Self {
        Assignment {
            by_value,
            alloca_offsets,
            raw_alloca_bytes,
            total_spill_bytes,
            has_calls,
            // Callee-saved tracking is filled in by the caller
            // (`linear_scan`) via the pub fields after construction;
            // the trivial allocator leaves them empty.
            used_callee_gpr_mask: 0,
            used_callee_fpr_mask: 0,
            param_entry_moves: Vec::new(),
            stack_param_entry_loads: Vec::new(),
        }
    }

    /// Total sp adjustment the prologue must carve = alloca + spill,
    /// pre-alignment. `FrameLayout::from_alloca_bytes` rounds the
    /// final value up to 16. Used by `compile::compile_function`.
    pub fn total_frame_bytes(&self) -> u32 {
        self.raw_alloca_bytes + self.total_spill_bytes
    }

    /// Lookup the register for a `ValueId`. Panics if unallocated.
    pub fn of(&self, vid: ValueId) -> Reg {
        *self
            .by_value
            .get(&vid.0)
            .unwrap_or_else(|| panic!("ValueId({}) not allocated", vid.0))
    }

    /// `true` if `vid` has a register assigned.
    pub fn contains(&self, vid: ValueId) -> bool {
        self.by_value.contains_key(&vid.0)
    }

    /// Byte offset of `vid`'s alloca slot from sp (after prologue
    /// SUB). Panics if `vid` is not an Alloca result.
    pub fn alloca_offset_of(&self, vid: ValueId) -> u32 {
        *self
            .alloca_offsets
            .get(&vid.0)
            .unwrap_or_else(|| panic!("ValueId({}) is not an Alloca result", vid.0))
    }

    /// Resolve a GPR-shaped def site. Returns `(reg, spill_off)`:
    ///
    ///   - `(g, None)` when the value lives in a real GPR; emitters
    ///     should write the result into `g` directly.
    ///   - `(scratch, Some(off))` when the value is spilled —
    ///     emitters should write into `scratch` and then immediately
    ///     `STR scratch, [SP, #off]` (see `compile::write_def_spill_gpr`).
    ///
    /// Panics if `vid` is not allocated, holds an FPR / FPR spill, or
    /// is otherwise inapplicable to a GPR-class def.
    pub fn def_gpr(&self, vid: ValueId, scratch: Gpr) -> (Gpr, Option<u32>) {
        match self.of(vid) {
            Reg::Gpr(g) => (g, None),
            Reg::SpillGpr(off) => (scratch, Some(off)),
            Reg::Fpr(f) => panic!("def_gpr called for ValueId({}) in Fpr({f:?})", vid.0),
            Reg::SpillFpr(off) => panic!(
                "def_gpr called for ValueId({}) in SpillFpr(sp+{off})",
                vid.0
            ),
        }
    }

    /// Same as `def_gpr` for FPR defs.
    pub fn def_fpr(&self, vid: ValueId, scratch: Fpr) -> (Fpr, Option<u32>) {
        match self.of(vid) {
            Reg::Fpr(f) => (f, None),
            Reg::SpillFpr(off) => (scratch, Some(off)),
            Reg::Gpr(g) => panic!("def_fpr called for ValueId({}) in Gpr({g:?})", vid.0),
            Reg::SpillGpr(off) => panic!(
                "def_fpr called for ValueId({}) in SpillGpr(sp+{off})",
                vid.0
            ),
        }
    }
}

/// Trivial allocator: F64 values go to FPR slots, everything else to
/// GPR slots. The function's return value (if any) is placed in the
/// AAPCS64 return slot (x0 for int/ptr, v0/d0 for f64); the rest get
/// caller-saved scratch in definition order.
///
/// Single pass:
///   - Detect Alloca-shaped insts, assign each its byte offset within
///     the alloca region (slot 0 at offset 0, slot 1 at offset of
///     slot 0's rounded-to-8 size, etc).
///   - Assign every result `ValueId` a register from the appropriate
///     class pool.
///
/// Will panic if more than `CALLER_SAVED_SCRATCH.len()` GPR ValueIds
/// or `FP_CALLER_SAVED_SCRATCH.len()` FPR ValueIds need scratch — S3
/// lands Linear Scan with spill support before that limit becomes
/// load-bearing.
/// `true` iff the instruction lowers to a `BL` site at codegen time.
/// This is what determines whether the prologue must save FP/LR
/// (the AAPCS64 link register) — every BL clobbers x30.
///
/// Currently the BL sites are:
///   - `Call(FuncId, _)` / `CallIndirect(_, _, _)` (direct and
///     indirect SSA call surface, S4-A/C),
///   - `BinOp(FRem, _, _)` — lowered to a libm `_fmod` BL by
///     `compile::binop` (S2-D).
pub(crate) fn inst_emits_bl(kind: &InstKind) -> bool {
    // CtpopRangeSum emits no BL, but its canned SIMD loop clobbers
    // V0-V7 + V16-V18 and X9-X12 — a subset of a call's caller-saved
    // clobber set — so it needs the same treatment FRem gets: mark
    // the site so live-across values relocate to callee-saved homes.
    matches!(
        kind,
        InstKind::Call(_, _)
            | InstKind::CallIndirect(_, _, _)
            | InstKind::BinOp(BinOp::FRem, _, _)
            | InstKind::CtpopRangeSum(_, _, _)
    )
}

pub fn allocate_trivial(func: &Function) -> Assignment {
    let ret_vids = collect_ret_value_ids(func);
    let mut by_value: HashMap<u32, Reg> = HashMap::new();
    let mut alloca_offsets: HashMap<u32, u32> = HashMap::new();
    let mut gpr_idx = 0usize;
    let mut fpr_idx = 0usize;
    let mut next_alloca_offset: u32 = 0;
    let mut has_calls = false;

    // AAPCS64 §5.4.2 — int and fp params travel in *separate* lanes:
    // first 8 int/ptr params land in X0..X7, first 8 fp params land
    // in V0..V7. The two indices count independently.
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
        by_value.insert(param.0, reg);
    }

    for block in &func.blocks {
        for inst in &block.insts {
            // Record alloca slot offset before register assignment
            // (the alloca's *result* register holds the slot address,
            // computed by `compile::mem::emit_alloca` as ADD reg,
            // sp, #slot_offset).
            if let Some(slot_size) = alloca_slot_size(&inst.kind) {
                let result = inst.result.expect("Alloca must have result");
                alloca_offsets.insert(result.0, next_alloca_offset);
                next_alloca_offset += slot_size;
            }

            if inst_emits_bl(&inst.kind) {
                has_calls = true;
            }

            let Some(result) = inst.result else {
                continue; // void inst (e.g. Store)
            };

            let ty = func
                .values
                .get(result.0 as usize)
                .map(|vi| &vi.ty)
                .expect("ValueId out of bounds");
            let is_fp = matches!(ty, Type::F64);
            let is_ret = ret_vids.contains(&result.0);

            let reg = if is_ret {
                if is_fp {
                    Reg::Fpr(aapcs64::FP_ARG_RET[0])
                } else {
                    Reg::Gpr(aapcs64::ARG_RET[0])
                }
            } else if is_fp {
                let f = aapcs64::FP_CALLER_SAVED_SCRATCH[fpr_idx];
                fpr_idx += 1;
                Reg::Fpr(f)
            } else {
                let g = aapcs64::CALLER_SAVED_SCRATCH[gpr_idx];
                gpr_idx += 1;
                Reg::Gpr(g)
            };
            by_value.insert(result.0, reg);
        }
    }

    Assignment {
        by_value,
        alloca_offsets,
        raw_alloca_bytes: next_alloca_offset,
        // Trivial allocator never spills — it panics on pool
        // exhaustion. LS-2/LS-3 produce real spill_bytes.
        total_spill_bytes: 0,
        has_calls,
        // Trivial allocator predates call-crossing analysis; it never
        // parks values in callee-saved registers (it panics on pool
        // overflow before that becomes relevant).
        used_callee_gpr_mask: 0,
        used_callee_fpr_mask: 0,
        param_entry_moves: Vec::new(),
        stack_param_entry_loads: Vec::new(),
    }
}

/// Slot byte size for an Alloca-shaped inst. Returns `None` for any
/// other InstKind. Sizes round up to 8 to keep all slots 8-aligned.
pub(crate) fn alloca_slot_size(kind: &InstKind) -> Option<u32> {
    match kind {
        InstKind::Alloca(ty) => Some(align_up_8(type_size_bytes(ty))),
        InstKind::AllocaBytes(n) => Some(align_up_8(*n as u32)),
        _ => None,
    }
}

fn align_up_8(n: u32) -> u32 {
    (n + 7) & !7
}

/// Byte size of an SSA `Type` for stack allocation purposes. All
/// reference/heap types and machine-word primitives are 8 bytes;
/// I32/Bool round up to 8 via the caller. `Void` is not allocable.
fn type_size_bytes(ty: &Type) -> u32 {
    match ty {
        Type::Void => panic!("Alloca(Void) is not allocable"),
        Type::I32 => 4,
        Type::Bool => 1,
        // All 64-bit primitives + every heap-shaped reference type.
        _ => 8,
    }
}

/// Collect every `ValueId` referenced as a ret target across all
/// blocks (multi-block functions can have multiple `Ret(Some(...))`
/// sites — each branch path produces its own ret value).
///
/// All such values share the AAPCS64 ret register slot because they
/// are mutually exclusive at runtime: only one Ret fires per
/// function invocation. The trivial allocator places every one of
/// them into the same physical register (x0 for int/ptr, v0 for
/// F64). Within-block aliasing is not a concern under SSA
/// single-assignment semantics + per-block control-flow boundary.
pub(crate) fn collect_ret_value_ids(func: &Function) -> HashSet<u32> {
    let mut out = HashSet::new();
    for block in &func.blocks {
        if let Terminator::Ret(Some(torajs_core::ssa::Operand::Value(v))) = &block.term {
            out.insert(v.0);
        }
    }
    out
}

/// Backwards-compat helper for callers that know the value is in a
/// GPR slot. Cleaner than `.as_gpr()` callsites all over emit_inst.
#[allow(dead_code)]
pub(crate) fn require_gpr(reg: Reg) -> Gpr {
    reg.as_gpr()
}

/// Same for FPR.
#[allow(dead_code)]
pub(crate) fn require_fpr(reg: Reg) -> Fpr {
    reg.as_fpr()
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    fn build_one_plus_two() -> Function {
        let v0 = ValueId(0);
        Function {
            name: "one_plus_two".into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(BinOp::Add, Operand::ConstI64(1), Operand::ConstI64(2)),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    #[test]
    fn one_plus_two_allocates_v0_to_x0() {
        let func = build_one_plus_two();
        let alloc = allocate_trivial(&func);
        let v0 = ValueId(0);
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X0));
        assert!(alloc.contains(v0));
    }

    /// `fn identity(x: i64) -> i64 { x }` — param ValueId must land
    /// in X0 (AAPCS64 int arg lane 0). Prior to this fix the trivial
    /// allocator only walked inst.result so the param's ValueId was
    /// unmapped → `allocate_trivial -> Assignment.of()` panicked.
    #[test]
    fn single_i64_param_lands_in_x0() {
        let x = ValueId(0);
        let func = Function {
            name: "identity".into(),
            params: vec![x],
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("x".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: Vec::new(),
                term: Terminator::Ret(Some(Operand::Value(x))),
            }],
            current_origin: None,
        };
        let alloc = allocate_trivial(&func);
        assert_eq!(alloc.of(x), Reg::Gpr(Gpr::X0));
    }

    /// `fn mix(a: i64, b: f64, c: i64, d: f64) -> i64 { a }` —
    /// AAPCS64 §5.4.2 routes int/fp params through *independent*
    /// X/V lanes, so a/c land in X0/X1 and b/d land in V0/V1.
    #[test]
    fn mixed_int_fp_params_split_into_separate_lanes() {
        let a = ValueId(0);
        let b = ValueId(1);
        let c = ValueId(2);
        let d = ValueId(3);
        let func = Function {
            name: "mix".into(),
            params: vec![a, b, c, d],
            ret: Type::I64,
            values: vec![
                ValueInfo {
                    ty: Type::I64,
                    name: Some("a".into()),
                },
                ValueInfo {
                    ty: Type::F64,
                    name: Some("b".into()),
                },
                ValueInfo {
                    ty: Type::I64,
                    name: Some("c".into()),
                },
                ValueInfo {
                    ty: Type::F64,
                    name: Some("d".into()),
                },
            ],
            blocks: vec![Block {
                id: BlockId(0),
                insts: Vec::new(),
                term: Terminator::Ret(Some(Operand::Value(a))),
            }],
            current_origin: None,
        };
        let alloc = allocate_trivial(&func);
        assert_eq!(alloc.of(a), Reg::Gpr(Gpr::X0));
        assert_eq!(alloc.of(b), Reg::Fpr(Fpr::V0));
        assert_eq!(alloc.of(c), Reg::Gpr(Gpr::X1));
        assert_eq!(alloc.of(d), Reg::Fpr(Fpr::V1));
    }

    #[test]
    fn f64_ret_value_goes_to_v0() {
        let v0 = ValueId(0);
        let func = Function {
            name: "one_point_five_plus_two_point_five".into(),
            params: Vec::new(),
            ret: Type::F64,
            values: vec![ValueInfo {
                ty: Type::F64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(
                        BinOp::FAdd,
                        Operand::ConstF64(1.5),
                        Operand::ConstF64(2.5),
                    ),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        };
        let alloc = allocate_trivial(&func);
        assert_eq!(alloc.of(v0), Reg::Fpr(Fpr::V0));
    }
}
