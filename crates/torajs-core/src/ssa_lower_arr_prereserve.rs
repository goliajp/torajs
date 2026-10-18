//! The pre-reserved fast push — the three emit shapes that a
//! [`PreReserveState`](crate::ssa_lower::PreReserveState) is made of.
//!
//! A caller that has already proved the destination cannot grow
//! (`arr_reserve` above the loop, and nothing in the body can shift or
//! unshift it) owns three loop-invariant facts: the data base, the byte
//! offset of slot[0], and a running length living in an alloca that
//! mem2reg promotes to a register. Appending is then a slot store and an
//! add — no call, no capacity test, and no read of the cell's own length
//! word, which would put a store-to-load forward on the loop-carried
//! dependency chain.
//!
//! Whether the cell's own length word may lag behind that running
//! count is the caller's to establish, and it rides on
//! [`PreReserveState::defer_len`]. It used to be assumed. The shapes
//! these lanes accept constrain the push *statement* and leave the
//! push *argument* free, so an argument could read the very word the
//! loop was holding back:
//!
//! ```ignore
//!   for (let i = 0; i < 5; i = i + 1) { xs.push(xs.length); }
//!   //  answered 0,0,0,0,0 instead of 0,1,2,3,4
//! ```
//!
//! No throw is needed for that, though a throw showed it too: an
//! argument that throws leaves by an edge the settlement does not sit
//! on, and the whole loop's appends went with it.
//!
//! Writing the word on every append fixes both and costs about half
//! the run of a push-dominated loop (measured: 10M `xs.push(i)`
//! appends, +55%) — that is one extra store against a body of two
//! instructions. So the lanes prove instead: the `map` / `filter`
//! destination is a temporary no user code can name, and the `for` /
//! `while` lanes ask whether every push argument in the body is inert
//! ([`crate::ssa_lower_push_loop_detect::push_args_all_inert`]). What
//! they cannot prove pays the store.
//!
//! For a REFCOUNTED slot the deferred word is sound either way, and
//! this is the question the shape invites. Every runtime walker bounds
//! an array by its `len` (`torajs_cycle::arr::arr_len_of`), so slots
//! written so far are invisible to a collection running mid-loop.
//! Invisibility can only cost collection, never soundness: each slot's
//! stake is counted the moment it is stored, so a cell reachable only
//! through one of them carries an rc the trial deletion never finds a
//! reason to decrement, and is kept. That window closes at the
//! settlement.

/// The pre-reserve lanes' own state, carried across a function body.
///
/// Lived as loose fields on `LowerCtx` until the invariance proof
/// below needed a second one; a struct here keeps the god-context
/// from growing a field per question this file learns to ask.
pub(crate) struct PreReserve {
    /// v0.6+1 perf checkpoint — push-loop pre-reserve fast-push state.
    ///
    /// When the for-loop lowerer detects a canonical fill loop
    /// (`for (let i = 0; i < N; i++) xs.push(_)`), it:
    ///   1. Emits `arr_reserve(xs, len + N)` once before the loop.
    ///   2. Hoists `slot0` (the address of slot[0]: data pointer plus
    ///      `head * 8`) into a loop-invariant register; allocas an i64
    ///      `len_slot` initialized to the array's len.
    ///   3. Inside the loop, arr.push lower emits inline IR:
    ///      `StoreDyn val at (slot0 + len*8)` plus `len_slot++`. NO
    ///      call to arr_push_unchecked, NO per-iter head load — slot0
    ///      is hoisted, len lives in the mem2reg-promotable alloca.
    ///   4. After the loop, the final len is written back to the
    ///      array's len field at +8.
    ///
    /// Multi-array support deliberate: a body that pushes to two
    /// distinct arrays in lockstep still benefits — each gets its
    /// own state entry. Conservative: only fires when the for-loop's
    /// full body shape matches the detector.
    pub(crate) unchecked_for: std::collections::HashMap<String, crate::ssa_lower::PreReserveState>,
    /// Array bindings this body made for itself — filled as the
    /// lowering walks past each `let xs = [ ... ]`. A name declared
    /// twice moves to `shadowed` instead: one set keyed by name
    /// cannot tell two same-named bindings apart.
    fresh: std::collections::HashSet<String>,
    /// Names that must never be answered for: declared more than once
    /// in this body, or a parameter, whose cell belongs to the caller
    /// and may have been handed in twice.
    shadowed: std::collections::HashSet<String>,
    /// Names this body writes: its own assignments plus those of
    /// every lifted closure it constructs, which are the only bodies
    /// that can write its bindings. Primed by
    /// `LowerCtx::prime_body_binding_sets`, which already computes
    /// exactly this set for the capture-box decision.
    reassigned: std::collections::HashSet<String>,
}

