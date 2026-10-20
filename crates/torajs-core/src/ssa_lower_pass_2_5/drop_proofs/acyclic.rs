//! Whether anything can buffer a cell of a class sid as a cycle root.
//!
//! A release fn buffers a cell whose count stays above zero as a
//! candidate root of a garbage cycle, and tests every dying cell for
//! the buffer's flag. A cell no cycle passes through needs neither: the
//! acyclic cells Bacon and Rajan's collector never buffers. Asked for a
//! closed, sealed sid with no prefix path through it, three things
//! together make every cell of the sid acyclic and unbuffered:
//!
//! - only cells of the sid hold cells of the sid: no other layout, no
//!   array element type and no closure env names it, and a running
//!   function stores one only into a local slot or an own-sid field;
//! - an own-sid field is written only while its cell is private: the
//!   store's base is the function's fresh allocation, or the parameter
//!   0 every direct call passes such a cell in (a constructor), and the
//!   cell goes nowhere but local slots, loads, stores, compares,
//!   parameter 0 of such a function, a release, or the return of the
//!   function that allocated it. The value stored is not a private
//!   cell, so it was published before the cell it goes into, and every
//!   edge between cells of the sid runs from a later cell to an earlier
//!   one: none closes a cycle;
//! - no running function but a release fn hands a cell of the sid to a
//!   runtime entry that could keep or buffer it.
//!
//! The collector itself then never meets such a cell: nothing it walks
//! holds one. Only the running functions that hold cells of the sid are
//! read: no other one stores into one, stores one, or hands one on.

use crate::ssa::id_hash::HashSet;
use crate::ssa::{FuncId, InstKind, Module, Operand, StructId, Terminator, Type, ValueId};
use crate::ssa_lower::OBJ_HEADER_SIZE;

use super::super::cell_slot_flow::{EnvLayouts, elem_obj};
use super::undef::Shared;

const OBJ_ALLOC: &str = "__torajs_obj_alloc";

/// Runtime entries that take a cell and keep nothing of it: the free,
/// the expando and WeakRef releases of a dying cell, the nullish check.
const KEEPS_NOTHING: &[&str] = &[
    "__torajs_obj_drop_sized",
    "__torajs_obj_free_cell",
    "__torajs_obj_props_drop_slow",
    "__torajs_weakref_target_dying",
    "__torajs_heap_nullish_check",
];

/// No cell of `sid` can sit on a cycle, and nothing but its release fn
/// can hand one to the cycle buffer.
pub(super) fn unbuffered(module: &Module, shared: &Shared, envs: &EnvLayouts, sid: u32) -> bool {
    if super::undef::held_elsewhere(module, envs, sid) {
        return false;
    }
    let mut p = Private::new(module, shared, sid);
    p.settle();
    p.scan.iter().all(|&i| p.fn_ok(i))
}

struct Private<'a> {
    module: &'a Module,
    shared: &'a Shared<'a>,
    sid: u32,
    own: Type,
    offs: Vec<u64>,
    alloc: Option<usize>,
    /// The running functions that hold cells of the sid, release fns
    /// aside.
    scan: Vec<usize>,
    in_scan: Vec<bool>,
    /// Per function, whether its parameter 0 is a private cell.
    ctor: Vec<bool>,
    /// Per function read, the values that are private cells.
    private: Vec<Vec<bool>>,
    /// Per function, the local slots that hold nothing but private cells.
    private_slots: Vec<HashSet<ValueId>>,
}

impl<'a> Private<'a> {
    fn new(module: &'a Module, shared: &'a Shared<'a>, sid: u32) -> Self {
        let own = Type::Obj(StructId(sid));
        let offs = (module.struct_layouts[sid as usize].iter().enumerate())
            .filter(|(_, (_, t))| *t == own)
            .map(|(i, _)| OBJ_HEADER_SIZE + 8 * i as u64)
            .collect();
        let (entered, called) = (&shared.entered, &shared.called);
        let releases = &module.class_drop_fns;
        let scan: Vec<usize> = (shared.mentions(sid).iter().copied())
            .filter(|i| !releases.contains(&FuncId(*i as u32)))
            .collect();
        let mut in_scan = vec![false; module.funcs.len()];
        scan.iter().for_each(|i| in_scan[*i] = true);
        // a call from a function not read passes no private cell
        let ctor = (module.funcs.iter().enumerate())
            .map(|(i, f)| {
                !entered[i]
                    && called[i]
                    && f.params.first().is_some_and(|p| f.value_type(*p) == own)
                    && shared.callers[i].iter().all(|c| in_scan[*c])
            })
            .collect();
        Private {
            module,
            shared,
            sid,
            own,
            offs,
            alloc: module.funcs.iter().position(|f| f.name == OBJ_ALLOC),
            scan,
            in_scan,
            ctor,
            private: module.funcs.iter().map(|_| Vec::new()).collect(),
            private_slots: module.funcs.iter().map(|_| HashSet::default()).collect(),
        }
    }

