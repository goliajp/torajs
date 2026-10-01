//! Per-class release fns `__drop_<C>` — pre-registration and lookup.
//!
//! A class instance whose layout holds an `Obj` field releases through
//! one out-of-line body per sid instead of the inline expansion
//! `ssa_lower_emit_drop_value::obj` emits at every site: the self-
//! referential field no longer falls back to the runtime's layout-table
//! walk (`__torajs_obj_drop_rc`, ~145 instructions per child), and a
//! site is one call. The body is `class_drop`'s.
//!
//! The unit is the sid, not the class name: structurally identical
//! classes intern onto one sid (`ssa_lower_pass_0_5::intern_or_finalize`)
//! while runtime tags stay per name, so the fn is named after the first
//! class (in sort order) aliased to the sid and its body answers every
//! tag aliased there. The FuncId is registered here, before the fn list
//! freezes at the signatures snapshot; the body is filled in after the
//! body passes, when every layout is final. The fn is found by sid
//! through `LowerCtx::class_drop_fids` and listed in
//! `Module::class_drop_fns`, never by its name — a user fn may spell
//! the same one.

use std::collections::{HashMap, HashSet};

use crate::ast::{Ast, PropKey};
use crate::ssa::{self, FuncId, Module, StructId, Type};
use crate::ssa_lower::{LowerCtx, intern_fn_sig};

pub(crate) struct ClassDropFn {
    pub sid: StructId,
    pub fid: FuncId,
    pub name: String,
}

/// The pre-registered `__drop_` fn for `sid`, if the sid has one.
pub(crate) fn class_drop_fid(ctx: &LowerCtx<'_>, sid: StructId) -> Option<FuncId> {
    ctx.class_drop_fids.get(&sid.0).copied()
}

/// Reserve a `__drop_<C>` FuncId for every class sid whose layout holds
/// an `Obj` field. A generic class has no alias sid and its
/// specializations are not class names, so neither takes part.
pub(crate) fn run(
    ast: &Ast,
    module: &mut Module,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
    aliases: &HashMap<String, Type>,
    struct_layouts: &[Vec<(PropKey, Type)>],
) -> Vec<ClassDropFn> {
    let mut class_names: Vec<&String> = ast.class_parents.keys().collect();
    class_names.sort();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut out = Vec::new();
    for cname in class_names {
        let Some(Type::Obj(sid)) = aliases.get(cname) else {
            continue;
        };
        if !seen.insert(sid.0) {
            continue;
        }
        let layout = &struct_layouts[sid.0 as usize];
        if !layout.iter().any(|(_, fty)| matches!(fty, Type::Obj(_))) {
            continue;
        }
        let name = format!("__drop_{cname}");
        let fid = FuncId(module.funcs.len() as u32);
        let sig = intern_fn_sig(fn_sigs, vec![Type::Ptr], Type::Void);
        fn_sig_ids.insert(fid, sig);
        module.funcs.push(ssa::Function::new(&name, Type::Void));
        module.class_drop_fns.push(fid);
        out.push(ClassDropFn {
            sid: *sid,
            fid,
            name,
        });
    }
    out
}
