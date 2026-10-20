//! The capacity a fill loop reserves before its unchecked appends, and
//! the RangeError of an append past the longest array.

use super::ARRAY_LENGTH_MAX;
use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

impl LowerCtx<'_> {
    /// The capacity, in slots, that `bound` trips of `per_trip`
    /// appends each need on top of the elements of `arr` (of type
    /// `arr_ty`): an element held in place takes one slot per field.
    /// A capacity counts from the buffer's start and a shifted array's
    /// elements start at its deque head, so the head is added.
    ///
    /// Any shape but one plain append a trip multiplies, and a count
    /// past the buffer's u32 capacity would leave the loop's unchecked
    /// stores writing past it; such trips may take the length past the
    /// u32 elements an array holds (§10.4.2.4), where an append raises
    /// a RangeError and every append before it stays. So the test
    /// branches to `slow`, minted on the first test a loop needs, where
    /// the caller lowers the loop once more with checked pushes.
    pub(crate) fn emit_reserve_slots(
        &mut self,
        arr_ty: Type,
        arr: ValueId,
        bound: Operand,
        per_trip: i64,
        slow: &mut Option<BlockId>,
    ) -> ValueId {
        let k = match arr_ty {
            Type::Arr(a) => {
                let elem = self.arr_layouts[a.0 as usize];
                crate::ssa_lower::inline_elem::elem_stride(self.struct_layouts, elem) as i64 / 8
            }
            _ => 1,
        };
        let cb = self.cur_block;
        let len = InstKind::Load(Type::I64, Operand::Value(arr), ARR_LEN_OFF);
        let len = self.f.append_inst(cb, len, Type::I64, None);
        let head = self.reserve_head(arr);
        if k == 1 && per_trip == 1 {
            let total = self.bin(SsaBinOp::Add, Operand::Value(len), bound, Type::I64);
            return self.plus_head(total, head);
        }
        let max = ARRAY_LENGTH_MAX / k;
        let added = self.bin(SsaBinOp::Mul, bound, Operand::ConstI64(per_trip), Type::I64);
        let total = self.bin(SsaBinOp::Add, Operand::Value(len), added, Type::I64);
        let slots = match k {
            1 => total,
            _ => self.bin(SsaBinOp::Mul, total, Operand::ConstI64(k), Type::I64),
        };
        let slots = Operand::Value(self.plus_head(slots, head));
        let many = self.cmp(IPred::Sgt, bound, Operand::ConstI64(max / per_trip));
        let past = self.cmp(IPred::Sgt, slots, Operand::ConstI64(ARRAY_LENGTH_MAX));
        let long = self.bin(SsaBinOp::Or, many, past, Type::Bool);
        let slow_blk = *slow.get_or_insert_with(|| self.f.add_block());
        let cont_blk = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: long,
                then_blk: slow_blk,
                else_blk: cont_blk,
            },
        );
        self.cur_block = cont_blk;
        match slots {
            Operand::Value(v) => v,
            _ => unreachable!("an emitted sum is a value"),
        }
    }

    /// The deque head of the array a reserve is for, in slots; none
    /// where the census pins every head at 0.
    fn reserve_head(&mut self, arr: ValueId) -> Option<Operand> {
        (!self.proofs.arr_head_pinned()).then(|| {
            let x8 = self.emit_arr_head_x8(Operand::Value(arr));
            self.bin(SsaBinOp::LShr, x8, Operand::ConstI64(3), Type::I64)
        })
    }

    fn plus_head(&mut self, v: Operand, head: Option<Operand>) -> ValueId {
        let sum = match head {
            Some(h) => self.bin(SsaBinOp::Add, v, h, Type::I64),
            None => v,
        };
        match sum {
            Operand::Value(v) => v,
            _ => unreachable!("an emitted sum is a value"),
        }
    }

    /// `long` raises "Invalid array length" (the length check of
    /// `__torajs_arr_set_length_validate`, handed a length one past the
    /// largest) and leaves by the throw edge.
    pub(crate) fn emit_too_long_guard(&mut self, long: Operand) {
        let raise_blk = self.f.add_block();
        let cont_blk = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: long,
                then_blk: raise_blk,
                else_blk: cont_blk,
            },
        );
        self.cur_block = raise_blk;
        let check = InstKind::Call(
            self.intrinsics.arr_set_length_validate,
            vec![
                Operand::ConstI64(2),
                Operand::ConstI64(ARRAY_LENGTH_MAX + 1),
            ],
        );
        self.f.append_void(raise_blk, check);
        // the validate always raises
        self.emit_throw_now();
        self.cur_block = cont_blk;
    }
}
