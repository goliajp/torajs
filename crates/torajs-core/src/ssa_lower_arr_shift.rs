//! `xs.shift()` lowering, split out of `ssa_lower_arr_mutators.rs`.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_CAP_HEAD_OFF, ARR_LEN_OFF, LowerCtx};
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
                // W4 — the slot's raw 8 bytes travel in a GPR; decode
                // f64 elems explicitly.
                let raw_ty = if elem_ty == Type::F64 {
                    Type::I64
                } else {
                    elem_ty
                };
                let v = self.emit_arr_shift_head(cur_arr, len, raw_ty);
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

    /// The head bump of a non-empty typed shift, opened inline: read
    /// logical[0], move the head one slot on, drop the length by one.
    /// A receiver carrying shadow marks (`FLAG_ARR_EXOTIC_INDEX`, its
    /// holes are keyed by index and must move with the slots) takes
    /// the runtime `__torajs_arr_shift`, which does the same bump and
    /// then moves the marks. The slot's stake passes to the result on
    /// both paths, as it does for pop.
    fn emit_arr_shift_head(&mut self, arr: ValueId, len: ValueId, raw_ty: Type) -> ValueId {
        let out = self.alloca(raw_ty, Some("__shift_head"));
        let marked = self.emit_arr_exotic_index_test(arr);
        let slow_blk = self.f.add_block();
        let fast_blk = self.f.add_block();
        let join_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(marked),
                then_blk: slow_blk,
                else_blk: fast_blk,
            },
        );
        self.cur_block = slow_blk;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.arr_shift, vec![Operand::Value(arr)]),
            raw_ty,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(v), Operand::Value(out), 0),
        );
        self.f.set_term(slow_blk, Terminator::Br(join_blk));

        self.cur_block = fast_blk;
        let v = self.emit_arr_shift_bump(arr, len, raw_ty);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(v), Operand::Value(out), 0),
        );
        self.f.set_term(fast_blk, Terminator::Br(join_blk));

        self.cur_block = join_blk;
        self.f.append_inst(
            self.cur_block,
            InstKind::Load(raw_ty, Operand::Value(out), 0),
            raw_ty,
            None,
        )
    }

    /// Whether the array carries shadow hole marks. The flags are the
    /// header word's top 16 bits; testing the bit in place lets the
    /// branch be one `tst`.
    fn emit_arr_exotic_index_test(&mut self, arr: ValueId) -> ValueId {
        let hdr = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(arr), 0),
            Type::I64,
            None,
        );
        let mask = (u64::from(torajs_rc::FLAG_ARR_EXOTIC_INDEX) << 48) as i64;
        let marks = self.emit_i64_binop(SsaBinOp::And, hdr, mask);
        self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(marks), Operand::ConstI64(0)),
            Type::Bool,
            None,
        )
    }

    /// The unmarked bump: load logical[0], add 1 << 32 to the packed
    /// cap/head word (the head is its high half, so the cap is
    /// untouched), store len - 1. Stays in the current block.
    fn emit_arr_shift_bump(&mut self, arr: ValueId, len: ValueId, raw_ty: Type) -> ValueId {
        let packed = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(arr), ARR_CAP_HEAD_OFF),
            Type::I64,
            None,
        );
        let head = self.emit_i64_binop(SsaBinOp::LShr, packed, 32);
        let off = self.emit_i64_binop(SsaBinOp::Shl, head, 3);
        let data = self.emit_arr_data_ptr(Operand::Value(arr));
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(raw_ty, data, Operand::Value(off)),
            raw_ty,
            None,
        );
        let bumped = self.emit_i64_binop(SsaBinOp::Add, packed, 1 << 32);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(bumped),
                Operand::Value(arr),
                ARR_CAP_HEAD_OFF,
            ),
        );
        let new_len = self.emit_i64_binop(SsaBinOp::Sub, len, 1);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(new_len), Operand::Value(arr), ARR_LEN_OFF),
        );
        v
    }

    fn emit_i64_binop(&mut self, op: SsaBinOp, lhs: ValueId, rhs: i64) -> ValueId {
        self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(op, Operand::Value(lhs), Operand::ConstI64(rhs)),
            Type::I64,
            None,
        )
    }
}