impl PreReserve {
    pub(crate) fn new(params: &[crate::ast::Param]) -> Self {
        Self {
            unchecked_for: std::collections::HashMap::new(),
            fresh: std::collections::HashSet::new(),
            shadowed: params.iter().map(|p| p.name.clone()).collect(),
            reassigned: std::collections::HashSet::new(),
        }
    }

    /// Hand over the body-scoped assigned-name set. Primed once per
    /// body, before any statement lowers.
    pub(crate) fn prime_reassigned(&mut self, assigned: &std::collections::HashSet<String>) {
        self.reassigned.clone_from(assigned);
    }

    /// Whether this body (or a closure it builds) assigns `name`
    /// anywhere. Name-keyed, so a shadowing binding answers for both —
    /// callers use it only where "yes" is the safe side.
    pub(crate) fn body_reassigns(&self, name: &str) -> bool {
        self.reassigned.contains(name)
    }

    /// Note a `let name = <array literal>` the lowering just walked past.
    pub(crate) fn note_array_literal_let(&mut self, name: &str) {
        if !self.fresh.insert(name.to_string()) {
            self.shadowed.insert(name.to_string());
        }
    }

    /// True when `name` is an array this body built and nothing else
    /// can be reaching — so its length cannot move except through
    /// this very name.
    ///
    /// Asked of both sides of a pre-reserve install: of every array
    /// the loop fills, and of the array a `A.length` bound reads.
    /// Either side answering yes settles the aliasing question for
    /// the pair — see [`bound_is_invariant`].
    ///
    /// [`bound_is_invariant`]: crate::ssa_lower_push_loop_detect
    ///
    /// Three questions, and the interesting one is answered already.
    /// The 11-A1 escape visitor marks a binding the moment it is
    /// aliased (`let y = xs` marks both), passed to a call, stored
    /// into a heap cell, returned, or captured — every way a second
    /// name could come to denote the same array. So a name absent
    /// from `deque_arrs` has no second name, with one gap: two
    /// *parameters* can be one array (`f(a, a)`) and neither is
    /// marked, because nothing escaped. Requiring the array to be a
    /// literal this body wrote closes it — a cell made here is not a
    /// cell the caller could have passed in twice.
    ///
    /// The third question is reassignment. `xs = getArr()` leaves the
    /// escape visitor silent: the value flowing in is a call, and no
    /// binding name flows with it, so the name stays `fresh` from its
    /// declaration while denoting something else. So a name this body
    /// writes is refused. Only this body and the closures it builds
    /// can write its bindings, which is the scope
    /// `prime_body_binding_sets` already collects for the capture-box
    /// decision; asking the whole program instead let a `dst = …` in
    /// an unrelated function cost every `dst` in the program its
    /// reservation, measured at 3.5x on a 10M copy.
    pub(crate) fn owns_alone(
        &self,
        deque_arrs: &std::collections::HashSet<String>,
        name: &str,
    ) -> bool {
        if self.shadowed.contains(name) || !self.fresh.contains(name) {
            return false;
        }
        if deque_arrs.contains(name) {
            return false;
        }
        !self.reassigned.contains(name)
    }
}

use crate::ssa::{BinOp as SsaBinOp, IPred, InstKind, Operand, Terminator, Type, ValueId};
use crate::ssa_lower::{ARR_LEN_OFF, LowerCtx, PreReserveState};
use crate::ssa_lower_str_arr_inplace::emit_arr_lock_bits_guard;

/// An append adds an index, which §10.1.8.1 refuses on a receiver that
/// is not extensible — and both seal and freeze imply that.
const EXTEND_LOCK_BITS: u16 =
    torajs_rc::FLAG_FROZEN | torajs_rc::FLAG_SEALED | torajs_rc::FLAG_NON_EXTENSIBLE;

