//! Whether the undefined sentinel can reach an own-sid field of a sid.
//!
//! Asked for a closed, sealed sid with no prefix path through it: then
//! every write into its cells is a typed store the running functions
//! show, through a value typed as the sid. The answer is a least fixed
//! point of "may be the sentinel" over the pointer values of those
//! functions, their local slots, returns and parameters. The sentinel
//! is a global; a value is clear when it is null, a fresh allocation,
//! a read of an own-sid field (what is being proven, by induction over
//! the stores), a local slot every store into which is clear, the
//! return of a function whose returns are all clear, or a parameter
//! every direct call passes clear. Anything else may be the sentinel:
//! a runtime call's answer, any other slot read, a parameter of a
//! function entered through an address or never called directly.

use std::collections::HashMap;

use crate::ssa::{
    FuncId, Function, InstKind, Module, Operand, StructId, Terminator, Type, ValueId,
};
use crate::ssa_lower::OBJ_HEADER_SIZE;

use super::super::cell_slot_flow::{EnvLayouts, elem_obj};

const OBJ_ALLOC: &str = "__torajs_obj_alloc";

/// Some layout other than `sid`'s, an array element type or a closure
/// env names cells of `sid`. The release walks those too and pushes
/// what it finds there without the per-cell test, and this proof reads
/// only the stores into the sid's own fields.
pub(super) fn held_elsewhere(module: &Module, envs: &EnvLayouts, sid: u32) -> bool {
    let names = |t: &Type| elem_obj(module, *t).is_some_and(|s| s.0 == sid);
    let layouts = (module.struct_layouts.iter().enumerate())
        .any(|(i, l)| i != sid as usize && l.iter().any(|(_, t)| names(t)));
    let arrays = (0..module.arr_layouts.len()).any(|a| names(&module.arr_layouts[a]));
    let closures = envs.values().flatten().any(|(t, _)| names(t));
    layouts || arrays || closures
}

/// No store a running function makes into an own-sid field of a cell of
/// `sid` can store the undefined sentinel.
pub(super) fn own_fields_defined(
    module: &Module,
    skip: &[bool],
    shared: &Shared,
    sid: u32,
) -> bool {
    let own = Type::Obj(StructId(sid));
    let offs: Vec<u64> = (module.struct_layouts[sid as usize].iter().enumerate())
        .filter(|(_, (_, t))| *t == own)
        .map(|(i, _)| OBJ_HEADER_SIZE + 8 * i as u64)
        .collect();
    let mut a = Analysis::new(module, skip, shared, own, offs);
    a.settle();
    a.stores_clear()
}

/// What every proof reads of the functions that run: which they are,
/// which of all are entered through an address or a table and which
/// they call directly, and the local slots of each.
pub(super) struct Shared {
    pub running: Vec<usize>,
    pub entered: Vec<bool>,
    pub called: Vec<bool>,
    pub slots: Vec<HashMap<ValueId, Slot>>,
}

impl Shared {
    pub(super) fn new(module: &Module, skip: &[bool]) -> Shared {
        let running = running(module, skip);
        let (entered, called) = entries(module, &running);
        let mut slots: Vec<HashMap<ValueId, Slot>> =
            module.funcs.iter().map(|_| HashMap::new()).collect();
        for &i in &running {
            slots[i] = local_slots(&module.funcs[i]);
        }
        Shared {
            running,
            entered,
            called,
            slots,
        }
    }
}

/// A local slot: what is stored through it, and whether its address
/// goes anywhere but the pointer of a load or a store.
#[derive(Default)]
pub(super) struct Slot {
    pub stores: Vec<Operand>,
    pub escaped: bool,
}

struct Analysis<'a> {
    module: &'a Module,
    skip: &'a [bool],
    own: Type,
    offs: Vec<u64>,
    alloc: Option<usize>,
    /// The functions that run and have a body.
    running: &'a [usize],
    /// Per function, the values that may be the sentinel.
    maybe: Vec<Vec<bool>>,
    /// Per function, whether some return may be the sentinel.
    ret: Vec<bool>,
    /// Per function, which parameters may be the sentinel.
    params: Vec<Vec<bool>>,
    /// Per function, its local slots.
    slots: &'a [HashMap<ValueId, Slot>],
}

impl<'a> Analysis<'a> {
    fn new(
        module: &'a Module,
        skip: &'a [bool],
        shared: &'a Shared,
        own: Type,
        offs: Vec<u64>,
    ) -> Self {
        let n = module.funcs.len();
        let (entered, called) = (&shared.entered, &shared.called);
        let params = (module.funcs.iter().enumerate())
            .map(|(i, f)| vec![entered[i] || !called[i]; f.params.len()])
            .collect();
        Analysis {
            module,
            skip,
            own,
            offs,
            alloc: module.funcs.iter().position(|f| f.name == OBJ_ALLOC),
            running: &shared.running,
            maybe: (module.funcs.iter())
                .map(|f| vec![false; f.values.len()])
                .collect(),
            ret: vec![false; n],
            params,
            slots: &shared.slots,
        }
    }

    /// Whether `o`, an operand of function `i`, may be the sentinel.
    fn op(&self, i: usize, o: &Operand) -> bool {
        match o {
            Operand::ConstPtrNull | Operand::ConstI64(0) => false,
            Operand::Value(v) => {
                let f = &self.module.funcs[i];
                if let Some(k) = f.params.iter().position(|p| p == v) {
                    return self.params[i][k];
                }
                match f.value_type(*v) {
                    Type::Obj(_) | Type::Ptr => self.maybe[i][v.0 as usize],
                    _ => true,
                }
            }
            _ => true,
        }
    }

