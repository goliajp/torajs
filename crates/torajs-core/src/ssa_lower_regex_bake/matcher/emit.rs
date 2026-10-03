//! The one-pass walk (`MatcherIr::exec`) as an SSA function over the
//! payload's code units: `(units, len, start, out) -> end | -1`. A
//! narrow matcher reads bytes (`LoadU8Dyn`), a wide one 16-bit units
//! (`LoadU16Dyn`); the walk is the same.
//!
//! One block per state. A state reads its unit once and dispatches it
//! over its edges (`dispatch.rs`); an edge's saves are stores into
//! `out` at the current position; the scan over start positions and
//! the loop-carried cursor live in stack slots the mid-end promotes.
//! Under `u` / `v` over UTF-16 a state whose edges reach the
//! surrogates reads a pair as its code point and steps over both
//! halves, and the start scan steps the same way. A sticky matcher
//! has no scan: the attempt at `start` is the only one.
//!
//! A state that probes the position (`probes`) first branches on the
//! bits it reads — a decision tree in `probes.rs` — and each leaf is
//! the variant those bits select, emitted like a plain state.
//!
//! The row is written in place. An accept that has edges may be
//! followed by writes the walk later abandons, so the slots such a
//! walk can still touch (`slots_written_past_accept`) are copied
//! aside at the accept and put back when the walk dies with that
//! accept as its answer.
//!
//! When the start state enters one state on a class that state loops
//! on (`leading_run_state`), a dead attempt resumes the scan past the
//! run it began (`resume.rs`) instead of one unit after its start.

mod class;
mod dispatch;
mod probes;
mod resume;

use crate::ssa::StringLiteral;
use crate::ssa::{BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId};
use torajs_regex::onepass::{Edge, MatcherIr, Variant};

/// `strings` is the module's string table, which takes the class maps
/// the walk reads (`class.rs`).
pub(crate) fn walk(name: &str, ir: &MatcherIr, strings: &mut Vec<StringLiteral>) -> Function {
    let mut f = Function::new(name, Type::I64);
    let units = f.add_param(Type::Ptr, "units");
    let len = f.add_param(Type::I64, "len");
    let start = f.add_param(Type::I64, "start");
    let out = f.add_param(Type::Ptr, "out");
    let entry = f.add_block();
    let scan = f.add_block();
    let attempt = f.add_block();
    let next_st = f.add_block();
    let dead = f.add_block();
    let restore = f.add_block();
    let miss = f.add_block();
    let states: Vec<BlockId> = ir.states.iter().map(|_| f.add_block()).collect();
    let held = ir.slots_written_past_accept();
    let run = if ir.anchored {
        None
    } else {
        ir.leading_run_state()
    };

    let st_slot = f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, Some("st"));
    let pos_slot = f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, Some("pos"));
    let last_slot = f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, Some("last"));
    let pair_slots = ir.pairs.then(|| {
        (
            f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, Some("cp")),
            f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, Some("step")),
        )
    });
    let resume = run.map(|s| {
        let slot = f.append_inst(
            entry,
            InstKind::Alloca(Type::I64),
            Type::Ptr,
            Some("resume"),
        );
        (s as usize, slot)
    });
    let backup = (!held.is_empty()).then(|| {
        f.append_inst(
            entry,
            InstKind::AllocaBytes(8 * held.len() as u64),
            Type::Ptr,
            Some("held"),
        )
    });
    f.append_void(
        entry,
        InstKind::Store(Operand::Value(start), Operand::Value(st_slot), 0),
    );
    f.set_term(entry, Terminator::Br(scan));

    let mut e = Emit {
        f,
        ir,
        units,
        len,
        out,
        st_slot,
        pos_slot,
        last_slot,
        pair_slots,
        backup,
        held,
        resume,
        states,
        next_st,
        dead,
        maps: class::Maps::new(strings, entry),
    };

    // scan: every start position up to and including `len`
    let st = e.load(scan, st_slot, 0);
    let past = e.icmp(scan, IPred::Sgt, st, Operand::Value(len));
    e.cond_br(scan, past, miss, attempt);
    e.store(attempt, Operand::Value(st), pos_slot, 0);
    e.f.set_term(attempt, Terminator::Br(e.states[0]));

    if ir.anchored {
        e.f.set_term(next_st, Terminator::Br(miss));
    } else {
        e.scan_advance(scan);
    }

    // dead: the walk found no edge; the last accept answers if there was one
    let last = e.load(dead, last_slot, 0);
    let hit = e.icmp(dead, IPred::Sge, last, Operand::ConstI64(0));
    let again = e.resume_scan();
    e.cond_br(dead, hit, restore, again);
    if let Some(backup) = e.backup {
        for (k, &slot) in e.held.clone().iter().enumerate() {
            let v = e.load(restore, backup, 8 * k as u64);
            e.store(restore, Operand::Value(v), out, 8 * slot as u64);
        }
    }
    e.answer(restore, Operand::Value(last));
    e.f.set_term(miss, Terminator::Ret(Some(Operand::ConstI64(-1))));

    for i in 0..ir.states.len() {
        e.state(i);
    }
    e.f
}

