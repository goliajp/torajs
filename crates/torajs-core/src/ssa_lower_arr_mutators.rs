//! Array mutator method lowering — `pop` / `shift` / `unshift`
//! receiver dispatch, split out of the ssa_lower Member-call arm
//! (file-size known-debt: ssa_lower.rs only shrinks). Semantics are
//! unchanged; W4 added the f64-elem raw-slot bit transport here
//! (`raw_slot_arg` / BitCastI64ToF64 on the shift return).

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp as SsaBinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx};
use crate::ssa_lower_arr_layout::ArrLenLock;

impl<'a> LowerCtx<'a> {
    /// `xs[i]` index operand — lower + ToInteger coerce. JS index
    /// expressions are number-typed; an F64 value reaching here
    /// (`xs[6/3]`, `xs[-(-j)]`) must come back to i64 before slot
    /// arithmetic or the codegen materializes an Fpr where a Gpr is
    /// required. Fractional indices keep dynobj property semantics
    /// out of scope (typed-tier truncates, same bar as str charAt).
    pub(crate) fn lower_index_operand(&mut self, index: ExprId) -> Operand {
        // Cluster #4 logical-assign fingerprint — a pinned key was
        // already evaluated once (§13.15.2 evaluates the Reference
        // once); reuse the value instead of re-running the expression.
        let raw = match &self.pins.compound_key {
            Some((mid, mop)) if *mid == index => *mop,
            _ => self.lower_expr(index),
        };
        self.coerce_to_i64(raw)
    }

    /// `xs.pop()` / `xs.shift()` / `xs.unshift(v)` on Ident receivers
    /// (local or const-global Array<T>), Arr-typed Member receivers
    /// (`b.arr.shift()`, chunk 628), and Arr-typed Index receivers
    /// (`z[i].pop()`, chunk 700). Returns None when the callee is
    /// some other member-call shape — the caller's dispatch chain
    /// continues.
    pub(crate) fn try_lower_arr_pop_shift_unshift(
        &mut self,
        callee: ExprId,
        args: &[ExprId],
    ) -> Option<Operand> {
        let temp = self.lower_mutator_temp(callee, args);
        let r = self
            .try_arr_pop(callee, args)
            .or_else(|| self.try_arr_shift(callee, args))
            .or_else(|| self.try_arr_unshift(callee, args));
        if let Some(tok) = temp {
            // A pre-lowered receiver has a resolver arm, and none of
            // the three declines after resolving it.
            debug_assert!(r.is_some(), "mutator declined a pre-lowered temp receiver");
            self.release_mutator_temp(tok);
        }
        r
    }

