//! The parsed JSON tree, read by node: a borrowed object or array
//! cell, a field or element of it, and the typed value built from a
//! node that fits its slot (the any-lane retry of an annotated
//! `JSON.parse`, `super::from_any`).

use crate::ssa::{ArrId, BinOp, IPred, InstKind, Operand, StructId, Terminator, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, OBJ_HEADER_SIZE};
use torajs_wtf8::Wtf8;

impl LowerCtx<'_> {
    /// The owned value of `ty` built from the node `v`, which fits it.
    /// `site` names the slot for the cast kernels, which cannot reject
    /// a node that fits.
    pub(super) fn emit_json_build(&mut self, v: Operand, ty: Type, site: Operand) -> Operand {
        let scalar = |ctx: &mut Self, kernel, ret| {
            Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(kernel, vec![v, site]),
                ret,
                None,
            ))
        };
        let i = &self.intrinsics;
        let (number, boolean, string) = (i.anyv_cast_number, i.anyv_cast_bool, i.anyv_cast_str);
        match ty {
            Type::F64 => scalar(self, number, Type::F64),
            Type::I64 => {
                let n = scalar(self, number, Type::F64);
                self.coerce_to_i64(n)
            }
            Type::Bool => scalar(self, boolean, Type::Bool),
            Type::Str => scalar(self, string, Type::Str),
            Type::Any => {
                let block = self.cur_block;
                self.emit_any_rc_inc_in(block, v);
                v
            }
            Type::Obj(sid) => self.emit_json_build_obj(v, sid, site),
            Type::Arr(arr_id) => self.emit_json_build_arr(v, ty, arr_id, site),
            other => panic!("ssa-lower: JSON.parse into type {other:?} not yet supported"),
        }
    }

    pub(super) fn emit_json_build_obj(
        &mut self,
        v: Operand,
        sid: StructId,
        site: Operand,
    ) -> Operand {
        let layout = self.struct_layouts[sid.0 as usize].clone();
        let cell = self.emit_json_obj_cell(sid, layout.len());
        let p = self.emit_json_cell_ptr(v);
        for (i, (fname, fty)) in layout.iter().enumerate() {
            let fv = self.emit_json_field_any(p, fname);
            let built = self.emit_json_build(fv, *fty, site);
            let off = OBJ_HEADER_SIZE + (i as u64) * 8;
            self.f.append_void(
                self.cur_block,
                InstKind::Store(built, Operand::Value(cell), off),
            );
        }
        Operand::Value(cell)
    }

    pub(super) fn emit_json_build_arr(
        &mut self,
        v: Operand,
        slot_ty: Type,
        arr_id: ArrId,
        site: Operand,
    ) -> Operand {
        let elem_ty = self.arr_layouts[arr_id.0 as usize];
        let arr = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.arr_alloc, vec![Operand::ConstI64(0)]),
            slot_ty,
            None,
        );
        let p = self.emit_json_cell_ptr(v);
        self.emit_json_each(p, &mut |ctx, e| {
            let built = ctx.emit_json_build(e, elem_ty, site);
            let raw = ctx.raw_slot_arg(built);
            ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.arr_push, vec![Operand::Value(arr), raw]),
                slot_ty,
                None,
            );
        });
        Operand::Value(arr)
    }

    /// The cell pointer of a node that is an object or an array,
    /// borrowed from the parsed tree.
    pub(super) fn emit_json_cell_ptr(&mut self, v: Operand) -> Operand {
        let bits = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_unbox_value, vec![v]),
            Type::I64,
            None,
        );
        Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::IntToPtr(Operand::Value(bits)),
            Type::Ptr,
            None,
        ))
    }

    /// The own entry `key` of the parsed object `p`, borrowed (an
    /// absent key reads `undefined`). A parsed tree holds no accessor
    /// and no inline short string, so the probe pair is the entry.
    pub(super) fn emit_json_field_any(&mut self, p: Operand, key: &Wtf8) -> Operand {
        let key = Operand::Value(self.intern_string_literal(key));
        let probe = |ctx: &mut Self, kernel| {
            Operand::Value(ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(kernel, vec![p, key]),
                Type::I64,
                None,
            ))
        };
        let (get_tag, get_value) = (
            self.intrinsics.dynobj_get_tag,
            self.intrinsics.dynobj_get_value,
        );
        let tag = probe(self, get_tag);
        let val = probe(self, get_value);
        Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_box, vec![tag, val]),
            Type::Any,
            None,
        ))
    }

    /// Run `body` on each element of the parsed array `p`, borrowed.
    pub(super) fn emit_json_each(&mut self, p: Operand, body: &mut dyn FnMut(&mut Self, Operand)) {
        let len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, p, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let idx = self.alloca(Type::I64, Some("__json_idx"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(idx), 0),
        );
        let header = self.f.add_block();
        let step = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = header;
        let i = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(idx), 0),
            Type::I64,
            None,
        );
        let more = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i), Operand::Value(len)),
            Type::Bool,
            None,
        );
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(more),
                then_blk: step,
                else_blk: after,
            },
        );
        self.cur_block = step;
        let e = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_get_any_boxed,
                vec![p, Operand::Value(i)],
            ),
            Type::Any,
            None,
        );
        body(self, Operand::Value(e));
        let next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(BinOp::Add, Operand::Value(i), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(next), Operand::Value(idx), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = after;
    }
}
