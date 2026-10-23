//! The functions an async body suspending over a frame needs besides
//! its own (`ast::desugar_async_ssa_gate` picks the bodies): a resume
//! function, a drop and a trace per body, and the runtime calls they
//! make (`torajs-promise` `frame`).
//!
//! The FuncIds are taken before the bodies lower, like the env drops'
//! (`ssa_lower_env_drop_setup`): a body's wrapper calls its resume by
//! id, and the function list is fixed by then. Nothing is declared
//! for a program with no such body, so its module is unchanged.
//!
//! Once a body is lowered [`install`] puts it in its resume slot,
//! reading its parameters from the frame, and builds the wrapper that
//! callers reach under the body's own id (`async_wrapper`). The trace
//! reports the links and the parameter words here; the split pass
//! (torajs-egraph `suspend_split`) adds what each suspension point
//! keeps.

use std::collections::HashMap;

use crate::ast::Ast;
use crate::ssa::suspend::{FRAME_DIRECT_OFF, FRAME_PARAMS_OFF, FRAME_SIZE_PLACEHOLDER_BASE};
use crate::ssa::{
    self, BinOp, FuncId, Inst, InstKind, Module, Operand, SigId, SuspendFn, SuspendRuntime, Type,
};
use crate::ssa_lower_synthesize_main::declare_intrinsic;

/// The runtime calls of the frame code.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FrameCalls {
    pub(crate) result_alloc: FuncId,
    pub(crate) await_promise: FuncId,
    pub(crate) await_direct: FuncId,
    pub(crate) ret: FuncId,
    pub(crate) throw: FuncId,
    pub(crate) unwind: FuncId,
    /// The placeholder the body asks whether its frame is cancelled
    /// through (`suspend::cancel`); [`frame_body`] reads the word.
    pub(crate) cancelled: FuncId,
    pub(crate) release_links: FuncId,
    pub(crate) trace_links: FuncId,
    /// The trace visit callback's signature.
    pub(crate) visit_sig: SigId,
}

/// The three functions reserved for one body.
#[derive(Clone, Copy)]
struct Slots {
    resume: FuncId,
    drop: FuncId,
    trace: FuncId,
}

/// What [`preregister`] reserved.
#[derive(Default)]
pub(crate) struct SuspendSetup {
    slots: HashMap<String, Slots>,
    calls: Option<FrameCalls>,
}

pub(crate) fn preregister(
    ast: &Ast,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
) -> SuspendSetup {
    super::async_main::declare(ast, module, fn_table);
    let mut names: Vec<&String> = (ast.async_ssa.fns.iter())
        .filter(|n| fn_table.contains_key(n.as_str()))
        .collect();
    if names.is_empty() {
        return SuspendSetup::default();
    }
    names.sort();
    let mut decl = |name: &str, params: &[Type], ret: Type| {
        declare_intrinsic(module, fn_table, name, params, ret)
    };
    let p = Type::Ptr;
    let calls = FrameCalls {
        result_alloc: decl("__torajs_frame_result_alloc", &[Type::I64], Type::Promise),
        await_promise: decl(
            "__torajs_frame_await_promise",
            &[p, Type::Promise],
            Type::Void,
        ),
        await_direct: decl("__torajs_frame_await_direct", &[p], Type::Void),
        ret: decl(
            "__torajs_frame_return",
            &[p, Type::I64, Type::I64],
            Type::Void,
        ),
        throw: decl("__torajs_frame_throw", &[p], Type::Void),
        unwind: decl("__torajs_frame_unwind", &[p], Type::Void),
        cancelled: decl(super::suspend::CANCELLED_PROBE, &[], Type::I64),
        release_links: decl("__torajs_frame_release_links", &[p], Type::Void),
        trace_links: decl("__torajs_frame_trace_links", &[p, p, p], Type::Void),
        visit_sig: crate::ssa_lower::intern_fn_sig(fn_sigs, vec![Type::I64, p, p, p], Type::Void),
    };
    let mut slots = HashMap::new();
    for name in names {
        let resume = module.add_function(stub(&format!("__async_resume_{name}")));
        let drop = module.add_function(stub(&format!("__async_drop_{name}")));
        let trace = module.add_function(stub(&format!("__async_trace_{name}")));
        slots.insert(
            name.clone(),
            Slots {
                resume,
                drop,
                trace,
            },
        );
    }
    SuspendSetup {
        slots,
        calls: Some(calls),
    }
}

