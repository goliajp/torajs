//! Str-byte-level helper pair for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 389 — Path A.3-batch10.
//!
//! Two methods forming the "byte-addressable Str/Substr access"
//! layer — the substrate that ~all inline Str comparison / switch
//! fast paths lower through:
//!
//! - `emit_str_data_base(op, ty)` — compute the byte-data location
//!   for a Str / Substr operand, returned as
//!   `(base_ptr, byte_offset_into_base)`. Caller then uses
//!   `LoadDyn(byte_ty, base_ptr, total_offset)` where
//!   `total_offset = base_offset + per_byte_index`. Str is inline at
//!   `(self, 16)`; Substr resolves parent + STR_HDR(16) + view_offset
//!   via 2 loads + 1 add.
//! - `emit_inline_str_eq_bytes(other, bytes)` — emit inline
//!   byte-by-byte `Str === &[u8]` comparison, short-circuiting on
//!   first mismatch. Skips the `__torajs_str_eq` C-runtime fn-call so
//!   LLVM can unroll the 1-2 byte cases and collapse longer ones to a
//!   single wide load + cmp. Callers: `Stmt::Switch` fast path
//!   (ssa_lower_stmt_switch) + `try_inline_str_eq_with_literal`
//!   (still in ssa_lower.rs).
//!
//! Method bodies preserved byte-for-byte; the sibling reaches LowerCtx
//! fields via `impl<'a> super::LowerCtx<'a>`, so call sites need zero
//! edits (call sites in ssa_lower_stmt_switch and the remaining
//! try_inline_str_eq_with_literal in ssa_lower.rs still call via
//! `ctx.emit_inline_str_eq_bytes(...)` / `self.emit_str_data_base(...)`
//! which resolve to this impl automatically).

