//! Fixed-width SIMD values and the instructions that make them.
//!
//! A `Type::Vec` value lives in one 128-bit vector register. Memory
//! traffic reuses the typed `Load` / `Store` (16 bytes at the address,
//! no alignment requirement); a loop-carried vector is a multi-def
//! `Copy(Type::Vec(..), op)` cell like any scalar one. Only passes that
//! run after the egraph pass create these values.

use super::Operand;

/// Lane layout of a vector value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VecShape {
    /// two i64 lanes
    I64x2,
}

impl VecShape {
    pub fn as_str(self) -> &'static str {
        match self {
            VecShape::I64x2 => "i64x2",
        }
    }

    /// Bytes one value occupies in memory and in a register.
    pub fn bytes(self) -> u64 {
        match self {
            VecShape::I64x2 => 16,
        }
    }
}

/// Lane-wise integer operation; lanes wrap like the scalar `BinOp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VecOp {
    Add,
    Or,
}

impl VecOp {
    pub fn as_str(self) -> &'static str {
        match self {
            VecOp::Add => "add",
            VecOp::Or => "or",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VecInst {
    /// `%v = vec.splat <shape> <scalar>` — every lane holds the scalar.
    Splat(VecShape, Operand),
    /// `%v = vec.<op> <a>, <b>` — lane-wise; both operands are vectors
    /// of the result's shape.
    BinOp(VecOp, Operand, Operand),
    /// `%x = vec.reduce.<op> <v>` — folds every lane into one i64.
    Reduce(VecOp, Operand),
}

impl VecInst {
    pub fn operands(&self) -> [Option<&Operand>; 2] {
        match self {
            VecInst::Splat(_, a) | VecInst::Reduce(_, a) => [Some(a), None],
            VecInst::BinOp(_, a, b) => [Some(a), Some(b)],
        }
    }

    pub fn operands_mut(&mut self) -> [Option<&mut Operand>; 2] {
        match self {
            VecInst::Splat(_, a) | VecInst::Reduce(_, a) => [Some(a), None],
            VecInst::BinOp(_, a, b) => [Some(a), Some(b)],
        }
    }

    pub fn mnemonic(&self) -> String {
        match self {
            VecInst::Splat(shape, _) => format!("vec.splat {}", shape.as_str()),
            VecInst::BinOp(op, _, _) => format!("vec.{}", op.as_str()),
            VecInst::Reduce(op, _) => format!("vec.reduce.{}", op.as_str()),
        }
    }
}
