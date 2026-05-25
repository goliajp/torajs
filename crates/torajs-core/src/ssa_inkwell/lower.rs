//! `FnLower` — per-fn lowering state + small impl methods (`run`,
//! `lower_term`, `operand`, `operand_int`).
//!
//! The big `lower_inst` method lives in a sibling file
//! (`ssa_inkwell/lower_inst.rs`) as a separate impl block of the
//! same `FnLower` type — Rust allows multiple impl blocks across
//! files in the same crate, so the type's methods are spread for
//! file-size hygiene without changing the user-facing API.
//!
//! Extracted from `ssa_inkwell.rs` (2026-05-25, god-file decomp
//! batch 21).

use std::collections::HashMap;

use inkwell::AddressSpace;
use inkwell::context::Context;
use inkwell::values::{BasicValueEnum, FunctionValue, IntValue};

use super::DebugCtx;
use crate::ssa::{self as s, Operand, Terminator};

pub(super) struct FnLower<'a, 'ctx> {
    pub(super) ctx: &'ctx Context,
    pub(super) builder: &'a inkwell::builder::Builder<'ctx>,
    pub(super) ssa_fn: &'a s::Function,
    pub(super) llvm_fn: FunctionValue<'ctx>,
    pub(super) fn_map: &'a [FunctionValue<'ctx>],
    pub(super) string_globals: &'a [inkwell::values::GlobalValue<'ctx>],
    /// Phase P-rpn — per-literal Str-shaped statics; same indexing as
    /// `string_globals`. Resolved by `InstKind::StaticStrRef`.
    pub(super) static_str_globals: &'a [inkwell::values::GlobalValue<'ctx>],
    /// Phase K.3 — module-level data globals indexed by name. Looked
    /// up by `InstKind::GlobalRef` to yield the slot's pointer value.
    pub(super) data_globals: &'a HashMap<String, inkwell::values::GlobalValue<'ctx>>,
    /// T-24 — per-class vtable globals (`__vtable_<C>` → const ptr
    /// array). Resolved by `InstKind::GlobalRef` after `data_globals`
    /// lookup misses, so vtable references piggyback on the existing
    /// SSA primitive without a new InstKind.
    pub(super) vtable_globals: &'a HashMap<String, inkwell::values::GlobalValue<'ctx>>,
    /// Whole SSA module — needed by `InstKind::CallIndirect` to look up
    /// the signature interner. Read-only; no mutation. M2 Phase B Stage 3.
    pub(super) ssa_module: &'a s::Module,
    /// v0.3 #4 D-3 — Optional source-location resolver. When present,
    /// per-Inst `lower_inst` looks up `inst.origin` → `ast.expr_spans`
    /// → `byte_to_line_col` → DILocation, attaching it to subsequent
    /// build_* calls so DWARF backtraces resolve to `.ts:line:col`.
    /// None when the caller didn't supply ast / source_path.
    pub(super) ast: Option<&'a crate::ast::Ast>,
    pub(super) debug_ctx: Option<&'a DebugCtx<'ctx>>,
    pub(super) block_map: HashMap<u32, inkwell::basic_block::BasicBlock<'ctx>>,
    pub(super) value_map: HashMap<u32, BasicValueEnum<'ctx>>,
}

