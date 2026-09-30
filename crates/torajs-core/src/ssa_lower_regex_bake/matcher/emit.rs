//! The one-pass walk (`MatcherIr::exec`) as an SSA function over a
//! Latin-1 payload: `(units, len, start, out) -> end | -1`.
//!
//! One block per state. A state reads its byte once and dispatches it
//! through a chain of unsigned range compares; an edge's saves are
//! stores into `out` at the current position; the scan over start
//! positions and the loop-carried cursor live in stack slots the
//! mid-end promotes.
//!
//! The row is written in place. An accept that has edges may be
//! followed by writes the walk later abandons, so the slots such a
//! walk can still touch (`slots_written_past_accept`) are copied
//! aside at the accept and put back when the walk dies with that
//! accept as its answer.

use crate::ssa::{BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId};
use torajs_regex::onepass::MatcherIr;

pub(crate) fn latin1(name: &str, ir: &MatcherIr) -> Function {
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

    let st = e.load(next_st, st_slot, 0);
    let bumped = e.binop(next_st, BinOp::Add, st, Operand::ConstI64(1));
    e.store(next_st, Operand::Value(bumped), st_slot, 0);
    e.f.set_term(next_st, Terminator::Br(scan));

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
        self.f.set_term(
            blk,
            Terminator::CondBr {
                cond: Operand::Value(at_end),
                then_blk: fail,
                else_blk: read,
            },
        );
        let b = self.f.append_inst(
            read,
            InstKind::LoadU8Dyn(Operand::Value(self.units), Operand::Value(pos)),
            Type::I64,
            None,
        );
        let mut cur = read;
        let edges = &node.edges;
        for (j, edge) in edges.iter().enumerate() {
            let taken = self.f.add_block();
            let next = if j + 1 == edges.len() {
                fail
            } else {
                self.f.add_block()
            };
            if edge.lo == 0 && edge.hi == 255 {
                self.f.set_term(cur, Terminator::Br(taken));
            } else {
                let span = edge.hi as i64 - edge.lo as i64 + 1;
                let cond = if span == 1 {
                    self.icmp(cur, IPred::Eq, b, Operand::ConstI64(edge.lo as i64))
                } else {
                    let rel = if edge.lo == 0 {
                        b
                    } else {
                        self.binop(cur, BinOp::Sub, b, Operand::ConstI64(edge.lo as i64))
                    };
                    self.icmp(cur, IPred::Ult, rel, Operand::ConstI64(span))
                };
                self.f.set_term(
                    cur,
                    Terminator::CondBr {
                        cond: Operand::Value(cond),
                        then_blk: taken,
                        else_blk: next,
                    },
                );
            }
            if fresh_on_edge {
                self.begin_attempt(taken);
            }
            for &slot in &edge.saves {
                self.store(taken, Operand::Value(pos), out, 8 * slot as u64);
            }
            let advanced = self.binop(taken, BinOp::Add, pos, Operand::ConstI64(1));
            self.store(taken, Operand::Value(advanced), self.pos_slot, 0);
            let target = self.states[edge.target as usize];
            self.f.set_term(taken, Terminator::Br(target));
            cur = next;
        }
    }
}
