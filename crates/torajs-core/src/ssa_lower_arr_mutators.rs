//! Array mutator method lowering — `pop` / `shift` / `unshift`
//! receiver dispatch, split out of the ssa_lower Member-call arm
//! (file-size known-debt: ssa_lower.rs only shrinks). Semantics are
//! unchanged; W4 added the f64-elem raw-slot bit transport here
//! (`raw_slot_arg` / BitCastI64ToF64 on the shift return).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, InstKind, Operand, Type};
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
            && args.is_empty()
            && let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
        {
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
                let new_len = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Sub, Operand::Value(len), Operand::ConstI64(1)),
                    Type::I64,
                    None,
                );
                // T-13.5: head-aware byte offset for arr.pop()'s
                // last-element load.
                let off = self.emit_arr_slot_byte_offset(
                    Operand::Value(cur_arr),
                    Operand::Value(new_len),
                    3,
                    false,
                );
                let elem = self.f.append_inst(
                    self.cur_block,
                    InstKind::LoadDyn(elem_ty, Operand::Value(cur_arr), off),
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
                return Some(Operand::Value(elem));
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
            && args.is_empty()
            && let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
        {
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
                if elem_ty == Type::F64 {
                    let fv = self.f.append_inst(
                        self.cur_block,
                        InstKind::BitCastI64ToF64(Operand::Value(v)),
                        Type::F64,
                        None,
                    );
                    return Some(Operand::Value(fv));
                }
                return Some(Operand::Value(v));
            }
        }
        None
    }

    /// `xs.unshift(v)` — realloc-and-store-back, mirrors push.
    fn try_arr_unshift(&mut self, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
        // `xs.unshift(v)` — same realloc-and-store-back shape
        // as push (a), but the runtime helper memmoves slots
        // right + writes slot[0] before returning the new ptr.
        if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(callee)
            && name == "unshift"
            && args.len() == 1
            && let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
            && let Some(info) = self.locals.get(recv_name).copied()
            && let Type::Arr(arr_id) = info.ty
        {
            let arr_ty = info.ty;
            let elem_ty = self.arr_layouts[arr_id.0 as usize];
            let cur_arr = self.f.append_inst(
                self.cur_block,
                InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
                arr_ty,
                None,
            );
            let mut val = self.lower_expr(args[0]);
            if !elem_ty.is_refcounted() {
                self.consume_if_ident(args[0]);
            }
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
                    InstKind::Store(Operand::Value(new_arr), Operand::Value(env_ptr), env_offset),
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
        None
    }
}
