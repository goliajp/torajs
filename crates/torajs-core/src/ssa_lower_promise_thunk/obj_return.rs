//! The link evidence that some settle may be handed an object.
//!
//! A handler's or an async body's return settles its promise through
//! torajs-promise `settle_handler_return`, which probes an object
//! return for a `then` (§27.2.1.3.2 steps 8-9). The probe is the
//! generic member read, so the link stubs it unless something can hand
//! the settle an object: a handler twin (the runtime's own entry), or
//! a call site whose static return form is an object or `any`. The
//! kernels carry that form as a constant, so this module reads it off
//! the lowered calls and, where one names such a form, emits a rooted
//! fn whose call keeps `__torajs_promise_obj_return_mark` live. Nothing
//! calls the root; its `___torajs_` name roots it in the user-fn strip.

use std::collections::HashMap;

use crate::ssa::{self, FuncId, InstKind, Module, Operand, Terminator, Type};
use crate::ssa_lower_promise_repr_mark::{REPR_ANY, REPR_HEAP};

const MARK_SYM: &str = "__torajs_promise_obj_return_mark";
const ROOT_NAME: &str = "___torajs_promise_obj_return_root";

/// Each kernel that settles through the handler-return path, with the
/// argument positions of its return-form words (low byte = repr code).
const KERNELS: [(&str, &[usize]); 6] = [
    ("__torajs_promise_then_simple", &[2]),
    ("__torajs_promise_then_closure", &[2]),
    ("__torajs_promise_catch_simple", &[2]),
    ("__torajs_promise_catch_closure", &[2]),
    ("__torajs_promise_then2", &[2, 4]),
    ("__torajs_frame_return", &[1]),
];

/// Emit the root when a lowered kernel call names an object or `any`
/// return form, or one the lowering did not fold to a constant, or a
/// suspending body returns one (its `__torajs_frame_return` calls are
/// placed after lowering, by the suspend split, off `ret_repr`).
pub(crate) fn root_obj_return_mark(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, ssa::SigId>,
) {
    if !names_object_return(module) {
        return;
    }
    let mark = crate::ssa_lower_synthesize_main::declare_intrinsic(
        module,
        fn_table,
        MARK_SYM,
        &[],
        Type::Void,
    );
    let fid = FuncId(module.funcs.len() as u32);
    let sig = crate::ssa_lower::intern_fn_sig(fn_sigs, Vec::new(), Type::Void);
    fn_table.insert(ROOT_NAME.to_string(), fid);
    fn_sig_ids.insert(fid, sig);
    let mut f = ssa::Function::new(ROOT_NAME, Type::Void);
    let entry = f.add_block();
    f.append_void(entry, InstKind::Call(mark, Vec::new()));
    f.set_term(entry, Terminator::Ret(None));
    module.funcs.push(f);
}

fn names_object_return(module: &Module) -> bool {
    // by the callee's name, whichever declaration a call names
    let words = |fid: &FuncId| {
        let name = module.funcs.get(fid.0 as usize)?.name.as_str();
        KERNELS.iter().find(|(k, _)| *k == name).map(|(_, w)| *w)
    };
    let object_repr = |r: i64| matches!(r, REPR_HEAP | REPR_ANY);
    if module.suspend_fns.iter().any(|s| object_repr(s.ret_repr)) {
        return true;
    }
    let object_form = |op: Option<&Operand>| match op {
        Some(Operand::ConstI64(word)) => object_repr(word & 0xff),
        _ => true,
    };
    (module.funcs.iter())
        .flat_map(|f| f.blocks.iter().flat_map(|b| &b.insts))
        .any(|inst| {
            let InstKind::Call(fid, args) = &inst.kind else {
                return false;
            };
            words(fid).is_some_and(|w| w.iter().any(|&i| object_form(args.get(i))))
        })
}
