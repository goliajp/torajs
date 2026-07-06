//! Array mutator method lowering — `pop` / `shift` / `unshift`
//! receiver dispatch, split out of the ssa_lower Member-call arm
//! (file-size known-debt: ssa_lower.rs only shrinks). Semantics are
//! unchanged; W4 added the f64-elem raw-slot bit transport here
//! (`raw_slot_arg` / BitCastI64ToF64 on the shift return).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};

impl<'a> LowerCtx<'a> {
    /// `xs[i]` index operand — lower + ToInteger coerce. JS index
    /// expressions are number-typed; an F64 value reaching here
    /// (`xs[6/3]`, `xs[-(-j)]`) must come back to i64 before slot
    /// arithmetic or the codegen materializes an Fpr where a Gpr is
    /// required. Fractional indices keep dynobj property semantics
    /// out of scope (typed-tier truncates, same bar as str charAt).
    pub(crate) fn lower_index_operand(&mut self, index: ExprId) -> Operand {
        let raw = self.lower_expr(index);
        self.coerce_to_i64(raw)
    }

    /// `xs.pop()` / `xs.shift()` / `xs.unshift(v)` on Ident receivers
    /// (local or const-global Array<T>). Returns None when the callee
    /// is some other member-call shape — the caller's dispatch chain
    /// continues.
    pub(crate) fn try_lower_arr_pop_shift_unshift(
        &mut self,
        callee: ExprId,
        args: &[ExprId],
    ) -> Option<Operand> {
        if let Some(r) = self.try_arr_pop(callee, args) {
            return Some(r);
        }
        if let Some(r) = self.try_arr_shift(callee, args) {
            return Some(r);
        }
        self.try_arr_unshift(callee, args)
    }