    /// A constructor stays one while every direct call passes it a
    /// private cell. A function's private cells change only when it
    /// stops being a constructor, and then only its own calls are read
    /// again.
    fn settle(&mut self) {
        for k in 0..self.scan.len() {
            let i = self.scan[k];
            (self.private[i], self.private_slots[i]) = self.local(i);
        }
        let module = self.module;
        let mut work = self.scan.clone();
        while let Some(i) = work.pop() {
            for inst in module.funcs[i].blocks.iter().flat_map(|b| &b.insts) {
                if let InstKind::Call(g, args) = &inst.kind
                    && self.ctor[g.0 as usize]
                    && !args.first().is_some_and(|a| is(&self.private[i], a))
                {
                    let g = g.0 as usize;
                    self.ctor[g] = false;
                    if self.in_scan[g] {
                        (self.private[g], self.private_slots[g]) = self.local(g);
                        work.push(g);
                    }
                }
            }
        }
    }

    /// The private cells of function `i`, and the slots that hold only
    /// them: the greatest set of slots whose stores are all private
    /// cells or null.
    fn local(&self, i: usize) -> (Vec<bool>, HashSet<ValueId>) {
        let all = self.shared.slots(i);
        let mut slots: HashSet<ValueId> = (all.iter())
            .filter(|(_, s)| !s.escaped && !s.stores.is_empty())
            .map(|(v, _)| *v)
            .collect();
        loop {
            let private = self.values(i, &slots);
            let before = slots.len();
            slots.retain(|s| {
                (all[s].stores.iter()).all(|v| *v == Operand::ConstPtrNull || is(&private, v))
            });
            if slots.len() == before {
                return (private, slots);
            }
        }
    }

    fn values(&self, i: usize, slots: &HashSet<ValueId>) -> Vec<bool> {
        let f = &self.module.funcs[i];
        let mut private = vec![false; f.values.len()];
        if self.ctor[i] {
            private[f.params[0].0 as usize] = true;
        }
        loop {
            let mut moved = false;
            for inst in f.blocks.iter().flat_map(|b| &b.insts) {
                let Some(r) = inst.result.filter(|r| !private[r.0 as usize]) else {
                    continue;
                };
                let yes = match &inst.kind {
                    InstKind::Call(t, _) => {
                        Some(t.0 as usize) == self.alloc && f.value_type(r) == self.own
                    }
                    InstKind::Load(_, Operand::Value(p), 0) => slots.contains(p),
                    InstKind::Copy(_, o) | InstKind::Identity(o) => is(&private, o),
                    _ => false,
                };
                if yes {
                    private[r.0 as usize] = true;
                    moved = true;
                }
            }
            if !moved {
                return private;
            }
        }
    }

    /// Function `i` writes own-sid fields of private cells only, lets no
    /// private cell go anywhere else, and keeps cells of the sid in its
    /// local slots and own-sid fields.
    fn fn_ok(&self, i: usize) -> bool {
        let f = &self.module.funcs[i];
        let private = &self.private[i];
        let holds = |o: &Operand| match o {
            Operand::Value(v) => {
                elem_obj(self.module, f.value_type(*v)).is_some_and(|s| s.0 == self.sid)
            }
            _ => false,
        };
        let own = |o: &Operand| matches!(o, Operand::Value(b) if f.value_type(*b) == self.own);
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            let ok = match &inst.kind {
                // a bare pointer read out of an own-sid field could be
                // written through where no store shows the sid
                InstKind::Load(Type::Ptr, p, off) => !(own(p) && self.offs.contains(off)),
                InstKind::Load(..)
                | InstKind::ICmp(..)
                | InstKind::Copy(..)
                | InstKind::Identity(..)
                | InstKind::RcInc(_) => true,
                InstKind::Store(v, p, off) if own(p) && self.offs.contains(off) => {
                    is(private, p) && !is(private, v)
                }
                InstKind::Store(v, p, _) if is(private, v) => {
                    matches!(p, Operand::Value(s) if self.private_slots[i].contains(s))
                }
                InstKind::Store(v, p, _) if holds(v) => {
                    matches!(p, Operand::Value(s) if self.shared.slots(i).get(s).is_some_and(|s| !s.escaped))
                }
                InstKind::Store(..) => true,
                InstKind::StoreDyn(v, p, _) | InstKind::StoreDynScaled8(v, p, _) => {
                    !own(p) && !holds(v) && !is(private, v)
                }
                InstKind::Call(g, args) => self.call_ok(i, *g, args, &holds),
                k => {
                    let mut ok = true;
                    crate::ssa::visit_value_operands(k, |v| ok &= !private[v.0 as usize]);
                    ok
                }
            };
            if !ok {
                return false;
            }
        }
        // a constructor's cell is still its caller's to finish
        (f.blocks.iter()).all(|b| match &b.term {
            Terminator::Ret(Some(o)) => !(self.ctor[i] && is(private, o)),
            _ => true,
        })
    }

    fn call_ok(
        &self,
        i: usize,
        g: FuncId,
        args: &[Operand],
        holds: &dyn Fn(&Operand) -> bool,
    ) -> bool {
        let callee = &self.module.funcs[g.0 as usize];
        let runtime = callee.blocks.is_empty();
        let release = self.module.class_drop_fns.contains(&g);
        let keeps_nothing = runtime && KEEPS_NOTHING.contains(&callee.name.as_str());
        args.iter().enumerate().all(|(k, a)| {
            if is(&self.private[i], a) {
                (k == 0 && self.ctor[g.0 as usize]) || release || keeps_nothing
            } else {
                !holds(a) || !runtime || keeps_nothing
            }
        })
    }
}

fn is(private: &[bool], o: &Operand) -> bool {
    matches!(o, Operand::Value(v) if private.get(v.0 as usize) == Some(&true))
}
