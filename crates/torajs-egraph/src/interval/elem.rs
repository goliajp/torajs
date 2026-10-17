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

use crate::id_hash::{HashMap, HashSet};

use torajs_core::ssa::{
    BinOp, FuncId, Function, InstKind, Operand, Terminator, ValueId, visit_value_operands,
};

use super::elem_alias::classify_values;
use super::elem_header;
use super::elem_kernels::*;

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

pub(super) fn name_of(names: &[String], f: FuncId) -> &str {
    names.get(f.0 as usize).map(String::as_str).unwrap_or("")
}

pub(super) fn val(op: &Operand) -> Option<ValueId> {
    match op {
        Operand::Value(v) => Some(*v),
        _ => None,
    }
}

/// Track every allocation in `func` whose every use is admitted.
/// `callee_names` is index-aligned with `Module::funcs`; an empty
/// slice means no call can be recognized, so nothing is tracked.
pub(super) fn collect(func: &Function, callee_names: &[String]) -> ElemPoints {
    let Some(mut a) = analyze(func, callee_names, false) else {
        return ElemPoints {
            writes: Vec::new(),
            reads: HashMap::default(),
        };
    };
    let live = &a.classes.live;
    let ordered: Vec<(ValueId, Vec<Operand>)> = a
        .roots
        .iter()
        .filter(|r| live.contains(r))
        .filter_map(|r| a.writes.remove(r).map(|w| (*r, w)))
        .collect();
    a.reads.retain(|_, r| live.contains(r));
    ElemPoints {
        writes: ordered,
        reads: a.reads,
    }
}

/// Which values name each tracked array (`class`: the array itself,
/// `base`: its data pointer, both keyed to the allocation root) and
/// which roots stay fully tracked once every use has been admitted
/// (`live`). A live root's address never leaves the function's SSA:
/// nothing but the admitted instructions can read or write it.
pub(crate) struct ArrayClasses {
    pub(crate) class: HashMap<ValueId, ValueId>,
    pub(crate) base: HashMap<ValueId, ValueId>,
    pub(crate) live: HashSet<ValueId>,
}

/// The escape discipline of [`collect`] on its own, for the header
/// passes (`load_licm`, `len_promote`), which also track frame-cell
/// array literals and admit the typed store's header kernels
/// (`elem_header`).
pub(crate) fn array_classes(func: &Function, callee_names: &[String]) -> Option<ArrayClasses> {
    analyze(func, callee_names, true).map(|a| a.classes)
}

pub(super) struct Analysis {
    roots: Vec<ValueId>,
    pub(super) classes: ArrayClasses,
    writes: HashMap<ValueId, Vec<Operand>>,
    reads: HashMap<ValueId, ValueId>,
}

