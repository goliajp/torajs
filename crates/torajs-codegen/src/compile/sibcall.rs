//! Sibling calls: a block whose call's result is the function's
//! return value leaves through the callee. The arguments go to their
//! registers, the epilogue releases this frame, and a `B` to the callee
//! (a `BR` through a fn pointer) stands for `BL` + `RET`, so the callee
//! returns straight to this function's caller and a recursion in tail
//! position runs in constant stack (ECMA-262 §15.10, proper tail
//! calls).
//!
//! A block qualifies when, as codegen sees it:
//! - it ends in `%r = call f(args)` (or `call_indirect`) and `ret %r`,
//!   or in that call, `%x = copy %r` and a jump to a block that is just
//!   `ret %x` (the shape a call in tail position takes once a return
//!   joins several paths), or in that call and the pending-throw probe
//!   whose continue edge returns `%r` that way and whose propagate edge
//!   returns a constant. The probe only hands a throw the callee left
//!   pending to this function's caller, which tests the flag itself
//!   when the callee returns to it directly; and a callee that throws
//!   returns a value nobody reads. A probe whose propagate edge does
//!   anything else (a catch, a finally, a scope drop) breaks the shape
//! - between the call and the return there is nothing else but
//!   releases of values proven immediate, which touch nothing. A
//!   release of an argument or a local the caller owns breaks the
//!   shape, so those calls keep `BL`
//! - `f` has a body (an extern or intrinsic call site is rewritten or
//!   opened inline downstream) and returns this function's type; a fn
//!   pointer's signature returns it
//! - every argument travels in a register: a stack argument would sit
//!   in the frame the epilogue releases
//! - the function takes no stack parameter of its own, and no address
//!   in its frame is used as anything but the address of a load or
//!   store: an argument pointing into the frame (a call's argv buffer)
//!   would dangle once the epilogue releases it
//!
//! Registers saved around the call hold values only what follows the
//! call reads, and a sibling call runs none of it.

use torajs_core::ssa::id_hash::{HashMap, HashSet};
use torajs_core::ssa::{
    ANY_SLOT_TAG_HEAP, Block, BlockId, FuncId, Function, Inst, InstKind, Operand, Terminator,
    ValueId, visit_value_operands,
};

use super::call_args::{has_stack_lane, pass_args};
use super::operand::materialize_operand_gpr;
use super::throw_probe::is_throw_probe;
use super::{ModuleFacts, landing, write_u32};
use crate::enc::{b_imm26, br_reg, mov_x_reg};
use crate::frame::FrameLayout;
use crate::reg::Gpr;
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

pub(super) const ARITH_PAIR: &str = "__torajs_anyv_arith_pair";

/// Where a fn pointer waits for the `BR`: IP1 is no value's home, and
/// neither the argument setup nor the epilogue writes it.
const FN_PTR_PARK: Gpr = Gpr::X17;

enum Target<'a> {
    Func(FuncId),
    Ptr(&'a Operand),
}

pub(super) struct Sibling<'a> {
    /// index of the call in the block; what follows it is not emitted
    pub(super) at: usize,
    target: Target<'a>,
    args: &'a [Operand],
}