impl<'a, 'ctx> FnLower<'a, 'ctx> {
    pub(super) fn run(mut self) {
        // Phase 1: pre-create LLVM blocks for every SSA block so terminators
        // can reference forward blocks.
        for b in &self.ssa_fn.blocks {
            let bb = self
                .ctx
                .append_basic_block(self.llvm_fn, &format!("bb{}", b.id.0));
            self.block_map.insert(b.id.0, bb);
        }
        // Bind params: SSA params → LLVM function parameters, by position.
        for (i, &p) in self.ssa_fn.params.iter().enumerate() {
            let v = self
                .llvm_fn
                .get_nth_param(i as u32)
                .expect("param count mismatch");
            self.value_map.insert(p.0, v);
        }
        // Phase 2: lower each block.
        for (b_idx, b) in self.ssa_fn.blocks.iter().enumerate() {
            let bb = self.block_map[&b.id.0];
            self.builder.position_at_end(bb);
            /* v0.3 #3.c — at the start of `main`'s entry block, emit
             * an init call to capture argc/argv into runtime globals
             * for `process.argv` / `Bun.argv` access. The LLVM main
             * is widened to `(i32 argc, ptr argv)` by declare_ssa_fn;
             * here we forward those params to __torajs_argv_init.
             * Done before the user's main body runs. */
            if b_idx == 0 && self.ssa_fn.name == "main" {
                if let (Some(argc), Some(argv)) =
                    (self.llvm_fn.get_nth_param(0), self.llvm_fn.get_nth_param(1))
                {
                    /* fn_map indexes by the SSA module's func order;
                     * find __torajs_argv_init by name in the SSA fns. */
                    for (i, sf) in self.ssa_module.funcs.iter().enumerate() {
                        if sf.name == "__torajs_argv_init" {
                            let init_fn = self.fn_map[i];
                            self.builder
                                .build_call(init_fn, &[argc.into(), argv.into()], "")
                                .unwrap();
                            break;
                        }
                    }
                }
            }
            for inst in &b.insts {
                self.lower_inst(inst);
            }
            self.lower_term(&b.term);
        }
    }

    pub(super) fn lower_term(&self, t: &Terminator) {
        match t {
            Terminator::Br(b) => {
                let bb = self.block_map[&b.0];
                self.builder.build_unconditional_branch(bb).unwrap();
            }
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            } => {
                let cv = self.operand_int(cond); // i1
                let tb = self.block_map[&then_blk.0];
                let eb = self.block_map[&else_blk.0];
                self.builder.build_conditional_branch(cv, tb, eb).unwrap();
            }
            Terminator::Ret(maybe) => match maybe {
                Some(o) => {
                    let v = self.operand(o);
                    // M4.3 — same ptr↔i64 cast as the Call boundary,
                    // applied at Ret. Throw's `ret <sentinel>` always
                    // emits ConstI64(0); when the fn's signature
                    // returns ptr-shaped (string / obj / arr / closure),
                    // LLVM rejects `ret i64` against `ret ptr` without
                    // an explicit inttoptr.
                    let ret_ty = self.llvm_fn.get_type().get_return_type();
                    let coerced: BasicValueEnum = match (v, ret_ty) {
                        (BasicValueEnum::IntValue(iv), Some(rt)) if rt.is_pointer_type() => {
                            let ptr_t = self.ctx.ptr_type(AddressSpace::default());
                            self.builder.build_int_to_ptr(iv, ptr_t, "").unwrap().into()
                        }
                        (BasicValueEnum::PointerValue(pv), Some(rt)) if rt.is_int_type() => {
                            let i64_t = self.ctx.i64_type();
                            self.builder.build_ptr_to_int(pv, i64_t, "").unwrap().into()
                        }
                        _ => v,
                    };
                    self.builder.build_return(Some(&coerced)).unwrap();
                }
                None => {
                    self.builder.build_return(None).unwrap();
                }
            },
            Terminator::Unreachable => {
                self.builder.build_unreachable().unwrap();
            }
        }
    }

    pub(super) fn operand(&self, o: &Operand) -> BasicValueEnum<'ctx> {
        match o {
            Operand::Value(v) => *self
                .value_map
                .get(&v.0)
                .unwrap_or_else(|| panic!("unmapped SSA value {}", v.0)),
            Operand::ConstI64(n) => {
                BasicValueEnum::IntValue(self.ctx.i64_type().const_int(*n as u64, true))
            }
            Operand::ConstI32(n) => {
                BasicValueEnum::IntValue(self.ctx.i32_type().const_int(*n as u64, true))
            }
            Operand::ConstF64(n) => BasicValueEnum::FloatValue(self.ctx.f64_type().const_float(*n)),
            Operand::ConstBool(b) => {
                BasicValueEnum::IntValue(self.ctx.bool_type().const_int(*b as u64, false))
            }
            Operand::ConstPtrNull => BasicValueEnum::PointerValue(
                self.ctx
                    .ptr_type(inkwell::AddressSpace::default())
                    .const_null(),
            ),
        }
    }

    pub(super) fn operand_int(&self, o: &Operand) -> IntValue<'ctx> {
        self.operand(o).into_int_value()
    }
}
