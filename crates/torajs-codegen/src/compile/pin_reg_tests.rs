use super::pin_reg::{PIN_DATA, PIN_LEN, PIN_SCALARS, PinRole, home};
use super::{ModuleFacts, compile_function_at};
use crate::enc::ldr_x_imm12;
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
