//! What an `any` value can hold, as far as the module says.
//!
//! A generic specialization widened to `any` still receives, from
//! every call site in the module, a box of a typed cell or a null;
//! storing such a value into a struct-typed field is not a crossing
//! of unknown layout, and `cell_slot_flow` must not open the field's
//! sid for it. [`AnyHolds`] answers, for an `any` operand, the layouts
//! it can carry: it follows boxes, slot reads back to the stores that
//! filled them, and parameters back to their call sites. Every value
//! starts at "nothing" and only widens, one instruction at a time,
//! until a pass over the module moves nothing: the least fixed point,
//! which an async state machine's slots (each stored from a dozen
//! places that read it back) reach in a few passes where a recursive
//! walk of the same edges does not terminate in practice. A parameter
//! of a function whose address is taken — by an instruction, or by a
//! method table, a vtable or the fn-name registry, which the runtime
//! calls through — or that no call in the module reaches, holds
//! anything. A boxed adapter nothing references is never entered: the
//! runtime reaches an adapter only through an address stored for it,
//! so what such a body hands on reaches nobody ([`dead_adapters`]).

use crate::ssa::id_hash::{HashMap, HashSet};

use crate::ssa::{FuncId, Function, InstKind, Module, Operand, StructId, Type, ValueId};

mod reach;
pub(super) use reach::dead_adapters;
use reach::reached_and_taken;

const UNBOX_VALUE: &str = "__torajs_anyv_unbox_value";
const ARG_TO_STRUCT: &str = "__torajs_anyv_arg_to_struct";
pub(super) const BOXED_ENTRY_PREFIX: &str = "__boxed_";
const BOX_HEAP: i64 = 4;
const BOX_NULL: i64 = 0;
const BOX_UNDEFINED: i64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Holds {
    /// Only cells of these layouts, or null.
    Cells(Vec<StructId>),
    Unknown,
}

impl Holds {
    fn none() -> Holds {
        Holds::Cells(Vec::new())
    }

    fn union(self, other: Holds) -> Holds {
        match (self, other) {
            (Holds::Unknown, _) | (_, Holds::Unknown) => Holds::Unknown,
            (Holds::Cells(mut a), Holds::Cells(b)) => {
                for s in b {
                    if !a.contains(&s) {
                        a.push(s);
                    }
                }
                Holds::Cells(a)
            }
        }
    }
}

pub(crate) struct AnyHolds<'a> {
    module: &'a Module,
    facts: Vec<FnFacts<'a>>,
    /// Per function, what each value not typed as a struct holds.
    values: Vec<HashMap<ValueId, Holds>>,
    params: HashMap<(FuncId, usize), Holds>,
    unbox_value: Option<usize>,
    arg_to_struct: Option<usize>,
    dead: Vec<bool>,
    /// Functions entered through an address: an instruction takes it
    /// or a table holds it.
    taken: HashSet<FuncId>,
}

#[derive(Default)]
struct FnFacts<'a> {
    /// Every operand stored at offset 0 of each `Alloca` of the
    /// function. A slot behind any other pointer (a capture box, a
    /// frame) is written from other functions too, and nothing here
    /// folds it.
    stores: HashMap<ValueId, Vec<&'a Operand>>,
    params: HashMap<ValueId, usize>,
}

