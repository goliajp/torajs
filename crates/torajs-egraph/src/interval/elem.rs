//! Element points — the interval lattice's key extended from "an SSA
//! value" to "the elements of an allocation site".
//!
//! A `number[]` whose elements are stored in i64 slots reads back
//! through `LoadDyn`, whose transfer function has nothing to say: the
//! load is opaque, so the value comes out `full_i64`, so the `sitofp`
//! that feeds an accumulator is never exact, so the accumulator never
//! demotes. The elements have a range — every value written into them
//! has one — and the load is the one place it cannot be seen.
//!
//! This module finds the arrays whose element range can be stated, and
//! the loads that read them. It is a textbook allocation-site heap
//! abstraction (Cousot); `interval/mod.rs` then runs the element point
//! through the same Kleene ascent as any other multi-def cell, because
//! that is exactly what it is — a cell whose defs are stores rather
//! than instructions.
//!
//! # The direction that matters
//!
//! `torajs-arr` exports on the order of 135 entry points. Asking of
//! each "does it write elements?" is a shape where missing one is
//! silent-wrong: the fact would be narrower than the values that
//! actually land in the slots, the accumulator would demote on a
//! promise nothing keeps, and the program would quietly compute the
//! wrong number.
//!
//! So the question is asked the other way. An allocation is tracked
//! only when EVERY use of it — and of the data pointer loaded out of
//! it — is one of the handful of shapes listed below. A runtime
//! function nobody taught this module about therefore makes the
//! analysis give up on that array rather than assume it harmless, and
//! a new one added tomorrow does the same without anyone remembering
//! to come here. This mirrors `rc_peephole::collect_unescaped_slots`
//! for stack slots; the difference is that an array must survive
//! passing through `arr_reserve`, so the opening has to be by name
//! rather than "no call at all".
//!
//! # What the length may do
//!
//! The fact claims to cover every in-bounds slot, so a length that
//! advances past slots nobody wrote would break it. `__torajs_arr_alloc`
//! sets `len = 0` whatever capacity it is handed and `__torajs_arr_reserve`
//! does not touch it, so length only moves through a `Store` at
//! `ARR_LEN_OFF`. Every lowering that emits one
//! (`grep -rn 'InstKind::Store(.*ARR_LEN_OFF' crates/torajs-core/src`:
//! array literal, `Array.of`, `Object.keys` / `.values` / `.entries`,
//! the substr→str materializer, and the pre-reserved push loop's
//! writeback) stores a count of slots it has just filled. Any other
//! route to the length goes through a runtime call, and an
//! unrecognized call takes the array out of the analysis.

use std::collections::{HashMap, HashSet};

use torajs_core::ssa::{FuncId, Function, InstKind, Operand, Terminator, ValueId};

/// Array header offsets. Mirrors `torajs_core::ssa_lower`'s
/// `ARR_LEN_OFF` / `ARR_PROPS_OFF` / `ARR_DATA_PTR_OFF` (private to
/// that crate) and, through them, `torajs_arr::layout`.
const ARR_LEN_OFF: u64 = 8;
const ARR_CAP_HEAD_OFF: u64 = 16;
const ARR_DATA_PTR_OFF: u64 = 32;

/// Defines a fresh array whose elements are all still unwritten.
const ALLOC_CALLS: &[&str] = &["__torajs_arr_alloc", "__torajs_arr_alloc_pooled"];
/// Returns the same array (possibly reallocated, existing elements
/// carried over) and writes no element.
const IDENTITY_CALLS: &[&str] = &["__torajs_arr_reserve"];
/// Returns the same array AND writes argument 1 into an element.
const IDENTITY_PUSH_CALLS: &[&str] = &["__torajs_arr_push"];
/// Writes argument 1 into an element; returns nothing.
const VOID_PUSH_CALLS: &[&str] = &["__torajs_arr_push_unchecked"];
/// Touches the header or frees the block; no element value effect.
const VOID_INERT_CALLS: &[&str] = &[
    "__torajs_arr_mark_kind",
    "__torajs_arr_drop",
    "__torajs_arr_drop_scalar",
    "__torajs_arr_drop_any",
    "__torajs_arr_drop_heap",
    "__torajs_arr_drop_str_elems",
];