/// The block's sibling call, if it has one.
pub(super) fn plan<'a>(
    func: &Function,
    block: &'a Block,
    blocks_by_id: &HashMap<u32, &Block>,
    alloc: &Assignment,
    facts: &ModuleFacts,
) -> Option<Sibling<'a>> {
    let at = block
        .insts
        .iter()
        .rposition(|i| matches!(i.kind, InstKind::Call(..) | InstKind::CallIndirect(..)))?;
    let call = &block.insts[at];
    let result = call.result?;
    let (target, args, ret, param_types) = match &call.kind {
        InstKind::Call(callee, args) => (
            Target::Func(*callee),
            args,
            facts.fn_rets.get(callee.0 as usize).copied().flatten()?,
            facts.fn_sigs.get(callee.0 as usize).map(Vec::as_slice),
        ),
        // the argument setup of an indirect call trusts the operands'
        // own classes (call::emit_call_indirect); the lanes are judged
        // the same way
        InstKind::CallIndirect(sig, fn_ptr, args) => (
            Target::Ptr(fn_ptr),
            args,
            facts.sig_rets.get(sig.0 as usize).copied()?,
            None,
        ),
        _ => return None,
    };
    let shape = Shape {
        func,
        blocks_by_id,
        facts,
    };
    (ret == func.ret
        && func.value_type(result) == func.ret
        && alloc.stack_param_entry_loads.is_empty()
        && !has_stack_lane(args, alloc, param_types)
        && shape.leaves_with(block, at, result)
        && !frame_escapes(func))
    .then_some(Sibling { at, target, args })
}

struct Shape<'a, 'b> {
    func: &'a Function,
    blocks_by_id: &'a HashMap<u32, &'b Block>,
    facts: &'a ModuleFacts,
}

impl Shape<'_, '_> {
    /// Whether everything after the call at `at` returns its result
    /// `r` and does nothing else.
    fn leaves_with(&self, block: &Block, at: usize, r: ValueId) -> bool {
        let tail = &block.insts[at + 1..];
        match &block.term {
            Terminator::Ret(Some(Operand::Value(v))) => *v == r && self.all_inert(tail),
            Terminator::Br(t) => match tail.split_last() {
                Some((copy, rest)) => {
                    self.all_inert(rest)
                        && self
                            .copy_of(copy, r)
                            .is_some_and(|x| self.returns_through(*t, x))
                }
                None => false,
            },
            Terminator::CondBr {
                cond: Operand::Value(c),
                then_blk,
                else_blk,
            } => match tail {
                [rest @ .., g, l, test] => {
                    self.all_inert(rest)
                        && is_throw_probe(g, l, test, *c)
                        && self.returns_through(*else_blk, r)
                        && self.returns_constant(*then_blk)
                }
                _ => false,
            },
            _ => false,
        }
    }

    /// Whether jumping to `target` with `v` in hand returns `v`: the
    /// target is just `ret v`, or a join that copies `v` into the next
    /// join's value and jumps on (one join per enclosing return, once
    /// inlining has nested a tail call's body inside another's).
    fn returns_through(&self, mut target: BlockId, mut v: ValueId) -> bool {
        for _ in 0..8 {
            let Some(j) = self.landing(target) else {
                return false;
            };
            match &j.term {
                Terminator::Ret(Some(Operand::Value(x))) => {
                    return *x == v && self.all_inert(&j.insts);
                }
                Terminator::Br(next) => {
                    let Some((copy, rest)) = j.insts.split_last() else {
                        return false;
                    };
                    let Some(w) = self.copy_of(copy, v).filter(|_| self.all_inert(rest)) else {
                        return false;
                    };
                    v = w;
                    target = *next;
                }
                _ => return false,
            }
        }
        false
    }

    /// The propagate edge of a probe: nothing but a return of a
    /// constant.
    fn returns_constant(&self, target: BlockId) -> bool {
        self.landing(target).is_some_and(|j| {
            matches!(&j.term, Terminator::Ret(Some(op)) if !matches!(op, Operand::Value(_)))
                && self.all_inert(&j.insts)
        })
    }

    fn landing(&self, target: BlockId) -> Option<&Block> {
        let id = landing::resolve_forwarded_target(self.blocks_by_id, target);
        self.blocks_by_id.get(&id.0).copied()
    }

    /// `%w = copy %v` at the return type: `w`.
    fn copy_of(&self, inst: &Inst, v: ValueId) -> Option<ValueId> {
        match &inst.kind {
            InstKind::Copy(ty, Operand::Value(r)) if *r == v && *ty == self.func.ret => inst.result,
            _ => None,
        }
    }

