use super::throw_reg::Role;
use super::{ModuleFacts, compile_function_at};
use crate::enc::{movz_imm, ret};
use crate::reg::Gpr;
use crate::reloc::{CallTarget, Reloc, RelocKind};
use torajs_core::ssa::{
    BinOp, BlockId, FuncId, Function, IPred, Inst, InstKind, Module, Operand, THROW_ACTIVE_SYM,
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
    ret_probed(&mut f, join, r);
    f
}

/// Probe the call that left `r` in `b`: pass a throw on, else return `r`.
fn ret_probed(f: &mut Function, b: BlockId, r: ValueId) {
    let thrown = propagate(f);
    let done = f.add_block();
    probe(f, b, thrown, done);
    f.set_term(done, Terminator::Ret(Some(Operand::Value(r))));
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
    ret_probed(&mut f, go, r);
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

/// `callee(1)` with no probe after it, as the front end leaves a call
/// it judges unable to throw.
fn lax(callee: FuncId) -> Function {
    let mut f = Function::new("lax", Type::I64);
    let b = f.add_block();
    f.add_param(Type::I64, "x");
    let r = call(&mut f, b, callee);
    f.set_term(b, Terminator::Ret(Some(Operand::Value(r))));
    f
}

#[test]
fn a_call_left_without_a_probe_keeps_its_callee_off_x21() {
    let r = |signals, reads, guards| Role {
        signals,
        reads,
        guards,
    };
    // should the call throw after all, its caller runs on with x21 set:
    // a take in a later callee keeps x21 and clears the flag, and a
    // later signalling call that returns normally would read as a throw
    let mut m = module();
    m.funcs.push(lax(THROWER));
    let plan = ModuleFacts::of(&m).throw_reg;
    assert_eq!(plan.role(THROWER), r(false, false, false));
    assert_eq!(plan.role(PASSER), r(true, false, false));
    // a reader passes a throw on with x21 set: guarded, it puts back
    // the x21 it found
    let mut m = module();
    m.funcs.push(lax(PASSER));
    let plan = ModuleFacts::of(&m).throw_reg;
    assert_eq!(plan.role(THROWER), r(true, false, false));
    assert_eq!(plan.role(PASSER), r(false, true, true));
    assert_eq!(plan.role(MAIN), r(false, false, false));
}

/// Whether x21 is zeroed after the call to the take (else before it).
fn zeroes_after_take(w: &[u32], relocs: &[Reloc]) -> bool {
    let site = (relocs.iter())
        .find(|r| {
            matches!(&r.kind,
                RelocKind::CallSite { target: CallTarget::Func(f) } if *f == TAKE)
        })
        .expect("a take");
    let zero = (w.iter().position(|&x| x == movz_imm(Gpr::X21, 0, 0))).expect("a zeroing");
    zero > site.byte_offset as usize / 4
}

/// `r = thrower(1)`, its throw edge a block of coalesced copies into
/// a catch that takes: the branch lands past that block, so x21 is
/// zeroed after the take instead. `x`, read after the take, takes a
/// callee-saved register, and the copy shares one with its source.
fn hollow_catcher() -> Function {
    let mut f = Function::new("hollow_catcher", Type::I64);
    let b = f.add_block();
    let x = f.add_param(Type::I64, "x");
    let r = call(&mut f, b, THROWER);
    let (caught, catch, cont) = (f.add_block(), f.add_block(), f.add_block());
    probe(&mut f, b, caught, cont);
    let j = f.append_inst(
        caught,
        InstKind::Copy(Type::I64, Operand::Value(r)),
        Type::I64,
        None,
    );
    f.set_term(caught, Terminator::Br(catch));
    f.append_inst(catch, InstKind::Call(TAKE, Vec::new()), Type::I64, None);
    let keep = InstKind::BinOp(BinOp::Add, Operand::Value(x), Operand::ConstI64(0));
    f.append_inst(catch, keep, Type::I64, None);
    f.set_term(catch, Terminator::Ret(Some(Operand::Value(j))));
    f.set_term(cont, Terminator::Ret(Some(Operand::Value(r))));
    f
}

#[test]
fn a_throw_edge_through_a_hollow_block_zeroes_after_the_take() {
    let mut m = module();
    m.funcs.push(hollow_catcher());
    let (w, relocs) = compiled(&m, FuncId(m.funcs.len() as u32 - 1));
    assert_eq!(zeroes(&w), 1, "{w:#x?}");
    assert!(zeroes_after_take(&w, &relocs), "{w:#x?}");
}

/// A take before any call or probe, then `thrower(1)`.
fn early_taker(name: &str) -> Function {
    let mut f = Function::new(name, Type::I64);
    let b = f.add_block();
    f.add_param(Type::I64, "x");
    f.append_inst(b, InstKind::Call(TAKE, Vec::new()), Type::I64, None);
    let r = call(&mut f, b, THROWER);
    ret_probed(&mut f, b, r);
    f
}

#[test]
fn an_unguarded_reader_zeroes_after_a_take_before_any_probe() {
    let mut m = module();
    m.funcs.push(early_taker("early_taker"));
    let id = FuncId(m.funcs.len() as u32 - 1);
    assert!(!ModuleFacts::of(&m).throw_reg.role(id).guards);
    let (w, relocs) = compiled(&m, id);
    assert_eq!(zeroes(&w), 1, "{w:#x?}");
    assert!(zeroes_after_take(&w, &relocs), "{w:#x?}");
    // guarded, it is entered with x21 zeroed, and the take leaves it so
    let mut m = module();
    m.funcs.push(early_taker("taken_twin"));
    let id = FuncId(m.funcs.len() as u32 - 1);
    take_address(&mut m, id);
    assert!(ModuleFacts::of(&m).throw_reg.role(id).guards);
    let (w, relocs) = compiled(&m, id);
    assert_eq!(zeroes(&w), 1, "{w:#x?}");
    assert!(!zeroes_after_take(&w, &relocs), "{w:#x?}");
}

/// Main takes the address of `id`, as the runtime holds a boxed entry.
fn take_address(m: &mut Module, id: FuncId) {
    let addr = Inst {
        result: None,
        kind: InstKind::FnAddr(id),
        origin: None,
    };
    m.funcs[MAIN.0 as usize].blocks[0].insts.insert(0, addr);
}

/// `r = callee(1)` left without a probe, then `then(1)` probed if
/// given, else `return r`.
fn wrapper(callee: FuncId, then: Option<FuncId>) -> Function {
    let mut f = Function::new("wrapper", Type::I64);
    let b = f.add_block();
    f.add_param(Type::I64, "x");
    let r = call(&mut f, b, callee);
    match then {
        Some(g) => {
            let s = call(&mut f, b, g);
            ret_probed(&mut f, b, s);
        }
        None => f.set_term(b, Terminator::Ret(Some(Operand::Value(r)))),
    }
    f
}

#[test]
fn a_call_left_without_a_probe_is_harmless_when_nothing_reads_x21_after_it() {
    // a guarded caller that returns right after puts x21 back as found
    let mut m = module();
    m.funcs.push(wrapper(THROWER, None));
    let id = FuncId(m.funcs.len() as u32 - 1);
    take_address(&mut m, id);
    let plan = ModuleFacts::of(&m).throw_reg;
    assert!(plan.role(THROWER).signals);
    assert!(plan.role(id).guards);
    // a later call that may leave x21 set is probed by reading x21
    let mut m = module();
    m.funcs.push(wrapper(THROWER, Some(PASSER)));
    let id = FuncId(m.funcs.len() as u32 - 1);
    take_address(&mut m, id);
    let plan = ModuleFacts::of(&m).throw_reg;
    assert!(!plan.role(THROWER).signals);
    assert!(plan.role(PASSER).signals);
}
