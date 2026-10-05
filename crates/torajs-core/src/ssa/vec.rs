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
    /// two f64 lanes
    F64x2,
}

impl VecShape {
    pub fn as_str(self) -> &'static str {
        match self {
            VecShape::I64x2 => "i64x2",
            VecShape::F64x2 => "f64x2",
        }
    }

    /// Bytes one value occupies in memory and in a register.
    pub fn bytes(self) -> u64 {
        match self {
            VecShape::I64x2 | VecShape::F64x2 => 16,
        }
    }
}

/// Lane-wise operation; integer lanes wrap like the scalar `BinOp`,
/// float lanes round like it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VecOp {
    Add,
    Or,
    FDiv,
}

impl VecOp {
    pub fn as_str(self) -> &'static str {
        match self {
            VecOp::Add => "add",
            VecOp::Or => "or",
            VecOp::FDiv => "fdiv",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VecInst {
    /// `%v = vec.splat <shape> <scalar>` — every lane holds the scalar.
    Splat(VecShape, Operand),
    /// `%v = vec.pair <lo>, <hi>` — lane 0 holds `lo`, lane 1 `hi`;
    /// both scalars are f64 and the result is `F64x2`.
    Pair(Operand, Operand),
    /// `%v = vec.<op> <a>, <b>` — lane-wise; both operands are vectors
    /// of the result's shape.
    BinOp(VecOp, Operand, Operand),
    /// `%v = vec.fptosi <a>` — each f64 lane to i64, rounding toward
    /// zero and saturating, NaN to 0: the lane-wise `FpToSi`.
    FpToSi(Operand),
    /// `%x = vec.lane <k> <v>` — lane `k` of an `I64x2` as an i64.
    Lane(u8, Operand),
    /// `%x = vec.reduce.<op> <v>` — folds every lane into one i64.
    Reduce(VecOp, Operand),
}

impl VecInst {
    pub fn operands(&self) -> [Option<&Operand>; 2] {
        match self {
            VecInst::Splat(_, a)
            | VecInst::FpToSi(a)
            | VecInst::Lane(_, a)
            | VecInst::Reduce(_, a) => [Some(a), None],
            VecInst::Pair(a, b) | VecInst::BinOp(_, a, b) => [Some(a), Some(b)],
        }
    }

    pub fn operands_mut(&mut self) -> [Option<&mut Operand>; 2] {
        match self {
            VecInst::Splat(_, a)
            | VecInst::FpToSi(a)
            | VecInst::Lane(_, a)
            | VecInst::Reduce(_, a) => [Some(a), None],
            VecInst::Pair(a, b) | VecInst::BinOp(_, a, b) => [Some(a), Some(b)],
        }
    }

    pub fn mnemonic(&self) -> String {
        match self {
            VecInst::Splat(shape, _) => format!("vec.splat {}", shape.as_str()),
            VecInst::Pair(..) => "vec.pair".into(),
            VecInst::BinOp(op, _, _) => format!("vec.{}", op.as_str()),
            VecInst::FpToSi(_) => "vec.fptosi".into(),
            VecInst::Lane(k, _) => format!("vec.lane {k}"),
            VecInst::Reduce(op, _) => format!("vec.reduce.{}", op.as_str()),
        }
    }
}
