use super::super::compile_function;
use torajs_core::ssa::{
    BinOp, BlockId, Function, IPred, InstKind, Operand, Terminator, Type, ValueId,
};

const SXTW: (u32, u32) = (0xFFFF_FC00, 0x9340_7C00);
const SCVTF_W: (u32, u32) = (0xFFFF_FC00, 0x1E62_0000);
const SCVTF_X: (u32, u32) = (0xFFFF_FC00, 0x9E62_0000);
const LSL32: (u32, u32) = (0xFFFF_FC00, 0xD360_7C00);
const ASR32: (u32, u32) = (0xFFFF_FC00, 0x9360_FC00);
const FJCVTZS: (u32, u32) = (0xFFFF_FC00, 0x1E7E_0000);

fn count(f: &Function, (mask, base): (u32, u32)) -> usize {
    compile_function(f)
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .filter(|w| w & mask == base)
        .count()
}

fn inst(f: &mut Function, b: BlockId, kind: InstKind, ty: Type) -> ValueId {
    f.append_inst(b, kind, ty, None)
}

fn bin(f: &mut Function, b: BlockId, op: BinOp, x: ValueId, y: Operand) -> ValueId {
    inst(f, b, InstKind::BinOp(op, Operand::Value(x), y), Type::I64)
}

/// `fn(a: f64) -> R { use(a | 0) }`, with `use` building the returned
/// operand from the int32.
fn int32_of(ret: Type, body: impl FnOnce(&mut Function, BlockId, ValueId) -> ValueId) -> Function {
    let mut f = Function::new("t", ret);
    let a = f.add_param(Type::F64, "a");
    let b = f.add_block();
    let r = inst(&mut f, b, InstKind::FpToInt32(Operand::Value(a)), Type::I64);
    let out = body(&mut f, b, r);
    f.set_term(b, Terminator::Ret(Some(Operand::Value(out))));
    f
}

#[test]
fn an_int32_converted_back_to_f64_skips_its_sign_extension() {
    let f = int32_of(Type::F64, |f, b, r| {
        inst(f, b, InstKind::SiToFp(Operand::Value(r)), Type::F64)
    });
    assert_eq!(count(&f, SXTW), 0);
    assert_eq!(count(&f, SCVTF_W), 1);
    assert_eq!(count(&f, SCVTF_X), 0);
}

#[test]
fn an_int32_returned_as_i64_keeps_its_sign_extension() {
    let f = int32_of(Type::I64, |_, _, r| r);
    assert_eq!(count(&f, SXTW), 1);
}

#[test]
fn an_int32_stored_or_compared_keeps_its_sign_extension() {
    let stored = int32_of(Type::F64, |f, b, r| {
        let p = f.add_param(Type::Ptr, "p");
        f.append_void(b, InstKind::Store(Operand::Value(r), Operand::Value(p), 0));
        inst(f, b, InstKind::SiToFp(Operand::Value(r)), Type::F64)
    });
    assert_eq!(count(&stored, SXTW), 1);
    let compared = int32_of(Type::I64, |f, b, r| {
        let c = inst(
            f,
            b,
            InstKind::ICmp(IPred::Slt, Operand::Value(r), Operand::ConstI64(0)),
            Type::Bool,
        );
        inst(f, b, InstKind::ZExtBoolToI64(Operand::Value(c)), Type::I64)
    });
    assert_eq!(count(&compared, SXTW), 1);
}

#[test]
fn a_sum_read_whole_keeps_its_operands_sign_extended() {
    let f = int32_of(Type::I64, |f, b, r| {
        bin(f, b, BinOp::Add, r, Operand::ConstI64(1))
    });
    assert_eq!(count(&f, SXTW), 1);
}

#[test]
fn an_add_whose_low_half_alone_is_read_frees_its_operands() {
    // (r + r) << 32 >> 32 returned: the pair is one sxtw, r none
    let f = int32_of(Type::I64, |f, b, r| {
        let s = bin(f, b, BinOp::Add, r, Operand::Value(r));
        let t = bin(f, b, BinOp::Shl, s, Operand::ConstI64(32));
        bin(f, b, BinOp::AShr, t, Operand::ConstI64(32))
    });
    assert_eq!(count(&f, SXTW), 1);
    assert_eq!(count(&f, LSL32), 0);
    assert_eq!(count(&f, ASR32), 0);
}

/// `Math.imul(a, b)` returned as a number.
fn imul() -> Function {
    let mut f = Function::new("imul", Type::F64);
    let a = f.add_param(Type::F64, "a");
    let c = f.add_param(Type::F64, "b");
    let b = f.add_block();
    let x = inst(&mut f, b, InstKind::FpToInt32(Operand::Value(a)), Type::I64);
    let y = inst(&mut f, b, InstKind::FpToInt32(Operand::Value(c)), Type::I64);
    let m = bin(&mut f, b, BinOp::Mul, x, Operand::Value(y));
    let t = bin(&mut f, b, BinOp::Shl, m, Operand::ConstI64(32));
    let r = bin(&mut f, b, BinOp::AShr, t, Operand::ConstI64(32));
    let d = inst(&mut f, b, InstKind::SiToFp(Operand::Value(r)), Type::F64);
    f.set_term(b, Terminator::Ret(Some(Operand::Value(d))));
    f
}

#[test]
fn imul_back_to_f64_is_two_conversions_a_mul_and_a_w_convert() {
    let f = imul();
    assert_eq!(count(&f, FJCVTZS), 2);
    assert_eq!(count(&f, SXTW), 0);
    assert_eq!(count(&f, LSL32), 0);
    assert_eq!(count(&f, ASR32), 0);
    assert_eq!(count(&f, SCVTF_W), 1);
    assert_eq!(count(&f, SCVTF_X), 0);
}

#[test]
fn a_pair_whose_result_is_also_returned_keeps_it_defined() {
    // r = sext32(m) is converted and returned: one sxtw for the pair
    let mut f = Function::new("t", Type::I64);
    let a = f.add_param(Type::F64, "a");
    let b = f.add_block();
    let x = inst(&mut f, b, InstKind::FpToInt32(Operand::Value(a)), Type::I64);
    let m = bin(&mut f, b, BinOp::Mul, x, Operand::Value(x));
    let t = bin(&mut f, b, BinOp::Shl, m, Operand::ConstI64(32));
    let r = bin(&mut f, b, BinOp::AShr, t, Operand::ConstI64(32));
    let d = inst(&mut f, b, InstKind::SiToFp(Operand::Value(r)), Type::F64);
    let p = f.add_param(Type::Ptr, "p");
    f.append_void(b, InstKind::Store(Operand::Value(d), Operand::Value(p), 0));
    f.set_term(b, Terminator::Ret(Some(Operand::Value(r))));
    assert_eq!(count(&f, SXTW), 1);
    assert_eq!(count(&f, ASR32), 0);
    assert_eq!(count(&f, SCVTF_W), 1);
}