/// A reserved slot no lowered body filled: never reached, and kept a
/// definition so no reference to it is left unresolved.
fn stub(name: &str) -> ssa::Function {
    let mut f = ssa::Function::new(name, Type::Void);
    f.add_param(Type::Ptr, "frame");
    let b = f.add_block();
    f.set_term(b, ssa::Terminator::Unreachable);
    f
}

/// `(frame, visit, ctx)`, the closure trace ABI: the frame's links,
/// then each parameter word in `traced` (its index is its word number;
/// the split pass appends the suspension points' words).
fn frame_trace(name: &str, calls: &FrameCalls, traced: &[u32]) -> ssa::Function {
    let mut f = ssa::Function::new(format!("__async_trace_{name}"), Type::Void);
    let args: Vec<Operand> = ["frame", "visit", "ctx"]
        .iter()
        .map(|n| Operand::Value(f.add_param(Type::Ptr, n)))
        .collect();
    let b = f.add_block();
    f.append_void(b, InstKind::Call(calls.trace_links, args.clone()));
    for &i in traced {
        let off = FRAME_PARAMS_OFF + 8 * u64::from(i);
        let base = f.append_inst(b, InstKind::PtrToInt(args[0]), Type::I64, None);
        let at = InstKind::BinOp(
            BinOp::Add,
            Operand::Value(base),
            Operand::ConstI64(off as i64),
        );
        let at = f.append_inst(b, at, Type::I64, None);
        let addr = f.append_inst(b, InstKind::IntToPtr(Operand::Value(at)), Type::Ptr, None);
        let child = f.append_inst(b, InstKind::Load(Type::Ptr, args[0], off), Type::Ptr, None);
        let idx = Operand::ConstI64((off / 8) as i64);
        let visit = vec![idx, Operand::Value(child), Operand::Value(addr), args[2]];
        f.append_void(b, InstKind::CallIndirect(calls.visit_sig, args[1], visit));
    }
    f.set_term(b, ssa::Terminator::Ret(None));
    f
}

/// Put the lowered `f` in its place: under `fid`, or, for a body that
/// suspends, in its resume slot behind a wrapper under `fid`.
pub(crate) fn install(
    module: &mut Module,
    fid: FuncId,
    mut f: ssa::Function,
    setup: &SuspendSetup,
    intrinsics: &crate::ssa_lower::Intrinsics,
    arr_layouts: &ssa::ArrLayouts,
) {
    let (Some(&slots), Some(calls)) = (setup.slots.get(&f.name), setup.calls) else {
        module.funcs[fid.0 as usize] = f;
        return;
    };
    mark_returned_arrays(&mut f, arr_layouts, intrinsics.arr_mark_kind);
    let index = module.suspend_fns.len();
    let size_placeholder = FRAME_SIZE_PLACEHOLDER_BASE + index as i64;
    let params: Vec<(String, Type)> = (f.params.iter())
        .map(|&p| (f.value_name(p)[1..].to_string(), f.value_type(p)))
        .collect();
    let traced_params: Vec<u32> = (params.iter().enumerate())
        .filter(|(_, (n, t))| super::async_wrapper::param_held(n, *t) && *t != Type::Substr)
        .map(|(i, _)| i as u32)
        .collect();
    let ret_repr = ret_repr(f.ret);
    let name = f.name.clone();
    let desc_sym = format!("__frame_desc_{index}");
    let shape = super::async_wrapper::FrameShape {
        params: &params,
        size_placeholder,
        resume: slots.resume,
        calls,
    };
    module.funcs[slots.resume.0 as usize] = frame_body(f, &name, calls.cancelled);
    module.funcs[slots.drop.0 as usize] =
        super::async_wrapper::build_drop(&name, &shape, intrinsics);
    module.funcs[slots.trace.0 as usize] = frame_trace(&name, &calls, &traced_params);
    let mut wrapper =
        super::async_wrapper::build_wrapper(&name, &shape, &desc_sym, ret_repr, intrinsics);
    super::async_wrapper::answer_as(&mut wrapper, module.funcs[fid.0 as usize].ret);
    module.funcs[fid.0 as usize] = wrapper;
    module.suspend_fns.push(SuspendFn {
        wrapper: fid,
        resume: slots.resume,
        drop: slots.drop,
        trace: slots.trace,
        traced_params,
        desc_sym,
        size_placeholder,
        nparams: params.len() as u32,
        ret_repr,
    });
    module.suspend_runtime = Some(SuspendRuntime {
        trace_links: calls.trace_links,
        visit_sig: calls.visit_sig,
        await_promise: calls.await_promise,
        await_direct: calls.await_direct,
        ret: calls.ret,
        throw: calls.throw,
        obj_alloc: intrinsics.obj_alloc,
        obj_drop_sized: intrinsics.obj_drop_sized,
    });
}

