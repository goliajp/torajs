//! The object face of the caller-typed `JSON.parse`: one cell of the
//! annotation's layout, its declared fields parsed in order.

use crate::ssa::{self, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower::OBJ_HEADER_SIZE;
use torajs_wtf8::Wtf8;

impl LowerCtx<'_> {
    /// `Type::Obj` shape — eat `{`, alloc + header-init, then parse
    /// each declared field in order via [`Self::emit_json_field`],
    /// and strictly enforce the closing `}`.
    pub(super) fn lower_json_parse_obj(
        &mut self,
        text_op: Operand,
        cursor_ptr: Operand,
        sid: crate::ssa::StructId,
    ) -> Operand {
        {
            let layout = self.struct_layouts[sid.0 as usize].clone();
            // eat '{'
            self.f.append_void(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.json_eat_char,
                    vec![text_op, cursor_ptr, Operand::ConstI64(b'{' as i64)],
                ),
            );
            // alloc object — same shape as obj literal lowering:
            // header + N fields × 8 B. `obj_alloc` is plain malloc,
            // so class_tag and vtable_ptr MUST be stored explicitly:
            // a garbage class_tag can alias a real class id and hand
            // the drop/cycle walkers the wrong ClassLayout. Parsed
            // objects are anonymous structs — the anon-stamp pool
            // assigns a registered tag so the layout-driven walkers
            // release the parsed fields.
            let total_size = OBJ_HEADER_SIZE + (layout.len() as u64) * 8;
            let obj_ptr_v = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.obj_alloc,
                    vec![Operand::ConstI64(total_size as i64)],
                ),
                Type::Ptr,
                None,
            );
            self.emit_obj_header_init(Operand::Value(obj_ptr_v));
            let anon_tag = self.anon_stamp_pool.borrow_mut().assign_or_get(sid);
            self.f.append_void(
                self.cur_block,
                InstKind::Store(
                    Operand::ConstI64(anon_tag as i64),
                    Operand::Value(obj_ptr_v),
                    crate::ssa_lower::OBJ_CLASS_TAG_OFF,
                ),
            );
            crate::ssa_lower_iface::store_lit_vtable(self, obj_ptr_v, sid);
            let obj_ptr = Operand::Value(obj_ptr_v);
            // arr_first('{', '}') — handle empty object.
            let first = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.json_arr_first,
                    vec![text_op, cursor_ptr, Operand::ConstI64(b'}' as i64)],
                ),
                Type::I64,
                None,
            );
            let nonempty = self.f.append_inst(
                self.cur_block,
                InstKind::ICmp(IPred::Ne, Operand::Value(first), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let body = self.f.add_block();
            let after = self.f.add_block();
            self.f.set_term(
                self.cur_block,
                Terminator::CondBr {
                    cond: Operand::Value(nonempty),
                    then_blk: body,
                    else_blk: after,
                },
            );
            self.cur_block = body;
            // For each field in declared order: parse the key,
            // verify it matches the expected field name, eat ':',
            // recurse on the field's type, store at field offset.
            // After the last field, no separator step is needed —
            // the leading-element path already consumed `'}'` if
            // empty; the per-field flow expects a `,` between
            // fields and a `}` after the last. arr_step is
            // emitted between fields and after the last field
            // (terminator='}').
            for (i, (fname, fty)) in layout.iter().enumerate() {
                self.emit_json_field(text_op, cursor_ptr, obj_ptr, i, fname, *fty);
            }
            // After the last field, expect '}' — emit arr_step
            // with terminator='}' which consumes either ',' (and
            // would loop, but we're done) or '}'. To strictly
            // enforce the closing brace, we eat '}' directly.
            self.f.append_void(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.json_eat_char,
                    vec![text_op, cursor_ptr, Operand::ConstI64(b'}' as i64)],
                ),
            );
            self.f.set_term(self.cur_block, Terminator::Br(after));
            self.cur_block = after;
            obj_ptr
        }
    }

    /// One declared field — inter-field `,` step (i > 0), key parse +
    /// name verify (throw on mismatch), eat `:`, recursive value
    /// parse, store at the field offset.
    fn emit_json_field(
        &mut self,
        text_op: Operand,
        cursor_ptr: Operand,
        obj_ptr: Operand,
        i: usize,
        fname: &Wtf8,
        fty: Type,
    ) {
        if i > 0 {
            // arr_step expects ',' (continue) or '}' (end);
            // only ',' is valid here since we still have
            // more declared fields.
            let step = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.json_arr_step,
                    vec![text_op, cursor_ptr, Operand::ConstI64(b'}' as i64)],
                ),
                Type::I64,
                None,
            );
            let _ = step; // throw fires on syntactic error
        }
        // parse key string
        let key = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.json_parse_string, vec![text_op, cursor_ptr]),
            Type::Str,
            None,
        );
        // Verify key matches expected field name. If not,
        // throw a clear error.
        let bytes = fname.as_bytes().to_vec();
        let want_len = bytes.len() as i64;
        let want_sid = ssa::StringId((self.string_id_base + self.new_strings.len()) as u32);
        self.new_strings
            .push(ssa::StringLiteral::from_latin1_bytes(bytes));
        let want_ptr = self.f.append_inst(
            self.cur_block,
            InstKind::StringRef(want_sid),
            Type::Ptr,
            None,
        );
        let eq = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.str_eq_cstr,
                vec![
                    Operand::Value(key),
                    Operand::Value(want_ptr),
                    Operand::ConstI64(want_len),
                ],
            ),
            Type::I64,
            None,
        );
        let key_ok = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(eq), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let ok_blk = self.f.add_block();
        let bad_blk = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(key_ok),
                then_blk: ok_blk,
                else_blk: bad_blk,
            },
        );
        // bad path: drop the parsed key + throw.
        self.cur_block = bad_blk;
        self.emit_drop_value(Operand::Value(key), Type::Str);
        let err_msg = format!("JSON.parse: expected field \"{fname}\"");
        let err_str = self.intern_string_literal(&err_msg);
        // P4.7 — throw_set now takes (tag, value). String
        // err lowers as ANY_HEAP=4 with the str ptr as
        // value; the runtime helper torajs_throw_*_error
        // C-side callers follow the same shape.
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.throw_set,
                vec![Operand::ConstI64(4), Operand::Value(err_str)],
            ),
        );
        self.f.set_term(self.cur_block, Terminator::Br(ok_blk));
        self.cur_block = ok_blk;
        // Drop the parsed key (we only needed its bytes
        // for the equality check).
        self.emit_drop_value(Operand::Value(key), Type::Str);
        // eat ':'
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.json_eat_char,
                vec![text_op, cursor_ptr, Operand::ConstI64(b':' as i64)],
            ),
        );
        // Parse field value (recursive).
        let fv = self.lower_json_parse(text_op, cursor_ptr, fty);
        // Store at field offset.
        let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
        self.f
            .append_void(self.cur_block, InstKind::Store(fv, obj_ptr, field_off));
    }
}
