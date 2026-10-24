use super::pin_reg::{PIN_DATA, PIN_LEN, PIN_SCALARS, PinRole, home};
use super::{ModuleFacts, compile_function_at};
use crate::enc::{add_imm, ldr_x_imm12, mov_x_reg};
use crate::reg::{Gpr, Reg};
use torajs_core::ssa::{
    BinOp, FuncId, Function, IPred, InstKind, Module, Operand, PinRegs, PinWord, StructId,
    THROW_ACTIVE_SYM, Terminator, Type,
};

const ENTRY: FuncId = FuncId(0);
const BODY: FuncId = FuncId(1);
const OFF: u64 = 32;

/// `entry(p) = body(p)`, probing the throw flag after the call.
fn entry() -> Function {
    let mut f = Function::new("__cm_P__e", Type::I64);
    let p = f.add_param(Type::Obj(StructId(0)), "p");
    let (b0, ok, thrown) = (f.add_block(), f.add_block(), f.add_block());
    let call = InstKind::Call(BODY, vec![Operand::Value(p)]);
    let r = f.append_inst(b0, call, Type::I64, None);
    let g = f.append_inst(
        b0,
        InstKind::GlobalRef(THROW_ACTIVE_SYM.into()),
        Type::Ptr,
        None,
    );
    let l = f.append_inst(
        b0,
        InstKind::Load(Type::I64, Operand::Value(g), 0),
        Type::I64,
        None,
    );
    let c = InstKind::ICmp(IPred::Ne, Operand::Value(l), Operand::ConstI64(0));
    let c = f.append_inst(b0, c, Type::Bool, None);
    f.set_term(
        b0,
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk: thrown,
            else_blk: ok,
        },
    );
    f.set_term(ok, Terminator::Ret(Some(Operand::Value(r))));
    f.append_void(thrown, InstKind::ThrowSignal);
    f.set_term(thrown, Terminator::Ret(Some(Operand::ConstI64(0))));
    f
}

/// `body(p)`: the length plus the first element, with eight values
/// live across a call to itself.
fn body() -> Function {
    let mut f = Function::new(".L__cm_P__e.pin", Type::I64);
    let p = f.add_param(Type::Obj(StructId(0)), "p");
    let b = f.add_block();
    let len = f.append_inst(b, InstKind::Pinned(PinWord::Len), Type::I64, None);
    let data = f.append_inst(b, InstKind::Pinned(PinWord::Data), Type::Ptr, None);
    let first = InstKind::Load(Type::I64, Operand::Value(data), 0);
    let mut acc = f.append_inst(b, first, Type::I64, None);
    let mut held = Vec::new();
    for k in 0..8 {
        let add = InstKind::BinOp(BinOp::Add, Operand::Value(acc), Operand::ConstI64(k));
        acc = f.append_inst(b, add, Type::I64, None);
        held.push(acc);
    }
    f.append_inst(
        b,
        InstKind::Call(BODY, vec![Operand::Value(p)]),
        Type::I64,
        None,
    );
    for h in held {
        let add = InstKind::BinOp(BinOp::Add, Operand::Value(acc), Operand::Value(h));
        acc = f.append_inst(b, add, Type::I64, None);
    }
    let sum = InstKind::BinOp(BinOp::Add, Operand::Value(acc), Operand::Value(len));
    let sum = f.append_inst(b, sum, Type::I64, None);
    f.set_term(b, Terminator::Ret(Some(Operand::Value(sum))));
    f
}

fn module() -> Module {
    Module {
        funcs: vec![entry(), body()],
        pin_regs: PinRegs {
            inner: vec![BODY],
            entries: vec![(ENTRY, BODY, OFF)],
            scalars: Vec::new(),
        },
        ..Default::default()
    }
}

fn words(m: &Module, f: FuncId) -> Vec<u32> {
    let c = compile_function_at(&m.funcs[f.0 as usize], f, &ModuleFacts::of(m));
    (c.bytes.as_chunks::<4>().0.iter())
        .map(|w| u32::from_le_bytes(*w))
        .collect()
}

#[test]
fn an_entry_loads_the_two_words_from_its_receiver_and_calls_its_body() {
    let m = module();
    let w = words(&m, ENTRY);
    let loads = [
        ldr_x_imm12(PIN_DATA, Gpr::X0, OFF as u32),
        ldr_x_imm12(PIN_LEN, PIN_DATA, 8),
        ldr_x_imm12(PIN_DATA, PIN_DATA, 32),
    ];
    let at = (w.windows(3))
        .position(|s| s == loads)
        .expect("the entry loads");
    // after the prologue saved the caller's x27 / x28 (an STP or STR of
    // them comes first), and the body is called
    let names = |i: &u32| [i & 0x1F, (i >> 10) & 0x1F];
    let saved: Vec<u32> = w[..at].iter().flat_map(names).collect();
    assert!(
        saved.contains(&27) && saved.contains(&28),
        "{:08x?}",
        &w[..at]
    );
    assert!(w.iter().any(|i| i & 0xFC00_0000 == 0x9400_0000));
}