use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Compute the byte-data location for a Str / Substr operand,
    /// returned as `(base_ptr, byte_offset_into_base)`. Caller uses
    /// LoadDyn(type, base_ptr, total_offset) where total_offset =
    /// base_byte_offset + per-byte index.
    ///
    /// For OWNED Str: `(self, 16)` — bytes inline at self+16.
    /// For Substr: `(parent_ptr, STR_HDR(16) + offset)` — the parent's
    ///   bytes start at parent+16, view starts at parent+16+offset.
    ///   The view offset counts code units, so this is the byte offset
    ///   only into a Latin-1 parent.
    /// Returns `(base_ptr, base_offset_value_or_const)`.
    pub(crate) fn emit_str_data_base(&mut self, op: Operand, ty: Type) -> (Operand, Operand) {
        match ty {
            Type::Str => (op, Operand::ConstI64(16)),
            Type::Substr => {
                let parent = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, op, 16),
                    Type::Ptr,
                    None,
                );
                let offset = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, op, 24),
                    Type::I64,
                    None,
                );
                // 16 + offset → byte offset into parent
                let total_off = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Add, Operand::Value(offset), Operand::ConstI64(16)),
                    Type::I64,
                    None,
                );
                (Operand::Value(parent), Operand::Value(total_off))
            }
            other => panic!("emit_str_data_base: unsupported type {other:?}"),
        }
    }

    /// Emit inline byte-by-byte `Str === &[u8]` comparison. Returns a
    /// bool Operand. Walks bytes [0..bytes.len()) of `other`; first
    /// mismatch short-circuits to false. For len=0 just returns
    /// `len(other) == 0`.
    ///
    /// Skips the `__torajs_str_eq` C-runtime fn-call (which lives in
    /// a separately-compiled module so LLVM can't inline it). For tiny
    /// literals (1-2 bytes) this unrolls to a few cycles; for longer
    /// (up to caller-defined cap) LLVM's loop opts often collapse to
    /// a single wide load + cmp.
    pub(crate) fn emit_inline_str_eq_bytes(&mut self, other: Operand, bytes: &[u8]) -> Operand {
        // For Substr we still load len at offset 8 (same as Str), but
        // bytes are accessed via (parent_data + offset). Compute the
        // data pointer once per call, then per-byte loads use it.
        let other_ty = self.operand_ty(&other);
        let result_slot = self.alloca_in_entry(Type::Bool, Some("__streq_r"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(result_slot), 0),
        );
        let done_blk = self.f.add_block();
        // step 1: len-eq. Str/Substr layout fork lives in the
        // ssa_lower_str sidekick — see load_str_or_substr_length.
        let other_len = match crate::ssa_lower_str::load_str_or_substr_length(self, other, other_ty)
        {
            Operand::Value(v) => v,
            _ => unreachable!("length helper always yields a Value"),
        };
        let len_eq = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(
                IPred::Eq,
                Operand::Value(other_len),
                Operand::ConstI64(bytes.len() as i64),
            ),
            Type::Bool,
            None,
        );
        let cmp_blk = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(len_eq),
                then_blk: cmp_blk,
                else_blk: done_blk,
            },
        );
        self.cur_block = cmp_blk;
        if bytes.is_empty() {
            // len-eq alone determines truth.
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::ConstBool(true), Operand::Value(result_slot), 0),
            );
            self.f.set_term(self.cur_block, Terminator::Br(done_blk));
        } else {
            // Compute (base_ptr, base_offset) once. For Str: (self, 16) —
            // const-folded immediate. For Substr: 2 loads + 1 add to
            // resolve parent + 16 + view_offset, amortized over chain.
            let (base, base_off) = self.emit_str_data_base(other, other_ty);
            // `base` is the Str that holds the payload (the operand
            // itself, or a view's parent), so its header carries the
            // encoding. A UTF-16 payload of the same length compares
            // unit by unit at stride 2.
            let hdr = self.f.append_inst(
                self.cur_block,
                InstKind::Load(Type::I64, base.clone(), 0),
                Type::I64,
                None,
            );
            let lat_bit = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(
                    SsaBinOp::And,
                    Operand::Value(hdr),
                    Operand::ConstI64(HDR_MASK_IS_LATIN1),
                ),
                Type::I64,
                None,
            );
            let is_lat = self.f.append_inst(
                self.cur_block,
                InstKind::ICmp(IPred::Ne, Operand::Value(lat_bit), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let narrow = self.f.add_block();
            let wide = self.f.add_block();
            self.f.set_term(
                self.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(is_lat),
                    then_blk: narrow,
                    else_blk: wide,
                },
            );
            // A view's offset counts code units: `16 + off` bytes into
            // a Latin-1 parent, `16 + 2 * off` into a UTF-16 one.
            let wide_off = match &base_off {
                Operand::ConstI64(k) => Operand::ConstI64(*k),
                narrow_off => {
                    let twice = self.f.append_inst(
                        wide,
                        InstKind::BinOp(SsaBinOp::Add, narrow_off.clone(), narrow_off.clone()),
                        Type::I64,
                        None,
                    );
                    let off = self.f.append_inst(
                        wide,
                        InstKind::BinOp(
                            SsaBinOp::Sub,
                            Operand::Value(twice),
                            Operand::ConstI64(16),
                        ),
                        Type::I64,
                        None,
                    );
                    Operand::Value(off)
                }
            };
            let chain = UnitChain {
                base,
                bytes,
                result_slot,
                done_blk,
            };
            self.emit_unit_chain(&chain, narrow, base_off, 1, 0xff);
            self.emit_unit_chain(&chain, wide, wide_off, 2, 0xffff);
        }
        self.cur_block = done_blk;
        let r = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Bool, Operand::Value(result_slot), 0),
            Type::Bool,
            None,
        );
        Operand::Value(r)
    }

    /// One unit-by-unit compare chain of [`Self::emit_inline_str_eq_bytes`]
    /// starting at `entry`: unit `i` is read at `base_off + i * stride`
    /// and masked to the unit width; the first mismatch exits false.
    fn emit_unit_chain(
        &mut self,
        c: &UnitChain<'_>,
        entry: BlockId,
        base_off: Operand,
        stride: i64,
        mask: i32,
    ) {
        let mut cur = entry;
        for (i, &want) in c.bytes.iter().enumerate() {
            let off_i = self.f.append_inst(
                cur,
                InstKind::BinOp(
                    SsaBinOp::Add,
                    base_off.clone(),
                    Operand::ConstI64(i as i64 * stride),
                ),
                Type::I64,
                None,
            );
            let raw = self.f.append_inst(
                cur,
                InstKind::LoadDyn(Type::I32, c.base.clone(), Operand::Value(off_i)),
                Type::I32,
                None,
            );
            let unit = self.f.append_inst(
                cur,
                InstKind::BinOp(SsaBinOp::And, Operand::Value(raw), Operand::ConstI32(mask)),
                Type::I32,
                None,
            );
            let eq = self.f.append_inst(
                cur,
                InstKind::ICmp(
                    IPred::Eq,
                    Operand::Value(unit),
                    Operand::ConstI32(want as i32),
                ),
                Type::Bool,
                None,
            );
            let next = self.f.add_block();
            self.f.set_term(
                cur,
                Terminator::CondBr {
                    cond: Operand::Value(eq),
                    then_blk: next,
                    else_blk: c.done_blk,
                },
            );
            cur = next;
        }
        self.f.append_void(
            cur,
            InstKind::Store(Operand::ConstBool(true), Operand::Value(c.result_slot), 0),
        );
        self.f.set_term(cur, Terminator::Br(c.done_blk));
    }
}

/// What both encoding arms of an inline literal compare share.
struct UnitChain<'b> {
    base: Operand,
    bytes: &'b [u8],
    result_slot: ValueId,
    done_blk: BlockId,
}

/// `IS_LATIN1` in the packed Str header word (`flags u16 @6`, bit 1).
const HDR_MASK_IS_LATIN1: i64 = 0x0002i64 << 48;