/// Each array the body returns marked with its element kind, as
/// `Promise.resolve` marks the array it settles with: the result is
/// read in the `any` lane too (a `.then` handler taking `any`), which
/// reads the elements by the mark.
fn mark_returned_arrays(f: &mut ssa::Function, arr_layouts: &ssa::ArrLayouts, mark: FuncId) {
    for b in &mut f.blocks {
        let ssa::Terminator::Ret(Some(Operand::Value(v))) = b.term else {
            continue;
        };
        let Type::Arr(id) = f.values[v.0 as usize].ty else {
            continue;
        };
        let elem = arr_layouts[id.0 as usize];
        let chain = crate::ssa_lower_arr_kind_mark::arr_kind_chain_of(arr_layouts, &elem, 0);
        if chain != 0 {
            let args = vec![Operand::Value(v), Operand::ConstI64(chain as i64)];
            b.insts.push(Inst {
                result: None,
                kind: InstKind::Call(mark, args),
                origin: None,
            });
        }
    }
}

/// The body as a function of its frame: the one parameter is the
/// frame, each of the body's parameters is read from the frame's
/// parameter area on entry, under the value it had, and each call of
/// the `cancelled` placeholder becomes the read of the frame's cancel
/// word.
fn frame_body(mut f: ssa::Function, name: &str, cancelled: FuncId) -> ssa::Function {
    f.name = format!("__async_resume_{name}");
    let params = std::mem::take(&mut f.params);
    let frame = f.add_param(Type::Ptr, "__frame");
    let loads = params.iter().enumerate().map(|(i, &p)| Inst {
        result: Some(p),
        kind: InstKind::Load(
            f.value_type(p),
            Operand::Value(frame),
            FRAME_PARAMS_OFF + 8 * i as u64,
        ),
        origin: None,
    });
    let loads: Vec<Inst> = loads.collect();
    f.blocks[0].insts.splice(0..0, loads);
    for i in f.blocks.iter_mut().flat_map(|b| b.insts.iter_mut()) {
        if matches!(&i.kind, InstKind::Call(fid, _) if *fid == cancelled) {
            i.kind = InstKind::Load(Type::I64, Operand::Value(frame), FRAME_DIRECT_OFF);
        }
    }
    f
}

/// The value form a body returning `ty` settles its result with
/// (`torajs-promise` `REPR_*`): the form `Promise.resolve` of a value
/// of that type stamps, a number at the width the body returns it
/// (the width of the promise's value point, which every `await` of a
/// call to the body reads).
fn ret_repr(ty: Type) -> i64 {
    use crate::ssa_lower_promise_repr_mark as m;
    match m::promise_value_repr(&ty, false, false) {
        Some(r) => r,
        None if ty.is_refcounted() => m::REPR_HEAP,
        None => m::REPR_I64,
    }
}