    fn all_inert(&self, insts: &[Inst]) -> bool {
        insts.iter().all(|i| match &i.kind {
            InstKind::RcDecAny(Operand::Value(x)) => self.is_immediate(*x),
            InstKind::ThrowSignal => true,
            _ => false,
        })
    }

    /// Whether `x` is never a heap cell, so releasing it touches
    /// nothing: a box of an immediate tag, or `any` arithmetic with an
    /// operand of an immediate tag. ToNumeric of a primitive that is
    /// not a BigInt is a Number, and a BigInt beside a Number throws
    /// (§13.15.3), so the result is a number, or `undefined` with the
    /// throw pending.
    fn is_immediate(&self, x: ValueId) -> bool {
        let Some(def) = self.def_of(x) else {
            return false;
        };
        match def {
            InstKind::BoxAny(t, _) => *t != ANY_SLOT_TAG_HEAP,
            InstKind::BoxInt32(_) | InstKind::BoxF64(_) => true,
            InstKind::Call(f, args) if Some(*f) == self.facts.arith_pair => {
                let imm_tag = |i: usize| matches!(args.get(i), Some(Operand::ConstI64(t)) if *t != ANY_SLOT_TAG_HEAP);
                args.len() == 5 && (imm_tag(1) || imm_tag(3))
            }
            _ => false,
        }
    }

    fn def_of(&self, x: ValueId) -> Option<&InstKind> {
        let mut defs = (self.func.blocks.iter())
            .flat_map(|b| b.insts.iter())
            .filter(|i| i.result == Some(x));
        let def = defs.next()?;
        // a value a join assigns on several edges has no one def
        defs.next().is_none().then_some(&def.kind)
    }
}

/// Whether an address in the frame is used other than to load or
/// store through it.
fn frame_escapes(func: &Function) -> bool {
    let slots: HashSet<ValueId> = (func.blocks.iter())
        .flat_map(|b| b.insts.iter())
        .filter(|i| matches!(i.kind, InstKind::Alloca(_) | InstKind::AllocaBytes(_)))
        .filter_map(|i| i.result)
        .collect();
    if slots.is_empty() {
        return false;
    }
    let is_slot = |op: &Operand| matches!(op, Operand::Value(v) if slots.contains(v));
    let insts = func.blocks.iter().flat_map(|b| b.insts.iter());
    let mut escaped = false;
    for i in insts {
        match &i.kind {
            InstKind::Load(_, _, _) => {}
            InstKind::Store(val, _, _) => escaped |= is_slot(val),
            k => visit_value_operands(k, |v| escaped |= slots.contains(&v)),
        }
    }
    escaped
        || func.blocks.iter().any(|b| match &b.term {
            Terminator::Ret(Some(op)) | Terminator::CondBr { cond: op, .. } => is_slot(op),
            _ => false,
        })
}

