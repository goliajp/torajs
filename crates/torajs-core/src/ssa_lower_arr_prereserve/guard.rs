//! The guarded append of a fill loop that reserves nothing in front
//! (the string walk, `crate::ssa_lower_while_push_fast::walk`).
//!
//! The bare fill lanes know their trip count, so the reserve above the
//! loop covers every append and an append is a store at the running
//! length. A walk knows no count. It reads, once in front of the loop,
//! the budget: the elements the buffer holds past its deque head. The
//! running length stays in a register, and each append compares it
//! with the budget: below it, the append is a store; at or past it, the
//! append takes a cold edge, which grows the buffer when the slot is
//! not there. A grow moves the buffer, so an append reads the buffer's
//! address off the header next to its store.
//!
//! The budget is never read again: a cold edge leaves it as it was,
//! and every append after the buffer's first grow in one run of the
//! loop takes the cold edge, which then finds the room the grow made
//! and only stores. A budget that changed on the cold edge would be one
//! more value carried around every arm of the loop, which costs the
//! compile more than the loop gains (a buffer that a pooled cell
//! brings, the common case, is never outgrown). The budget counts from
//! the deque head, so a receiver that a `shift` left with one never
//! stores past its buffer; a cold edge that folds the head back only
//! makes more room. A program with no road to `shift` pins every head
//! at 0 (`arr_head_census`), and an array that holds its class in place
//! never has one (the value-class census refuses a class whose arrays'
//! head may move); those read none.
//!
//! A scalar element's cold edge is the out-of-line push kernel: it
//! folds the head back, grows, raises the TypeError of a receiver with
//! a lock bit (§10.1.8.1) and the RangeError past the u32 length
//! (§10.4.2.4), and stores the element. A receiver with a lock bit has
//! no budget (the capacity is read through a mask that is 0 for it),
//! so its first append takes that edge and raises there, after the
//! effects before it, and a walk that appends nothing raises nothing.
//! An element held in place has no kernel: its cold edge grows the
//! buffer when it is full (`inline_push::grow::emit_grow_to_fit`) and the
//! store follows on the fast edge. The appends of one walk share the
//! block that raises the RangeError (`PreReserve::walk_raise`).

