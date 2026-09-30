//! The one-pass walk (`MatcherIr::exec`) as an SSA function over the
//! payload's code units: `(units, len, start, out) -> end | -1`. A
//! narrow matcher reads bytes (`LoadU8Dyn`), a wide one 16-bit units
//! (`LoadU16Dyn`); the walk is the same.
//!
//! One block per state. A state reads its unit once and dispatches it
//! through a chain of unsigned range compares; an edge's saves are
//! stores into `out` at the current position; the scan over start
//! positions and the loop-carried cursor live in stack slots the
//! mid-end promotes. Under `u` / `v` over UTF-16 an edge that admits
//! a high surrogate steps over the low one that follows it, and so
//! does the start scan.
//!
//! The row is written in place. An accept that has edges may be
//! followed by writes the walk later abandons, so the slots such a
//! walk can still touch (`slots_written_past_accept`) are copied
//! aside at the accept and put back when the walk dies with that
//! accept as its answer.

use crate::ssa::{BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId};
use torajs_regex::onepass::MatcherIr;

pub(crate) fn walk(name: &str, ir: &MatcherIr) -> Function {
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

    let st_slot = f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, Some("st"));
    let pos_slot = f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, Some("pos"));
    let last_slot = f.append_inst(entry, InstKind::Alloca(Type::I64), Type::Ptr, Some("last"));
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
        backup,
        held,
        states,
        next_st,
        dead,
    };

    // scan: every start position up to and including `len`
    let st = e.load(scan, st_slot, 0);
    let past = e.icmp(scan, IPred::Sgt, st, Operand::Value(len));
    e.f.set_term(
        scan,
        Terminator::CondBr {
            cond: Operand::Value(past),
            then_blk: miss,
            else_blk: attempt,
        },
    );
    e.store(attempt, Operand::Value(st), pos_slot, 0);
    e.f.set_term(attempt, Terminator::Br(e.states[0]));

    e.scan_advance(scan);

    // dead: the walk found no edge; the last accept answers if there was one
    let last = e.load(dead, last_slot, 0);
    let hit = e.icmp(dead, IPred::Sge, last, Operand::ConstI64(0));
    e.f.set_term(
        dead,
        Terminator::CondBr {
            cond: Operand::Value(hit),
            then_blk: restore,
            else_blk: next_st,
        },
    );
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
    backup: Option<ValueId>,
    held: Vec<u32>,
    states: Vec<BlockId>,
    next_st: BlockId,
    dead: BlockId,
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

    /// Store `pos + 1` — or `pos + 2` when the unit `u` read at `pos`
    /// is a high surrogate followed by a low one — into `slot`, then
    /// branch to `target`. Answers the block that stores the single
    /// step, for a caller that already knows it is at the end.
    fn pair_advance(
        &mut self,
        blk: BlockId,
        pos: ValueId,
        u: ValueId,
        slot: ValueId,
        target: BlockId,
    ) -> BlockId {
        let one = self.f.add_block();
        let next1 = self.binop(one, BinOp::Add, pos, Operand::ConstI64(1));
        self.store(one, Operand::Value(next1), slot, 0);
        self.f.set_term(one, Terminator::Br(target));
        let two = self.f.add_block();
        let next2 = self.binop(two, BinOp::Add, pos, Operand::ConstI64(2));
        self.store(two, Operand::Value(next2), slot, 0);
        self.f.set_term(two, Terminator::Br(target));
        let check = self.f.add_block();
        let read = self.f.add_block();
        let is_high = self.in_range(blk, u, 0xD800, 0xDBFF);
        self.cond_br(blk, is_high, check, one);
        let next1 = self.binop(check, BinOp::Add, pos, Operand::ConstI64(1));
        let at_end = self.icmp(check, IPred::Eq, next1, Operand::Value(self.len));
        self.cond_br(check, at_end, one, read);
        let u2 = self.unit(read, next1);
        let is_low = self.in_range(read, u2, 0xDC00, 0xDFFF);
        self.cond_br(read, is_low, two, one);
        one
    }

    /// `next_st`: the next start position of the scan
    fn scan_advance(&mut self, scan: BlockId) {
        let next_st = self.next_st;
        let st = self.load(next_st, self.st_slot, 0);
        if !self.ir.pairs {
            let bumped = self.binop(next_st, BinOp::Add, st, Operand::ConstI64(1));
            self.store(next_st, Operand::Value(bumped), self.st_slot, 0);
            self.f.set_term(next_st, Terminator::Br(scan));
            return;
        }
        let read = self.f.add_block();
        let u = self.unit(read, st);
        let one = self.pair_advance(read, st, u, self.st_slot, scan);
        let at_end = self.icmp(next_st, IPred::Eq, st, Operand::Value(self.len));
        self.cond_br(next_st, at_end, one, read);
    }

    fn state(&mut self, i: usize) {
        let ir = self.ir;
        let node = &ir.states[i];
        let blk = self.states[i];
        let out = self.out;
        // the start state is never an edge target, so an attempt begins
        // either at its top (it can accept before consuming) or on the
        // edge it takes
        let fresh_on_edge = i == 0 && node.accept.is_none();
        if i == 0 && node.accept.is_some() {
            self.begin_attempt(blk);
        }
        let pos = self.load(blk, self.pos_slot, 0);
        if let Some(acc) = &node.accept {
            for &slot in acc {
                self.store(blk, Operand::Value(pos), out, 8 * slot as u64);
            }
            if node.edges.is_empty() {
                self.answer(blk, Operand::Value(pos));
                return;
            }
            if let Some(backup) = self.backup {
                for (k, &slot) in self.held.clone().iter().enumerate() {
                    let v = self.load(blk, out, 8 * slot as u64);
                    self.store(blk, Operand::Value(v), backup, 8 * k as u64);
                }
            }
            self.store(blk, Operand::Value(pos), self.last_slot, 0);
        }
        let fail = if fresh_on_edge {
            self.next_st
        } else {
            self.dead
        };
        if node.edges.is_empty() {
            self.f.set_term(blk, Terminator::Br(fail));
            return;
        }
        let read = self.f.add_block();
        let at_end = self.icmp(blk, IPred::Eq, pos, Operand::Value(self.len));
        self.cond_br(blk, at_end, fail, read);
        let u = self.unit(read, pos);
        let mut cur = read;
        let edges = &node.edges;
        for (j, edge) in edges.iter().enumerate() {
            let taken = self.f.add_block();
            let next = if j + 1 == edges.len() {
                fail
            } else {
                self.f.add_block()
            };
            if edge.lo == 0 && edge.hi == ir.unit_max() {
                self.f.set_term(cur, Terminator::Br(taken));
            } else {
                let cond = self.in_range(cur, u, edge.lo, edge.hi);
                self.cond_br(cur, cond, taken, next);
            }
            if fresh_on_edge {
                self.begin_attempt(taken);
            }
            for &slot in &edge.saves {
                self.store(taken, Operand::Value(pos), out, 8 * slot as u64);
            }
            let target = self.states[edge.target as usize];
            if ir.pairs && edge.hi >= 0xD800 && edge.lo <= 0xDBFF {
                self.pair_advance(taken, pos, u, self.pos_slot, target);
            } else {
                let advanced = self.binop(taken, BinOp::Add, pos, Operand::ConstI64(1));
                self.store(taken, Operand::Value(advanced), self.pos_slot, 0);
                self.f.set_term(taken, Terminator::Br(target));
            }
            cur = next;
        }
    }
}
