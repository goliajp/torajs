//! ②.7 / M6.3 — caller-typed `JSON.parse` lowering: the slot
//! annotation drives a cursor-based typed parse (scalars, arrays,
//! named-type structs, nested). Extracted from `ssa_lower.rs`
//! (file-size known-debt). The number-domain faces of the target
//! arrive pre-widened to F64 (`num_width/json_seed.rs` — JSON text
//! is runtime data, narrow faces are unprovable).

use crate::ssa::{self, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower::OBJ_HEADER_SIZE;

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

    /// `Type::Obj` shape — eat `{`, alloc + header-init, then parse
    /// each declared field in order via [`Self::emit_json_field`],
    /// and strictly enforce the closing `}`.
    fn lower_json_parse_obj(
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
            self.f.append_void(
                self.cur_block,
                InstKind::Store(
                    Operand::ConstPtrNull,
                    Operand::Value(obj_ptr_v),
                    crate::ssa_lower::OBJ_VTABLE_OFF,
                ),
            );
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
        fname: &str,
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