/// `header`: track for the header passes only — see [`array_classes`].
pub(super) fn analyze(func: &Function, callee_names: &[String], header: bool) -> Option<Analysis> {
    if callee_names.is_empty() {
        return None;
    }
    // with no allocation call there is no root but a frame cell
    let allocates = || {
        func.blocks.iter().flat_map(|b| &b.insts).any(|i| {
            matches!(&i.kind, InstKind::Call(f, _) if ALLOC_CALLS.contains(&name_of(callee_names, *f)))
        })
    };
    if !header && !allocates() {
        return None;
    }

    let mut defs: HashMap<ValueId, Vec<&InstKind>> = HashMap::default();
    for block in &func.blocks {
        for inst in &block.insts {
            if let Some(r) = inst.result {
                defs.entry(r).or_default().push(&inst.kind);
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
    let stack = if header {
        elem_header::frame_cell_roots(func, &defs)
    } else {
        HashSet::default()
    };
    roots.extend(stack.iter().copied());
    if roots.is_empty() {
        return None;
    }
    roots.sort_by_key(|v| v.0);

    let seeds = elem_header::inline_data_ptrs(&defs, &stack);
    let (class, base) = classify_values(&defs, &roots, callee_names, &seeds);
    let mut live: HashSet<ValueId> = roots.iter().copied().collect();
    let mut writes: HashMap<ValueId, Vec<Operand>> = HashMap::default();
    let mut reads: HashMap<ValueId, ValueId> = HashMap::default();

    for block in &func.blocks {
        for inst in &block.insts {
            if header
                && elem_header::admitted(
                    inst.result,
                    &inst.kind,
                    &class,
                    &base,
                    &stack,
                    callee_names,
                )
            {
                continue;
            }
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
    Some(Analysis {
        roots,
        classes: ArrayClasses { class, base, live },
        writes,
        reads,
    })
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
    // an instruction naming no tracked value neither admits nor kills
    let mut named = false;
    visit_value_operands(kind, |v| {
        named |= class.contains_key(&v) || base.contains_key(&v);
    });
    if !named {
        return;
    }
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
                } else if HOLE_WRITE_CALLS.contains(&n) {
                    record_write(writes, rep, None);
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
                        ARR_HEADER_WORD_OFF | ARR_HEADER_FLAGS_OFF | ARR_LEN_OFF
                        | ARR_CAP_HEAD_OFF => {}
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
        InstKind::Copy(_, op) | InstKind::Identity(op) | InstKind::BinOp(BinOp::Add, op, _) => {
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

    visit_value_operands(kind, |v| {
        if !consumed.contains(&v) {
            kill.extend(class.get(&v).copied());
            kill.extend(base.get(&v).copied());
        }
    });
    for rep in kill {
        live.remove(&rep);
    }
}

fn record_write(writes: &mut HashMap<ValueId, Vec<Operand>>, rep: ValueId, op: Option<&Operand>) {
    match op {
        Some(o) => writes.entry(rep).or_default().push(*o),
        // A write whose value is not spelled out is a value nothing
        // can bound.
        None => {
            writes.entry(rep).or_default().push(Operand::ConstPtrNull);
        }
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
    const TAIL_GUARD: u32 = 4;

    fn names() -> Vec<String> {
        vec![
            "__torajs_arr_alloc".into(),
            "__torajs_arr_reserve".into(),
            "__torajs_arr_push".into(),
            "user_fn".into(),
            "__torajs_arr_delete_tail_guard".into(),
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

    // The two header-read spellings of the length / extensibility
    // guards. Rotation 623: the pre-reserve lane's frozen gate read
    // the flags half-word and lost the array here — the sum loop that
    // followed fell out of float demotion and the cell read +55%,
    // which three A/B rounds had attributed to the fill loop.
    #[test]
    fn a_header_word_read_keeps_the_array() {
        let f = arr_fn(
            5,
            200,
            vec![inst(
                6,
                InstKind::Load(Type::I64, v(0), ARR_HEADER_WORD_OFF),
            )],
        );
        let facts = analyze_function_with(&f, &names());
        assert_eq!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }

    #[test]
    fn a_header_flags_read_keeps_the_array() {
        let f = arr_fn(
            5,
            200,
            vec![inst(
                6,
                InstKind::Load(Type::I32, v(0), ARR_HEADER_FLAGS_OFF),
            )],
        );
        let facts = analyze_function_with(&f, &names());
        assert_eq!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }

    #[test]
    fn a_length_lock_raiser_keeps_the_array() {
        let f = arr_fn(
            5,
            200,
            vec![inst(6, InstKind::Call(FuncId(TAIL_GUARD), vec![v(0)]))],
        );
        let facts = analyze_function_with(&f, &names());
        assert_eq!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }

    #[test]
    fn a_write_through_an_advanced_data_pointer_joins() {
        let f = arr_fn(
            5,
            200,
            vec![
                inst(6, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(16))),
                void(InstKind::StoreDynScaled8(
                    Operand::ConstI64(300),
                    v(6),
                    Operand::ConstI64(1),
                )),
            ],
        );
        let facts = analyze_function_with(&f, &names());
        assert_eq!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 300)));
    }

    #[test]
    fn a_data_pointer_moved_back_takes_the_array_out() {
        let f = arr_fn(
            5,
            200,
            vec![
                inst(6, InstKind::BinOp(BinOp::Add, v(1), Operand::ConstI64(-8))),
                void(InstKind::StoreDyn(
                    Operand::ConstI64(300),
                    v(6),
                    Operand::ConstI64(0),
                )),
            ],
        );
        let facts = analyze_function_with(&f, &names());
        assert_ne!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
        assert_ne!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 300)));
    }

    #[test]
    fn an_advanced_array_pointer_takes_the_array_out() {
        let f = arr_fn(
            5,
            200,
            vec![
                inst(6, InstKind::BinOp(BinOp::Add, v(0), Operand::ConstI64(16))),
                void(InstKind::StoreDyn(
                    Operand::ConstI64(300),
                    v(6),
                    Operand::ConstI64(0),
                )),
            ],
        );
        let facts = analyze_function_with(&f, &names());
        assert_ne!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
        assert_ne!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 300)));
    }

    #[test]
    fn without_callee_names_nothing_is_tracked() {
        let f = arr_fn(5, 200, vec![]);
        let facts = analyze_function_with(&f, &[]);
        assert_ne!(facts.get(&ValueId(5)), Some(&NumFact::new(5, 200)));
    }
}
