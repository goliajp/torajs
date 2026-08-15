//! vtable completeness for generic classes — instantiating a class
//! instantiates every chain method it declares (the C++
//! class-template rule). The factory's mono suffix names the
//! instance's vtable row, and `populate_vtables` slot resolution
//! tries `__cm_<C>__<m>` under that SAME suffix first — an override
//! specialization no call site ever seeded would leave the slot to
//! fall back on an ancestor's impl (a silently lost override,
//! probe-proven: a generic subclass overriding a non-generic base's
//! method has a non-generic dispatcher, so no retarget names the
//! subclass impl anywhere). Split from `check_monomorph.rs` at the
//! 500-line file cap.

use std::collections::{HashMap, VecDeque};

use crate::ast::Ast;
use crate::check::Type;
use crate::check_monomorph::WorkItem;
use crate::ssa_lower_generics_monomorph::Generics;

/// Queue the chain-method specializations riding a `__new_<C>`
/// factory instantiation. Method-level EXTRA type params can't ride
/// the class-only suffix, so those stay out (their slot resolves
/// bare or misses honestly).
#[allow(clippy::too_many_arguments)]
pub(crate) fn seed_class_chain_methods(
    owned_ast: &Ast,
    generics: &Generics,
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut VecDeque<WorkItem>,
    factory_name: &str,
    factory_type_params_len: usize,
    spec_suffix: &str,
    arg_anns: &[String],
    type_args: &[Type],
) {
    let Some(cname) = factory_name.strip_prefix("__new_") else {
        return;
    };
    if !owned_ast.class_parents.contains_key(cname) {
        return;
    }
    let mut m_names: Vec<&String> = owned_ast.method_index.keys().collect();
    m_names.sort();
    for m in m_names {
        let cm = format!("__cm_{cname}__{m}");
        let Some((cm_tps, ..)) = generics.get(&cm) else {
            continue;
        };
        if cm_tps.len() != factory_type_params_len {
            continue;
        }
        let key = (cm.clone(), arg_anns.to_vec());
        if cache.contains_key(&key) {
            continue;
        }
        cache.insert(key, format!("{cm}{spec_suffix}"));
        worklist.push_back((cm, arg_anns.to_vec(), type_args.to_vec()));
    }
}
