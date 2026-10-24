//! The two functions built around an async body that suspends over a
//! frame (`async_setup`): the wrapper callers reach, and the
//! frame's drop.
//!
//! The wrapper is ECMA-262 §15.8.4 EvaluateAsyncFunctionBody with
//! §27.7.5.2 AsyncBlockStart: the result promise exists before the
//! body runs, and the body runs at once, up to its first suspension
//! (a parameter default that throws rejects the promise, through the
//! body's own exception path). The frame takes a reference on every
//! parameter that holds one, since the body may outlive the caller's
//! borrow; the drop gives them back.
//!
//! The frame's byte size is known only once the split pass has
//! counted its slots, so both functions carry a placeholder constant
//! the pass replaces.

use super::async_setup::FrameCalls;
use crate::ssa::suspend::{
    FRAME_AWAITING_OFF, FRAME_DESC_OFF, FRAME_DIRECT_OFF, FRAME_NEXT_OFF, FRAME_PARAMS_OFF,
    FRAME_RESULT_OFF, FRAME_STATE_OFF,
};
use crate::ssa::{self, FuncId, IPred, InstKind, Operand, Terminator, Type};
use crate::ssa_lower::Intrinsics;

/// What both functions know of the frame.
pub(crate) struct FrameShape<'a> {
    /// The body's parameters, in frame order.
    pub(crate) params: &'a [(String, Type)],
    pub(crate) size_placeholder: i64,
    pub(crate) resume: FuncId,
    pub(crate) calls: FrameCalls,
}

/// The argv buffer an `arguments`-reading body is handed: the
/// caller's, dead once the call returns.
const ARGV: &str = "__torajs_argv";

/// Whether the frame holds a reference on a parameter of `ty`. The
/// closure env (`__env`, a `Ptr`) is a cell like any other. The argv
/// buffer is no cell: the frame keeps its word, unowned, for the run
/// the call makes ([`assert_argv_read_before_awaits`]). Any other
/// `Ptr` parameter is typed `null` or `undefined` (a specialization of
/// an unannotated parameter for such an argument): its word is the
/// null pointer, no cell to hold.
pub(crate) fn param_held(name: &str, ty: Type) -> bool {
    match ty {
        Type::Ptr => name == "__env",
        Type::Inline(_) => panic!(
            "internal error: async parameter `{name}` borrows an array element the frame cannot keep"
        ),
        t => !t.is_copy(),
    }
}

/// The argv buffer's one reader is the arguments object's
/// materialization heading the body (`ast::arguments_object_synth`),
/// which runs inside the call, before the first await. A read a
/// suspension can reach would see a buffer that is gone, so it fails
/// the build instead.
pub(crate) fn assert_argv_read_before_awaits(f: &ssa::Function) {
    let Some(&argv) = (f.params.iter()).find(|&&p| f.value_name(p)[1..] == *ARGV) else {
        return;
    };
    let mut seen = vec![false; f.blocks.len()];
    let mut work: Vec<(usize, usize)> = Vec::new();
    for (bi, b) in f.blocks.iter().enumerate() {
        if let Some(at) = (b.insts.iter()).position(|i| matches!(i.kind, InstKind::Suspend(..))) {
            work.push((bi, at + 1));
        }
    }
    while let Some((bi, from)) = work.pop() {
        for inst in &f.blocks[bi].insts[from..] {
            ssa::visit_value_operands(&inst.kind, |v| {
                assert!(
                    v != argv,
                    "internal error: `{}` reads its argv buffer after an await",
                    f.name
                )
            });
        }
        let next = match f.blocks[bi].term {
            Terminator::Br(t) => vec![t],
            Terminator::CondBr {
                then_blk, else_blk, ..
            } => vec![then_blk, else_blk],
            _ => vec![],
        };
        for t in next {
            if !std::mem::replace(&mut seen[t.0 as usize], true) {
                work.push((t.0 as usize, 0));
            }
        }
    }
}

fn param_off(i: usize) -> u64 {
    FRAME_PARAMS_OFF + 8 * i as u64
}

