//! Sibling calls: a block whose direct call's result is the function's
//! return value leaves through the callee. The arguments go to their
//! registers, the epilogue releases this frame, and a `B` to the callee
//! stands for `BL` + `RET`, so the callee returns straight to this
//! function's caller and a recursion in tail position runs in constant
//! stack (ECMA-262 §15.10, proper tail calls).
//!
//! A block qualifies when, as codegen sees it:
//! - it ends in `%r = call f(args)` and `ret %r`, or in that call,
//!   `%x = copy %r` and a jump to a block that is just `ret %x`
//!   (the shape a call in tail position takes once a return joins
//!   several paths); anything after the call — a pending-throw check,
//!   a release of an argument the caller owns, a scope drop — breaks
//!   the shape, so those calls keep `BL`
//! - `f` has a body (an extern or intrinsic call site is rewritten or
//!   opened inline downstream) and returns this function's type
//! - every argument travels in a register: a stack argument would sit
//!   in the frame the epilogue releases
//! - no register is saved around the call and the function takes no
//!   stack parameter of its own

use torajs_core::ssa::id_hash::HashMap;
use torajs_core::ssa::{Block, FuncId, Function, InstKind, Operand, Terminator};

use super::call_args::{has_stack_lane, pass_args};
use super::{ModuleFacts, landing, write_u32};
use crate::enc::b_imm26;
use crate::frame::FrameLayout;
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

pub(super) struct Sibling<'a> {
    /// index of the call in the block; what follows it is not emitted
    pub(super) at: usize,
    callee: FuncId,
    args: &'a [Operand],
}

/// The block's sibling call, if it has one. `block_slot` is the slot
/// index of the block's first instruction, as liveness numbers them.
pub(super) fn plan<'a>(
    func: &Function,
    block: &'a Block,
    blocks_by_id: &HashMap<u32, &Block>,
    alloc: &Assignment,
    facts: &ModuleFacts,
    block_slot: u32,
) -> Option<Sibling<'a>> {
    let n = block.insts.len();
    let returned = match &block.term {
        Terminator::Ret(Some(Operand::Value(v))) => *v,
        Terminator::Br(t) => {
            let copy = block.insts.last()?;
            let InstKind::Copy(ty, Operand::Value(r)) = &copy.kind else {
                return None;
            };
            if *ty != func.ret || !returns_through(func, blocks_by_id, *t, copy.result?) {
                return None;
            }
            *r
        }
        _ => return None,
    };
    let at = match &block.term {
        Terminator::Ret(_) => n.checked_sub(1)?,
        _ => n.checked_sub(2)?,
    };
    let call = &block.insts[at];
    let InstKind::Call(callee, args) = &call.kind else {
        return None;
    };
    let ret = facts.fn_rets.get(callee.0 as usize).copied().flatten()?;
    let param_types = facts.fn_sigs.get(callee.0 as usize).map(Vec::as_slice);
    let site = block_slot + at as u32;
    (call.result == Some(returned)
        && ret == func.ret
        && func.value_type(returned) == func.ret
        && alloc.stack_param_entry_loads.is_empty()
        && alloc.call_saves.get(&site).is_none_or(Vec::is_empty)
        && alloc.vec_call_saves.get(&site).is_none_or(Vec::is_empty)
        && !has_stack_lane(args, alloc, param_types))
    .then_some(Sibling {
        at,
        callee: *callee,
        args,
    })
}

/// Whether jumping to `target` with `v` in hand returns `v`: the
/// target is just `ret v`, or a join that copies `v` into the next
/// join's value and jumps on (one join per enclosing return, once
/// inlining has nested a tail call's body inside another's).
fn returns_through(
    func: &Function,
    blocks_by_id: &HashMap<u32, &Block>,
    mut target: torajs_core::ssa::BlockId,
    mut v: torajs_core::ssa::ValueId,
) -> bool {
    for _ in 0..8 {
        let Some(j) = blocks_by_id.get(&landing::resolve_forwarded_target(blocks_by_id, target).0)
        else {
            return false;
        };
        match (&j.term, j.insts.as_slice()) {
            (Terminator::Ret(Some(Operand::Value(x))), []) => return *x == v,
            (Terminator::Br(next), [copy]) => match &copy.kind {
                InstKind::Copy(ty, Operand::Value(r)) if *r == v && *ty == func.ret => {
                    let Some(w) = copy.result else { return false };
                    v = w;
                    target = *next;
                }
                _ => return false,
            },
            _ => return false,
        }
    }
    false
}

/// Arguments, epilogue, then `B` to the callee with the call-site
/// relocation a `BL` would carry.
pub(super) fn emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    s: &Sibling<'_>,
    alloc: &Assignment,
    facts: &ModuleFacts,
    frame: &FrameLayout,
) {
    let param_types = facts.fn_sigs.get(s.callee.0 as usize).map(Vec::as_slice);
    pass_args(bytes, s.args, alloc, param_types);
    frame.emit_epilogue(bytes);
    relocs.push(Reloc {
        byte_offset: bytes.len() as u32,
        kind: RelocKind::CallSite {
            target: CallTarget::Func(s.callee),
        },
    });
    write_u32(bytes, b_imm26(0));
}
