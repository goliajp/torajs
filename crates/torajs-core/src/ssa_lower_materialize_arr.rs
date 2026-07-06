//! Boundary materialize helper for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 381 — Path A.3-batch2.
//!
//! Single method:
//!
//! - `materialize_arr_substr_to_str(src, declared_ty)` — takes an
//!   `Array<Substr>` and returns a fresh `Array<Str>` with each element
//!   `substr_to_owned`'d, then drops the source array. Used at fn /
//!   closure return sites where the declared type is `Array<Str>` but
//!   the body produced `Array<Substr>` (e.g. closure body
//!   `s => s.split("")`).
//!
//! The body compiles a per-element loop with condbr header / body /
//! after blocks, reading each `Substr` via `LoadDyn` at the head-aware
//! source offset and storing the owned `Str` at the raw physical offset
//! into the freshly-alloc'd destination (head=0).
//!
//! Method body is byte-for-byte preserved from the source; the sibling
//! reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`, so call
//! sites need zero edits.

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

impl<'a> LowerCtx<'a> {
    /// Boundary materialize: take an Array<Substr> and return a fresh
    /// Array<Str> with each element substr_to_owned'd. Drops the
    /// source array (its element-walk dec's parents; the new array's
    /// elements own the bytes outright). Used at fn / closure return
    /// sites where the declared type is Array<Str> but the body
    /// produced Array<Substr> (e.g. closure body `s => s.split("")`).
    pub(crate) fn materialize_arr_substr_to_str(
        &mut self,
        src: Operand,
        declared_ty: Type,
    ) -> Operand {
        let src_len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, src, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let dst = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.arr_alloc, vec![Operand::Value(src_len)]),
            declared_ty,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(src_len), Operand::Value(dst), ARR_LEN_OFF),
        );
        // Per-element loop: substr_to_owned each.
        let i_slot = self.alloca(Type::I64, Some("__mat_i"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
        );
        let header = self.f.add_block();
        let body = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = header;
        let i_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let cmp = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(src_len)),
            Type::Bool,
            None,
        );
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(cmp),
                then_blk: body,
                else_blk: after,
            },
        );
        self.cur_block = body;
        // T-13.5: src may be shifted (head>0) — use head-aware offset.
        // dst is freshly allocated above so head=0; store through its
        // data pointer at i*8.
        let (src_off_base, src_off) =
            self.emit_arr_slot_byte_offset(src.clone(), Operand::Value(i_now), 3, false);
        let dst_data = self.emit_arr_data_ptr(Operand::Value(dst));
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        let substr_v = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(Type::Substr, src_off_base.clone(), src_off),
            Type::Substr,
            None,
        );
        let owned = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.substr_to_owned,
                vec![Operand::Value(substr_v)],
            ),
            Type::Str,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::StoreDyn(Operand::Value(owned), dst_data.clone(), Operand::Value(off)),
        );
        let i_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = after;
        // Drop the source Array<Substr> — its element-walk dec's each
        // substr (which dec's parent), then frees the array block.
        let src_arr_substr_ty = self.operand_ty(&src);
        self.emit_drop_value(src, src_arr_substr_ty);
        Operand::Value(dst)
    }
}
