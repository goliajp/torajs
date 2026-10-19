//! Suspension points: where a function parks, returns to its caller,
//! and is later re-entered at the same place.
//!
//! `InstKind::Suspend` exists only between lowering and the split pass.
//! Until the split it behaves as a call that may raise: not pure, not
//! inlinable through, and a barrier to every retain / release window.
//! The split pass turns each one into a store of the live state, a
//! return, and a landing block; codegen never sees one.
//!
//! The functions that hold suspensions are listed in
//! `Module::suspend_fns`: each was lowered as a resume function over
//! one heap frame (the layout `torajs-promise` `frame` documents), with
//! a wrapper that makes the frame and a drop that unwinds it.

use super::{FuncId, Type};

/// What the suspension waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuspendKind {
    /// `await v` (ECMA-262 Await).
    Await,
}

/// How the operand is waited on, fixed by its static type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuspendLane {
    /// The operand is statically not a thenable: one job later the
    /// function resumes, and the await's value is the operand the
    /// body kept. The suspension has no result.
    Direct,
    /// The operand is an owned promise cell; the function resumes once
    /// it settled, and the suspension's result is that cell, owned
    /// (the body reads its value in the given lane, `Any` when the
    /// inner type is unknown, and releases it).
    Promise(Type),
}

impl SuspendKind {
    pub fn name(self) -> &'static str {
        match self {
            SuspendKind::Await => "await",
        }
    }
}

impl SuspendLane {
    /// The result type of `suspend <kind> <lane>, %src`, `None` when it
    /// has none.
    pub fn result_type(self) -> Option<Type> {
        match self {
            SuspendLane::Direct => None,
            SuspendLane::Promise(_) => Some(Type::Promise),
        }
    }
}

/// Frame word offsets (`torajs-promise` `frame::AsyncFrame`).
pub const FRAME_DESC_OFF: u64 = 8;
pub const FRAME_STATE_OFF: u64 = 16;
pub const FRAME_DIRECT_OFF: u64 = 24;
pub const FRAME_RESULT_OFF: u64 = 32;
pub const FRAME_NEXT_OFF: u64 = 40;
pub const FRAME_AWAITING_OFF: u64 = 48;
/// The parameter area starts here; the slots follow it.
pub const FRAME_PARAMS_OFF: u64 = 56;

/// One function lowered over a frame.
#[derive(Debug, Clone)]
pub struct SuspendFn {
    /// The function callers reach: makes the frame and the result
    /// promise, runs the body to its first suspension, answers the
    /// promise.
    pub wrapper: FuncId,
    /// `(frame) -> void` once split: the body, entered at the point
    /// the frame's state names. Before the split its return type is
    /// the body's own and every `Ret` carries the returned value.
    pub resume: FuncId,
    /// `(frame) -> void`: the frame's drop.
    pub drop: FuncId,
    /// `(frame, visit, ctx) -> void`, the closure trace ABI: the
    /// links, the parameter words the frame holds, and, once split,
    /// the words the suspension point the frame is parked at keeps.
    pub trace: FuncId,
    /// Parameter words the trace reports (those the frame holds a
    /// reference on, but a substring view, which no cycle crosses).
    pub traced_params: Vec<u32>,
    /// The symbol of the frame's descriptor: three rows, the resume,
    /// drop and trace functions, each relative to the descriptor.
    pub desc_sym: String,
    /// The constant the frame's byte size stands in as until the split
    /// knows it (in the wrapper's allocation and the drop's free).
    pub size_placeholder: i64,
    /// Parameter words in the frame, before the slots.
    pub nparams: u32,
    /// The value form a `return` settles the result with (a
    /// `torajs-promise` `REPR_*` code).
    pub ret_repr: i64,
}

/// The runtime calls the split pass and the frame code emit.
#[derive(Debug, Clone, Copy)]
pub struct SuspendRuntime {
    /// `__torajs_frame_trace_links`, the part of every frame's trace
    /// the runtime owns.
    pub trace_links: FuncId,
    /// The trace visit callback's signature.
    pub visit_sig: super::SigId,
    pub await_promise: FuncId,
    pub await_direct: FuncId,
    pub ret: FuncId,
    pub throw: FuncId,
    pub obj_alloc: FuncId,
    pub obj_drop_sized: FuncId,
}

/// Set in the trace index of a word a suspension point keeps
/// (`torajs-promise` `frame::TRACE_STATE_WORD`).
pub const TRACE_STATE_WORD: i64 = 1 << 40;

/// The base of the frame-size placeholders: the high half no
/// allocation size has, the function's index in the low half.
pub const FRAME_SIZE_PLACEHOLDER_BASE: i64 = 0x5AF7_0000_0000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_direct_suspension_has_no_result() {
        assert_eq!(SuspendLane::Direct.result_type(), None);
    }

    #[test]
    fn a_promise_suspension_answers_the_settled_cell() {
        let lane = SuspendLane::Promise(Type::I64);
        assert_eq!(lane.result_type(), Some(Type::Promise));
        assert_eq!(SuspendKind::Await.name(), "await");
    }
}