impl<'a> AnyHolds<'a> {
    pub(crate) fn of(module: &'a Module) -> AnyHolds<'a> {
        let find = |name: &str| module.funcs.iter().position(|f| f.name == name);
        let mut holds = AnyHolds {
            module,
            // a body-less function has no instruction to read its own
            // slots or parameters
            facts: (module.funcs.iter())
                .map(|f| {
                    if f.blocks.is_empty() {
                        FnFacts::default()
                    } else {
                        FnFacts::of(f)
                    }
                })
                .collect(),
            values: vec![HashMap::default(); module.funcs.len()],
            params: HashMap::default(),
            unbox_value: find(UNBOX_VALUE),
            arg_to_struct: find(ARG_TO_STRUCT),
            dead: dead_adapters(module),
            taken: HashSet::default(),
        };
        holds.seed_params();
        while holds.pass() {}
        holds
    }

    /// Every `any` parameter a direct call can reach starts at
    /// nothing; one of an address-taken or uncalled function starts
    /// at [`Holds::Unknown`].
    fn seed_params(&mut self) {
        let (reached, taken) = reached_and_taken(self.module, &self.dead);
        for (i, f) in self.module.funcs.iter().enumerate() {
            let fid = FuncId(i as u32);
            let start = if reached.contains(&fid) && !taken.contains(&fid) {
                Holds::none()
            } else {
                Holds::Unknown
            };
            for (idx, p) in f.params.iter().enumerate() {
                if f.value_type(*p) == Type::Any {
                    self.params.insert((fid, idx), start.clone());
                }
            }
        }
        self.taken = taken;
    }

    /// One pass over every instruction of the module; `true` when
    /// something widened.
    fn pass(&mut self) -> bool {
        let mut changed = false;
        for (i, f) in self.module.funcs.iter().enumerate() {
            if self.dead[i] {
                continue;
            }
            let fid = FuncId(i as u32);
            for inst in f.blocks.iter().flat_map(|b| &b.insts) {
                if let Some(v) = inst.result
                    && !matches!(f.value_type(v), Type::Obj(_))
                {
                    let step = self.step(fid, &inst.kind);
                    changed |= widen(&mut self.values[i], v, step);
                }
                if let InstKind::Call(callee, args) = &inst.kind {
                    for (idx, arg) in args.iter().enumerate() {
                        let key = (*callee, idx);
                        if matches!(self.params.get(&key), Some(Holds::Cells(_))) {
                            let step = self.eval(fid, arg);
                            changed |= widen(&mut self.params, key, step);
                        }
                    }
                }
            }
        }
        changed
    }

    /// What the instruction's result holds, from what its operands
    /// hold now.
    fn step(&self, fid: FuncId, kind: &InstKind) -> Holds {
        let f = &self.module.funcs[fid.0 as usize];
        match kind {
            InstKind::BoxAny(BOX_NULL | BOX_UNDEFINED, _) => Holds::none(),
            InstKind::BoxAny(BOX_HEAP, Operand::Value(c))
            | InstKind::PtrToInt(Operand::Value(c)) => match f.value_type(*c) {
                Type::Obj(sid) => Holds::Cells(vec![sid]),
                _ => Holds::Unknown,
            },
            InstKind::Load(_, Operand::Value(slot), 0) => {
                match self.facts[fid.0 as usize].stores.get(slot) {
                    Some(stored) => stored
                        .iter()
                        .fold(Holds::none(), |h, s| h.union(self.eval(fid, s))),
                    None => Holds::Unknown,
                }
            }
            InstKind::Copy(_, a) | InstKind::Identity(a) | InstKind::IntToPtr(a) => {
                self.eval(fid, a)
            }
            InstKind::Select(_, _, a, b) => self.eval(fid, a).union(self.eval(fid, b)),
            InstKind::Call(callee, args) => {
                let callee = callee.0 as usize;
                if Some(callee) == self.unbox_value
                    && let [inner] = args.as_slice()
                {
                    self.eval(fid, inner)
                } else if Some(callee) == self.arg_to_struct {
                    // the kernel answers a cell of the parameter's
                    // layout or of one extending it by prefix, or a
                    // non-cell: nothing a struct-typed slot cannot
                    // size (`torajs-anyvalue::arg_struct_coerce`)
                    Holds::none()
                } else {
                    Holds::Unknown
                }
            }
            _ => Holds::Unknown,
        }
    }

    /// A boxed adapter nothing references ([`dead_adapters`]).
    pub(crate) fn is_dead(&self, fid: FuncId) -> bool {
        self.dead[fid.0 as usize]
    }

    /// Every function entered through an address, in module order.
    pub(crate) fn address_taken(&self) -> Vec<FuncId> {
        let mut fids: Vec<FuncId> = self.taken.iter().copied().collect();
        fids.sort_by_key(|f| f.0);
        fids
    }

    pub(crate) fn eval(&self, fid: FuncId, op: &Operand) -> Holds {
        let f = &self.module.funcs[fid.0 as usize];
        let v = match op {
            Operand::ConstPtrNull | Operand::ConstI64(0) => return Holds::none(),
            Operand::Value(v) => *v,
            _ => return Holds::Unknown,
        };
        if let Type::Obj(sid) = f.value_type(v) {
            return Holds::Cells(vec![sid]);
        }
        if let Some(idx) = self.facts[fid.0 as usize].params.get(&v) {
            // a boxed entry's env slot carries the receiver the any
            // lane dispatched on: the method was chosen by that
            // instance's class tag, so it fits the method's `this`
            if *idx == 0 && f.name.starts_with(BOXED_ENTRY_PREFIX) {
                return Holds::none();
            }
            return self
                .params
                .get(&(fid, *idx))
                .cloned()
                .unwrap_or(Holds::Unknown);
        }
        // a value defined further down the function is still at
        // nothing on the first pass; a later pass reads its answer
        self.values[fid.0 as usize]
            .get(&v)
            .cloned()
            .unwrap_or_else(Holds::none)
    }
}

/// Union `step` into the entry at `key`; `true` when the entry grew.
fn widen<K: std::hash::Hash + Eq>(table: &mut HashMap<K, Holds>, key: K, step: Holds) -> bool {
    let old = table.get(&key).cloned().unwrap_or_else(Holds::none);
    let new = old.clone().union(step);
    let grew = new != old;
    if grew || !table.contains_key(&key) {
        table.insert(key, new);
    }
    grew
}

impl<'a> FnFacts<'a> {
    fn of(f: &'a Function) -> FnFacts<'a> {
        let mut facts = FnFacts {
            stores: HashMap::default(),
            params: f.params.iter().enumerate().map(|(i, p)| (*p, i)).collect(),
        };
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            if let (InstKind::Alloca(_), Some(v)) = (&inst.kind, inst.result) {
                facts.stores.entry(v).or_default();
            }
        }
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            if let InstKind::Store(v, Operand::Value(slot), 0) = &inst.kind
                && let Some(stored) = facts.stores.get_mut(slot)
            {
                stored.push(v);
            }
        }
        facts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssa::{BlockId, Terminator};

