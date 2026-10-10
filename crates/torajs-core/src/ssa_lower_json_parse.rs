//! ②.7 / M6.3 — caller-typed `JSON.parse` lowering: the slot
//! annotation drives a cursor-based typed parse (scalars, arrays,
//! named-type structs, nested). Extracted from `ssa_lower.rs`
//! (file-size known-debt). The number-domain faces of the target
//! arrive pre-widened to F64 (`num_width/json_seed.rs` — JSON text
//! is runtime data, narrow faces are unprovable).

mod obj;

use crate::ssa::{IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;

impl LowerCtx<'_> {
    /// M6.3 — recursive `JSON.parse` codegen. The caller drives type
    /// inference (`let v: T = JSON.parse(text)`) so this fn sees the
    /// concrete `slot_ty` and emits per-shape intrinsic calls,
    /// threading a single cursor alloca through every recursive
    /// call. Each helper advances the cursor past its consumed
    /// token; on syntactic mismatch the helper sets the throw_value
    /// global, and the caller is responsible for emitting a
    /// `__torajs_throw_check` after this returns (matches the
    /// throw_check shape used elsewhere in ssa_lower).
    ///
    /// `text_op` must be a `Type::Str` operand; `cursor_slot` is a
    /// pointer to an alloca'd `i64` initialized to 0 by the caller.
    pub(crate) fn lower_json_parse(
        &mut self,
        text_op: Operand,
        cursor_ptr: Operand,
        slot_ty: Type,
    ) -> Operand {
        match slot_ty {
            Type::I64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.json_parse_int, vec![text_op, cursor_ptr]),
                    Type::I64,
                    None,
                );
                Operand::Value(v)
            }
            Type::F64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.json_parse_float, vec![text_op, cursor_ptr]),
                    Type::F64,
                    None,
                );
                Operand::Value(v)
            }
            Type::Bool => {
                // Helper returns I64 (0/1); coerce by ne-zero.
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.json_parse_bool, vec![text_op, cursor_ptr]),
                    Type::I64,
                    None,
                );
                let b = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Ne, Operand::Value(v), Operand::ConstI64(0)),
                    Type::Bool,
                    None,
                );
                Operand::Value(b)
            }
            Type::Str => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.json_parse_string, vec![text_op, cursor_ptr]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::Arr(arr_id) => self.lower_json_parse_arr(text_op, cursor_ptr, slot_ty, arr_id),
            Type::Obj(sid) => self.lower_json_parse_obj(text_op, cursor_ptr, sid),
            // RFC 20260808-json-parse-any blade 2 — an `any` slot has
            // no static shape to unfold: the whole text parses at
            // runtime (kernel does its own ToString / cursor /
            // trailing-garbage check; the shared cursor stays unread).
            Type::Any => {
                let arg = self.box_to_any(text_op);
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.json_parse_any, vec![arg]),
                    Type::Any,
                    None,
                );
                Operand::Value(v)
            }
            other => panic!("ssa-lower: JSON.parse into type {other:?} not yet supported"),
        }
    }

    /// `Type::Arr` shape — eat `[`, alloc, then the first/step
    /// header-body loop pushing recursively parsed elements.
    fn lower_json_parse_arr(
        &mut self,
        text_op: Operand,
        cursor_ptr: Operand,
        slot_ty: Type,
        arr_id: crate::ssa::ArrId,
    ) -> Operand {
        {
            let elem_ty = self.arr_layouts[arr_id.0 as usize];
            // eat '['
            self.f.append_void(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.json_eat_char,
                    vec![text_op, cursor_ptr, Operand::ConstI64(b'[' as i64)],
                ),
            );
            // alloc array (cap=0; grows via push). arr_push returns
            // a (possibly realloc'd) pointer — we MUST round-trip
            // each push through an alloca slot or subsequent
            // pushes scribble over freed memory after the first
            // realloc.
            let initial = self.f.append_inst(
                self.cur_block,
                InstKind::Call(self.intrinsics.arr_alloc, vec![Operand::ConstI64(0)]),
                slot_ty,
                None,
            );
            let arr_slot = self.alloca(slot_ty, Some("__json_arr"));
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::Value(initial), Operand::Value(arr_slot), 0),
            );
            // arr_first('[', ']') — 0 if immediately ']' (consumed),
            // 1 if a value follows (NOT consumed).
            let first = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.json_arr_first,
                    vec![text_op, cursor_ptr, Operand::ConstI64(b']' as i64)],
                ),
                Type::I64,
                None,
            );
            let header = self.f.add_block();
            let body = self.f.add_block();
            let after = self.f.add_block();
            let nonempty = self.f.append_inst(
                self.cur_block,
                InstKind::ICmp(IPred::Ne, Operand::Value(first), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            self.f.set_term(
                self.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(nonempty),
                    then_blk: body,
                    else_blk: after,
                },
            );
            self.cur_block = body;
            let elem = self.lower_json_parse(text_op, cursor_ptr, elem_ty);
            // ②.7 — array slots are 8 raw bytes; an F64 elem
            // passes its BITS through arr_push's i64 value param
            // (same raw_slot_arg adapter the map/filter push uses
            // — without it codegen fptosi'd the float and the
            // slot read back denormals).
            let elem = self.raw_slot_arg(elem);
            let cur_arr = self.f.append_inst(
                self.cur_block,
                InstKind::Load(slot_ty, Operand::Value(arr_slot), 0),
                slot_ty,
                None,
            );
            let new_arr = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.arr_push,
                    vec![Operand::Value(cur_arr), elem],
                ),
                slot_ty,
                None,
            );
            // B1 — push never moves the cell; write-back retired.
            let _ = new_arr;
            self.f.set_term(self.cur_block, Terminator::Br(header));
            self.cur_block = header;
            let step = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.json_arr_step,
                    vec![text_op, cursor_ptr, Operand::ConstI64(b']' as i64)],
                ),
                Type::I64,
                None,
            );
            let cont = self.f.append_inst(
                self.cur_block,
                InstKind::ICmp(IPred::Eq, Operand::Value(step), Operand::ConstI64(1)),
                Type::Bool,
                None,
            );
            self.f.set_term(
                self.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(cont),
                    then_blk: body,
                    else_blk: after,
                },
            );
            self.cur_block = after;
            let final_arr = self.f.append_inst(
                self.cur_block,
                InstKind::Load(slot_ty, Operand::Value(arr_slot), 0),
                slot_ty,
                None,
            );
            Operand::Value(final_arr)
        }
    }
}
