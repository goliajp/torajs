//! Which functions can move an array's length or data pointer while
//! they run: grow it, shrink it, shift it, or hand it to code that
//! could.
//!
//! The question is asked the way `interval/elem.rs` in the e-graph asks
//! it: rather than list the instructions that change an array, list
//! the few that are known not to, and count every other one as a
//! change. A function changes arrays when it
//!
//! - stores into an array cell's header (a typed base, or an untyped
//!   one at a header word's offset): the length, the capacity and
//!   head word, the data pointer;
//! - hands an array out of the typed lanes, after which it can be
//!   reached under another layout or as bare bits: boxed into `any`,
//!   taken apart into bits, retyped by a copy or a select, passed where
//!   the callee takes something else, returned where the function
//!   returns something else, stored at a constant offset through an
//!   untyped pointer (a store at a dynamic offset only writes the
//!   elements of a typed array, which read back as arrays), or
//!   parked in a stack slot that is read back as anything but an array
//!   or whose address goes anywhere but a load or a store;
//! - calls a runtime function not in [`READ_ONLY`] (one that can run
//!   user code, such as a sort comparator or a `constructor` getter
//!   the species lookup reaches, is not in it), calls through a
//!   pointer, or suspends.
//!
//! Whatever the layout: an array of one layout can be held by a slot of
//! another, so a change to any array counts as a change to all of them.
//! A store into a field of a typed cell keeps the array in the lanes:
//! the field's layout types every read of it.

use crate::ssa::id_hash::HashSet;
use crate::ssa::{
    Function, InstKind, Module, Operand, Terminator, Type, ValueId, visit_value_operands,
};
use crate::ssa_lower::{ARR_CAP_HEAD_OFF, ARR_DATA_PTR_OFF, ARR_LEN_OFF};

/// The packed `cap | head` word's upper half.
const ARR_HEAD_OFF: u64 = ARR_CAP_HEAD_OFF + 4;

/// Runtime functions that leave every live array's length and data
/// pointer as they are and run no user code: they test and throw,
/// allocate and free cells, mark an array's element kind in its flags,
/// release a reference (which frees only what died, never an array a
/// live field holds), or read the header (`load_licm`'s list). A
/// release fn's own calls are among them: the expando bag's release,
/// the weak observers' notice (a finalization callback is queued, not
/// run), the cycle buffer (whose collection frees only garbage and
/// marks colours in the header), the untyped release. So are the
/// pieces of a throw: building its message, boxing it, recording it.
const READ_ONLY: &[&str] = &[
    "__torajs_heap_nullish_check",
    "__torajs_arr_null_check",
    "__torajs_arr_oob_throw",
    "__torajs_arr_mark_kind",
    "__torajs_obj_alloc",
    "__torajs_obj_drop_sized",
    "__torajs_obj_free_cell",
    "__torajs_drop_release",
    "__torajs_arr_delete_tail_guard",
    "__torajs_obj_props_drop_slow",
    "__torajs_obj_buffer_slow",
    "__torajs_obj_drop_rc",
    "__torajs_cycle_buffer",
    "__torajs_weakref_target_dying",
    "__torajs_i64_to_str",
    "__torajs_str_concat",
    "__torajs_str_drop",
    "__torajs_anyv_box_str_slot",
    "__torajs_throw_set",
];
/// The array releases.
const DROP_PREFIX: &str = "__torajs_arr_drop";

/// The first instruction of `f` that can change an array, by name
/// (a runtime call's with its callee).
pub(super) fn site(module: &Module, f: &Function) -> Option<String> {
    let slots = Slots::of(f);
    let site = f.blocks.iter().flat_map(|b| &b.insts).find_map(|inst| {
        let why = inst_site(module, f, &slots, &inst.kind, inst.result);
        match &inst.kind {
            InstKind::Call(t, _) if why == RUNTIME_CALL => {
                Some(format!("{why} to {}", module.funcs[t.0 as usize].name))
            }
            _ => (!why.is_empty()).then(|| why.to_string()),
        }
    });
    site.or_else(|| {
        f.blocks.iter().find_map(|b| match &b.term {
            Terminator::Ret(Some(v)) if is_arr(f, v) && !matches!(f.ret, Type::Arr(_)) => {
                Some("an array returned untyped".to_string())
            }
            _ => None,
        })
    })
}