    fn result(&self, i: usize, kind: &InstKind) -> bool {
        let f = &self.module.funcs[i];
        match kind {
            InstKind::Call(t, _) if Some(t.0 as usize) == self.alloc => false,
            InstKind::Call(t, _) => {
                let c = t.0 as usize;
                self.module.funcs[c].blocks.is_empty() || self.skip[c] || self.ret[c]
            }
            InstKind::Load(_, Operand::Value(p), off) => match self.slots[i].get(p) {
                Some(s) => s.escaped || s.stores.iter().any(|v| self.op(i, v)),
                None => !(f.value_type(*p) == self.own && self.offs.contains(off)),
            },
            InstKind::Copy(_, o) | InstKind::Identity(o) => self.op(i, o),
            InstKind::Select(_, _, a, b) => self.op(i, a) || self.op(i, b),
            _ => true,
        }
    }

    /// Raise every fact to what the running functions give it, until
    /// nothing moves.
    fn settle(&mut self) {
        let module = self.module;
        loop {
            let mut moved = false;
            for k in 0..self.running.len() {
                let i = self.running[k];
                let f = &module.funcs[i];
                for inst in f.blocks.iter().flat_map(|b| &b.insts) {
                    if let Some(r) = inst.result
                        && matches!(f.value_type(r), Type::Obj(_) | Type::Ptr)
                        && !self.maybe[i][r.0 as usize]
                        && self.result(i, &inst.kind)
                    {
                        self.maybe[i][r.0 as usize] = true;
                        moved = true;
                    }
                    if let InstKind::Call(t, args) = &inst.kind {
                        moved |= self.pass_args(i, *t, args);
                    }
                }
                let mut rets = f.blocks.iter().filter_map(|b| match &b.term {
                    Terminator::Ret(Some(o)) => Some(o),
                    _ => None,
                });
                if !self.ret[i] && rets.any(|o| self.op(i, o)) {
                    self.ret[i] = true;
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }
    }

    fn pass_args(&mut self, i: usize, callee: FuncId, args: &[Operand]) -> bool {
        let c = callee.0 as usize;
        let mut moved = false;
        for (k, a) in args.iter().enumerate().take(self.params[c].len()) {
            if !self.params[c][k] && self.op(i, a) {
                self.params[c][k] = true;
                moved = true;
            }
        }
        moved
    }

    /// Every store into an own-sid field is clear, and no dynamic store
    /// goes into a cell of the sid, in the running functions.
    fn stores_clear(&self) -> bool {
        self.running.iter().all(|&i| {
            let f = &self.module.funcs[i];
            (f.blocks.iter().flat_map(|b| &b.insts)).all(|inst| match &inst.kind {
                InstKind::Store(v, Operand::Value(p), off)
                    if f.value_type(*p) == self.own && self.offs.contains(off) =>
                {
                    !self.op(i, v)
                }
                InstKind::StoreDyn(_, Operand::Value(p), _)
                | InstKind::StoreDynScaled8(_, Operand::Value(p), _) => {
                    f.value_type(*p) != self.own
                }
                _ => true,
            })
        })
    }
}

/// The functions `skip` leaves that have a body.
fn running(module: &Module, skip: &[bool]) -> Vec<usize> {
    (0..module.funcs.len())
        .filter(|i| !skip[*i] && !module.funcs[*i].blocks.is_empty())
        .collect()
}

/// Per function, whether something enters it through an address or a
/// table (`entered`), and whether one of `running` calls it directly
/// (`called`).
fn entries(module: &Module, running: &[usize]) -> (Vec<bool>, Vec<bool>) {
    let n = module.funcs.len();
    let mut entered = vec![false; n];
    let mut called = vec![false; n];
    for &i in running {
        for inst in module.funcs[i].blocks.iter().flat_map(|b| &b.insts) {
            match &inst.kind {
                InstKind::Call(t, _) => called[t.0 as usize] = true,
                InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) => entered[t.0 as usize] = true,
                _ => {}
            }
        }
    }
    let tables = (module.vtable_globals.iter())
        .flat_map(|v| v.fn_ids.iter().flatten().copied())
        .chain((module.suspend_fns.iter()).flat_map(|s| [s.resume, s.drop, s.trace]));
    for t in tables {
        entered[t.0 as usize] = true;
    }
    (entered, called)
}

/// The local slots of `f`, each with what is stored through it and
/// whether its address goes anywhere but a load or a store's pointer.
fn local_slots(f: &Function) -> HashMap<ValueId, Slot> {
    let insts = || f.blocks.iter().flat_map(|b| &b.insts);
    let mut slots: HashMap<ValueId, Slot> = insts()
        .filter(|i| matches!(i.kind, InstKind::Alloca(_)))
        .filter_map(|i| i.result)
        .map(|v| (v, Slot::default()))
        .collect();
    if slots.is_empty() {
        return slots;
    }
    let escape = |slots: &mut HashMap<ValueId, Slot>, o: &Operand| {
        if let Operand::Value(v) = o
            && let Some(s) = slots.get_mut(v)
        {
            s.escaped = true;
        }
    };
    for inst in insts() {
        match &inst.kind {
            InstKind::Load(..) => {}
            InstKind::Store(v, Operand::Value(p), _) if slots.contains_key(p) => {
                escape(&mut slots, v);
                slots.get_mut(p).expect("a slot").stores.push(*v);
            }
            InstKind::Store(v, _, _) => escape(&mut slots, v),
            k => crate::ssa::visit_value_operands(k, |v| escape(&mut slots, &Operand::Value(v))),
        }
    }
    for b in &f.blocks {
        if let Terminator::Ret(Some(o)) | Terminator::CondBr { cond: o, .. } = &b.term {
            escape(&mut slots, o);
        }
    }
    slots
}