    /// bug-327 C1 — shared empty-array guard for `pop` / `shift`.
    /// Emits `0 >=u len` CondBr — index 0 out of bounds, the bounds
    /// check the guard is, so block layout and the branch odds read
    /// the empty arm as the rare one and codegen tests it with one
    /// `cbz`. The empty block stores the ES-spec
    /// result (undefined for Any elems via the VALUE_UNDEFINED
    /// NaN-box; the typed-tier zero value otherwise — typed-tier
    /// truncation bar, same as charAt) into a fresh result slot and
    /// jumps to the join block. Leaves `cur_block` on the non-empty
    /// path; the caller emits the mutation there and closes with
    /// [`Self::emit_pop_shift_join`]. In-bounds cost: one cmp+branch.
    pub(crate) fn emit_pop_shift_empty_guard(
        &mut self,
        len: ValueId,
        elem_ty: Type,
    ) -> (ValueId, BlockId) {
        let slot_ty = crate::ssa_lower_nullable_guard::undefable_read_ty(elem_ty);
        let result_slot = self.alloca(slot_ty, Some("__pop_shift_result"));
        let is_empty = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Uge, Operand::ConstI64(0), Operand::Value(len)),
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
                    InstKind::BoxAny(5, Operand::ConstI64(0)),
                    Type::Any,
                    None,
                );
                Operand::Value(undef)
            }
            // Each width that HAS a way to spell undefined uses it:
            // the F64 sentinel, or the per-type immortal cell an
            // optional field / a `find` miss hands out. The rest keep
            // the type's zero — there is no bit pattern there to do
            // better with (an I64 `number` slot included, which is the
            // narrower-than-JS form, not a decision about this exit).
            Type::F64 => Operand::ConstF64(f64::from_bits(
                crate::ssa_lower_undef_f64_source::F64_UNDEF_SENTINEL_BITS,
            )),
            // A bool slot is exactly two states, so the promoted read
            // answers with the tag every tagged value uses for
            // `undefined` (was `false` — a hit value).
            Type::Bool => {
                let undef = self.f.append_inst(
                    self.cur_block,
                    InstKind::BoxAny(5, Operand::ConstI64(0)),
                    Type::Any,
                    None,
                );
                Operand::Value(undef)
            }
            t => match self.str_undef_sentinel_for(t) {
                Some(cell) => cell,
                None if t.is_refcounted() || t == Type::Ptr => Operand::ConstPtrNull,
                None => Operand::ConstI64(0),
            },
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
    pub(crate) fn emit_pop_shift_join(
        &mut self,
        guard: (ValueId, BlockId),
        elem: Operand,
        elem_ty: Type,
    ) -> Operand {
        let (result_slot, join_blk) = guard;
        let slot_ty = crate::ssa_lower_nullable_guard::undefable_read_ty(elem_ty);
        // A popped / shifted hole is `undefined`, but its slot word is 0
        // (`null`); pointer-shaped elements answer their undefined cell.
        if let Some(cell) = self.str_undef_sentinel_for(elem_ty) {
            let bits =
                self.f
                    .append_inst(self.cur_block, InstKind::PtrToInt(elem), Type::I64, None);
            let hole = self.f.append_inst(
                self.cur_block,
                InstKind::ICmp(IPred::Eq, Operand::Value(bits), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            let hole_blk = self.f.add_block();
            let live_blk = self.f.add_block();
            let cb = self.cur_block;
            self.f.set_term(
                cb,
                Terminator::CondBr {
                    cond: Operand::Value(hole),
                    then_blk: hole_blk,
                    else_blk: live_blk,
                },
            );
            self.cur_block = hole_blk;
            self.store_popped(cell, elem_ty, slot_ty, result_slot);
            self.f.set_term(hole_blk, Terminator::Br(join_blk));
            self.cur_block = live_blk;
        }
        self.store_popped(elem, elem_ty, slot_ty, result_slot);
        let cb = self.cur_block;
        self.f.set_term(cb, Terminator::Br(join_blk));
        self.cur_block = join_blk;
        let result = self.f.append_inst(
            self.cur_block,
            InstKind::Load(slot_ty, Operand::Value(result_slot), 0),
            slot_ty,
            None,
        );
        Operand::Value(result)
    }

    /// Store one exit of a pop / shift into the join's result slot,
    /// boxed when the slot widened to carry `undefined`.
    fn store_popped(&mut self, v: Operand, elem_ty: Type, slot_ty: Type, result_slot: ValueId) {
        let stored = if slot_ty == elem_ty {
            v
        } else {
            self.box_to_any(v)
        };
        self.f.append_void(
            self.cur_block,
            InstKind::Store(stored, Operand::Value(result_slot), 0),
        );
    }

    /// `xs.pop()` — in-place len decrement + tail-slot load.
    fn try_arr_pop(&mut self, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
        // `xs.pop()` — any receiver resolve_mutator_arr_receiver
        // admits. Pop reads-and-mutates the block in place
        // (decrements len, no realloc), so no receiver shape
        // needs a write-back of the array pointer.
        if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(callee)
            && name == "pop"
        {
            let recv_id = *recv_id;
            let (arr_op, arr_ty) = self.resolve_mutator_arr_receiver(recv_id)?;
            // S288 — accept any trailing operands per ES §23.1.3.20
            // trailing-arg ignore; lower-and-drop before the in-place
            // pop emit so step()-style side-effect exprs fire (S272
            // idiom). Runtime helper reads no operands beyond recv.
            for &a in args.iter() {
                let _ = self.lower_expr(a);
            }
            {
                let arr_id = match arr_ty {
                    Type::Arr(id) => id,
                    _ => unreachable!(),
                };
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                // RFC 20260713 blade 4 — frozen / RO-length receivers
                // throw before the empty short-circuit (§23.1.3.20
                // step 3.b writes length even when empty).
                let lock = if elem_ty == Type::Any {
                    ArrLenLock::DeleteTail
                } else {
                    ArrLenLock::DeleteTailTyped
                };
                // The typed pop's length is read ahead of the guard:
                // the guard's raiser either throws or returns with the
                // length untouched, and a read in the guard's entry
                // block shares an extended block with the caller's own
                // `xs.length` test, where late GVN folds the two.
                let early_len = match (elem_ty, &arr_op) {
                    (Type::Any, _) => None,
                    (_, Operand::Value(v)) => Some(self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(*v), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    )),
                    _ => unreachable!(),
                };
                self.emit_arr_len_lock_guard(arr_op, lock);
                // Chunk 628 — static Arr<Any> receivers go through the
                // kind-aware runtime pop (typed-behind-any blocks rebox
                // per elem kind; empty → undefined built in). The old
                // static LoadDyn read NaN-box-misread a typed block's
                // raw slots (SIGSEGV on heap-kind deref).
                if elem_ty == Type::Any {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.arr_any_pop, vec![arr_op]),
                        Type::Any,
                        None,
                    );
                    return Some(Operand::Value(v));
                }
                let cur_arr = match arr_op {
                    Operand::Value(v) => v,
                    _ => unreachable!(),
                };
                let len = early_len.expect("typed pop read its length ahead of the guard");
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
                // last-element load — head-aware only when the receiver
                // can actually have a head. An array that is never
                // `shift`ed keeps head at 0, so the fold is a no-op
                // that still costs a chained load of the head word plus
                // four ALU ops, and its trailing `Add` blocks
                // SCALED_ADDR from folding the element load into the
                // AGU. This is the predicate the Index read lane has
                // always passed (`ssa_lower_index.rs:65`); pop was
                // pinned to `false`. The `shift` twin stays `false` —
                // it is the operation that creates a head.
                let recv_non_deque = self.arr_expr_is_non_deque(recv_id);
                let (off_base, off) = self.emit_arr_slot_byte_offset(
                    Operand::Value(cur_arr),
                    Operand::Value(new_len),
                    8,
                    recv_non_deque,
                );
                let elem = self.f.append_inst(
                    self.cur_block,
                    InstKind::LoadDyn(elem_ty, off_base, off),
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

    /// `xs.unshift(v)` — realloc-and-store-back, mirrors push.
    fn try_arr_unshift(&mut self, callee: ExprId, args: &[ExprId]) -> Option<Operand> {
        // `xs.unshift(v)` — same realloc-and-store-back shape
        // as push, but the runtime helper memmoves slots right
        // + writes slot[0] before returning the new ptr. Chunk
        // 628 collapsed the duplicated local/global receiver
        // arms into resolve_mutator_arr_receiver (which also
        // admits Arr-typed Member receivers) — B1 fixed the
        // cell across grow, so no receiver shape needs a
        // write-back and the value-form emit serves all three.
        if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(callee)
            && name == "unshift"
            && args.len() == 1
        {
            let recv_id = *recv_id;
            let (arr_op, arr_ty) = self.resolve_mutator_arr_receiver(recv_id)?;
            let arr_id = match arr_ty {
                Type::Arr(id) => id,
                _ => unreachable!(),
            };
            let elem_ty = self.arr_layouts[arr_id.0 as usize];
            // Array<Any> — (tag, value) pair prepend through the
            // adopt-contract helper (push twin); the typed flow
            // below would raw-write the value over a NaN-box slot.
            // The helper kind-dispatches typed-behind-any blocks.
            if matches!(elem_ty, Type::Any) {
                return Some(self.emit_arr_any_unshift_at_value(arr_op, args[0], arr_ty));
            }
            // Chunk 743 — the full push coercion (chain-mark, bool →
            // i64, W4 width align, Substr → owned Str materialize):
            // pre-743 unshift only had the mark + W4 pieces, so
            // `ss.unshift(s[1])` stored the Substr VIEW pointer into a
            // Str slot — the two block layouts diverge past the
            // header, and every Str-layout read (join / print) walked
            // garbage.
            let (val, val_owned_from_substr) =
                crate::ssa_lower_call_arr_push::coerce_push_value(self, args[0], elem_ty);
            let unshift_arg = self.raw_slot_arg(val);
            // Chunk 742's stake dance (push twin of chunk 733; churn
            // probes closure-unshift 30.4MB / nested-arr 44.8MB /
            // owned-str 16MB vs 6.3MB flat) lives in the shared
            // helper, on both edges.
            let new_arr = crate::ssa_lower_arr_grow_kernel::emit_grow_kernel_checked(
                self,
                self.intrinsics.arr_unshift,
                arr_op,
                unshift_arg,
                args[0],
                val,
                elem_ty,
                val_owned_from_substr,
                arr_ty,
            );
            // B1 — cell fixed across grow; slot write-back +
            // captured-env mirror retired.
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
