//! Filling the `__drop_<C>` bodies (`class_drop`) once every layout is
//! final: each class sid's tag set, then one release fn per sid.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::ast::PropKey;
use crate::ssa::{self, FuncId, Module, Operand, Terminator, Type};
use crate::ssa_lower::Intrinsics;
use crate::ssa_lower_emit_drop_value::class_drop::{Cx, emit_release};
use crate::ssa_lower_emit_drop_value::class_drop_setup::ClassDropFn;
use crate::ssa_lower_emit_drop_value::class_drop_worklist as worklist;

#[allow(clippy::too_many_arguments)]
pub(crate) fn populate_class_drop_bodies(
    class_drops: &[ClassDropFn],
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    struct_layouts: &[Vec<(PropKey, Type)>],
    arr_layouts: &crate::ssa::ArrLayouts,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    intrinsics: &Intrinsics,
    module: &mut Module,
) {
    let sid_of = |cn: &String| match aliases.get(cn) {
        Some(Type::Obj(s)) => Some(s.0),
        _ => None,
    };
    let mut class_sids: HashSet<u32> = class_name_to_tag.keys().filter_map(sid_of).collect();
    // sids a factory stamped a class tag onto besides the class's own
    for sid in 0..struct_layouts.len() as u32 {
        if anon_stamp_pool
            .borrow()
            .is_class_minted(crate::ssa::StructId(sid))
        {
            class_sids.insert(sid);
        }
    }
    let drop_fids: HashMap<u32, FuncId> = class_drops.iter().map(|d| (d.sid.0, d.fid)).collect();
    let mut tags: HashMap<u32, Vec<u32>> = HashMap::new();
    for (cn, tag) in class_name_to_tag {
        if let Some(sid) = sid_of(cn) {
            tags.entry(sid).or_default().push(*tag);
        }
    }
    for (sid, tag) in anon_stamp_pool.borrow().sid_to_tag_iter() {
        if !class_sids.contains(&sid.0) {
            tags.entry(sid.0).or_insert_with(|| vec![0]).push(tag);
        }
    }
    tags.values_mut().for_each(|t| t.sort_unstable());
    for d in class_drops {
        let mut f = ssa::Function::new(&d.name, Type::Void);
        let p = f.add_param(Type::Ptr, "p");
        let prelude = f.add_block();
        let entry = f.add_block();
        let mut cx = Cx {
            layouts: struct_layouts,
            arr_layouts,
            class_sids: &class_sids,
            tags: &tags,
            drop_fids: &drop_fids,
            intr: intrinsics,
            prelude,
            cold: RefCell::new(Vec::new()),
            worklist: None,
        };
        let (sid, p) = (d.sid.0, Operand::Value(p));
        let mut stack = Vec::new();
        if worklist::holds_own_sid(&cx, sid) {
            let is_class = class_sids.contains(&sid);
            cx.worklist = Some(worklist::alloc(&mut f, prelude, sid, d.fid, p, is_class));
            let wl = cx.worklist.as_ref().expect("just set");
            worklist::emit_loop(&mut f, wl, entry, |f, head, c, pop| {
                emit_release(f, &cx, head, c, sid, &mut stack, Some(pop))
            });
        } else {
            let after = emit_release(&mut f, &cx, entry, p, sid, &mut stack, None);
            f.set_term(after, Terminator::Ret(None));
        }
        for leg in cx.cold.take() {
            leg(&mut f);
        }
        f.set_term(prelude, Terminator::Br(entry));
        module.funcs[d.fid.0 as usize] = f;
    }
}