    fn module() -> Module {
        Module {
            struct_layouts: vec![Vec::new(), Vec::new()],
            ..Default::default()
        }
    }

    fn push(m: &mut Module, f: Function) -> FuncId {
        let id = FuncId(m.funcs.len() as u32);
        m.funcs.push(f);
        id
    }

    fn cell(f: &mut Function, entry: BlockId, sid: u32) -> Operand {
        Operand::Value(f.append_inst(
            entry,
            InstKind::AllocaBytes(32),
            Type::Obj(StructId(sid)),
            None,
        ))
    }

    fn boxed(f: &mut Function, entry: BlockId, tag: i64, v: Operand) -> Operand {
        Operand::Value(f.append_inst(entry, InstKind::BoxAny(tag, v), Type::Any, None))
    }

    #[test]
    fn a_widened_parameter_holds_what_every_call_site_boxes() {
        let mut m = module();
        let mut callee = Function::new("ctor", Type::Void);
        let p = callee.add_param(Type::Any, "l");
        let entry = callee.add_block();
        let slot = callee.append_inst(entry, InstKind::Alloca(Type::Any), Type::Ptr, None);
        callee.append_void(
            entry,
            InstKind::Store(Operand::Value(p), Operand::Value(slot), 0),
        );
        let read = callee.append_inst(
            entry,
            InstKind::Load(Type::Any, Operand::Value(slot), 0),
            Type::Any,
            None,
        );
        callee.set_term(entry, Terminator::Ret(None));
        let ctor = push(&mut m, callee);
        let mut caller = Function::new("f", Type::Void);
        let entry = caller.add_block();
        let c = cell(&mut caller, entry, 1);
        let b = boxed(&mut caller, entry, BOX_HEAP, c);
        caller.append_void(entry, InstKind::Call(ctor, vec![b]));
        let n = boxed(&mut caller, entry, BOX_NULL, Operand::ConstI64(0));
        caller.append_void(entry, InstKind::Call(ctor, vec![n]));
        caller.set_term(entry, Terminator::Ret(None));
        push(&mut m, caller);
        let holds = AnyHolds::of(&m);
        assert_eq!(
            holds.eval(ctor, &Operand::Value(read)),
            Holds::Cells(vec![StructId(1)])
        );
    }

    /// What an adapter nobody references hands on reaches no
    /// parameter; once something takes its address, it does.
    #[test]
    fn an_unreferenced_adapter_reaches_no_parameter() {
        let mut m = module();
        let mut callee = Function::new("ctor", Type::Void);
        let p = callee.add_param(Type::Any, "l");
        callee.add_block();
        let ctor = push(&mut m, callee);
        let mut adapter = Function::new("__boxed_ctor", Type::Void);
        let entry = adapter.add_block();
        let num = boxed(&mut adapter, entry, 2, Operand::ConstI64(7));
        adapter.append_void(entry, InstKind::Call(ctor, vec![num]));
        adapter.set_term(entry, Terminator::Ret(None));
        let a = push(&mut m, adapter);
        let mut caller = Function::new("f", Type::Void);
        let entry = caller.add_block();
        let c = cell(&mut caller, entry, 1);
        let b = boxed(&mut caller, entry, BOX_HEAP, c);
        caller.append_void(entry, InstKind::Call(ctor, vec![b]));
        caller.set_term(entry, Terminator::Ret(None));
        push(&mut m, caller);
        let holds = AnyHolds::of(&m);
        assert!(holds.is_dead(a));
        assert_eq!(
            holds.eval(ctor, &Operand::Value(p)),
            Holds::Cells(vec![StructId(1)])
        );
        let mut taker = Function::new("g", Type::Void);
        let entry = taker.add_block();
        taker.append_inst(entry, InstKind::FnAddr(a), Type::Ptr, None);
        taker.set_term(entry, Terminator::Ret(None));
        push(&mut m, taker);
        let holds = AnyHolds::of(&m);
        assert!(!holds.is_dead(a));
        assert_eq!(holds.eval(ctor, &Operand::Value(p)), Holds::Unknown);
    }