#[test]
fn a_body_reads_the_registers_and_gives_no_other_value_them() {
    let m = module();
    let f = &m.funcs[BODY.0 as usize];
    let reserved = [PIN_LEN, PIN_DATA];
    let a = crate::linear_scan::allocate_linear_scan_reserving(f, Default::default(), &reserved);
    for (i, _) in f.values.iter().enumerate() {
        let v = torajs_core::ssa::ValueId(i as u32);
        if !a.contains(v) {
            continue;
        }
        let want = match i {
            1 => Some(PIN_LEN),
            2 => Some(PIN_DATA),
            _ => None,
        };
        match (a.of(v), want) {
            (Reg::Gpr(g), Some(w)) => assert_eq!(g, w),
            (Reg::Gpr(g), None) => assert!(!reserved.contains(&g), "%{i} in {g:?}"),
            (_, Some(_)) => panic!("a pinned value off its register"),
            _ => {}
        }
    }
    assert_eq!(a.used_callee_gpr_mask & ((1 << 27) | (1 << 28)), 0);
    // the body compiles, and its frame never saves them
    let w = words(&m, BODY);
    assert!(!w.is_empty());
}

#[test]
fn a_cluster_with_scalars_reserves_and_saves_their_registers_too() {
    let mut m = module();
    m.pin_regs.scalars = vec![(ENTRY, 2), (BODY, 1)];
    let plan = ModuleFacts::of(&m).pin;
    assert_eq!(plan.role(ENTRY), PinRole::Entry(OFF as u32, 2));
    assert_eq!(plan.role(BODY), PinRole::Inner(1));
    assert_eq!(plan.role(BODY).reserved(), [PIN_LEN, PIN_DATA, Gpr::X26]);
    let saved = [PIN_LEN, PIN_DATA, Gpr::X26, Gpr::X25];
    let mask = saved.iter().fold(0u32, |m, g| m | (1 << g.idx()));
    assert_eq!(plan.role(ENTRY).saved_mask(), mask);
    assert_eq!(home(PinWord::Scalar(0)), PIN_SCALARS[0]);
    assert_eq!(home(PinWord::Scalar(1)), Gpr::X25);
}

#[test]
fn a_cluster_without_scalars_keeps_the_two_registers() {
    let plan = ModuleFacts::of(&module()).pin;
    assert_eq!(plan.role(BODY), PinRole::Inner(0));
    assert_eq!(plan.role(BODY).reserved(), [PIN_LEN, PIN_DATA]);
    let mask = (1 << PIN_LEN.idx()) | (1 << PIN_DATA.idx());
    assert_eq!(plan.role(ENTRY).saved_mask(), mask);
}

/// `body(p)`: `%r = pinned s0`, `%t = %r + 5`, `%n = %r + 1`,
/// `pin_set s0, %n` (and a second read of `%n` when `twice`), ret `%t`.
fn scalar_body(twice: bool) -> Function {
    let mut f = Function::new(".L__cm_P__e.pin", Type::I64);
    f.add_param(Type::Obj(StructId(0)), "p");
    let b = f.add_block();
    let r = f.append_inst(b, InstKind::Pinned(PinWord::Scalar(0)), Type::I64, None);
    let five = InstKind::BinOp(BinOp::Add, Operand::Value(r), Operand::ConstI64(5));
    let t = f.append_inst(b, five, Type::I64, None);
    let one = InstKind::BinOp(BinOp::Add, Operand::Value(r), Operand::ConstI64(1));
    let n = f.append_inst(b, one, Type::I64, None);
    f.append_void(b, InstKind::PinSet(PinWord::Scalar(0), Operand::Value(n)));
    let ret = match twice {
        true => {
            let s = InstKind::BinOp(BinOp::Add, Operand::Value(t), Operand::Value(n));
            f.append_inst(b, s, Type::I64, None)
        }
        false => t,
    };
    f.set_term(b, Terminator::Ret(Some(Operand::Value(ret))));
    f
}