/// Arguments, epilogue, then `B` to the callee with the call-site
/// relocation a `BL` would carry, or `BR` through the fn pointer.
pub(super) fn emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    s: &Sibling<'_>,
    alloc: &Assignment,
    facts: &ModuleFacts,
    frame: &FrameLayout,
) {
    match s.target {
        Target::Func(callee) => {
            let param_types = facts.fn_sigs.get(callee.0 as usize).map(Vec::as_slice);
            pass_args(bytes, s.args, alloc, param_types);
            frame.emit_epilogue(bytes);
            relocs.push(Reloc {
                byte_offset: bytes.len() as u32,
                kind: RelocKind::CallSite {
                    target: CallTarget::Func(callee),
                },
            });
            write_u32(bytes, b_imm26(0));
        }
        Target::Ptr(fn_ptr) => {
            // parked first: its home may be an argument register the
            // setup overwrites, or a callee-saved one the epilogue does
            let r = materialize_operand_gpr(bytes, fn_ptr, FN_PTR_PARK, alloc);
            if r != FN_PTR_PARK {
                write_u32(bytes, mov_x_reg(FN_PTR_PARK, r));
            }
            pass_args(bytes, s.args, alloc, None);
            frame.emit_epilogue(bytes);
            write_u32(bytes, br_reg(FN_PTR_PARK));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{ModuleFacts, compile_function_in};
    use crate::enc::{blr_reg, br_reg};
    use crate::reg::Gpr;
    use crate::reloc::{CallTarget, Reloc, RelocKind};
    use torajs_core::ssa::{
        BlockId, FuncId, Function, IPred, InstKind, Operand, SigId, THROW_ACTIVE_SYM, Terminator,
        Type, ValueId,
    };

    const CALLEE: FuncId = FuncId(0);
    const ARITH: FuncId = FuncId(1);
    const BR_X17: u32 = 0xD61F_0220;

    fn facts() -> ModuleFacts {
        ModuleFacts {
            fn_sigs: vec![vec![Type::Ptr, Type::Any], Vec::new()],
            fn_rets: vec![Some(Type::Any), None],
            sig_rets: vec![Type::Any],
            arith_pair: Some(ARITH),
            ..ModuleFacts::default()
        }
    }

    fn compile(f: &Function) -> (Vec<u32>, Vec<Reloc>) {
        let c = compile_function_in(f, &facts());
        let w = (c.bytes.as_chunks::<4>().0.iter())
            .map(|w| u32::from_le_bytes(*w))
            .collect();
        (w, c.relocs)
    }

    /// Appends the pending-throw probe to `b` and branches on it.
    fn probe(f: &mut Function, b: BlockId, on_throw: BlockId, ok: BlockId) {
        let sym = InstKind::GlobalRef(THROW_ACTIVE_SYM.into());
        let g = f.append_inst(b, sym, Type::Ptr, None);
        let load = InstKind::Load(Type::I64, Operand::Value(g), 0);
        let l = f.append_inst(b, load, Type::I64, None);
        let test = InstKind::ICmp(IPred::Ne, Operand::Value(l), Operand::ConstI64(0));
        let c = f.append_inst(b, test, Type::Bool, None);
        let cond = Operand::Value(c);
        let (then_blk, else_blk) = (on_throw, ok);
        f.set_term(
            b,
            Terminator::CondBr {
                cond,
                then_blk,
                else_blk,
            },
        );
    }

    type ArgOf = fn(&mut Function, BlockId, ValueId) -> Operand;
    type After = fn(&mut Function, BlockId, ValueId, ValueId);

    /// `t(env, n)`: `%a = n - 1` on `any`, a call with `(env, arg)`
    /// through the fn pointer at `env + 8` (to CALLEE when `direct`),
    /// the probe, `after` on both of its edges, then `ret %r` on the
    /// continue edge and `ret 0` on the propagate edge.
    fn tail_shape(direct: bool, arg: ArgOf, after: After) -> Function {
        let mut f = Function::new("t", Type::Any);
        let env = f.add_param(Type::Ptr, "env");
        let n = f.add_param(Type::Any, "n");
        let (b0, ok, thrown) = (f.add_block(), f.add_block(), f.add_block());
        let sub = [0, 4, -1, 2, 1].map(Operand::ConstI64);
        let mut sub = sub.to_vec();
        sub[2] = Operand::Value(n);
        let a = f.append_inst(b0, InstKind::Call(ARITH, sub), Type::Any, None);
        let args = vec![Operand::Value(env), arg(&mut f, b0, a)];
        let call = if direct {
            InstKind::Call(CALLEE, args)
        } else {
            let fn_ptr = InstKind::Load(Type::Ptr, Operand::Value(env), 8);
            let p = f.append_inst(b0, fn_ptr, Type::Ptr, None);
            InstKind::CallIndirect(SigId(0), Operand::Value(p), args)
        };
        let r = f.append_inst(b0, call, Type::Any, None);
        probe(&mut f, b0, thrown, ok);
        after(&mut f, ok, n, a);
        after(&mut f, thrown, n, a);
        f.set_term(ok, Terminator::Ret(Some(Operand::Value(r))));
        f.set_term(thrown, Terminator::Ret(Some(Operand::ConstI64(0))));
        f
    }

    fn arith_arg(_: &mut Function, _: BlockId, a: ValueId) -> Operand {
        Operand::Value(a)
    }

    fn nothing(_: &mut Function, _: BlockId, _: ValueId, _: ValueId) {}

    fn release_arith(f: &mut Function, b: BlockId, _: ValueId, a: ValueId) {
        f.append_void(b, InstKind::RcDecAny(Operand::Value(a)));
    }

    fn release_param(f: &mut Function, b: BlockId, n: ValueId, _: ValueId) {
        f.append_void(b, InstKind::RcDecAny(Operand::Value(n)));
    }

    fn is_blr(w: u32) -> bool {
        w & 0xFFFF_FC1F == blr_reg(Gpr::X0)
    }

    #[test]
    fn br_x17_word() {
        assert_eq!(br_reg(Gpr::X17), BR_X17);
    }

    #[test]
    fn a_probed_indirect_call_releasing_an_immediate_leaves_by_br() {
        let (w, _) = compile(&tail_shape(false, arith_arg, release_arith));
        assert!(w.contains(&BR_X17), "{w:#x?}");
        assert!(!w.iter().any(|&x| is_blr(x)), "{w:#x?}");
    }

    #[test]
    fn a_probed_direct_call_leaves_by_b() {
        let (w, relocs) = compile(&tail_shape(true, arith_arg, nothing));
        let site = relocs
            .iter()
            .find(|r| {
                matches!(
                    r.kind,
                    RelocKind::CallSite {
                        target: CallTarget::Func(CALLEE)
                    }
                )
            })
            .expect("a call site for the callee");
        // B, not BL
        assert_eq!(w[site.byte_offset as usize / 4] & 0xFC00_0000, 0x1400_0000);
    }

    #[test]
    fn releasing_a_parameter_after_the_call_keeps_the_frame() {
        let (w, _) = compile(&tail_shape(false, arith_arg, release_param));
        assert!(!w.contains(&BR_X17));
        assert!(w.iter().any(|&x| is_blr(x)));
    }

    #[test]
    fn a_propagate_edge_that_does_more_keeps_the_frame() {
        let mut f = tail_shape(false, arith_arg, nothing);
        let (env, thrown) = (f.params[0], BlockId(2));
        f.set_term(thrown, Terminator::Unreachable);
        let store = InstKind::Store(Operand::ConstI64(1), Operand::Value(env), 16);
        f.append_void(thrown, store);
        f.set_term(thrown, Terminator::Ret(Some(Operand::ConstI64(0))));
        let (w, _) = compile(&f);
        assert!(!w.contains(&BR_X17));
    }

    #[test]
    fn a_throw_mark_on_the_propagate_edge_emits_nothing() {
        let plain = tail_shape(false, arith_arg, nothing);
        let mut marked = plain.clone();
        let thrown = BlockId(2);
        marked.set_term(thrown, Terminator::Unreachable);
        marked.append_void(thrown, InstKind::ThrowSignal);
        marked.set_term(thrown, Terminator::Ret(Some(Operand::ConstI64(0))));
        let (w, _) = compile(&marked);
        assert!(w.contains(&BR_X17), "{w:#x?}");
        assert_eq!(w, compile(&plain).0);
    }

    #[test]
    fn an_argument_pointing_into_the_frame_keeps_the_frame() {
        let slot = |f: &mut Function, b: BlockId, _: ValueId| {
            Operand::Value(f.append_inst(b, InstKind::AllocaBytes(8), Type::Ptr, None))
        };
        let (w, _) = compile(&tail_shape(false, slot, nothing));
        assert!(!w.contains(&BR_X17));
    }
}