use crate::ast::ExprId;
use crate::ssa::{BinOp, BlockId, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::inline_push::APPEND_LOCK_BITS;
use crate::ssa_lower::{ARR_CAP_HEAD_OFF, ARR_LEN_OFF, LowerCtx, PreReserveState};

/// How a guarded fill loop tests for room in its buffer.
#[derive(Clone, Copy)]
pub(crate) struct Guard {
    /// the array's deque head is known to be 0
    pub(crate) headless: bool,
    /// the elements that fit past the head at loop entry; 0 for a
    /// receiver with a lock bit
    pub(crate) budget: Operand,
}

/// The block that raises the RangeError for the appends of one walk
/// (`emit_too_long_guard`), made by the first append that needs it,
/// and the throw state at the walk's entry.
#[derive(Clone, Copy)]
pub(crate) struct WalkRaise {
    pub(crate) blk: Option<BlockId>,
    live: usize,
    tries: usize,
    teardowns: usize,
    scopes: usize,
}

impl WalkRaise {
    pub(crate) fn at_entry(ctx: &LowerCtx<'_>) -> Self {
        Self {
            blk: None,
            live: ctx.temps.throw_live.iter().flatten().count(),
            tries: ctx.try_stack.len(),
            teardowns: ctx.for_of_teardown_stack.len(),
            scopes: ctx.scope_stack.len(),
        }
    }

    /// The shared block's throw path is filled once, by the first
    /// append, so every append that branches to it has to leave the
    /// same way: the same parked temps and try and for-of frames as at
    /// the walk's entry, and nothing to drop in a frame opened since.
    pub(crate) fn debug_assert_same(&self, ctx: &LowerCtx<'_>) {
        debug_assert_eq!(ctx.temps.throw_live.iter().flatten().count(), self.live);
        debug_assert_eq!(ctx.try_stack.len(), self.tries);
        debug_assert_eq!(ctx.for_of_teardown_stack.len(), self.teardowns);
        debug_assert!(!ctx.scopes_have_drops_from(self.scopes));
    }
}

/// The low half of the packed capacity and head word.
const CAP_MASK: i64 = 0xffff_ffff;

impl LowerCtx<'_> {
    /// The state of a guarded fill loop over `arr`, whose elements take
    /// `k` slots each and whose head may be known to be 0; emit it in
    /// the preheader. With `test_lock`, a receiver with a lock bit has
    /// no budget.
    pub(crate) fn emit_guarded_state(
        &mut self,
        arr: ValueId,
        k: u64,
        headless: bool,
        test_lock: bool,
    ) -> PreReserveState {
        let a = Operand::Value(arr);
        let cb = self.cur_block;
        let len = InstKind::Load(Type::I64, a, ARR_LEN_OFF);
        let len = Operand::Value(self.f.append_inst(cb, len, Type::I64, None));
        let len_slot = self.alloca(Type::I64, Some("__push_len"));
        let cb = self.cur_block;
        let store = InstKind::Store(len, Operand::Value(len_slot), 0);
        self.f.append_void(cb, store);
        let cap_mask = match test_lock {
            false => Operand::ConstI64(CAP_MASK),
            true => {
                let cb = self.cur_block;
                let hdr = InstKind::Load(Type::I64, a, 0);
                let hdr = Operand::Value(self.f.append_inst(cb, hdr, Type::I64, None));
                let lock = Operand::ConstI64((u64::from(APPEND_LOCK_BITS) << 48) as i64);
                let bits = self.bin(BinOp::And, hdr, lock, Type::I64);
                // all ones when a lock bit is set, else 0, by arithmetic:
                // the lowering may not form a select
                let neg = self.bin(BinOp::Sub, Operand::ConstI64(0), bits, Type::I64);
                let any = self.bin(BinOp::Or, bits, neg, Type::I64);
                let set = self.bin(BinOp::AShr, any, Operand::ConstI64(63), Type::I64);
                let clear = self.bin(BinOp::Xor, set, Operand::ConstI64(-1), Type::I64);
                self.bin(BinOp::And, clear, Operand::ConstI64(CAP_MASK), Type::I64)
            }
        };
        let budget = self.emit_budget(arr, k, headless, cap_mask);
        PreReserveState {
            arr_ptr: arr,
            slot0: None,
            len_slot,
            defer_len: true,
            guard: Some(Guard { headless, budget }),
        }
    }

    /// The elements of `k` slots that fit past the head, the capacity
    /// read through `cap_mask`.
    fn emit_budget(&mut self, arr: ValueId, k: u64, headless: bool, cap_mask: Operand) -> Operand {
        let cb = self.cur_block;
        let packed = InstKind::Load(Type::I64, Operand::Value(arr), ARR_CAP_HEAD_OFF);
        let packed = Operand::Value(self.f.append_inst(cb, packed, Type::I64, None));
        let cap = self.bin(BinOp::And, packed, cap_mask, Type::I64);
        let room = match headless {
            true => cap,
            false => {
                let head = self.bin(BinOp::LShr, packed, Operand::ConstI64(32), Type::I64);
                self.bin(BinOp::Sub, cap, head, Type::I64)
            }
        };
        match k {
            1 => room,
            k if k.is_power_of_two() => {
                let log2 = Operand::ConstI64(i64::from(k.trailing_zeros()));
                self.bin(BinOp::AShr, room, log2, Type::I64)
            }
            k => self.bin(BinOp::SDiv, room, Operand::ConstI64(k as i64), Type::I64),
        }
    }

    /// Branch to the fast edge when the slot at the running length is
    /// within the budget, else to a fresh cold block, which is left as
    /// the current block; answers the running length and the fast
    /// block.
    pub(crate) fn emit_room_test(&mut self, st: PreReserveState, g: Guard) -> (ValueId, BlockId) {
        let len = self.emit_prereserved_len(st);
        // signed: a locked receiver with a head has a negative budget
        let fits = self.cmp(IPred::Slt, Operand::Value(len), g.budget);
        let fast_blk = self.f.add_block();
        let cold_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: fits,
                then_blk: fast_blk,
                else_blk: cold_blk,
            },
        );
        self.cur_block = cold_blk;
        (len, fast_blk)
    }

    /// Settle the running length `len` into the cell's length word, on
    /// a cold edge that may raise or that hands the cell to the kernel.
    pub(crate) fn emit_settle_len(&mut self, st: PreReserveState, len: ValueId) {
        let store = InstKind::Store(Operand::Value(len), Operand::Value(st.arr_ptr), ARR_LEN_OFF);
        self.f.append_void(self.cur_block, store);
    }

    /// The slot[0] address an append on the fast edge stores from.
    pub(crate) fn emit_slot0(&mut self, arr: ValueId, g: Guard) -> Operand {
        let data = self.emit_arr_data_ptr(Operand::Value(arr));
        if g.headless {
            return data;
        }
        let x8 = self.emit_arr_head_x8(Operand::Value(arr));
        let cb = self.cur_block;
        let slot0 = InstKind::BinOp(BinOp::Add, data, x8);
        Operand::Value(self.f.append_inst(cb, slot0, Type::Ptr, None))
    }

    /// Append the scalar `val`, the value of `arg`, on a guarded fill
    /// loop; answers the new length. A walk appends numbers and
    /// booleans only, so the element holds no stake.
    pub(crate) fn emit_guarded_push(
        &mut self,
        st: PreReserveState,
        g: Guard,
        val: Operand,
        arg: ExprId,
    ) -> ValueId {
        let (len, fast_blk) = self.emit_room_test(st, g);
        // the kernel reads the length word
        self.emit_settle_len(st, len);
        let join_blk = self.f.add_block();
        let token = self.park_owned_temp(arg, &val);
        let slot_arg = self.raw_slot_arg(val);
        let cold = self.fn_table[crate::ssa_lower_arr_grow_kernel::PUSH_COLD];
        let arr_ty = self.f.value_type(st.arr_ptr);
        let call = InstKind::Call(cold, vec![Operand::Value(st.arr_ptr), slot_arg]);
        self.f.append_inst(self.cur_block, call, arr_ty, None);
        self.emit_throw_check(None);
        self.unpark_owned_temp(token);
        let cb = self.cur_block;
        let grown = InstKind::Load(Type::I64, Operand::Value(st.arr_ptr), ARR_LEN_OFF);
        let grown = self.f.append_inst(cb, grown, Type::I64, None);
        let store = InstKind::Store(Operand::Value(grown), Operand::Value(st.len_slot), 0);
        self.f.append_void(cb, store);
        self.f.set_term(cb, Terminator::Br(join_blk));

        self.cur_block = fast_blk;
        let slot0 = self.emit_slot0(st.arr_ptr, g);
        let off = self.bin(
            BinOp::Mul,
            Operand::Value(len),
            Operand::ConstI64(8),
            Type::I64,
        );
        let cb = self.cur_block;
        self.f.append_void(cb, InstKind::StoreDyn(val, slot0, off));
        self.emit_prereserved_bump(st, len);
        let cb = self.cur_block;
        self.f.set_term(cb, Terminator::Br(join_blk));
        self.cur_block = join_blk;
        self.emit_prereserved_len(st)
    }
}
