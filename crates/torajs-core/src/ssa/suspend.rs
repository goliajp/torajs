//! Suspension points: where a function parks, returns to its caller,
//! and is later re-entered at the same place with a value.
//!
//! `InstKind::Suspend` exists only between lowering and the split pass.
//! Until the split it behaves as a call that may raise: not pure, not
//! inlinable through, and a barrier to every retain / release window.
//! The split pass turns each one into a store of the live state, a
//! return, and a landing block; codegen never sees one.

use super::Type;

/// What the suspension waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuspendKind {
    /// `await v` (ECMA-262 Await): resumes with the settled value, or
    /// raises the rejection reason.
    Await,
}

/// How the operand is waited on, fixed by its static type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuspendLane {
    /// The operand is statically not a thenable: one job later the
    /// suspension resumes with the operand itself.
    Direct,
    /// The operand is an owned promise cell; the suspension resumes
    /// with its settled value read in the given lane (`Any` when the
    /// inner type is unknown).
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
    /// The result type of `%v = suspend <kind> <lane>, %src` given the
    /// operand's type.
    pub fn result_type(self, operand_ty: Type) -> Type {
        match self {
            SuspendLane::Direct => operand_ty,
            SuspendLane::Promise(t) => t,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_lane_resumes_with_the_operand_type() {
        assert_eq!(SuspendLane::Direct.result_type(Type::F64), Type::F64);
    }

    #[test]
    fn promise_lane_resumes_with_the_inner_lane() {
        let lane = SuspendLane::Promise(Type::I64);
        assert_eq!(lane.result_type(Type::Promise), Type::I64);
        assert_eq!(SuspendKind::Await.name(), "await");
    }
}