    /// bug-327 C1 — shared empty-array guard for `pop` / `shift`.
    /// Emits `len == 0` CondBr: the empty block stores the ES-spec
    /// result (undefined for Any elems via the VALUE_UNDEFINED
    /// NaN-box; the typed-tier zero value otherwise — typed-tier
    /// truncation bar, same as charAt) into a fresh result slot and
    /// jumps to the join block. Leaves `cur_block` on the non-empty
    /// path; the caller emits the mutation there and closes with
    /// [`Self::emit_pop_shift_join`]. In-bounds cost: one cmp+branch.
    fn emit_pop_shift_empty_guard(&mut self, len: ValueId, elem_ty: Type) -> (ValueId, BlockId) {
        let result_slot = self.alloca(elem_ty, Some("__pop_shift_result"));
        let is_empty = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(len), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let empty_blk = self.f.add_block();
        let nonempty_blk = self.f.add_block();
        let join_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(is_empty),
                then_blk: empty_blk,
                else_blk: nonempty_blk,
            },
        );
        self.cur_block = empty_blk;
        let empty_val = match elem_ty {
            Type::Any => {
                // ANY_UNDEF=5 through the already-declared
                // `__torajs_anyv_box_from_pair` — tag 5 maps to the
                // VALUE_UNDEFINED NaN-box immediate.
                let undef = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_box,
                        vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                    ),
                    Type::Any,
                    None,
                );
                Operand::Value(undef)
            }
            Type::F64 => Operand::ConstF64(0.0),
            Type::Bool => Operand::ConstBool(false),
            t if t.is_refcounted() => Operand::ConstPtrNull,
            Type::Ptr => Operand::ConstPtrNull,
            _ => Operand::ConstI64(0),
        };
        self.f.append_void(
            self.cur_block,
            InstKind::Store(empty_val, Operand::Value(result_slot), 0),
        );
        self.f.set_term(empty_blk, Terminator::Br(join_blk));
        self.cur_block = nonempty_blk;
        (result_slot, join_blk)
    }

    /// Close the non-empty path opened by
    /// [`Self::emit_pop_shift_empty_guard`]: store the popped /
    /// shifted element into the result slot, branch to the join
    /// block, and reload the slot there as the expression result.
    fn emit_pop_shift_join(
        &mut self,
        guard: (ValueId, BlockId),
        elem: Operand,
        elem_ty: Type,
    ) -> Operand {
        let (result_slot, join_blk) = guard;
        self.f.append_void(
            self.cur_block,
            InstKind::Store(elem, Operand::Value(result_slot), 0),
        );
        let cb = self.cur_block;
        self.f.set_term(cb, Terminator::Br(join_blk));
        self.cur_block = join_blk;
        let result = self.f.append_inst(
            self.cur_block,
            InstKind::Load(elem_ty, Operand::Value(result_slot), 0),
            elem_ty,
            None,
        );
        Operand::Value(result)
    }

    /// `xs.pop()` — in-place len decrement + tail-slot load.
    fn try_arr_pop(&mut self, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
        // `xs.pop()` — receiver is either an Ident bound to a
        // typed Array<T> local OR an Ident bound to a K.3
        // const-global Array<T>. Pop reads-and-mutates the
        // slot in place (decrements len, no realloc) so the
        // global-receiver path is safe: the in-place len
        // mutation persists on the global heap object even
        // without a write-back of the array pointer.
        // Empty-array `pop` is UB in this subset (no
        // undefined element type) — matches the unchecked
        // convention used elsewhere.
        if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(callee)
            && name == "pop"
            && let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
        {
            // S288 — accept any trailing operands per ES §23.1.3.20
            // trailing-arg ignore; lower-and-drop before the in-place
            // pop emit so step()-style side-effect exprs fire (S272
            // idiom). Runtime helper reads no operands beyond recv.
            for &a in args.iter() {
                let _ = self.lower_expr(a);
            }
            let recv_name = recv_name.clone();
            let resolved_arr: Option<(Operand, Type)> = if let Some(info) =
                self.locals.get(&recv_name).copied()
                && matches!(info.ty, Type::Arr(_))
            {
                let cur_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                    info.ty,
                    None,
                );
                Some((Operand::Value(cur_arr), info.ty))
            } else if let Some(gty) = self.globals.get(&recv_name).copied()
                && matches!(gty, Type::Arr(_))
            {
                let gref = self.f.append_inst(
                    self.cur_block,
                    InstKind::GlobalRef(recv_name.clone()),
                    Type::Ptr,
                    None,
                );
                let cur_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(gty, Operand::Value(gref), 0),
                    gty,
                    None,
                );
                Some((Operand::Value(cur_arr), gty))
            } else {
                None
            };
            if let Some((arr_op, arr_ty)) = resolved_arr {
                let arr_id = match arr_ty {
                    Type::Arr(id) => id,
                    _ => unreachable!(),
                };
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                let cur_arr = match arr_op {
                    Operand::Value(v) => v,
                    _ => unreachable!(),
                };
                let len = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(cur_arr), ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                // bug-327 C1 — empty-array guard. ES spec: `[].pop()`
                // yields undefined and leaves length untouched. The
                // pre-guard emit decremented len to -1 and loaded
                // slot[-1] (Any elems: garbage AnyValue deref →
                // SIGSEGV; typed elems: silent len=-1 corruption).
                let guard = self.emit_pop_shift_empty_guard(len, elem_ty);
                let new_len = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Sub, Operand::Value(len), Operand::ConstI64(1)),
                    Type::I64,
                    None,
                );
                // T-13.5: head-aware byte offset for arr.pop()'s
                // last-element load.
                let (off_base, off) = self.emit_arr_slot_byte_offset(
                    Operand::Value(cur_arr),
                    Operand::Value(new_len),
                    3,
                    false,
                );
                let elem = self.f.append_inst(
                    self.cur_block,
                    InstKind::LoadDyn(elem_ty, off_base.clone(), off),
                    elem_ty,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(
                        Operand::Value(new_len),
                        Operand::Value(cur_arr),
                        ARR_LEN_OFF,
                    ),
                );
                return Some(self.emit_pop_shift_join(guard, Operand::Value(elem), elem_ty));
            }
        }
        None
    }

    /// `xs.shift()` — deque head-bump via the runtime helper.
    fn try_arr_shift(&mut self, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
        // M1.2 — `xs.push(v)` special-case. Two receiver shapes:
        // `xs.shift()` — receiver is either an Ident bound to
        // a typed Array<T> local OR an Ident bound to a K.3
        // const-global Array<T>. Shift uses the head_offset
        // bump strategy (T-13.5 deque), so it mutates len +
        // head in place without realloc — same safety as pop
        // for the global-receiver path.
        if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(callee)
            && name == "shift"
            && let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
        {
            // S288 — accept any trailing operands per ES §23.1.3.24
            // trailing-arg ignore; lower-and-drop before the head-
            // bump emit so step()-style side-effect exprs fire (S272
            // idiom).
            for &a in args.iter() {
                let _ = self.lower_expr(a);
            }
            let recv_name = recv_name.clone();
            let resolved_arr: Option<(Operand, Type)> = if let Some(info) =
                self.locals.get(&recv_name).copied()
                && matches!(info.ty, Type::Arr(_))
            {
                let cur_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                    info.ty,
                    None,
                );
                Some((Operand::Value(cur_arr), info.ty))
            } else if let Some(gty) = self.globals.get(&recv_name).copied()
                && matches!(gty, Type::Arr(_))
            {
                let gref = self.f.append_inst(
                    self.cur_block,
                    InstKind::GlobalRef(recv_name.clone()),
                    Type::Ptr,
                    None,
                );
                let cur_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(gty, Operand::Value(gref), 0),
                    gty,
                    None,
                );
                Some((Operand::Value(cur_arr), gty))
            } else {
                None
            };
            if let Some((arr_op, arr_ty)) = resolved_arr {
                let arr_id = match arr_ty {
                    Type::Arr(id) => id,
                    _ => unreachable!(),
                };
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                // bug-327 C1 — empty-array guard, same shape as pop.
                // The pre-guard emit ran the head-bump helper with
                // len==0: `*len_p -= 1` underflowed the u64 length to
                // u64::MAX and the head slot read returned garbage.
                let cur_arr = match arr_op {
                    Operand::Value(v) => v,
                    _ => unreachable!(),
                };
                let len = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(cur_arr), ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                let guard = self.emit_pop_shift_empty_guard(len, elem_ty);
                // W4 — the helper returns the slot's raw 8
                // bytes in a GPR; decode f64 elems explicitly.
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.arr_shift, vec![arr_op]),
                    if elem_ty == Type::F64 {
                        Type::I64
                    } else {
                        elem_ty
                    },
                    None,
                );
                let elem = if elem_ty == Type::F64 {
                    let fv = self.f.append_inst(
                        self.cur_block,
                        InstKind::BitCastI64ToF64(Operand::Value(v)),
                        Type::F64,
                        None,
                    );
                    Operand::Value(fv)
                } else {
                    Operand::Value(v)
                };
                return Some(self.emit_pop_shift_join(guard, elem, elem_ty));
            }
        }
        None
    }

    /// `xs.unshift(v)` — realloc-and-store-back, mirrors push.
    fn try_arr_unshift(&mut self, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
        // `xs.unshift(v)` — same realloc-and-store-back shape
        // as push, but the runtime helper memmoves slots right
        // + writes slot[0] before returning the new ptr.
        if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(callee)
            && name == "unshift"
            && args.len() == 1
            && let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
        {
            let recv_name = recv_name.clone();
            // (a) Local-Ident receiver — load from stack slot,
            // unshift, store the new ptr back into the same slot.
            if let Some(info) = self.locals.get(&recv_name).copied()
                && let Type::Arr(arr_id) = info.ty
            {
                let arr_ty = info.ty;
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                // Array<Any> — (tag, value) pair prepend through the
                // adopt-contract helper (push twin); the typed flow
                // below would raw-write the value over a NaN-box slot.
                if matches!(elem_ty, Type::Any) {
                    return Some(self.emit_arr_any_unshift_at_slot(
                        Operand::Value(info.slot),
                        0,
                        args[0],
                        arr_ty,
                    ));
                }
                let cur_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
                    arr_ty,
                    None,
                );
                let mut val = self.lower_expr(args[0]);
                // Chunk 575 — stored arrays chain-mark (push twin).
                self.emit_arr_mark_kind(&val, &elem_ty);
                // W4 — align with the elem width (mirrors push).
                if elem_ty == Type::F64 && self.operand_ty(&val) == Type::I64 {
                    val = self.coerce_to_f64(val);
                }
                let unshift_arg = self.raw_slot_arg(val);
                let new_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_unshift,
                        vec![Operand::Value(cur_arr), unshift_arg],
                    ),
                    arr_ty,
                    None,
                );
                if elem_ty.is_refcounted() {
                    self.emit_rc_inc(val);
                }
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(new_arr), Operand::Value(info.slot), 0),
                );
                if let Some((env_slot, env_offset)) =
                    self.captured_arr_writeback.get(&info.slot).copied()
                {
                    let env_ptr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::Ptr, Operand::Value(env_slot), 0),
                        Type::Ptr,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::Value(new_arr),
                            Operand::Value(env_ptr),
                            env_offset,
                        ),
                    );
                }
                // chunk 9c — JS spec: unshift returns new length.
                // Runtime helper bumps `len + 1` into arr[#8] before
                // returning; mirror the .length getter.
                let new_len = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                return Some(Operand::Value(new_len));
            }
            // (b) K.3 const-global Array<T> receiver. Mirrors push's
            // K.8 path at ssa_lower.rs:19708: load the cur ptr via
            // GlobalRef, run the unshift helper, store back into the
            // same global slot. Without this, `const a: number[] = ...`
            // (top-level explicit-annotation const → registered as a
            // global by K.6) panicked "unsupported member call shape:
            // unshift" — the locals-only guard above missed it.
            if let Some(slot_ty) = self.globals.get(&recv_name).copied()
                && let Type::Arr(arr_id) = slot_ty
            {
                let arr_ty = slot_ty;
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                let slot_ptr = self.f.append_inst(
                    self.cur_block,
                    InstKind::GlobalRef(recv_name.clone()),
                    Type::Ptr,
                    None,
                );
                // Array<Any> — pair prepend, mirror of push's K.8 arm.
                if matches!(elem_ty, Type::Any) {
                    return Some(self.emit_arr_any_unshift_at_slot(
                        Operand::Value(slot_ptr),
                        0,
                        args[0],
                        arr_ty,
                    ));
                }
                let cur_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(arr_ty, Operand::Value(slot_ptr), 0),
                    arr_ty,
                    None,
                );
                let mut val = self.lower_expr(args[0]);
                // Chunk 575 — stored arrays chain-mark (push twin).
                self.emit_arr_mark_kind(&val, &elem_ty);
                if elem_ty == Type::F64 && self.operand_ty(&val) == Type::I64 {
                    val = self.coerce_to_f64(val);
                }
                let unshift_arg = self.raw_slot_arg(val);
                let new_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_unshift,
                        vec![Operand::Value(cur_arr), unshift_arg],
                    ),
                    arr_ty,
                    None,
                );
                if elem_ty.is_refcounted() {
                    self.emit_rc_inc(val);
                }
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(new_arr), Operand::Value(slot_ptr), 0),
                );
                let new_len = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(new_arr), ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                return Some(Operand::Value(new_len));
            }
        }
        None
    }
}
