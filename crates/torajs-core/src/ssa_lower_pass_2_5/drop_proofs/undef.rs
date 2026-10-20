//! Whether the undefined sentinel can reach an own-sid field of a sid.
//!
//! Asked for a closed, sealed sid with no prefix path through it: then
//! every write into its cells is a typed store the running functions
//! show, through a value typed as the sid. The answer is a least fixed
//! point of "may be the sentinel" over the pointer values of the
//! running functions that hold cells of the sid (the others neither
//! store into one nor read one), their local slots, returns and
//! parameters. The sentinel is a global; a value is clear when it is
//! null, a fresh allocation, a read of an own-sid field (what is being
//! proven, by induction over the stores), a local slot every store
//! into which is clear, the return of a function whose returns are all
//! clear, or a parameter every direct call passes clear. Anything else
//! may be the sentinel: a runtime call's answer or that of a function
//! not read, any other slot read, a parameter of a function entered
//! through an address, never called directly, or called from a
//! function not read.

use std::cell::OnceCell;

use crate::ssa::id_hash::HashMap;
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
pub(super) fn own_fields_defined(module: &Module, shared: &Shared, sid: u32) -> bool {
    let own = Type::Obj(StructId(sid));
    let offs: Vec<u64> = (module.struct_layouts[sid as usize].iter().enumerate())
        .filter(|(_, (_, t))| *t == own)
        .map(|(i, _)| OBJ_HEADER_SIZE + 8 * i as u64)
        .collect();
    let mut a = Analysis::new(module, shared, sid, own, offs);
    a.settle();
    a.stores_clear()
}

/// What every proof reads of the functions that run (the ones `skip`
/// leaves that have a body): which of all functions are entered through
/// an address or a table, which a running function calls directly and
/// from where, which running functions hold cells of each sid, and the
/// local slots of a running function, read when first asked for.
pub(super) struct Shared<'a> {
    module: &'a Module,
    pub skip: &'a [bool],
    pub entered: Vec<bool>,
    pub called: Vec<bool>,
    /// Per function, the running functions that call it directly.
    pub callers: Vec<Vec<usize>>,
    /// Per sid, the running functions some value of which holds cells
    /// of the sid: a cell, or an array of them.
    mentions: HashMap<u32, Vec<usize>>,
    slots: Vec<OnceCell<HashMap<ValueId, Slot>>>,
}

impl<'a> Shared<'a> {
    pub(super) fn new(module: &'a Module, skip: &'a [bool]) -> Self {
        let n = module.funcs.len();
        let mut entered = vec![false; n];
        let mut called = vec![false; n];
        let mut callers: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut mentions: HashMap<u32, Vec<usize>> = HashMap::default();
        for (i, f) in module.funcs.iter().enumerate() {
            if skip[i] || f.blocks.is_empty() {
                continue;
            }
            for inst in f.blocks.iter().flat_map(|b| &b.insts) {
                match &inst.kind {
                    InstKind::Call(t, _) => {
                        called[t.0 as usize] = true;
                        let c = &mut callers[t.0 as usize];
                        if c.last() != Some(&i) {
                            c.push(i);
                        }
                    }
                    InstKind::FnAddr(t) | InstKind::BoxedEntryAddr(t) => {
                        entered[t.0 as usize] = true
                    }
                    _ => {}
                }
            }
            let tys = f.values.iter().map(|v| v.ty).chain([f.ret]);
            for s in tys.filter_map(|t| elem_obj(module, t)) {
                let m = mentions.entry(s.0).or_default();
                if m.last() != Some(&i) {
                    m.push(i);
                }
            }
        }
        let tables = (module.vtable_globals.iter())
            .flat_map(|v| v.fn_ids.iter().flatten().copied())
            .chain((module.suspend_fns.iter()).flat_map(|s| [s.resume, s.drop, s.trace]));
        for t in tables {
            entered[t.0 as usize] = true;
        }
        Shared {
            module,
            skip,
            entered,
            called,
            callers,
            mentions,
            slots: (0..n).map(|_| OnceCell::new()).collect(),
        }
    }

    /// The running functions that hold cells of `sid`, in order.
    pub(super) fn mentions(&self, sid: u32) -> &[usize] {
        self.mentions.get(&sid).map_or(&[], Vec::as_slice)
    }

    /// The local slots of function `i`.
    pub(super) fn slots(&self, i: usize) -> &HashMap<ValueId, Slot> {
        self.slots[i].get_or_init(|| local_slots(&self.module.funcs[i]))
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
    shared: &'a Shared<'a>,
    own: Type,
    offs: Vec<u64>,
    alloc: Option<usize>,
    /// The running functions that hold cells of the sid, the ones read.
    read: &'a [usize],
    is_read: Vec<bool>,
    /// Per function read, the values that may be the sentinel.
    maybe: Vec<Vec<bool>>,
    /// Per function, whether some return may be the sentinel.
    ret: Vec<bool>,
    /// Per function read, which parameters may be the sentinel.
    params: Vec<Vec<bool>>,
}

impl<'a> Analysis<'a> {
    fn new(
        module: &'a Module,
        shared: &'a Shared<'a>,
        sid: u32,
        own: Type,
        offs: Vec<u64>,
    ) -> Self {
        let n = module.funcs.len();
        let read = shared.mentions(sid);
        let mut is_read = vec![false; n];
        let mut maybe = vec![Vec::new(); n];
        for &i in read {
            is_read[i] = true;
            maybe[i] = vec![false; module.funcs[i].values.len()];
        }
        let (entered, called) = (&shared.entered, &shared.called);
        let mut params = vec![Vec::new(); n];
        for &i in read {
            let blind = entered[i] || !called[i];
            let outside = shared.callers[i].iter().any(|c| !is_read[*c]);
            params[i] = vec![blind || outside; module.funcs[i].params.len()];
        }
        Analysis {
            module,
            shared,
            own,
            offs,
            alloc: module.funcs.iter().position(|f| f.name == OBJ_ALLOC),
            read,
            is_read,
            maybe,
            ret: vec![false; n],
            params,
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
                let unread = self.module.funcs[c].blocks.is_empty() || !self.is_read[c];
                unread || self.shared.skip[c] || self.ret[c]
            }
            InstKind::Load(_, Operand::Value(p), off) => match self.shared.slots(i).get(p) {
                Some(s) => s.escaped || s.stores.iter().any(|v| self.op(i, v)),
                None => !(f.value_type(*p) == self.own && self.offs.contains(off)),
            },
            InstKind::Copy(_, o) | InstKind::Identity(o) => self.op(i, o),
            InstKind::Select(_, _, a, b) => self.op(i, a) || self.op(i, b),
            _ => true,
        }
    }

    /// Raise every fact to what the functions read give it, until
    /// nothing moves.
    fn settle(&mut self) {
        let module = self.module;
        loop {
            let mut moved = false;
            for &i in self.read {
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
        self.read.iter().all(|&i| {
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
                // a bare pointer read out of an own-sid field could be
                // written through where no store shows the sid
                InstKind::Load(Type::Ptr, Operand::Value(p), off) => {
                    !(f.value_type(*p) == self.own && self.offs.contains(off))
                }
                _ => true,
            })
        })
    }
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
