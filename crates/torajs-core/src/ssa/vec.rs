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
    Sub,
    And,
    /// `a & !b`
    AndNot,
    Or,
    FAdd,
    FSub,
    FMul,
    FDiv,
    /// `F64x2` operands, an `I64x2` mask: a lane is all ones where the
    /// two f64 lanes are equal (NaN never is), else zero
    FCmEq,
    /// as `FCmEq`, for `a > b` and `a >= b` (NaN never is)
    FCmGt,
    FCmGe,
    /// `I64x2` operands, an `I64x2` mask: all ones where the signed
    /// lanes satisfy `a == b`, `a > b`, `a >= b`, else zero
    CmEq,
    CmGt,
    CmGe,
}

impl VecOp {
    pub fn as_str(self) -> &'static str {
        match self {
            VecOp::Add => "add",
            VecOp::Sub => "sub",
            VecOp::And => "and",
            VecOp::AndNot => "andnot",
            VecOp::Or => "or",
            VecOp::FAdd => "fadd",
            VecOp::FSub => "fsub",
            VecOp::FMul => "fmul",
            VecOp::FDiv => "fdiv",
            VecOp::FCmEq => "fcmeq",
            VecOp::FCmGt => "fcmgt",
            VecOp::FCmGe => "fcmge",
            VecOp::CmEq => "cmeq",
            VecOp::CmGt => "cmgt",
            VecOp::CmGe => "cmge",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VecInst {
    /// `%v = vec.splat <shape> <scalar>` — every lane holds the scalar.
    Splat(VecShape, Operand),
    /// `%v = vec.pair <shape> <lo>, <hi>` — lane 0 holds `lo`, lane 1
    /// `hi`; both scalars are the shape's lane type.
    Pair(VecShape, Operand, Operand),
    /// `%v = vec.<op> <a>, <b>` — lane-wise; both operands are vectors
    /// of the result's shape (the `FCm*` compares read `F64x2`).
    BinOp(VecOp, Operand, Operand),
    /// `%v = vec.select <mask>, <a>, <b>` — each lane from `a` where
    /// the `I64x2` mask lane is all ones, from `b` where it is zero;
    /// the mask's lanes are all ones or zero. `a`, `b` and the result
    /// share a shape.
    Select(Operand, Operand, Operand),
    /// `%v = vec.ext <a>, <b>` — lane 0 is `a`'s lane 1, lane 1 is
    /// `b`'s lane 0: the next lane along a run of vectors (ARM `ext`
    /// by one lane). Both operands are the result's shape.
    Ext(Operand, Operand),
    /// `%x = vec.any <v>` — a nonzero i64 where some lane of an
    /// `I64x2` mask is all ones, zero where every lane is zero; the
    /// lanes are all ones or zero.
    AnyLane(Operand),
    /// `%v = vec.fptosi <a>` — each f64 lane to i64, rounding toward
    /// zero and saturating, NaN to 0: the lane-wise `FpToSi`.
    FpToSi(Operand),
    /// `%v = vec.frintz <a>` — each f64 lane rounded toward zero, as
    /// f64: the lane-wise `trunc`.
    FRintZ(Operand),
    /// `%x = vec.lane <k> <v>` — lane `k` as the lane type: an i64 of
    /// an `I64x2`, an f64 of an `F64x2`.
    Lane(u8, Operand),
    /// `%x = vec.reduce.<op> <v>` — folds every lane into one i64.
    Reduce(VecOp, Operand),
}

impl VecInst {
    pub fn operands(&self) -> [Option<&Operand>; 3] {
        match self {
            VecInst::Splat(_, a)
            | VecInst::FpToSi(a)
            | VecInst::FRintZ(a)
            | VecInst::Lane(_, a)
            | VecInst::AnyLane(a)
            | VecInst::Reduce(_, a) => [Some(a), None, None],
            VecInst::Pair(_, a, b) | VecInst::BinOp(_, a, b) | VecInst::Ext(a, b) => {
                [Some(a), Some(b), None]
            }
            VecInst::Select(m, a, b) => [Some(m), Some(a), Some(b)],
        }
    }

    pub fn operands_mut(&mut self) -> [Option<&mut Operand>; 3] {
        match self {
            VecInst::Splat(_, a)
            | VecInst::FpToSi(a)
            | VecInst::FRintZ(a)
            | VecInst::Lane(_, a)
            | VecInst::AnyLane(a)
            | VecInst::Reduce(_, a) => [Some(a), None, None],
            VecInst::Pair(_, a, b) | VecInst::BinOp(_, a, b) | VecInst::Ext(a, b) => {
                [Some(a), Some(b), None]
            }
            VecInst::Select(m, a, b) => [Some(m), Some(a), Some(b)],
        }
    }

    pub fn mnemonic(&self) -> String {
        match self {
            VecInst::Splat(shape, _) => format!("vec.splat {}", shape.as_str()),
            VecInst::Pair(shape, ..) => format!("vec.pair {}", shape.as_str()),
            VecInst::BinOp(op, _, _) => format!("vec.{}", op.as_str()),
            VecInst::Select(..) => "vec.select".into(),
            VecInst::Ext(..) => "vec.ext".into(),
            VecInst::AnyLane(_) => "vec.any".into(),
            VecInst::FpToSi(_) => "vec.fptosi".into(),
            VecInst::FRintZ(_) => "vec.frintz".into(),
            VecInst::Lane(k, _) => format!("vec.lane {k}"),
            VecInst::Reduce(op, _) => format!("vec.reduce.{}", op.as_str()),
        }
    }
}
