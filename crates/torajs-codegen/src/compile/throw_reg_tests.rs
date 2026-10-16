use super::throw_reg::Role;
use super::{ModuleFacts, compile_function_at};
use crate::enc::{movz_imm, ret};
use crate::reg::Gpr;
use crate::reloc::{Reloc, RelocKind};
use torajs_core::ssa::{
    BinOp, BlockId, FuncId, Function, IPred, InstKind, Module, Operand, THROW_ACTIVE_SYM,
    Terminator, Type, ValueId,
};

const THROWER: FuncId = FuncId(0);
const PASSER: FuncId = FuncId(1);
const MAIN: FuncId = FuncId(2);
const TAKEN: FuncId = FuncId(3);
const TAKE: FuncId = FuncId(4);
const CATCHER: FuncId = FuncId(5);
const TAIL_TAKEN: FuncId = FuncId(6);
const TAIL_THROWER: FuncId = FuncId(7);
const TAIL_TO_TAKEN: FuncId = FuncId(8);

/// `%c = icmp ne (load @throw_active), 0; cond_br %c, thrown, cont`.
fn probe(f: &mut Function, b: BlockId, thrown: BlockId, cont: BlockId) {
    let g = f.append_inst(
        b,
        InstKind::GlobalRef(THROW_ACTIVE_SYM.into()),
        Type::Ptr,
        None,
    );
    let l = f.append_inst(
        b,
        InstKind::Load(Type::I64, Operand::Value(g), 0),
        Type::I64,
        None,
    );
    let c = f.append_inst(
        b,
        InstKind::ICmp(IPred::Ne, Operand::Value(l), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    f.set_term(
        b,
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk: thrown,
            else_blk: cont,
        },
    );
}

/// A block that hands a throw on.
fn propagate(f: &mut Function) -> BlockId {
    let b = f.add_block();
    f.append_void(b, InstKind::ThrowSignal);
    f.set_term(b, Terminator::Ret(Some(Operand::ConstI64(0))));
    b
}

fn call(f: &mut Function, b: BlockId, g: FuncId) -> ValueId {
    f.append_inst(
        b,
        InstKind::Call(g, vec![Operand::ConstI64(1)]),
        Type::I64,
        None,
    )
}

/// `x != 0 ? <throw> : x`
fn thrower() -> Function {
    let mut f = Function::new("thrower", Type::I64);
    let b = f.add_block();
    let x = f.add_param(Type::I64, "x");
    let thrown = propagate(&mut f);
    let ok = f.add_block();
    let c = f.append_inst(
        b,
        InstKind::ICmp(IPred::Ne, Operand::Value(x), Operand::ConstI64(0)),
        Type::Bool,
        None,
    );
    f.set_term(
        b,
        Terminator::CondBr {
            cond: Operand::Value(c),
            then_blk: thrown,
            else_blk: ok,
        },
    );
    f.set_term(ok, Terminator::Ret(Some(Operand::Value(x))));
    f
}

/// `thrower(1) + 1`, a throw passed on.
fn passer(name: &str, callee: FuncId) -> Function {
    let mut f = Function::new(name, Type::I64);
    let b = f.add_block();
    f.add_param(Type::I64, "x");
    let r = call(&mut f, b, callee);
    let thrown = propagate(&mut f);
    let ok = f.add_block();
    probe(&mut f, b, thrown, ok);
    let s = f.append_inst(
        ok,
        InstKind::BinOp(BinOp::Add, Operand::Value(r), Operand::ConstI64(1)),
        Type::I64,
        None,
    );
    f.set_term(ok, Terminator::Ret(Some(Operand::Value(s))));
    f
}

/// `try { thrower(1) } catch { take }`, then a call that may not throw.
fn catcher() -> Function {
    let mut f = Function::new("catcher", Type::I64);
    let b = f.add_block();
    f.add_param(Type::I64, "x");
    call(&mut f, b, THROWER);
    let caught = f.add_block();
    let join = f.add_block();
    probe(&mut f, b, caught, join);
    f.append_inst(caught, InstKind::Call(TAKE, Vec::new()), Type::I64, None);
    f.set_term(caught, Terminator::Br(join));
    let r = call(&mut f, join, THROWER);
    f.set_term(join, Terminator::Ret(Some(Operand::Value(r))));
    f
}

/// `return callee(1)` with a throw of its own on another arm.
fn tail(name: &str, callee: FuncId) -> Function {
    let mut f = Function::new(name, Type::I64);
    let b = f.add_block();
    let x = f.add_param(Type::I64, "x");
    let thrown = propagate(&mut f);
    let go = f.add_block();
    f.set_term(
        b,
        Terminator::CondBr {
            cond: Operand::Value(x),
            then_blk: thrown,
            else_blk: go,
        },
    );
    let r = call(&mut f, go, callee);
    f.set_term(go, Terminator::Ret(Some(Operand::Value(r))));
    f
}

fn module() -> Module {
    let mut m = Module::default();
    m.funcs.push(thrower());
    m.funcs.push(passer("passer", THROWER));
    let mut main = passer("main", PASSER);
    // two fn values, ahead of the call so the probe stays at the tail
    for g in [TAKEN, TAIL_TAKEN] {
        main.append_inst(BlockId(0), InstKind::FnAddr(g), Type::Ptr, None);
        let addr = main.blocks[0].insts.pop().expect("just appended");
        main.blocks[0].insts.insert(0, addr);
    }
    m.funcs.push(main);
    m.funcs.push(passer("taken", THROWER));
    m.funcs
        .push(Function::new("__torajs_throw_take", Type::I64));
    m.funcs.push(catcher());
    m.funcs.push(thrower());
    m.funcs[TAIL_TAKEN.0 as usize].name = "tail_taken".into();
    m.funcs.push(tail("tail_thrower", THROWER));
    m.funcs.push(tail("tail_to_taken", TAIL_TAKEN));
    m
}

fn compiled(m: &Module, f: FuncId) -> (Vec<u32>, Vec<Reloc>) {
    let facts = ModuleFacts::of(m);
    let c = compile_function_at(&m.funcs[f.0 as usize], f, &facts);
    let w = (c.bytes.as_chunks::<4>().0.iter())
        .map(|w| u32::from_le_bytes(*w))
        .collect();
    (w, c.relocs)
}

fn words(m: &Module, f: FuncId) -> Vec<u32> {
    compiled(m, f).0
}

fn is_cb_x21(w: u32) -> bool {
    w & 0xFE00_001F == 0xB400_0015
}

fn is_adrp(w: u32) -> bool {
    w & 0x9F00_0000 == 0x9000_0000
}

fn zeroes(w: &[u32]) -> usize {
    w.iter().filter(|&&x| x == movz_imm(Gpr::X21, 0, 0)).count()
}

#[test]
fn roles_follow_who_can_reach_whom() {
    let m = module();
    let plan = ModuleFacts::of(&m).throw_reg;
    let role = |f: FuncId| plan.role(f);
    let r = |signals, reads, guards| Role {
        signals,
        reads,
        guards,
    };
    assert_eq!(role(THROWER), r(true, false, false));
    assert_eq!(role(PASSER), r(true, true, false));
    // main and a function whose address is taken never signal, and are
    // entered with x21 holding someone else's value
    assert_eq!(role(MAIN), r(false, true, true));
    assert_eq!(role(TAKEN), r(false, true, true));
    assert_eq!(role(TAIL_TAKEN), r(false, false, false));
}

#[test]
fn a_reader_probes_x21_and_a_signaller_sets_it_after_the_epilogue() {
    let m = module();
    let w = words(&m, PASSER);
    assert!(w.iter().any(|&x| is_cb_x21(x)), "{w:#x?}");
    assert!(!w.iter().any(|&x| is_adrp(x)), "{w:#x?}");
    let set = movz_imm(Gpr::X21, 1, 0);
    assert!(w.windows(2).any(|p| p == [set, ret(Gpr::X30)]), "{w:#x?}");
}

#[test]
fn a_guarded_reader_zeroes_x21_after_saving_it() {
    let m = module();
    let w = words(&m, MAIN);
    let zero = w.iter().position(|&x| x == movz_imm(Gpr::X21, 0, 0));
    // STR / STP of x21 (Rt or Rt2 = 21) against SP
    let save = w
        .iter()
        .position(|&x| (x >> 5) & 0x1F == 31 && (x & 0x1F == 21 || (x >> 10) & 0x1F == 21));
    assert!(save.is_some_and(|s| zero.is_some_and(|z| s < z)), "{w:#x?}");
    assert!(w.iter().any(|&x| is_cb_x21(x)), "{w:#x?}");
}

#[test]
fn a_caught_throw_zeroes_x21_once_on_its_way_in() {
    let m = module();
    let w = words(&m, CATCHER);
    assert_eq!(zeroes(&w), 1, "{w:#x?}");
    // a take before any call: the caller may have made this call with a
    // throw pending and x21 set, so the take zeroes it all the same
    let mut m2 = module();
    let f = &mut m2.funcs[CATCHER.0 as usize];
    f.blocks[0].insts[0].kind = InstKind::Call(TAKE, Vec::new());
    assert_eq!(zeroes(&words(&m2, CATCHER)), 1);
}

#[test]
fn a_tail_call_keeps_the_register_protocol_on_both_sides() {
    let m = module();
    // the word at the call-site relocation: a B (sibling call) or a BL
    let leaves_by_b = |f: FuncId| {
        let (w, relocs) = compiled(&m, f);
        let site = (relocs.iter())
            .find(|r| matches!(r.kind, RelocKind::CallSite { .. }))
            .expect("one call");
        w[site.byte_offset as usize / 4] & 0xFC00_0000 == 0x1400_0000
    };
    // both signal: a sibling call
    assert!(leaves_by_b(TAIL_THROWER));
    // a callee that does not signal would hand its throw back in the
    // flag alone, to a caller that probes x21
    assert!(!leaves_by_b(TAIL_TO_TAKEN));
}
