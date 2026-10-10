//! The any-lane retry of an annotated `JSON.parse`.
//!
//! The typed lane parses the text straight into the annotation's
//! layout and stops at the first thing it cannot place: a key out of
//! the declared order, an extra or missing key, a value of another
//! kind, or text that is not JSON at all. It cannot tell these apart,
//! and §25.5.1 orders them: the text parses as a whole first (a
//! SyntaxError), and only a parsed value meets the annotation, which
//! is a checked cast of the user's own type (a TypeError when the
//! value does not have that shape). So a stopped typed lane discards
//! its error and partial value and re-parses the whole text here.
//! A value whose shape fits (an object with every declared field of
//! the declared kind, in any order, extra keys allowed) is copied into
//! a fresh cell of the annotation's layout; any other value raises
//! the TypeError.

use crate::ssa::{ArrId, BinOp, IPred, InstKind, Operand, StructId, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, OBJ_HEADER_SIZE};
use torajs_wtf8::Wtf8;

/// The node kinds `__torajs_json_value_is` answers.
const NUMBER: i64 = 0;
const BOOLEAN: i64 = 1;
const STRING: i64 = 2;
const OBJECT: i64 = 3;
const ARRAY: i64 = 4;

impl LowerCtx<'_> {
    /// `JSON.parse(text)` into a slot of `slot_ty` the program
    /// annotated with `ann`: the typed lane, and the any-lane retry
    /// when it stopped. Answers the owned value; on a pending
    /// SyntaxError / TypeError the answer is a zero the throw edge can
    /// release.
    pub(crate) fn lower_json_parse_annotated(
        &mut self,
        text_op: Operand,
        slot_ty: Type,
        class_depth: Option<u32>,
        ann: &str,
    ) -> Operand {
        let cursor = self.alloca(Type::I64, Some("__json_pos"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(cursor), 0),
        );
        let fast = self.lower_json_parse(text_op, Operand::Value(cursor), slot_ty, class_depth);
        if slot_ty == Type::Any {
            return fast;
        }
        let out = self.alloca(slot_ty, Some("__json_out"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(fast, Operand::Value(out), 0),
        );
        let active = self.emit_throw_active_load();
        let stopped = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let retry = self.f.add_block();
        let join = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(stopped),
                then_blk: retry,
                else_blk: join,
            },
        );
        self.cur_block = retry;
        self.emit_json_discard_pending();
        self.emit_drop_value(fast, slot_ty);
        self.emit_json_retry(text_op, slot_ty, class_depth, ann, out);
        self.f.set_term(self.cur_block, Terminator::Br(join));
        self.cur_block = join;
        Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::Load(slot_ty, Operand::Value(out), 0),
            slot_ty,
            None,
        ))
    }

    /// Take the typed lane's pending error and release it.
    fn emit_json_discard_pending(&mut self) {
        let tag = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.throw_take_tag, vec![]),
            Type::I64,
            None,
        );
        let val = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.throw_take, vec![]),
            Type::I64,
            None,
        );
        let boxed = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::Value(tag), Operand::Value(val)],
            ),
            Type::Any,
            None,
        );
        self.emit_drop_value(Operand::Value(boxed), Type::Any);
    }

    /// Parse `text` on the any lane and store into `out` the value
    /// built from it, or a zero under a pending SyntaxError (the text
    /// is not JSON) or TypeError (its value does not fit `slot_ty`).
    fn emit_json_retry(
        &mut self,
        text_op: Operand,
        slot_ty: Type,
        class_depth: Option<u32>,
        ann: &str,
        out: ValueId,
    ) {
        let arg = self.box_to_any(text_op);
        let parsed = Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.json_parse_any, vec![arg]),
            Type::Any,
            None,
        ));
        let ok = self.alloca(Type::Bool, Some("__json_fits"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstBool(true), Operand::Value(ok), 0),
        );
        let active = self.emit_throw_active_load();
        let parsed_ok = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(active), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let judge = self.f.add_block();
        let fail = self.f.add_block();
        let build = self.f.add_block();
        let reject = self.f.add_block();
        let done = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(parsed_ok),
                then_blk: judge,
                else_blk: fail,
            },
        );
        self.cur_block = judge;
        self.emit_json_fits(parsed, slot_ty, class_depth, ok);
        let fits = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Bool, Operand::Value(ok), 0),
            Type::Bool,
            None,
        );
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(fits),
                then_blk: build,
                else_blk: reject,
            },
        );
        self.cur_block = build;
        let site = self.intern_string_literal("JSON.parse");
        let v = self.emit_json_build(parsed, slot_ty, Operand::Value(site));
        self.f
            .append_void(self.cur_block, InstKind::Store(v, Operand::Value(out), 0));
        self.f.set_term(self.cur_block, Terminator::Br(done));
        self.cur_block = reject;
        let msg = format!("JSON.parse result does not have the annotated type `{ann}`");
        let msg_str = self.intern_string_literal(msg.as_str());
        self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.throw_type_error_str,
                vec![Operand::Value(msg_str)],
            ),
            Type::Void,
            None,
        );
        self.f.set_term(self.cur_block, Terminator::Br(fail));
        self.cur_block = fail;
        let zero = json_zero(slot_ty);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(zero, Operand::Value(out), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(done));
        self.cur_block = done;
        self.emit_drop_value(parsed, Type::Any);
    }

    /// Clear `ok` unless the borrowed node `v` has the shape of `ty`.
    /// A class slot never fits: a parsed object is not an instance.
    fn emit_json_fits(&mut self, v: Operand, ty: Type, class_depth: Option<u32>, ok: ValueId) {
        if class_depth == Some(0) {
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::ConstBool(false), Operand::Value(ok), 0),
            );
            return;
        }
        let kind = match ty {
            Type::Any => return,
            Type::F64 | Type::I64 => NUMBER,
            Type::Bool => BOOLEAN,
            Type::Str => STRING,
            Type::Obj(_) => OBJECT,
            Type::Arr(_) => ARRAY,
            other => panic!("ssa-lower: JSON.parse into type {other:?} not yet supported"),
        };
        let is = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.json_value_is,
                vec![v, Operand::ConstI64(kind)],
            ),
            Type::I64,
            None,
        );
        let yes = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(is), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let inner = self.f.add_block();
        let no = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(yes),
                then_blk: inner,
                else_blk: no,
            },
        );
        self.f.append_void(
            no,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(ok), 0),
        );
        self.f.set_term(no, Terminator::Br(after));
        self.cur_block = inner;
        match ty {
            Type::Obj(sid) => {
                let p = self.emit_json_cell_ptr(v);
                let layout = self.struct_layouts[sid.0 as usize].clone();
                for (fname, fty) in layout.iter() {
                    let fv = self.emit_json_field_any(p, fname);
                    self.emit_json_fits(fv, *fty, None, ok);
                }
            }
            Type::Arr(arr_id) => {
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                let p = self.emit_json_cell_ptr(v);
                let depth = class_depth.map(|d| d - 1);
                self.emit_json_each(p, &mut |ctx, e| ctx.emit_json_fits(e, elem_ty, depth, ok));
            }
            _ => {}
        }
        self.f.set_term(self.cur_block, Terminator::Br(after));
        self.cur_block = after;
    }

    /// The owned value of `ty` built from the node `v`, which fits it.
    /// `site` names the slot for the cast kernels, which cannot reject
    /// a node that fits.
    fn emit_json_build(&mut self, v: Operand, ty: Type, site: Operand) -> Operand {
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

    fn emit_json_build_obj(&mut self, v: Operand, sid: StructId, site: Operand) -> Operand {
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

    fn emit_json_build_arr(
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
    fn emit_json_cell_ptr(&mut self, v: Operand) -> Operand {
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
    fn emit_json_field_any(&mut self, p: Operand, key: &Wtf8) -> Operand {
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
    fn emit_json_each(&mut self, p: Operand, body: &mut dyn FnMut(&mut Self, Operand)) {
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

/// A value of `ty` the drop walker releases as nothing.
fn json_zero(ty: Type) -> Operand {
    match ty {
        Type::F64 => Operand::ConstF64(0.0),
        Type::I64 => Operand::ConstI64(0),
        Type::Bool => Operand::ConstBool(false),
        _ => Operand::ConstPtrNull,
    }
}
