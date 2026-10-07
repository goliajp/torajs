//! `xs.shift()` lowering, split out of `ssa_lower_arr_mutators.rs`.

use crate::ast::{Expr, ExprId};
use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};
use crate::ssa_lower_arr_layout::ArrLenLock;

impl LowerCtx<'_> {
    /// `xs.shift()` — deque head-bump via the runtime helper.
    pub(crate) fn try_arr_shift(&mut self, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
        // M1.2 — `xs.push(v)` special-case. Two receiver shapes:
        // `xs.shift()` — receiver is either an Ident bound to
        // a typed Array<T> local OR an Ident bound to a K.3
        // const-global Array<T>. Shift uses the head_offset
        // bump strategy (T-13.5 deque), so it mutates len +
        // head in place without realloc — same safety as pop
        // for the global-receiver path.
        if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(callee)
            && name == "shift"
        {
            let recv_id = *recv_id;
            let (arr_op, arr_ty) = self.resolve_mutator_arr_receiver(recv_id)?;
            // S288 — accept any trailing operands per ES §23.1.3.24
            // trailing-arg ignore; lower-and-drop before the head-
            // bump emit so step()-style side-effect exprs fire (S272
            // idiom).
            for &a in args.iter() {
                let _ = self.lower_expr(a);
            }
            {
                let arr_id = match arr_ty {
                    Type::Arr(id) => id,
                    _ => unreachable!(),
                };
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                // RFC 20260713 blade 4 — length-write lock, pop's twin;
                // the sealed refusal (the spec's partial move) is the raiser's.
                self.emit_arr_len_lock_guard(arr_op, ArrLenLock::ShiftDown);
                // Chunk 628 — static Arr<Any> receivers go through the
                // kind-aware runtime shift (pop's deque twin; see the
                // pop arm above).
                if elem_ty == Type::Any {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.arr_any_shift, vec![arr_op]),
                        Type::Any,
                        None,
                    );
                    return Some(Operand::Value(v));
                }
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
}
