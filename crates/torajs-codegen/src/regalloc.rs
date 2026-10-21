//! Register assignment shared by the Linear Scan allocator
//! (`linear_scan`) and the emitters: the `Assignment` map, alloca slot
//! sizing, and the per-instruction facts the allocator reads.

use torajs_core::ssa::id_hash::{HashMap, HashSet};
use torajs_core::ssa::{Function, InstKind, Terminator, Type, ValueId};

use crate::compile::InlineSites;
use crate::reg::{Fpr, Gpr, Reg};

/// Per-function register assignment.
#[derive(Debug, Clone)]
pub struct Assignment {
    /// Map from SSA `ValueId` (debug-stable u32) to the register class
    /// and slot it lives in for its entire lifetime. Spilled values
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
    /// 0 for leaf / non-crossing functions.
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
    /// Call-site slot index → `(reg, sp offset)` for each call-crossing
    /// value held in a caller-saved register: the compiler stores `reg`
    /// before that instruction and reloads it after.
    pub call_saves: HashMap<u32, Vec<(Reg, u32)>>,
    /// The same for each call-crossing vector, whose slot is 16 bytes:
    /// stored and reloaded whole around the instruction, fast paths
    /// included.
    pub vec_call_saves: HashMap<u32, Vec<(Fpr, u32)>>,
    /// By block position: the block runs at most once in 64 function
    /// entries by the static estimate (a throw or slow-path arm), so
    /// where it starts in a fetch line does not matter. Empty: none.
    pub cold_blocks: Vec<bool>,
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
            // (`linear_scan`) via the pub fields after construction.
            used_callee_gpr_mask: 0,
            used_callee_fpr_mask: 0,
            param_entry_moves: Vec::new(),
            stack_param_entry_loads: Vec::new(),
            call_saves: HashMap::default(),
            vec_call_saves: HashMap::default(),
            cold_blocks: Vec::new(),
        }
    }

    /// Is the block at position `bi` cold (`cold_blocks`)?
    pub(crate) fn is_cold(&self, bi: usize) -> bool {
        self.cold_blocks.get(bi).copied().unwrap_or(false)
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

    /// A copy with the given values (by id) re-homed — shrink-wrapping
    /// reads a relocated param from its argument register before the
    /// prologue has run the move.
    pub(crate) fn with_homes(&self, homes: &[(u32, Reg)]) -> Assignment {
        let mut out = self.clone();
        for &(v, r) in homes {
            out.by_value.insert(v, r);
        }
        out
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

    /// The frame offset of `vid`'s slot when `vid` is an alloca result,
    /// else `None`. A spilled alloca address is never stored: it is a
    /// constant offset from sp, so each read recomputes it.
    pub(crate) fn alloca_slot(&self, vid: ValueId) -> Option<u32> {
        self.alloca_offsets.get(&vid.0).copied()
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
    ///
    /// Copy coalescing may give `vid` the register of one of the
    /// inst's own operands: an emitter must read every operand before
    /// its first write to the returned register.
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

/// `true` iff the instruction is a call clobber site: a `BL`, or a
/// canned loop clobbering the same registers (`ssa::lowers_to_call`
/// lists them). Every one makes the prologue save FP/LR.
pub(crate) fn inst_emits_bl(kind: &InstKind) -> bool {
    torajs_core::ssa::lowers_to_call(kind)
}

/// How often a call site's `bl` actually runs, as a divisor of its
/// slot frequency: a release calls only on its slow arm (the last
/// reference, or a cell that may need buffering as a cycle root), a
/// cell allocation or free only when the thread cache's row is empty
/// or full.
pub(crate) fn call_site_rarity(kind: &InstKind, inline: InlineSites) -> u64 {
    let slow_arm = (1.0 / torajs_core::ssa::branch_prob::P_SLOW_PATH) as u64;
    match kind {
        InstKind::RcDecAny(_) => slow_arm,
        _ if inline.opens_inline(kind) => slow_arm,
        _ => 1,
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
/// function invocation, so they can share one physical register
/// (x0 for int/ptr, v0 for F64). Within-block aliasing is not a concern under SSA
/// single-assignment semantics + per-block control-flow boundary.
pub(crate) fn collect_ret_value_ids(func: &Function) -> HashSet<u32> {
    let mut out = HashSet::default();
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
        let alloc = crate::linear_scan::allocate_linear_scan(&func);
        let v0 = ValueId(0);
        assert_eq!(alloc.of(v0), Reg::Gpr(Gpr::X0));
        assert!(alloc.contains(v0));
    }

    /// `fn identity(x: i64) -> i64 { x }` — param ValueId must land
    /// in X0 (AAPCS64 int arg lane 0). Prior to this fix the trivial
    /// allocator only walked inst.result so the param's ValueId was
    /// unmapped → `Assignment.of()` panicked.
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
        let alloc = crate::linear_scan::allocate_linear_scan(&func);
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
        let alloc = crate::linear_scan::allocate_linear_scan(&func);
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
        let alloc = crate::linear_scan::allocate_linear_scan(&func);
        assert_eq!(alloc.of(v0), Reg::Fpr(Fpr::V0));
    }
}