/// The element points of one function.
pub(super) struct ElemPoints {
    /// Class representative → the operands written into its elements,
    /// in a deterministic order (the fixpoint joins them every round,
    /// and a `HashMap` iteration would leak its order into the
    /// artifact).
    pub(super) writes: Vec<(ValueId, Vec<Operand>)>,
    /// Element-load result → its class representative.
    pub(super) reads: HashMap<ValueId, ValueId>,
}

fn name_of(names: &[String], f: FuncId) -> &str {
    names.get(f.0 as usize).map(String::as_str).unwrap_or("")
}

fn val(op: &Operand) -> Option<ValueId> {
    match op {
        Operand::Value(v) => Some(*v),
        _ => None,
    }
}

/// Track every allocation in `func` whose every use is admitted.
/// `callee_names` is index-aligned with `Module::funcs`; an empty
/// slice means no call can be recognized, so nothing is tracked.
pub(super) fn collect(func: &Function, callee_names: &[String]) -> ElemPoints {
    let empty = ElemPoints {
        writes: Vec::new(),
        reads: HashMap::new(),
    };
    if callee_names.is_empty() {
        return empty;
    }

    let mut defs: HashMap<ValueId, Vec<InstKind>> = HashMap::new();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                defs.entry(r).or_default().push(inst.kind.clone());
            }
        }
    }

    // Roots: a single definition, and it is an allocation.
    let mut roots: Vec<ValueId> = defs
        .iter()
        .filter(|(_, ds)| ds.len() == 1)
        .filter(|(_, ds)| match &ds[0] {
            InstKind::Call(f, _) => ALLOC_CALLS.contains(&name_of(callee_names, *f)),
            _ => false,
        })
        .map(|(v, _)| *v)
        .collect();
    if roots.is_empty() {
        return empty;
    }
    roots.sort_by_key(|v| v.0);

    let (class, base) = classify_values(&defs, &roots, callee_names);
    let mut live: HashSet<ValueId> = roots.iter().copied().collect();
    let mut writes: HashMap<ValueId, Vec<Operand>> = HashMap::new();
    let mut reads: HashMap<ValueId, ValueId> = HashMap::new();

    for block in &func.blocks {
        for inst in &block.insts {
            scan_inst(
                inst.result,
                &inst.kind,
                &class,
                &base,
                callee_names,
                &mut live,
                &mut writes,
                &mut reads,
            );
        }
        // A value that leaves the function is out of reach.
        let escaping = match &block.term {
            Terminator::Ret(Some(op)) => val(op),
            Terminator::CondBr { cond, .. } => val(cond),
            _ => None,
        };
        if let Some(v) = escaping {
            for m in [class.get(&v), base.get(&v)].into_iter().flatten() {
                live.remove(m);
            }
        }
    }

    let ordered: Vec<(ValueId, Vec<Operand>)> = roots
        .iter()
        .filter(|r| live.contains(r))
        .filter_map(|r| writes.remove(r).map(|w| (*r, w)))
        .collect();
    reads.retain(|_, r| live.contains(r));
    ElemPoints {
        writes: ordered,
        reads,
    }
}

/// Optimistic membership: which values are aliases of a root, and
/// which are data pointers loaded out of one. Both are confirmed by
/// [`scan_inst`], which kills a class whose alias-producing use did
/// not land back in the same class.
fn classify_values(
    defs: &HashMap<ValueId, Vec<InstKind>>,
    roots: &[ValueId],
    names: &[String],
) -> (HashMap<ValueId, ValueId>, HashMap<ValueId, ValueId>) {
    let mut class: HashMap<ValueId, ValueId> = roots.iter().map(|r| (*r, *r)).collect();
    let mut base: HashMap<ValueId, ValueId> = HashMap::new();
    let mut ordered: Vec<ValueId> = defs.keys().copied().collect();
    ordered.sort_by_key(|v| v.0);
    let mut moving = true;
    while moving {
        moving = false;
        for v in &ordered {
            if class.contains_key(v) || base.contains_key(v) {
                continue;
            }
            // Every definition has to name the same class in the same
            // role, or the value may also hold an array this analysis
            // never saw.
            let mut agreed: Option<(ValueId, bool)> = None;
            let mut ok = true;
            for d in &defs[v] {
                let seen = match alias_source(d, names) {
                    // A copy of a data pointer stays a data pointer;
                    // a copy of the array stays the array.
                    Some((src, AliasKind::Same)) => class
                        .get(&src)
                        .map(|r| (*r, false))
                        .or_else(|| base.get(&src).map(|r| (*r, true))),
                    Some((src, AliasKind::DataPtr)) => class.get(&src).map(|r| (*r, true)),
                    None => None,
                };
                match (seen, agreed) {
                    (Some(x), None) => agreed = Some(x),
                    (Some(x), Some(y)) if x == y => {}
                    _ => ok = false,
                }
                if !ok {
                    break;
                }
            }
            let Some((rep, is_base)) = agreed.filter(|_| ok) else {
                continue;
            };
            if is_base { &mut base } else { &mut class }.insert(*v, rep);
            moving = true;
        }
    }
    (class, base)
}