fn scalar_module(twice: bool) -> Module {
    let mut m = module();
    m.funcs[BODY.0 as usize] = scalar_body(twice);
    m.pin_regs.scalars = vec![(ENTRY, 1), (BODY, 1)];
    m
}

/// Whether `w` holds a `mov x26, <any>`.
fn moves_into_x26(w: &[u32]) -> bool {
    let mov = mov_x_reg(Gpr::X26, Gpr::X0);
    w.iter().any(|i| i & 0xFFE0_FFFF == mov)
}

#[test]
fn a_scalar_bumped_by_one_is_one_add_in_its_register() {
    let w = words(&scalar_module(false), BODY);
    assert!(w.contains(&add_imm(Gpr::X26, Gpr::X26, 1)), "{w:08x?}");
    // `%t` reads the register before the bump
    let five = add_imm(Gpr::X0, Gpr::X26, 5);
    assert!(w.iter().any(|i| i & 0xFFFF_FFE0 == five), "{w:08x?}");
    assert!(!moves_into_x26(&w), "{w:08x?}");
}

#[test]
fn an_entry_loads_a_scalar_straight_into_its_register_and_stores_it_back() {
    let mut m = scalar_module(false);
    let f = &mut m.funcs[ENTRY.0 as usize];
    let p = Operand::Value(f.params[0]);
    let l = f.insert_inst(
        torajs_core::ssa::BlockId(0),
        0,
        InstKind::Load(Type::I64, p, 40),
        Type::I64,
    );
    let set = InstKind::PinSet(PinWord::Scalar(0), Operand::Value(l));
    f.blocks[0].insts.insert(1, inst(set));
    let q = f.insert_inst(
        torajs_core::ssa::BlockId(0),
        3,
        InstKind::Pinned(PinWord::Scalar(0)),
        Type::I64,
    );
    let back = InstKind::Store(Operand::Value(q), p, 40);
    f.blocks[0].insts.insert(4, inst(back));
    let w = words(&m, ENTRY);
    let at = |i: &u32| i & 0xFFFF_FC1F;
    let ldr26 = at(&ldr_x_imm12(Gpr::X26, Gpr::X0, 40));
    assert!(w.iter().any(|i| at(i) == ldr26), "{w:08x?}");
    assert!(!moves_into_x26(&w), "{w:08x?}");
    // the store back reads x26 (STR is LDR with bit 22 clear)
    let str26 = ldr_x_imm12(Gpr::X26, Gpr::X0, 40) & !(1 << 22);
    assert!(w.iter().any(|i| at(i) == at(&str26)), "{w:08x?}");
}

fn inst(kind: InstKind) -> torajs_core::ssa::Inst {
    torajs_core::ssa::Inst {
        result: None,
        kind,
        origin: None,
    }
}

#[test]
fn a_scalar_value_read_again_is_moved_into_the_register() {
    let w = words(&scalar_module(true), BODY);
    assert!(!w.contains(&add_imm(Gpr::X26, Gpr::X26, 1)), "{w:08x?}");
    assert!(moves_into_x26(&w), "{w:08x?}");
}

#[test]
fn a_read_used_past_a_fused_bump_is_copied_out_before_it() {
    // `%r = pinned s0`, `%c = copy %r`, `%n = %r + 1`, `pin_set s0, %n`,
    // ret `%c`: the copy leaves x26 before the add writes it
    let mut f = Function::new(".L__cm_P__e.pin", Type::I64);
    f.add_param(Type::Obj(StructId(0)), "p");
    let b = f.add_block();
    let r = f.append_inst(b, InstKind::Pinned(PinWord::Scalar(0)), Type::I64, None);
    let c = f.append_inst(
        b,
        InstKind::Copy(Type::I64, Operand::Value(r)),
        Type::I64,
        None,
    );
    let one = InstKind::BinOp(BinOp::Add, Operand::Value(r), Operand::ConstI64(1));
    let n = f.append_inst(b, one, Type::I64, None);
    f.append_void(b, InstKind::PinSet(PinWord::Scalar(0), Operand::Value(n)));
    f.set_term(b, Terminator::Ret(Some(Operand::Value(c))));
    let mut m = scalar_module(false);
    m.funcs[BODY.0 as usize] = f;
    let w = words(&m, BODY);
    let bump = w.iter().position(|i| *i == add_imm(Gpr::X26, Gpr::X26, 1));
    let out = mov_x_reg(Gpr::X0, Gpr::X26) & 0xFFFF_FFE0;
    let copy = w.iter().position(|i| i & 0xFFFF_FFE0 == out);
    assert!(
        matches!((copy, bump), (Some(c), Some(a)) if c < a),
        "{w:08x?}"
    );
}
