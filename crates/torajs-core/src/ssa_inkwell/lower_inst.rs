//! `FnLower::lower_inst` — per-Inst lowering. The biggest single
//! method of `FnLower`, split into its own file (separate impl
//! block of the same type) so `ssa_inkwell/lower.rs` stays focused
//! on the small lifecycle methods.
//!
//! Extracted from `ssa_inkwell.rs` (2026-05-25, god-file decomp
//! batch 21).
//!
//! CARVE-OUT (file-size.md §Carve-out #2 dispatch table): this
//! single 396-LOC method is structurally a `match` over every
//! `InstKind` variant. Per-variant arm bodies are short enough
//! individually; the length comes from variant count. Further
//! sub-split per variant is a follow-up batch.

use inkwell::AddressSpace;
use inkwell::debug_info::AsDIScope;
use inkwell::types::BasicMetadataTypeEnum;
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum};
use inkwell::{FloatPredicate, IntPredicate};

use super::ctx_f64;
use super::lower::FnLower;
use super::types::{basic_type, build_fn_type};
use crate::ssa::{self as s, BinOp, FPred, IPred, InstKind};

impl<'a, 'ctx> FnLower<'a, 'ctx> {
    pub(super) fn lower_inst(&mut self, inst: &s::Inst) {
        // v0.3 #4 D-3 — when DWARF debug info is enabled, look up
        // this Inst's `origin` ExprId, translate its byte span to
        // (line, col), and stamp a DILocation on the builder so all
        // build_* calls until the next override carry !dbg.
        // origin == None (synthetic Insts not tied to a user-Expr)
        // inherits the previous DILocation; this matches DWARF's
        // intent for compiler-emitted helper sequences.
        if let (Some(dctx), Some(ast), Some(eid), Some(sp)) = (
            self.debug_ctx,
            self.ast,
            inst.origin,
            self.llvm_fn.get_subprogram(),
        ) {
            let span = ast.expr_spans.get(eid.0 as usize).copied();
            if let Some(span) = span {
                let (line, col) = ast.byte_to_line_col(span.start);
                if line > 0 {
                    let loc = dctx.dibuilder.create_debug_location(
                        self.ctx,
                        line,
                        col,
                        sp.as_debug_info_scope(),
                        None,
                    );
                    self.builder.set_current_debug_location(loc);
                }
            }
        }
        let result_val = match &inst.kind {
            InstKind::BinOp(op, a, b) => {
                let r: BasicValueEnum = match op {
                    BinOp::Add
                    | BinOp::Sub
                    | BinOp::Mul
                    | BinOp::SDiv
                    | BinOp::SRem
                    | BinOp::And
                    | BinOp::Or
                    | BinOp::Xor
                    | BinOp::Shl
                    | BinOp::AShr
                    | BinOp::LShr => {
                        let av = self.operand_int(a);
                        let bv = self.operand_int(b);
                        let r = match op {
                            BinOp::Add => self.builder.build_int_add(av, bv, "").unwrap(),
                            BinOp::Sub => self.builder.build_int_sub(av, bv, "").unwrap(),
                            BinOp::Mul => self.builder.build_int_mul(av, bv, "").unwrap(),
                            BinOp::SDiv => self.builder.build_int_signed_div(av, bv, "").unwrap(),
                            BinOp::SRem => self.builder.build_int_signed_rem(av, bv, "").unwrap(),
                            BinOp::And => self.builder.build_and(av, bv, "").unwrap(),
                            BinOp::Or => self.builder.build_or(av, bv, "").unwrap(),
                            BinOp::Xor => self.builder.build_xor(av, bv, "").unwrap(),
                            BinOp::Shl => self.builder.build_left_shift(av, bv, "").unwrap(),
                            BinOp::AShr => {
                                self.builder.build_right_shift(av, bv, true, "").unwrap()
                            }
                            BinOp::LShr => {
                                self.builder.build_right_shift(av, bv, false, "").unwrap()
                            }
                            _ => unreachable!(),
                        };
                        BasicValueEnum::IntValue(r)
                    }
                    BinOp::FAdd | BinOp::FSub | BinOp::FMul | BinOp::FDiv | BinOp::FRem => {
                        let av = self.operand(a).into_float_value();
                        let bv = self.operand(b).into_float_value();
                        let r = match op {
                            BinOp::FAdd => self.builder.build_float_add(av, bv, "").unwrap(),
                            BinOp::FSub => self.builder.build_float_sub(av, bv, "").unwrap(),
                            BinOp::FMul => self.builder.build_float_mul(av, bv, "").unwrap(),
                            BinOp::FDiv => self.builder.build_float_div(av, bv, "").unwrap(),
                            BinOp::FRem => self.builder.build_float_rem(av, bv, "").unwrap(),
                            _ => unreachable!(),
                        };
                        BasicValueEnum::FloatValue(r)
                    }
                };
                Some(r)
            }
            InstKind::ICmp(p, a, b) => {
                let pred = match p {
                    IPred::Eq => IntPredicate::EQ,
                    IPred::Ne => IntPredicate::NE,
                    IPred::Slt => IntPredicate::SLT,
                    IPred::Sgt => IntPredicate::SGT,
                    IPred::Sle => IntPredicate::SLE,
                    IPred::Sge => IntPredicate::SGE,
                };
                // Allow pointer compares (used by `=== null` / `!== null`
                // and the optional-chain / nullish dispatchers). LLVM's
                // build_int_compare accepts ptr-typed operands; mixing
                // one ptr + one i64 needs an explicit ptrtoint cast on
                // the ptr side.
                let av_basic = self.operand(a);
                let bv_basic = self.operand(b);
                let av_is_ptr = matches!(av_basic, BasicValueEnum::PointerValue(_));
                let bv_is_ptr = matches!(bv_basic, BasicValueEnum::PointerValue(_));
                let r = if av_is_ptr && bv_is_ptr {
                    self.builder
                        .build_int_compare(
                            pred,
                            av_basic.into_pointer_value(),
                            bv_basic.into_pointer_value(),
                            "",
                        )
                        .unwrap()
                } else if av_is_ptr || bv_is_ptr {
                    let i64_t = self.ctx.i64_type();
                    let av_int = if av_is_ptr {
                        self.builder
                            .build_ptr_to_int(av_basic.into_pointer_value(), i64_t, "")
                            .unwrap()
                    } else {
                        av_basic.into_int_value()
                    };
                    let bv_int = if bv_is_ptr {
                        self.builder
                            .build_ptr_to_int(bv_basic.into_pointer_value(), i64_t, "")
                            .unwrap()
                    } else {
                        bv_basic.into_int_value()
                    };
                    self.builder
                        .build_int_compare(pred, av_int, bv_int, "")
                        .unwrap()
                } else {
                    let av = av_basic.into_int_value();
                    let bv = bv_basic.into_int_value();
                    self.builder.build_int_compare(pred, av, bv, "").unwrap()
                };
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::FCmp(p, a, b) => {
                let av = self.operand(a).into_float_value();
                let bv = self.operand(b).into_float_value();
                let pred = match p {
                    FPred::Oeq => FloatPredicate::OEQ,
                    FPred::One => FloatPredicate::ONE,
                    FPred::Olt => FloatPredicate::OLT,
                    FPred::Ogt => FloatPredicate::OGT,
                    FPred::Ole => FloatPredicate::OLE,
                    FPred::Oge => FloatPredicate::OGE,
                    FPred::Une => FloatPredicate::UNE,
                };
                let r = self.builder.build_float_compare(pred, av, bv, "").unwrap();
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::SiToFp(op) => {
                let v = self.operand_int(op);
                let f = ctx_f64(self.ctx);
                let r = self.builder.build_signed_int_to_float(v, f, "").unwrap();
                Some(BasicValueEnum::FloatValue(r))
            }
            InstKind::FpToSi(op) => {
                let v = self.operand(op).into_float_value();
                let i = self.ctx.i64_type();
                let r = self.builder.build_float_to_signed_int(v, i, "").unwrap();
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::ZExtBoolToI64(op) => {
                let v = self.operand_int(op);
                let i64_ty = self.ctx.i64_type();
                let r = self.builder.build_int_z_extend(v, i64_ty, "").unwrap();
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::BitCastF64ToI64(op) => {
                // T-10.d.ii — pun the f64's IEEE 754 bit pattern as i64
                // for the ANY_F64 tagged-slot stash. LLVM `bitcast`
                // preserves bits exactly (vs `fptosi` which truncates).
                let v = self.operand(op).into_float_value();
                let i64_ty = self.ctx.i64_type();
                let r = self.builder.build_bit_cast(v, i64_ty, "").unwrap();
                Some(r)
            }
            InstKind::BitCastI64ToF64(op) => {
                let v = self.operand_int(op);
                let f64_ty = self.ctx.f64_type();
                let r = self.builder.build_bit_cast(v, f64_ty, "").unwrap();
                Some(r)
            }
            InstKind::IntToPtr(op) => {
                // T-15.g.6.c — i64 → ptr (opaque pointer at LLVM 22).
                // Used by the await Member-access dispatch when
                // Promise<T>'s inner T is heap-typed: runtime helper
                // returns int64_t per its C ABI; SSA needs the result
                // typed as ptr-shape so downstream Member / Index
                // instructions dispatch correctly.
                let v = self.operand_int(op);
                let ptr_ty = self.ctx.ptr_type(AddressSpace::default());
                let r = self.builder.build_int_to_ptr(v, ptr_ty, "").unwrap();
                Some(BasicValueEnum::PointerValue(r))
            }
            InstKind::TruncI64ToBool(op) => {
                // T-15.g.6.c — i64 → i1 narrow. Symmetric reverse
                // of ZExtBoolToI64. Pack/unpack across the Promise's
                // int64_t value slot.
                let v = self.operand_int(op);
                let i1_ty = self.ctx.bool_type();
                let r = self.builder.build_int_truncate(v, i1_ty, "").unwrap();
                Some(BasicValueEnum::IntValue(r))
            }
            InstKind::StringRef(sid) => {
                let g = self.string_globals[sid.0 as usize];
                Some(BasicValueEnum::PointerValue(g.as_pointer_value()))
            }
            InstKind::StaticStrRef(sid) => {
                let g = self.static_str_globals[sid.0 as usize];
                Some(BasicValueEnum::PointerValue(g.as_pointer_value()))
            }
            InstKind::GlobalRef(name) => {
                let g = self
                    .data_globals
                    .get(name)
                    .or_else(|| self.vtable_globals.get(name))
                    .unwrap_or_else(|| panic!("ssa-inkwell: unknown global `{name}`"));
                Some(BasicValueEnum::PointerValue(g.as_pointer_value()))
            }
            InstKind::Call(fid, args) => {
                // M6.1 / Array<string> — coerce ptr ↔ i64 args at the
                // call boundary. SSA's i64 / Ptr / Str / Obj / Arr /
                // FnSig / Closure are all 8-byte values but LLVM IR's
                // verifier requires explicit ptrtoint / inttoptr at call
                // sites where the function expected one but got the
                // other. (Cranelift is size-based and accepts either
                // silently, hence the JIT path was working before this
                // patch.) Only fires when the type kinds genuinely
                // differ — same-shape calls are zero-cost.
                let callee = self.fn_map[fid.0 as usize];
                let expected = callee.get_type().get_param_types();
                let i64_t = self.ctx.i64_type();
                let f64_t = self.ctx.f64_type();
                let ptr_t = self.ctx.ptr_type(AddressSpace::default());
                let mut argv: Vec<BasicMetadataValueEnum> = Vec::with_capacity(args.len());
                for (i, a) in args.iter().enumerate() {
                    let raw = self.operand(a);
                    let coerced: BasicValueEnum = if i < expected.len() {
                        match expected[i] {
                            BasicMetadataTypeEnum::IntType(it) => match raw {
                                BasicValueEnum::PointerValue(p) => {
                                    self.builder.build_ptr_to_int(p, i64_t, "").unwrap().into()
                                }
                                BasicValueEnum::FloatValue(f) => {
                                    // Float arg into an int param —
                                    // truncate via fptosi (matches JS
                                    // ToInt32 / ToUint32 prefix on
                                    // Math.imul / charAt-with-float-index
                                    // / parseInt-with-float-radix).
                                    let _ = it;
                                    self.builder
                                        .build_float_to_signed_int(f, i64_t, "")
                                        .unwrap()
                                        .into()
                                }
                                _ => raw,
                            },
                            BasicMetadataTypeEnum::FloatType(_) => match raw {
                                BasicValueEnum::IntValue(v) => self
                                    .builder
                                    .build_signed_int_to_float(v, f64_t, "")
                                    .unwrap()
                                    .into(),
                                _ => raw,
                            },
                            BasicMetadataTypeEnum::PointerType(_) => {
                                if let BasicValueEnum::IntValue(v) = raw {
                                    self.builder.build_int_to_ptr(v, ptr_t, "").unwrap().into()
                                } else {
                                    raw
                                }
                            }
                            _ => raw,
                        }
                    } else {
                        raw
                    };
                    argv.push(coerced.into());
                }
                let call = self.builder.build_call(callee, &argv, "").unwrap();
                let kind = call.try_as_basic_value();
                if kind.is_basic() {
                    Some(kind.unwrap_basic())
                } else {
                    None // void call
                }
            }
            InstKind::Alloca(t) => {
                let bt = basic_type(self.ctx, *t);
                let p = self.builder.build_alloca(bt, "").unwrap();
                Some(BasicValueEnum::PointerValue(p))
            }
            InstKind::AllocaBytes(n) => {
                // i8 array of n elements — yields a `[N x i8]*` of
                // exactly N bytes, 1-byte aligned by default. We bump
                // alignment to 8 since both SplitIter and Substr have
                // 8-byte fields.
                let i8_t = self.ctx.i8_type();
                let arr_t = i8_t.array_type(*n as u32);
                let p = self.builder.build_alloca(arr_t, "").unwrap();
                p.as_instruction().unwrap().set_alignment(8).unwrap();
                Some(BasicValueEnum::PointerValue(p))
            }
            InstKind::Load(t, ptr, offset) => {
                let bt = basic_type(self.ctx, *t);
                let p = self.operand(ptr).into_pointer_value();
                let p = if *offset == 0 {
                    p
                } else {
                    let i64_t = self.ctx.i64_type();
                    let i8_t = self.ctx.i8_type();
                    unsafe {
                        self.builder
                            .build_in_bounds_gep(i8_t, p, &[i64_t.const_int(*offset, false)], "")
                            .unwrap()
                    }
                };
                let v = self.builder.build_load(bt, p, "").unwrap();
                Some(v)
            }
            InstKind::Store(val, ptr, offset) => {
                let v = self.operand(val);
                let p = self.operand(ptr).into_pointer_value();
                let p = if *offset == 0 {
                    p
                } else {
                    let i64_t = self.ctx.i64_type();
                    let i8_t = self.ctx.i8_type();
                    unsafe {
                        self.builder
                            .build_in_bounds_gep(i8_t, p, &[i64_t.const_int(*offset, false)], "")
                            .unwrap()
                    }
                };
                self.builder.build_store(p, v).unwrap();
                None
            }
            InstKind::LoadDyn(t, base, off) => {
                let bt = basic_type(self.ctx, *t);
                let p = self.operand(base).into_pointer_value();
                let i8_t = self.ctx.i8_type();
                let off_v = self.operand_int(off);
                let addr = unsafe {
                    self.builder
                        .build_in_bounds_gep(i8_t, p, &[off_v], "")
                        .unwrap()
                };
                let v = self.builder.build_load(bt, addr, "").unwrap();
                Some(v)
            }
            InstKind::StoreDyn(val, base, off) => {
                let v = self.operand(val);
                let p = self.operand(base).into_pointer_value();
                let i8_t = self.ctx.i8_type();
                let off_v = self.operand_int(off);
                let addr = unsafe {
                    self.builder
                        .build_in_bounds_gep(i8_t, p, &[off_v], "")
                        .unwrap()
                };
                self.builder.build_store(addr, v).unwrap();
                None
            }
            InstKind::FnAddr(fid) => {
                // Take the address of an imported fn — Inkwell's
                // FunctionValue exposes its global address via
                // `as_global_value().as_pointer_value()`.
                let target = self.fn_map[fid.0 as usize];
                let p = target.as_global_value().as_pointer_value();
                Some(BasicValueEnum::PointerValue(p))
            }
            InstKind::CallIndirect(sig_id, ptr, args) => {
                // Look up the interned signature, build the LLVM
                // FunctionType, then build_indirect_call.
                let (params, ret) = self.ssa_module.signature(*sig_id).clone();
                let fn_t = build_fn_type(self.ctx, &params, ret);
                let p = self.operand(ptr).into_pointer_value();
                let argv: Vec<BasicMetadataValueEnum> =
                    args.iter().map(|a| self.operand(a).into()).collect();
                let call = self
                    .builder
                    .build_indirect_call(fn_t, p, &argv, "")
                    .unwrap();
                let kind = call.try_as_basic_value();
                if kind.is_basic() {
                    Some(kind.unwrap_basic())
                } else {
                    None
                }
            }
        };

        if let (Some(r), Some(v)) = (inst.result, result_val) {
            self.value_map.insert(r.0, v);
        }
    }
}