/// Appends per trip for each distinct array a fill loop fills, in
/// first-push order. The detector lists every push site, so a body
/// that pushes one array three times names it three times — and the
/// reserve used to run once per site on the same pre-loop length,
/// leaving room for one append a trip while the unchecked stores made
/// three: `for (…) { xs.push(a); xs.push(b) }` wrote a second bound's
/// worth of slots into whatever cells sat past the buffer.
pub(crate) fn push_counts(names: &[String]) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = Vec::new();
    for n in names {
        match out.iter_mut().find(|(m, _)| m == n) {
            Some((_, k)) => *k += 1,
            None => out.push((n.clone(), 1)),
        }
    }
    out
}

/// The longest an array may grow (§10.4.2.4: a length is a u32).
const ARRAY_LENGTH_MAX: i64 = u32::MAX as i64;

impl LowerCtx<'_> {
    /// The fill loop stores slots inline, so the refusal of a
    /// non-extensible receiver (§10.1.8.1 through Set's TypeError) has
    /// to come from the lowering. The install admits only push
    /// arguments that cannot run user code
    /// (`ssa_lower_push_loop_gate::push_args_cannot_call`) — a literal,
    /// a local, arithmetic over primitives, a typed array element — and
    /// those can neither freeze the receiver nor observe the order
    /// between their own evaluation and the throw, so ONE check in the
    /// preheader, gated on `bound > 0` (a zero-trip loop never touches
    /// the array), is exact and costs the loop nothing.
    ///
    /// The first version of this gate cost array-sum-1m +55% with the
    /// loop byte-identical: its header read took the array out of the
    /// interval analysis's element points, and the SUM loop after the
    /// fill lost float demotion (`interval/elem.rs` admits the read
    /// since). Measure the second loop, not only the one touched.
    /// The capacity, in slots, that `bound` trips of `per_trip`
    /// appends each need on top of `len` elements of the array
    /// `arr_ty`: an element held in place takes one slot per field.
    /// One plain append a trip is `len + bound`, the shape nearly
    /// every fill loop has.
    ///
    /// Any other shape multiplies, and a product past what the buffer
    /// can hold would leave the loop's unchecked stores writing past
    /// it. Those trips would take the length past 2^32 - 1, where the
    /// append that crosses it raises the RangeError of §10.4.2.4
    /// whatever else the loop does; the body is appends alone, so the
    /// error is raised here, before the loop, on a cold edge.
    pub(crate) fn emit_reserve_slots(
        &mut self,
        arr_ty: Type,
        len: ValueId,
        bound: Operand,
        per_trip: i64,
    ) -> ValueId {
        let k = match arr_ty {
            Type::Arr(a) => {
                let elem = self.arr_layouts[a.0 as usize];
                crate::ssa_lower::inline_elem::elem_stride(self.struct_layouts, elem) as i64 / 8
            }
            _ => 1,
        };
        if k == 1 && per_trip == 1 {
            return self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Add, Operand::Value(len), bound),
                Type::I64,
                None,
            );
        }
        self.emit_too_long_guard(bound, ARRAY_LENGTH_MAX / per_trip);
        let len_slots = if k == 1 {
            Operand::Value(len)
        } else {
            self.emit_i64(InstKind::BinOp(
                SsaBinOp::Mul,
                Operand::Value(len),
                Operand::ConstI64(k),
            ))
        };
        let room = self.emit_i64(InstKind::BinOp(
            SsaBinOp::Mul,
            bound,
            Operand::ConstI64(per_trip * k),
        ));
        self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, len_slots, room),
            Type::I64,
            None,
        )
    }

    /// `bound > max` raises "Invalid array length" (the length check
    /// of `__torajs_arr_set_length_validate`, handed a length one past
    /// the largest) and leaves by the throw edge.
    fn emit_too_long_guard(&mut self, bound: Operand, max: i64) {
        let long = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Sgt, bound, Operand::ConstI64(max)),
            Type::Bool,
            None,
        );
        let raise_blk = self.f.add_block();
        let cont_blk = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(long),
                then_blk: raise_blk,
                else_blk: cont_blk,
            },
        );
        self.cur_block = raise_blk;
        let check = InstKind::Call(
            self.intrinsics.arr_set_length_validate,
            vec![
                Operand::ConstI64(2),
                Operand::ConstI64(ARRAY_LENGTH_MAX + 1),
            ],
        );
        self.f.append_void(raise_blk, check);
        self.emit_throw_check(None);
        // the validate always raises, so the check never falls through
        let cb = self.cur_block;
        self.f.set_term(cb, Terminator::Unreachable);
        self.cur_block = cont_blk;
    }

    fn emit_i64(&mut self, kind: InstKind) -> Operand {
        Operand::Value(self.f.append_inst(self.cur_block, kind, Type::I64, None))
    }

    pub(crate) fn emit_prereserve_entry_gate(&mut self, reserved: ValueId, bound: Operand) {
        let trips = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Sgt, bound, Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        emit_arr_lock_bits_guard(
            self,
            Operand::Value(reserved),
            EXTEND_LOCK_BITS,
            Some(Operand::Value(trips)),
            None,
        );
    }

    /// Read the three loop-invariant facts off an array the caller has
    /// just reserved, and mint the running-length slot. Emit this in the
    /// preheader — `reserved` is the pointer `arr_reserve` answered, and
    /// B1 says the cell never moves, so all four stay valid for the whole
    /// loop.
    pub(crate) fn emit_prereserved_state(
        &mut self,
        reserved: ValueId,
        defer_len: bool,
    ) -> PreReserveState {
        let head_off = match self.emit_arr_head_x8(Operand::Value(reserved)) {
            Operand::Value(v) => v,
            _ => unreachable!("emit_arr_head_x8 returns a value"),
        };
        let data_ptr = match self.emit_arr_data_ptr(Operand::Value(reserved)) {
            Operand::Value(v) => v,
            _ => unreachable!("emit_arr_data_ptr returns a value"),
        };
        // slot[0]'s address rather than its offset, so the per-append
        // address is `slot0 + len * 8`, which the scaled-addressing fold
        // turns into a single register-offset store.
        let slot0 = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(data_ptr),
                Operand::Value(head_off),
            ),
            Type::Ptr,
            None,
        );
        let len_after = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(reserved), ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let len_slot = self.alloca(Type::I64, Some("__push_len"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(len_after), Operand::Value(len_slot), 0),
        );
        PreReserveState {
            arr_ptr: reserved,
            slot0,
            len_slot,
            defer_len,
        }
    }

    /// Append `val` at the running length: store the slot, bump the
    /// count. Answers the NEW length, which is what JS `push` answers.
    ///
    /// `val` crosses as its own type. This is a direct slot store, not a
    /// `__torajs_arr_*` argument, so `raw_slot_arg`'s f64 → i64 bit
    /// crossing is neither needed nor wanted here.
    pub(crate) fn emit_prereserved_push(&mut self, st: PreReserveState, val: Operand) -> ValueId {
        // an element held in place is built by `inline_push`
        assert!(
            !matches!(self.f.value_type(st.arr_ptr), Type::Arr(a)
                if matches!(self.arr_layouts[a.0 as usize], Type::Inline(_))),
            "ssa-lower: a word stored into an inline array"
        );
        let len_now = self.emit_prereserved_len(st);
        let len_x8 = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Mul, Operand::Value(len_now), Operand::ConstI64(8)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::StoreDyn(val, Operand::Value(st.slot0), Operand::Value(len_x8)),
        );
        self.emit_prereserved_bump(st, len_now)
    }

    /// The running length a pre-reserved append stores at.
    pub(crate) fn emit_prereserved_len(&mut self, st: PreReserveState) -> ValueId {
        self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(st.len_slot), 0),
            Type::I64,
            None,
        )
    }

    /// Count the element just stored at `len_now`: answers the new
    /// length, which is what JS `push` answers.
    pub(crate) fn emit_prereserved_bump(
        &mut self,
        st: PreReserveState,
        len_now: ValueId,
    ) -> ValueId {
        let len_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(len_now), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(len_next), Operand::Value(st.len_slot), 0),
        );
        // ...and into the cell, unless the lane proved nothing can
        // read it before the loop ends. The running count above is
        // what the next append reads; this store is what everyone
        // else reads, and nothing waits on it.
        if !st.defer_len {
            self.f.append_void(
                self.cur_block,
                InstKind::Store(
                    Operand::Value(len_next),
                    Operand::Value(st.arr_ptr),
                    ARR_LEN_OFF,
                ),
            );
        }
        len_next
    }

    /// Settle the cell's length word from the running count. Emitted
    /// at the loop's normal exit, and only where `defer_len` held the
    /// word back — otherwise every append already wrote it.
    pub(crate) fn emit_prereserved_len_writeback(&mut self, st: PreReserveState) {
        if !st.defer_len {
            return;
        }
        let final_len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(st.len_slot), 0),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(final_len),
                Operand::Value(st.arr_ptr),
                ARR_LEN_OFF,
            ),
        );
    }
}
