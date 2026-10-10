//! The object face of the caller-typed `JSON.parse`: one cell of the
//! annotation's layout, its declared fields parsed in order.
//!
//! This lane only parses text that spells the declared fields in
//! declared order. Anything else (a missing, reordered or extra key)
//! stops it with a pending error, and the declaration re-parses the
//! whole text on the any lane, where a text that is not JSON is a
//! SyntaxError before its shape is judged against the annotation
//! (`from_any.rs`). Every raise here and in the runtime token helpers
//! keeps the FIRST pending error, so a later token read off a
//! deranged cursor never allocates another one.

use crate::ssa::{self, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower::OBJ_HEADER_SIZE;
use torajs_wtf8::Wtf8;

impl LowerCtx<'_> {
    /// `Type::Obj` shape — eat `{`, alloc + header-init, then parse
    /// each declared field in order via [`Self::emit_json_field`],
    /// and strictly enforce the closing `}`. An object that closes
    /// before its last declared field leaves through the missing-field
    /// chain ([`Self::emit_json_missing_chain`]).
    pub(super) fn lower_json_parse_obj(
        &mut self,
        text_op: Operand,
        cursor_ptr: Operand,
        sid: crate::ssa::StructId,
    ) -> Operand {
        let layout = self.struct_layouts[sid.0 as usize].clone();
        // eat '{'
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.json_eat_char,
                vec![text_op, cursor_ptr, Operand::ConstI64(b'{' as i64)],
            ),
        );
        let obj_ptr_v = self.emit_json_obj_cell(sid, layout.len());
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
        let entry_blk = self.cur_block;
        let missing = self.emit_json_missing_chain(obj_ptr, &layout, after);
        self.f.set_term(
            entry_blk,
            Terminator::CondBr {
                cond: Operand::Value(nonempty),
                then_blk: body,
                else_blk: missing.first().copied().unwrap_or(after),
            },
        );
        self.cur_block = body;
        // For each field in declared order: parse the key,
        // verify it matches the expected field name, eat ':',
        // recurse on the field's type, store at field offset.
        // arr_step is emitted between fields (terminator='}'),
        // and a `}` there is a missing field.
        for (i, (fname, fty)) in layout.iter().enumerate() {
            if i > 0 {
                self.emit_json_field_step(text_op, cursor_ptr, missing[i]);
            }
            self.emit_json_field(text_op, cursor_ptr, obj_ptr, i, fname, *fty);
        }
        // After the last field, expect '}' — eat it directly to
        // strictly enforce the closing brace.
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

    /// A fresh cell of the anonymous struct `sid` with `n` fields, its
    /// header, class tag and vtable stored. `obj_alloc` is plain
    /// malloc, so class_tag and vtable_ptr MUST be stored explicitly:
    /// a garbage class_tag can alias a real class id and hand the
    /// drop/cycle walkers the wrong ClassLayout. The anon-stamp pool
    /// assigns a registered tag so the layout-driven walkers release
    /// the fields.
    pub(super) fn emit_json_obj_cell(&mut self, sid: crate::ssa::StructId, n: usize) -> ValueId {
        let total_size = OBJ_HEADER_SIZE + (n as u64) * 8;
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
        obj_ptr_v
    }

    /// The blocks an object that closed early leaves through: entry
    /// `i` stops the typed lane, then every field from `i` on is
    /// zeroed — the fields before `i` were already stored, and the
    /// any-lane retry drops the cell, so each slot must hold a value
    /// the drop walker can release (zero is null for every heap slot).
    /// Returns the entry block of each field.
    fn emit_json_missing_chain(
        &mut self,
        obj_ptr: Operand,
        layout: &[(crate::ast::PropKey, Type)],
        after: BlockId,
    ) -> Vec<BlockId> {
        let saved = self.cur_block;
        let entries: Vec<BlockId> = layout.iter().map(|_| self.f.add_block()).collect();
        let zeros: Vec<BlockId> = layout.iter().map(|_| self.f.add_block()).collect();
        for (i, (_, fty)) in layout.iter().enumerate() {
            // only the register class matters: a float slot takes 0.0,
            // every other slot the null pointer
            let zero = if matches!(fty, Type::F64) {
                Operand::ConstF64(0.0)
            } else {
                Operand::ConstPtrNull
            };
            self.cur_block = entries[i];
            self.emit_json_mismatch();
            self.f.set_term(self.cur_block, Terminator::Br(zeros[i]));
            let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
            self.f
                .append_void(zeros[i], InstKind::Store(zero, obj_ptr, field_off));
            let next = zeros.get(i + 1).copied().unwrap_or(after);
            self.f.set_term(zeros[i], Terminator::Br(next));
        }
        self.cur_block = saved;
        entries
    }

    /// Stop the typed lane: raise a TypeError unless the parse already
    /// holds a pending error (the first one wins). Leaves `cur_block`
    /// at the join, so the parse runs on to its end; the declaration
    /// then discards the error and judges the text on the any lane.
    pub(super) fn emit_json_mismatch(&mut self) {
        let active = self.emit_throw_active_load();
        let clear = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(active), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let raise = self.f.add_block();
        let join = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(clear),
                then_blk: raise,
                else_blk: join,
            },
        );
        self.cur_block = raise;
        let msg_str =
            self.intern_string_literal("JSON.parse: the text does not spell the annotated type");
        let raiser = self.intrinsics.throw_type_error_str;
        self.f.append_inst(
            self.cur_block,
            InstKind::Call(raiser, vec![Operand::Value(msg_str)]),
            Type::Void,
            None,
        );
        self.f.set_term(self.cur_block, Terminator::Br(join));
        self.cur_block = join;
    }

    /// The `,` between two declared fields. arr_step answers 1 on `,`
    /// and 0 on `}` (consumed): the object closed before the next
    /// declared field, which branches to that field's `missing` entry.
    fn emit_json_field_step(&mut self, text_op: Operand, cursor_ptr: Operand, missing: BlockId) {
        let step = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.json_arr_step,
                vec![text_op, cursor_ptr, Operand::ConstI64(b'}' as i64)],
            ),
            Type::I64,
            None,
        );
        let closed = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(step), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let more = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(closed),
                then_blk: missing,
                else_blk: more,
            },
        );
        self.cur_block = more;
    }

    /// One declared field — key parse + name verify (TypeError on
    /// mismatch), eat `:`, recursive value parse, store at the field
    /// offset.
    fn emit_json_field(
        &mut self,
        text_op: Operand,
        cursor_ptr: Operand,
        obj_ptr: Operand,
        i: usize,
        fname: &Wtf8,
        fty: Type,
    ) {
        // parse key string
        let key = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.json_parse_string, vec![text_op, cursor_ptr]),
            Type::Str,
            None,
        );
        // Verify key matches expected field name.
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
        self.cur_block = bad_blk;
        self.emit_json_mismatch();
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
        let fv = self.lower_json_parse(text_op, cursor_ptr, fty, None);
        // Store at field offset.
        let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
        self.f
            .append_void(self.cur_block, InstKind::Store(fv, obj_ptr, field_off));
    }
}