enum AliasKind {
    /// Same thing as the source operand.
    Same,
    /// The data pointer of the source array.
    DataPtr,
}

fn alias_source(kind: &InstKind, names: &[String]) -> Option<(ValueId, AliasKind)> {
    match kind {
        InstKind::Copy(_, op) | InstKind::Identity(op) => val(op).map(|v| (v, AliasKind::Same)),
        InstKind::Load(_, op, ARR_DATA_PTR_OFF) => val(op).map(|v| (v, AliasKind::DataPtr)),
        InstKind::Call(f, args) => {
            let n = name_of(names, *f);
            if IDENTITY_CALLS.contains(&n) || IDENTITY_PUSH_CALLS.contains(&n) {
                args.first().and_then(val).map(|v| (v, AliasKind::Same))
            } else {
                None
            }
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_inst(
    result: Option<ValueId>,
    kind: &InstKind,
    class: &HashMap<ValueId, ValueId>,
    base: &HashMap<ValueId, ValueId>,
    names: &[String],
    live: &mut HashSet<ValueId>,
    writes: &mut HashMap<ValueId, Vec<Operand>>,
    reads: &mut HashMap<ValueId, ValueId>,
) {
    // Operands this instruction is allowed to name; everything else it
    // mentions is an escape.
    let mut consumed: Vec<ValueId> = Vec::new();
    let mut kill: Vec<ValueId> = Vec::new();
    let lands_in = |rep: ValueId, m: &HashMap<ValueId, ValueId>| {
        result.and_then(|r| m.get(&r).copied()) == Some(rep)
    };

    match kind {
        InstKind::Call(f, args) => {
            let n = name_of(names, *f);
            if let Some(recv) = args.first().and_then(val)
                && let Some(&rep) = class.get(&recv)
            {
                consumed.push(recv);
                if IDENTITY_CALLS.contains(&n) {
                    if !lands_in(rep, class) {
                        kill.push(rep);
                    }
                } else if IDENTITY_PUSH_CALLS.contains(&n) {
                    if lands_in(rep, class) {
                        record_write(writes, rep, args.get(1));
                    } else {
                        kill.push(rep);
                    }
                } else if VOID_PUSH_CALLS.contains(&n) {
                    record_write(writes, rep, args.get(1));
                } else if !VOID_INERT_CALLS.contains(&n) {
                    kill.push(rep);
                }
            }
        }
        InstKind::Load(_, ptr, off) => {
            if let Some(p) = val(ptr) {
                if let Some(&rep) = class.get(&p) {
                    consumed.push(p);
                    match *off {
                        ARR_LEN_OFF | ARR_CAP_HEAD_OFF => {}
                        ARR_DATA_PTR_OFF if lands_in(rep, base) => {}
                        _ => kill.push(rep),
                    }
                } else if let Some(&rep) = base.get(&p) {
                    consumed.push(p);
                    if let Some(r) = result {
                        reads.insert(r, rep);
                    }
                }
            }
        }
        InstKind::Store(value, ptr, off) => {
            if let Some(p) = val(ptr) {
                if let Some(&rep) = class.get(&p) {
                    consumed.push(p);
                    if !matches!(*off, ARR_LEN_OFF | ARR_CAP_HEAD_OFF) {
                        kill.push(rep);
                    }
                } else if let Some(&rep) = base.get(&p) {
                    consumed.push(p);
                    record_write(writes, rep, Some(value));
                }
            }
        }
        InstKind::LoadDyn(_, b, _) | InstKind::LoadDynScaled8(_, b, _) => {
            if let Some(p) = val(b)
                && let Some(&rep) = base.get(&p)
            {
                consumed.push(p);
                if let Some(r) = result {
                    reads.insert(r, rep);
                }
            }
        }
        InstKind::StoreDyn(value, b, _) | InstKind::StoreDynScaled8(value, b, _) => {
            if let Some(p) = val(b)
                && let Some(&rep) = base.get(&p)
            {
                consumed.push(p);
                record_write(writes, rep, Some(value));
            }
        }
        // Comparing the pointer reads neither an element nor the
        // pointer's target, and yields a bool.
        InstKind::ICmp(_, a, b) => {
            consumed.extend(val(a));
            consumed.extend(val(b));
        }
        InstKind::Copy(_, op) | InstKind::Identity(op) => {
            if let Some(s) = val(op) {
                if let Some(&rep) = class.get(&s) {
                    consumed.push(s);
                    if !lands_in(rep, class) {
                        kill.push(rep);
                    }
                } else if let Some(&rep) = base.get(&s) {
                    consumed.push(s);
                    if !lands_in(rep, base) {
                        kill.push(rep);
                    }
                }
            }
        }
        _ => {}
    }

    for op in operands(kind) {
        if let Some(v) = val(&op)
            && !consumed.contains(&v)
        {
            kill.extend(class.get(&v).copied());
            kill.extend(base.get(&v).copied());
        }
    }
    for rep in kill {
        live.remove(&rep);
    }
}

fn record_write(writes: &mut HashMap<ValueId, Vec<Operand>>, rep: ValueId, op: Option<&Operand>) {
    match op {
        Some(o) => writes.entry(rep).or_default().push(o.clone()),
        // A write whose value is not spelled out is a value nothing
        // can bound.
        None => {
            writes.entry(rep).or_default().push(Operand::ConstPtrNull);
        }
    }
}

/// Every operand an instruction mentions.
fn operands(kind: &InstKind) -> Vec<Operand> {
    match kind {
        InstKind::BinOp(_, a, b)
        | InstKind::ICmp(_, a, b)
        | InstKind::FCmp(_, a, b)
        | InstKind::LoadDyn(_, a, b)
        | InstKind::LoadDynScaled8(_, a, b)
        | InstKind::LoadU8Dyn(a, b) => vec![a.clone(), b.clone()],
        InstKind::Call(_, args) => args.clone(),
        InstKind::CallIndirect(_, callee, args) => {
            let mut v = vec![callee.clone()];
            v.extend(args.iter().cloned());
            v
        }
        InstKind::Load(_, a, _) => vec![a.clone()],
        InstKind::Store(a, b, _) => vec![a.clone(), b.clone()],
        InstKind::StoreDyn(a, b, c)
        | InstKind::StoreDynScaled8(a, b, c)
        | InstKind::CtpopRangeSum(a, b, c) => vec![a.clone(), b.clone(), c.clone()],
        InstKind::Select(_, a, b, c) => vec![a.clone(), b.clone(), c.clone()],
        InstKind::SiToFp(a)
        | InstKind::FpToSi(a)
        | InstKind::ZExtBoolToI64(a)
        | InstKind::ZExtI32ToI64(a)
        | InstKind::BitCastF64ToI64(a)
        | InstKind::BitCastI64ToF64(a)
        | InstKind::IntToPtr(a)
        | InstKind::PtrToInt(a)
        | InstKind::TruncI64ToBool(a)
        | InstKind::Identity(a)
        | InstKind::Neg(a)
        | InstKind::Ctpop(a)
        | InstKind::Copy(_, a) => vec![a.clone()],
        InstKind::Alloca(_)
        | InstKind::AllocaBytes(_)
        | InstKind::StringRef(_)
        | InstKind::StaticStrRef(_)
        | InstKind::GlobalRef(_)
        | InstKind::FnAddr(_)
        | InstKind::BoxedEntryAddr(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{NumFact, analyze_function_with};
    use super::*;
    use torajs_core::ssa::{Block, BlockId, Inst, Type, ValueInfo};

    const ALLOC: u32 = 0;
    const RESERVE: u32 = 1;
    const PUSH: u32 = 2;
    const OTHER: u32 = 3;

    fn names() -> Vec<String> {
        vec![
            "__torajs_arr_alloc".into(),
            "__torajs_arr_reserve".into(),
            "__torajs_arr_push".into(),
            "user_fn".into(),
        ]
    }

    fn inst(result: u32, kind: InstKind) -> Inst {
        Inst {
            result: Some(ValueId(result)),
            kind,
            origin: None,
        }
    }

    fn void(kind: InstKind) -> Inst {
        Inst {
            result: None,
            kind,
            origin: None,
        }
    }

    fn v(id: u32) -> Operand {
        Operand::Value(ValueId(id))
    }

    /// One straight-line block: alloc, take the data pointer, store
    /// `first` and `second` into elements, read one back at %5. Extra
    /// instructions are appended before the read.
    fn arr_fn(first: i64, second: i64, extra: Vec<Inst>) -> Function {
        let mut insts = vec![
            inst(0, InstKind::Call(FuncId(ALLOC), vec![Operand::ConstI64(0)])),
            inst(1, InstKind::Load(Type::Ptr, v(0), ARR_DATA_PTR_OFF)),
            void(InstKind::StoreDyn(
                Operand::ConstI64(first),
                v(1),
                Operand::ConstI64(0),
            )),
            void(InstKind::StoreDyn(
                Operand::ConstI64(second),
                v(1),
                Operand::ConstI64(8),
            )),
        ];
        insts.extend(extra);
        insts.push(inst(
            5,
            InstKind::LoadDyn(Type::I64, v(1), Operand::ConstI64(0)),
        ));
        Function {
            name: "f".into(),
            params: vec![],
            ret: Type::Void,
            blocks: vec![Block {
                id: BlockId(0),
                insts,
                term: Terminator::Ret(None),
            }],
            values: (0..8)
                .map(|i| ValueInfo {
                    ty: if i == 1 { Type::Ptr } else { Type::I64 },
                    name: None,
                })
                .collect(),
            current_origin: None,
        }
    }

    #[test]
    fn element_read_joins_every_write() {
        let f = arr_fn(5, 200, vec![]);
        let facts = analyze_function_with(&f, &names());
        assert_eq!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }

    #[test]
    fn reserve_and_push_stay_in_the_class() {
        let mut f = arr_fn(5, 200, vec![]);
        f.blocks[0].insts.insert(
            1,
            inst(
                6,
                InstKind::Call(FuncId(RESERVE), vec![v(0), Operand::ConstI64(4)]),
            ),
        );
        f.blocks[0].insts.insert(
            2,
            inst(
                7,
                InstKind::Call(FuncId(PUSH), vec![v(6), Operand::ConstI64(300)]),
            ),
        );
        // the data pointer now comes off the pushed-through alias
        f.blocks[0].insts[3] = inst(1, InstKind::Load(Type::Ptr, v(7), ARR_DATA_PTR_OFF));
        let facts = analyze_function_with(&f, &names());
        assert_eq!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 300)));
    }

    #[test]
    fn an_unknown_call_takes_the_array_out() {
        let f = arr_fn(
            5,
            200,
            vec![void(InstKind::Call(FuncId(OTHER), vec![v(0)]))],
        );
        let facts = analyze_function_with(&f, &names());
        assert_ne!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }

    #[test]
    fn replacing_the_data_pointer_takes_the_array_out() {
        let f = arr_fn(
            5,
            200,
            vec![void(InstKind::Store(v(1), v(0), ARR_DATA_PTR_OFF))],
        );
        let facts = analyze_function_with(&f, &names());
        assert_ne!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }

    #[test]
    fn a_returned_array_takes_itself_out() {
        let mut f = arr_fn(5, 200, vec![]);
        f.blocks[0].term = Terminator::Ret(Some(v(0)));
        let facts = analyze_function_with(&f, &names());
        assert_ne!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }

    #[test]
    fn a_length_writeback_is_admitted() {
        let f = arr_fn(
            5,
            200,
            vec![void(InstKind::Store(
                Operand::ConstI64(2),
                v(0),
                ARR_LEN_OFF,
            ))],
        );
        let facts = analyze_function_with(&f, &names());
        assert_eq!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }

    #[test]
    fn without_callee_names_nothing_is_tracked() {
        let f = arr_fn(5, 200, vec![]);
        let facts = analyze_function_with(&f, &[]);
        assert_ne!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }
}