pub(crate) fn build_wrapper(
    name: &str,
    shape: &FrameShape<'_>,
    desc_sym: &str,
    ret_repr: i64,
    intrinsics: &Intrinsics,
) -> ssa::Function {
    let mut f = ssa::Function::new(name, Type::Promise);
    let params: Vec<_> = (shape.params.iter())
        .map(|(n, t)| (f.add_param(*t, n), *t, n.as_str()))
        .collect();
    let b = f.add_block();
    let frame = f.append_inst(
        b,
        InstKind::Call(
            intrinsics.obj_alloc,
            vec![Operand::ConstI64(shape.size_placeholder)],
        ),
        Type::Ptr,
        Some("frame"),
    );
    let fr = Operand::Value(frame);
    // a store writes the whole word, so the header goes in ascending
    // offset order, each store covering the one before's spill-over
    let tag = torajs_rc::Tag::AsyncFrame as i32;
    f.append_void(b, InstKind::Store(Operand::ConstI32(1), fr, 0));
    f.append_void(b, InstKind::Store(Operand::ConstI32(tag), fr, 4));
    let desc = f.append_inst(b, InstKind::GlobalRef(desc_sym.into()), Type::Ptr, None);
    f.append_void(b, InstKind::Store(Operand::Value(desc), fr, FRAME_DESC_OFF));
    f.append_void(
        b,
        InstKind::Store(Operand::ConstI64(0), fr, FRAME_STATE_OFF),
    );
    f.append_void(
        b,
        InstKind::Store(Operand::ConstI64(0), fr, FRAME_DIRECT_OFF),
    );
    let result = f.append_inst(
        b,
        InstKind::Call(shape.calls.result_alloc, vec![Operand::ConstI64(ret_repr)]),
        Type::Promise,
        Some("result"),
    );
    let res = Operand::Value(result);
    f.append_void(b, InstKind::Store(res, fr, FRAME_RESULT_OFF));
    f.append_void(b, InstKind::Store(Operand::ConstI64(0), fr, FRAME_NEXT_OFF));
    f.append_void(
        b,
        InstKind::Store(Operand::ConstI64(0), fr, FRAME_AWAITING_OFF),
    );
    for (i, &(p, ty, pname)) in params.iter().enumerate() {
        let op = Operand::Value(p);
        if param_held(pname, ty) {
            // the env is a cell, never an immediate: inc it as a typed one
            let held = if ty == Type::Ptr {
                Type::Obj(ssa::StructId(0))
            } else {
                ty
            };
            crate::ssa_lower_rc_emit::emit_rc_inc_in_fn(&mut f, b, op, held);
        }
        f.append_void(b, InstKind::Store(op, fr, param_off(i)));
    }
    // the caller's reference; the frame keeps its own
    crate::ssa_lower_rc_emit::emit_rc_inc_in_fn(&mut f, b, res, Type::Promise);
    f.append_void(b, InstKind::Call(shape.resume, vec![fr]));
    f.set_term(b, Terminator::Ret(Some(res)));
    f
}

/// `f` (answering the result promise) as a function answering `ret`,
/// its registered return type: a twin declared to answer `any` boxes
/// the promise.
pub(crate) fn answer_as(f: &mut ssa::Function, ret: Type) {
    if ret != Type::Any || f.ret == Type::Any {
        return;
    }
    for bi in 0..f.blocks.len() {
        let Terminator::Ret(Some(op)) = f.blocks[bi].term else {
            continue;
        };
        let b = ssa::BlockId(bi as u32);
        let at = f.blocks[bi].insts.len();
        let boxed = f.insert_inst(b, at, InstKind::BoxAny(ssa::ANY_SLOT_TAG_HEAP, op), ret);
        f.blocks[bi].term = Terminator::Ret(Some(Operand::Value(boxed)));
    }
    f.ret = ret;
}

/// The frame's drop: a frame left at a suspension point is first
/// unwound through its body (`torajs-promise` `frame::body`); then the
/// parameters' references, the links, and the block go.
pub(crate) fn build_drop(
    name: &str,
    shape: &FrameShape<'_>,
    intrinsics: &Intrinsics,
) -> ssa::Function {
    let mut f = ssa::Function::new(format!("__async_drop_{name}"), Type::Void);
    let frame = f.add_param(Type::Ptr, "frame");
    let fr = Operand::Value(frame);
    let entry = f.add_block();
    let unwind = f.add_block();
    let tail = f.add_block();
    let state = f.append_inst(
        entry,
        InstKind::Load(Type::I64, fr, FRAME_STATE_OFF),
        Type::I64,
        None,
    );
    let parked = f.append_inst(
        entry,
        InstKind::ICmp(IPred::Sgt, Operand::Value(state), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    f.set_term(
        entry,
        Terminator::CondBr {
            cond: Operand::Value(parked),
            then_blk: unwind,
            else_blk: tail,
        },
    );
    f.append_void(unwind, InstKind::Call(shape.calls.unwind, vec![fr]));
    f.set_term(unwind, Terminator::Br(tail));
    for (i, (pname, ty)) in shape.params.iter().enumerate() {
        if !param_held(pname, *ty) {
            continue;
        }
        let (load_ty, drop_fid) = match ty {
            Type::Any => (Type::Any, intrinsics.any_box_drop),
            Type::Substr => (Type::Substr, intrinsics.substr_drop),
            Type::Ptr => (Type::Ptr, intrinsics.value_drop_heap),
            other => (*other, intrinsics.value_drop_heap),
        };
        let v = f.append_inst(
            tail,
            InstKind::Load(load_ty, fr, param_off(i)),
            load_ty,
            None,
        );
        f.append_void(tail, InstKind::Call(drop_fid, vec![Operand::Value(v)]));
    }
    f.append_void(tail, InstKind::Call(shape.calls.release_links, vec![fr]));
    crate::ssa_lower_env_drop_and_ret_ty::emit_env_free_or_defer(
        &mut f,
        tail,
        frame,
        shape.size_placeholder as u64,
        FRAME_DESC_OFF,
        intrinsics.obj_drop_sized,
    );
    f
}