    #[test]
    fn an_address_taken_or_uncalled_function_holds_anything() {
        let mut m = module();
        let mut taken = Function::new("g", Type::Void);
        let p = taken.add_param(Type::Any, "x");
        taken.add_block();
        let g = push(&mut m, taken);
        let mut lone = Function::new("h", Type::Void);
        let q = lone.add_param(Type::Any, "x");
        lone.add_block();
        let h = push(&mut m, lone);
        let mut caller = Function::new("f", Type::Void);
        let entry = caller.add_block();
        caller.append_inst(entry, InstKind::FnAddr(g), Type::Ptr, None);
        let n = boxed(&mut caller, entry, BOX_NULL, Operand::ConstI64(0));
        caller.append_void(entry, InstKind::Call(g, vec![n]));
        caller.set_term(entry, Terminator::Ret(None));
        push(&mut m, caller);
        let holds = AnyHolds::of(&m);
        assert_eq!(holds.eval(g, &Operand::Value(p)), Holds::Unknown);
        assert_eq!(holds.eval(h, &Operand::Value(q)), Holds::Unknown);
    }

    #[test]
    fn a_function_a_table_holds_is_address_taken() {
        let mut m = module();
        let mut row = Function::new("__cm_C__m", Type::Void);
        let p = row.add_param(Type::Any, "x");
        row.add_block();
        let fid = push(&mut m, row);
        let mut caller = Function::new("f", Type::Void);
        let entry = caller.add_block();
        let n = boxed(&mut caller, entry, BOX_NULL, Operand::ConstI64(0));
        caller.append_void(entry, InstKind::Call(fid, vec![n]));
        caller.set_term(entry, Terminator::Ret(None));
        push(&mut m, caller);
        assert_eq!(
            AnyHolds::of(&m).eval(fid, &Operand::Value(p)),
            Holds::none()
        );
        m.vtable_globals.push(crate::ssa::VtableGlobal {
            class_name: "C".into(),
            fn_ids: vec![None, Some(fid)],
        });
        assert_eq!(
            AnyHolds::of(&m).eval(fid, &Operand::Value(p)),
            Holds::Unknown
        );
    }

    #[test]
    fn a_slot_behind_a_bare_pointer_is_not_folded() {
        let mut m = module();
        let mut f = Function::new("f", Type::Void);
        let entry = f.add_block();
        let bx = f.append_inst(entry, InstKind::Call(FuncId(0), vec![]), Type::Ptr, None);
        let n = boxed(&mut f, entry, BOX_NULL, Operand::ConstI64(0));
        f.append_void(entry, InstKind::Store(n, Operand::Value(bx), 0));
        let read = f.append_inst(
            entry,
            InstKind::Load(Type::Any, Operand::Value(bx), 0),
            Type::Any,
            None,
        );
        f.set_term(entry, Terminator::Ret(None));
        let fid = push(&mut m, f);
        assert_eq!(
            AnyHolds::of(&m).eval(fid, &Operand::Value(read)),
            Holds::Unknown
        );
    }

    #[test]
    fn a_box_of_a_non_cell_and_a_foreign_call_are_unknown() {
        let mut m = module();
        let mut f = Function::new("f", Type::Void);
        let entry = f.add_block();
        let num = boxed(&mut f, entry, 2, Operand::ConstI64(7));
        let call = f.append_inst(entry, InstKind::Call(FuncId(0), vec![]), Type::Any, None);
        f.set_term(entry, Terminator::Ret(None));
        let fid = push(&mut m, f);
        let holds = AnyHolds::of(&m);
        assert_eq!(holds.eval(fid, &num), Holds::Unknown);
        assert_eq!(holds.eval(fid, &Operand::Value(call)), Holds::Unknown);
    }
}
