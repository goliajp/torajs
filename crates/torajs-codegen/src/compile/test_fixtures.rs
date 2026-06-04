//! Shared test-only SSA Function builders.
//!
//! Each `build_*` returns a fully-formed single-block `Function` ready
//! to feed into `compile_function`. Sub-module tests use these so
//! every test file stays small and focused on byte-equal verification
//! rather than rebuilding SSA shapes from scratch.

#![cfg(test)]

use torajs_core::ssa::{
    BinOp, Block, BlockId, FPred, Function, IPred, Inst, InstKind, Operand, Terminator, Type,
    ValueId, ValueInfo,
};

/// Build a single-block integer binop fixture:
/// `fn <name>() -> i64 { <lhs> <op> <rhs> }`
pub(crate) fn build_binop_const(name: &str, op: BinOp, lhs: i64, rhs: i64) -> Function {
    let v0 = ValueId(0);
    Function {
        name: name.into(),
        params: Vec::new(),
        ret: Type::I64,
        values: vec![ValueInfo {
            ty: Type::I64,
            name: Some("v0".into()),
        }],
        blocks: vec![Block {
            id: BlockId(0),
            insts: vec![Inst {
                result: Some(v0),
                kind: InstKind::BinOp(op, Operand::ConstI64(lhs), Operand::ConstI64(rhs)),
                origin: None,
            }],
            term: Terminator::Ret(Some(Operand::Value(v0))),
        }],
        current_origin: None,
    }
}

/// S1 canonical baseline: `1 + 2`.
pub(crate) fn build_one_plus_two() -> Function {
    build_binop_const("one_plus_two", BinOp::Add, 1, 2)
}

/// Build a single-block f64 binop fixture with `ret: F64`:
/// `fn <name>() -> f64 { <lhs> <op> <rhs> }`
pub(crate) fn build_fp_binop_const(name: &str, op: BinOp, lhs: f64, rhs: f64) -> Function {
    let v0 = ValueId(0);
    Function {
        name: name.into(),
        params: Vec::new(),
        ret: Type::F64,
        values: vec![ValueInfo {
            ty: Type::F64,
            name: Some("v0".into()),
        }],
        blocks: vec![Block {
            id: BlockId(0),
            insts: vec![Inst {
                result: Some(v0),
                kind: InstKind::BinOp(op, Operand::ConstF64(lhs), Operand::ConstF64(rhs)),
                origin: None,
            }],
            term: Terminator::Ret(Some(Operand::Value(v0))),
        }],
        current_origin: None,
    }
}

/// Build a single-block ICmp fixture with `ret: Bool`:
/// `fn <name>() -> bool { lhs <pred> rhs }`
pub(crate) fn build_icmp_const(name: &str, pred: IPred, lhs: i64, rhs: i64) -> Function {
    let v0 = ValueId(0);
    Function {
        name: name.into(),
        params: Vec::new(),
        ret: Type::Bool,
        values: vec![ValueInfo {
            ty: Type::Bool,
            name: Some("v0".into()),
        }],
        blocks: vec![Block {
            id: BlockId(0),
            insts: vec![Inst {
                result: Some(v0),
                kind: InstKind::ICmp(pred, Operand::ConstI64(lhs), Operand::ConstI64(rhs)),
                origin: None,
            }],
            term: Terminator::Ret(Some(Operand::Value(v0))),
        }],
        current_origin: None,
    }
}

/// Build the canonical S3-A2 fixture exercising Alloca + Store +
/// Load + Ret in sequence:
///
/// ```text
/// fn alloca_store_load_42() -> i64 {
///     let p = alloca 8;
///     *p = 42;
///     ret *p
/// }
/// ```
pub(crate) fn build_alloca_store_load_42() -> Function {
    let v0 = ValueId(0); // Alloca result (Ptr)
    let v1 = ValueId(1); // Load result (I64), ret
    Function {
        name: "alloca_store_load_42".into(),
        params: Vec::new(),
        ret: Type::I64,
        values: vec![
            ValueInfo {
                ty: Type::Ptr,
                name: Some("v0".into()),
            },
            ValueInfo {
                ty: Type::I64,
                name: Some("v1".into()),
            },
        ],
        blocks: vec![Block {
            id: BlockId(0),
            insts: vec![
                Inst {
                    result: Some(v0),
                    kind: InstKind::AllocaBytes(8),
                    origin: None,
                },
                Inst {
                    result: None,
                    kind: InstKind::Store(Operand::ConstI64(42), Operand::Value(v0), 0),
                    origin: None,
                },
                Inst {
                    result: Some(v1),
                    kind: InstKind::Load(Type::I64, Operand::Value(v0), 0),
                    origin: None,
                },
            ],
            term: Terminator::Ret(Some(Operand::Value(v1))),
        }],
        current_origin: None,
    }
}

/// Build a single-block FCmp fixture with `ret: Bool`.
pub(crate) fn build_fcmp_const(name: &str, pred: FPred, lhs: f64, rhs: f64) -> Function {
    let v0 = ValueId(0);
    Function {
        name: name.into(),
        params: Vec::new(),
        ret: Type::Bool,
        values: vec![ValueInfo {
            ty: Type::Bool,
            name: Some("v0".into()),
        }],
        blocks: vec![Block {
            id: BlockId(0),
            insts: vec![Inst {
                result: Some(v0),
                kind: InstKind::FCmp(pred, Operand::ConstF64(lhs), Operand::ConstF64(rhs)),
                origin: None,
            }],
            term: Terminator::Ret(Some(Operand::Value(v0))),
        }],
        current_origin: None,
    }
}

/// Concatenate u32 instruction words into a little-endian byte stream,
/// matching what `write_u32` emits in production.
pub(crate) fn words_to_le_bytes(words: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(words.len() * 4);
    for w in words {
        out.extend_from_slice(&w.to_le_bytes());
    }
    out
}