struct Emit<'a> {
    f: Function,
    ir: &'a MatcherIr,
    units: ValueId,
    len: ValueId,
    out: ValueId,
    st_slot: ValueId,
    pos_slot: ValueId,
    last_slot: ValueId,
    /// the code point and the step of a unit read as a pair
    pair_slots: Option<(ValueId, ValueId)>,
    backup: Option<ValueId>,
    held: Vec<u32>,
    /// the run state and the slot holding the position its run ended at
    resume: Option<(usize, ValueId)>,
    states: Vec<BlockId>,
    next_st: BlockId,
    dead: BlockId,
    maps: class::Maps<'a>,
}

impl Emit<'_> {
    fn load(&mut self, blk: BlockId, ptr: ValueId, off: u64) -> ValueId {
        self.f.append_inst(
            blk,
            InstKind::Load(Type::I64, Operand::Value(ptr), off),
            Type::I64,
            None,
        )
    }

    fn store(&mut self, blk: BlockId, v: Operand, ptr: ValueId, off: u64) {
        self.f
            .append_void(blk, InstKind::Store(v, Operand::Value(ptr), off));
    }

    fn icmp(&mut self, blk: BlockId, pred: IPred, a: ValueId, b: Operand) -> ValueId {
        self.f.append_inst(
            blk,
            InstKind::ICmp(pred, Operand::Value(a), b),
            Type::Bool,
            None,
        )
    }

    fn binop(&mut self, blk: BlockId, op: BinOp, a: ValueId, b: Operand) -> ValueId {
        self.f.append_inst(
            blk,
            InstKind::BinOp(op, Operand::Value(a), b),
            Type::I64,
            None,
        )
    }

    fn cond_br(&mut self, blk: BlockId, cond: ValueId, then_blk: BlockId, else_blk: BlockId) {
        self.f.set_term(
            blk,
            Terminator::CondBr {
                cond: Operand::Value(cond),
                then_blk,
                else_blk,
            },
        );
    }

    /// the code unit at `pos`
    fn unit(&mut self, blk: BlockId, pos: ValueId) -> ValueId {
        let kind = if self.ir.wide {
            InstKind::LoadU16Dyn(Operand::Value(self.units), Operand::Value(pos))
        } else {
            InstKind::LoadU8Dyn(Operand::Value(self.units), Operand::Value(pos))
        };
        self.f.append_inst(blk, kind, Type::I64, None)
    }

    /// `lo <= u <= hi`
    fn in_range(&mut self, blk: BlockId, u: ValueId, lo: u32, hi: u32) -> ValueId {
        let span = hi as i64 - lo as i64 + 1;
        if span == 1 {
            return self.icmp(blk, IPred::Eq, u, Operand::ConstI64(lo as i64));
        }
        let rel = if lo == 0 {
            u
        } else {
            self.binop(blk, BinOp::Sub, u, Operand::ConstI64(lo as i64))
        };
        self.icmp(blk, IPred::Ult, rel, Operand::ConstI64(span))
    }

    /// `out[0] = st; out[1] = end; return end`
    fn answer(&mut self, blk: BlockId, end: Operand) {
        let st = self.load(blk, self.st_slot, 0);
        self.store(blk, Operand::Value(st), self.out, 0);
        self.store(blk, end, self.out, 8);
        self.f.set_term(blk, Terminator::Ret(Some(end)));
    }

    /// a fresh attempt: every group unset, no accept yet
    fn begin_attempt(&mut self, blk: BlockId) {
        for slot in 2..self.ir.stride {
            self.store(blk, Operand::ConstI64(-1), self.out, 8 * slot as u64);
        }
        self.store(blk, Operand::ConstI64(-1), self.last_slot, 0);
    }

    fn state(&mut self, i: usize) {
        let node = &self.ir.states[i];
        let blk = self.states[i];
        // the start state is never an edge target (the analysis seeds a
        // separate state for a loop back to pc 0), so an attempt begins
        // either at its top (it can accept before consuming) or on the
        // edge it takes
        if i == 0 && node.accepts_anywhere() {
            self.begin_attempt(blk);
        }
        let pos = self.load(blk, self.pos_slot, 0);
        if node.probes == 0 {
            self.variant(blk, pos, &node.variants[0], i);
            return;
        }
        let leaves: Vec<BlockId> = node.variants.iter().map(|_| self.f.add_block()).collect();
        self.probe_tree(blk, pos, node.probes, 0, 0, &leaves);
        for (v, &leaf) in node.variants.iter().zip(&leaves) {
            self.variant(leaf, pos, v, i);
        }
    }

    /// an attempt that begins on state 0's edge has no row to clear
    /// before the edge is taken
    fn fresh_on_edge(&self, i: usize) -> bool {
        i == 0 && !self.ir.states[0].accepts_anywhere()
    }

    /// The body of one variant of state `i` from `blk`, at `pos`.
    fn variant(&mut self, blk: BlockId, pos: ValueId, v: &Variant, i: usize) {
        let out = self.out;
        if let Some(acc) = &v.accept {
            for &slot in acc {
                self.store(blk, Operand::Value(pos), out, 8 * slot as u64);
            }
            for &slot in &v.accept_clears {
                self.store(blk, Operand::ConstI64(-1), out, 8 * slot as u64);
            }
            if v.edges.is_empty() {
                self.answer(blk, Operand::Value(pos));
                return;
            }
            if let Some(backup) = self.backup {
                for (k, &slot) in self.held.clone().iter().enumerate() {
                    let val = self.load(blk, out, 8 * slot as u64);
                    self.store(blk, Operand::Value(val), backup, 8 * k as u64);
                }
            }
            self.store(blk, Operand::Value(pos), self.last_slot, 0);
        }
        let fail = self.fail_block(i, pos);
        if v.edges.is_empty() {
            self.f.set_term(blk, Terminator::Br(fail));
            return;
        }
        let read = self.f.add_block();
        let at_end = self.icmp(blk, IPred::Eq, pos, Operand::Value(self.len));
        self.cond_br(blk, at_end, fail, read);
        let reaches_surrogates = v.edges.iter().any(|e| e.hi >= 0xD800);
        let (cur, u, step) = self.read_unit(read, pos, self.ir.pairs && reaches_surrogates);
        self.dispatch_edges(cur, u, pos, step, v, i, fail);
    }

    /// The body of an edge of state `i` from `blk`: its saves at
    /// `pos` and its clears, the cursor past the unit, and the branch to its target.
    fn take_edge(&mut self, blk: BlockId, pos: ValueId, edge: &Edge, step: Operand, i: usize) {
        if self.fresh_on_edge(i) {
            self.begin_attempt(blk);
        }
        self.note_run_end(blk, i, edge.target as usize, pos);
        for &slot in &edge.saves {
            self.store(blk, Operand::Value(pos), self.out, 8 * slot as u64);
        }
        for &slot in &edge.clears {
            self.store(blk, Operand::ConstI64(-1), self.out, 8 * slot as u64);
        }
        let advanced = self.binop(blk, BinOp::Add, pos, step);
        self.store(blk, Operand::Value(advanced), self.pos_slot, 0);
        let target = self.states[edge.target as usize];
        self.f.set_term(blk, Terminator::Br(target));
    }
}