const RUNTIME_CALL: &str = "a runtime call";

fn is_arr(f: &Function, op: &Operand) -> bool {
    matches!(op, Operand::Value(v) if matches!(f.value_type(*v), Type::Arr(_)))
}

/// Why `kind` can change an array, or `""`.
fn inst_site(
    module: &Module,
    f: &Function,
    slots: &Slots,
    kind: &InstKind,
    result: Option<ValueId>,
) -> &'static str {
    let arr = |op: &Operand| is_arr(f, op);
    let ptr = |op: &Operand| matches!(op, Operand::Value(v) if f.value_type(*v) == Type::Ptr);
    let retyped = || result.is_some_and(|r| !matches!(f.value_type(r), Type::Arr(_)));
    let header = [
        ARR_LEN_OFF,
        ARR_CAP_HEAD_OFF,
        ARR_HEAD_OFF,
        ARR_DATA_PTR_OFF,
    ];
    match kind {
        InstKind::Store(_, p, _) if arr(p) => "a header store",
        InstKind::Store(_, p, off) if ptr(p) && header.contains(off) => "an untyped header store",
        InstKind::Store(v, p, _) if arr(v) => slots.parks(f, p),
        InstKind::StoreDyn(_, p, _) | InstKind::StoreDynScaled8(_, p, _) if arr(p) => {
            "a header store"
        }
        InstKind::BoxAny(_, v) | InstKind::PtrToInt(v) if arr(v) => "an array leaving the lanes",
        InstKind::Copy(_, v) | InstKind::Identity(v) if arr(v) && retyped() => "an array retyped",
        InstKind::Select(_, _, a, b) if (arr(a) || arr(b)) && retyped() => "an array retyped",
        InstKind::CallIndirect(..) => "an indirect call",
        InstKind::Suspend(..) => "a suspension",
        InstKind::Call(t, args) => {
            let callee = &module.funcs[t.0 as usize];
            let name = callee.name.as_str();
            if callee.is_declaration() {
                if READ_ONLY.contains(&name) || name.starts_with(DROP_PREFIX) {
                    ""
                } else {
                    RUNTIME_CALL
                }
            } else if (callee.params.iter().zip(args))
                .any(|(p, a)| arr(a) && !matches!(callee.value_type(*p), Type::Arr(_)))
            {
                "an array passed untyped"
            } else {
                ""
            }
        }
        _ => "",
    }
}

/// The stack slots of a function, and which of them hold an array in
/// the lanes: read back only as an array, their address used by
/// nothing but a load or a store.
struct Slots {
    all: HashSet<ValueId>,
    typed: HashSet<ValueId>,
}

impl Slots {
    fn of(f: &Function) -> Self {
        let insts = || f.blocks.iter().flat_map(|b| &b.insts);
        let all: HashSet<ValueId> = (insts())
            .filter(|i| matches!(i.kind, InstKind::Alloca(_) | InstKind::AllocaBytes(_)))
            .filter_map(|i| i.result)
            .collect();
        let mut out: HashSet<ValueId> = HashSet::default();
        let mut leave = |v: ValueId| {
            if all.contains(&v) {
                out.insert(v);
            }
        };
        for inst in insts() {
            match &inst.kind {
                InstKind::Store(v, Operand::Value(_), 0) => {
                    if let Operand::Value(v) = v {
                        leave(*v);
                    }
                }
                InstKind::Load(Type::Arr(_), Operand::Value(_), 0) => {}
                InstKind::Load(_, Operand::Value(s), _) => leave(*s),
                kind => visit_value_operands(kind, &mut leave),
            }
        }
        let typed = all.iter().filter(|s| !out.contains(s)).copied().collect();
        Slots { all, typed }
    }

    /// Why storing an array through `p`, a pointer of `f`, takes it out
    /// of the lanes, or `""`.
    fn parks(&self, f: &Function, p: &Operand) -> &'static str {
        match p {
            Operand::Value(s) if self.all.contains(s) => {
                if self.typed.contains(s) {
                    ""
                } else {
                    "an array parked untyped"
                }
            }
            Operand::Value(s) if f.value_type(*s) == Type::Ptr => "an array stored untyped",
            _ => "",
        }
    }
}
