//! F64 undefined-NaN sentinel → Any-world crossers (RFC
//! 20260708-typed-arr-oob-read chunks 2-3), split from
//! `ssa_lower_any_box.rs` when chunk 3's pair mirror pushed that
//! file past the 500-line limit.
//!
//! A `number[]` OOB index read answers the sentinel bits
//! ([`crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS`]);
//! when such a possibly-sentinel F64 (statically gated by
//! [`crate::ssa_lower_undef_f64_source::is_undef_f64_source`])
//! crosses into the boxed-Any or (tag, value) world, these two
//! helpers branch on the bits so the any world sees a real
//! `undefined` instead of a NaN with our payload.

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS;

impl<'a> LowerCtx<'a> {
    /// RFC 20260708-typed-arr-oob-read chunk 2 — branch on the
    /// undefined-NaN sentinel bits: ANY_UNDEF box vs the plain
    /// F64 box. pub(crate) since chunk 3: the multi-arg
    /// `console.log` path routes possibly-sentinel F64 args here.
    pub(crate) fn box_f64_or_undef(&mut self, val: Operand) -> Operand {
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::BitCastF64ToI64(val),
            Type::I64,
            None,
        );
        let is_undef = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(
                IPred::Eq,
                Operand::Value(bits),
                Operand::ConstI64(F64_UNDEF_SENTINEL_BITS as i64),
            ),
            Type::Bool,
            None,
        );
        let undef_blk = self.f.add_block();
        let num_blk = self.f.add_block();
        let merge = self.f.add_block();
        let slot = self.alloca_in_entry(Type::Any, Some("__f64box"));
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(is_undef),
                then_blk: undef_blk,
                else_blk: num_blk,
            },
        );
        self.cur_block = undef_blk;
        let u = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(u), Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = num_blk;
        let n = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(3), Operand::Value(bits)],
            ),
            Type::Any,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(n), Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        let out = self.f.append_inst(
            merge,
            InstKind::Load(Type::Any, Operand::Value(slot), 0),
            Type::Any,
            None,
        );
        Operand::Value(out)
    }

    /// RFC 20260708-typed-arr-oob-read chunk 3 — (tag, value) pair
    /// mirror of [`Self::box_f64_or_undef`]: branch on the
    /// undefined-NaN sentinel bits and answer `(ANY_UNDEF, 0)` vs
    /// `(ANY_F64, bits)` through a pair of merge slots.
    pub(crate) fn tag_value_f64_or_undef(&mut self, val: Operand) -> (Operand, Operand) {
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::BitCastF64ToI64(val),
            Type::I64,
            None,
        );
        let is_undef = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(
                IPred::Eq,
                Operand::Value(bits),
                Operand::ConstI64(F64_UNDEF_SENTINEL_BITS as i64),
            ),
            Type::Bool,
            None,
        );
        let undef_blk = self.f.add_block();
        let num_blk = self.f.add_block();
        let merge = self.f.add_block();
        let tag_slot = self.alloca_in_entry(Type::I64, Some("__f64tag"));
        let val_slot = self.alloca_in_entry(Type::I64, Some("__f64val"));
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(is_undef),
                then_blk: undef_blk,
                else_blk: num_blk,
            },
        );
        self.f.append_void(
            undef_blk,
            InstKind::Store(Operand::ConstI64(5), Operand::Value(tag_slot), 0),
        );
        self.f.append_void(
            undef_blk,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(val_slot), 0),
        );
        self.f.set_term(undef_blk, Terminator::Br(merge));
        self.f.append_void(
            num_blk,
            InstKind::Store(Operand::ConstI64(3), Operand::Value(tag_slot), 0),
        );
        self.f.append_void(
            num_blk,
            InstKind::Store(Operand::Value(bits), Operand::Value(val_slot), 0),
        );
        self.f.set_term(num_blk, Terminator::Br(merge));
        self.cur_block = merge;
        let tag = self.f.append_inst(
            merge,
            InstKind::Load(Type::I64, Operand::Value(tag_slot), 0),
            Type::I64,
            None,
        );
        let v = self.f.append_inst(
            merge,
            InstKind::Load(Type::I64, Operand::Value(val_slot), 0),
            Type::I64,
            None,
        );
        (Operand::Value(tag), Operand::Value(v))
    }
}
