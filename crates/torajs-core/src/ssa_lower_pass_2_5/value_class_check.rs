//! The finished SSA's word on the value classes of pass 0.5
//! (`ssa_lower_pass_0_5::value_class_census`): in a live function, a
//! cell of one is never boxed into `any` nor taken apart into raw bits
//! (`BoxAny`, `PtrToInt`, or a call that takes it as an `any`, as a
//! cell of another layout, or as an integer other than the element
//! word of an array of its own class: the runtime boxes and prints
//! through such calls), an array of them neither, and no slot of its layout is open to a cell of
//! another ([`super::cell_slot_flow::open_sids`]). The census reads
//! syntax and this reads what lowering emitted, so the two fail in
//! different ways. `TORAJS_VALUE_CLASS_STATS=1` prints each
//! disagreement, then how many there were over how many live functions.

use std::collections::HashSet;

use crate::ast::Ast;
use crate::ssa::{FuncId, Function, InstKind, Module, Operand, StructId, Type};
use crate::ssa_lower::proofs::ProgramFacts;

use super::any_holds::AnyHolds;

/// A method twin the census proved no receiver enters is not live,
/// whatever references it.
pub(crate) fn report(module: &Module, ast: &Ast, facts: &ProgramFacts, open: &[bool]) {
    let value_classes = &facts.value_classes;
    if value_classes.is_empty() || std::env::var_os("TORAJS_VALUE_CLASS_STATS").is_none() {
        return;
    }
    let dead_twin =
        |name: &str| facts.twins_unreachable && ast.cmany_twins.values().any(|t| t == name);
    let mut sids: Vec<&StructId> = value_classes.iter().collect();
    sids.sort_by_key(|s| s.0);
    let mut found = 0;
    for sid in sids {
        if open[sid.0 as usize] {
            found += 1;
            eprintln!(
                "[value_class] census and SSA disagree: sid {} / open slot",
                sid.0
            );
        }
    }
    let holds = AnyHolds::of(module);
    let mut live = 0;
    for (i, f) in module.funcs.iter().enumerate() {
        if holds.is_dead(FuncId(i as u32)) || dead_twin(&f.name) {
            continue;
        }
        live += 1;
        for inst in f.blocks.iter().flat_map(|b| &b.insts) {
            for sid in escapes(module, f, &inst.kind, value_classes) {
                found += 1;
                eprintln!(
                    "[value_class] census and SSA disagree: sid {} / {}",
                    sid.0, f.name
                );
            }
        }
    }
    eprintln!("[value_class] SSA check: {found} disagreements over {live} live functions");
}

/// The value-class sids whose cells, or arrays of them, this
/// instruction hands to `any`, to raw bits or to a slot of another
/// layout.
fn escapes(
    module: &Module,
    f: &Function,
    kind: &InstKind,
    value_classes: &HashSet<StructId>,
) -> Vec<StructId> {
    let class_of = |t: Type| match t {
        Type::Obj(sid) => Some(sid),
        Type::Arr(a) => match module.arr_layouts[a.0 as usize] {
            Type::Obj(sid) => Some(sid),
            _ => None,
        },
        _ => None,
    };
    let held = |o: &Operand| match o {
        Operand::Value(v) => {
            let t = f.value_type(*v);
            class_of(t)
                .filter(|s| value_classes.contains(s))
                .map(|s| (s, t))
        }
        _ => None,
    };
    // an integer parameter takes the cell as an element word when
    // another argument is an array of its own class (a push)
    let elem_home = |own: Type, args: &[Operand]| {
        matches!(own, Type::Obj(_))
            && args.iter().any(|o| {
                matches!(o, Operand::Value(v) if matches!(f.value_type(*v),
                    Type::Arr(a) if module.arr_layouts[a.0 as usize] == own))
            })
    };
    let foreign = |own: Type, param: Option<Type>, args: &[Operand]| match param {
        Some(p) if p == own => false,
        Some(Type::Obj(_) | Type::Arr(_) | Type::Any) | None => true,
        Some(Type::I64) => !elem_home(own, args),
        Some(_) => false,
    };
    let args = |params: &[Type], args: &[Operand]| -> Vec<StructId> {
        (args.iter().enumerate())
            .filter_map(|(i, a)| {
                let (sid, own) = held(a)?;
                foreign(own, params.get(i).copied(), args).then_some(sid)
            })
            .collect()
    };
    match kind {
        InstKind::BoxAny(_, v) | InstKind::PtrToInt(v) => {
            held(v).map(|(s, _)| s).into_iter().collect()
        }
        InstKind::Call(fid, a) => {
            let callee = &module.funcs[fid.0 as usize];
            let params: Vec<Type> = callee
                .params
                .iter()
                .map(|p| callee.value_type(*p))
                .collect();
            args(&params, a)
        }
        InstKind::CallIndirect(sig, _, a) => {
            let params = module
                .signatures
                .get(sig.0 as usize)
                .map(|s| s.0.as_slice());
            args(params.unwrap_or_default(), a)
        }
        _ => Vec::new(),
    }
}
