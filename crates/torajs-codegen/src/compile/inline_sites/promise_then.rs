//! `__torajs_promise_then_simple(p, f, word)` and its `catch` twin
//! opened at the call site, for a source that is still pending.
//!
//! On a pending source with a plain handler (no closure env, no `any`
//! parameter) the kernel's whole work is a block off the thread
//! cache's row and a set of stores: the derived cell, fresh, with the
//! reaction parked in it, pushed onto the source's callback list
//! (`reaction::attach_plain_pending`). Nothing is queued and no handler
//! runs, so the stores are the whole observable effect. The site makes
//! them, each word once — the kernel writes the cell's fresh fields and
//! then the reaction over four of them. Everything else goes to the
//! call: a null source or handler, a settled source (whose reaction is
//! queued), an empty row.
//!
//! The derived cell's count is written as two: its own reference,
//! which the call answers, and the list's stake, which the kernel
//! takes with `rc_inc_cell` on the cell it has just made (count one,
//! no flags, so the inc is a plain +1).

use core::mem::offset_of;

use torajs_core::ssa::{FuncId, Inst, InstKind, Module, Operand};
use torajs_mmalloc::size_class::bucket_for;
use torajs_mmalloc::tlab::{FREE_LINK_OFF, TLAB_ROW_STRIDE};
use torajs_promise::layout::{
    CB_INLINE_TAG, PROMISE_SIZE, Promise, RX_CATCH, RX_THEN, STATE_PENDING, TAG_PROMISE,
};
use torajs_promise::then_box::{PARAM_ANY_FLAG, PARAM_REPR_SHIFT};
use torajs_rc::HeapHeader;

use super::super::cell::TLAB_SYM;
use super::super::rc_dec::{Fix, patch, place, store};
use super::super::refs::emit_adrp_add_pair;
use super::super::{
    ModuleFacts, OP_SCRATCH_LHS, OP_SCRATCH_RESULT_GPR, OP_SCRATCH_RHS, OP_SCRATCH_TMP,
    materialize_const_i64, materialize_operand_gpr, write_def_spill_gpr, write_u32,
};
use crate::enc::{
    ldp_x_off, ldr_x_imm12, ldrb_w_imm12, logical_imm, mov_x_reg, movz_imm, orr_imm, stp_x_off,
    str_x_imm12, strb_w_imm12, sub_imm,
};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;
use crate::reloc::Reloc;

const THEN_NAMES: [(&str, u8); 2] = [
    ("__torajs_promise_then_simple", RX_THEN),
    ("__torajs_promise_catch_simple", RX_CATCH),
];

const _: () = assert!(offset_of!(HeapHeader, refcount) == 0);
const _: () = assert!(offset_of!(HeapHeader, type_tag) == 4);
const _: () = assert!(STATE_PENDING == 0);
const STATE_OFF: u32 = offset_of!(Promise, state) as u32;
const HAS_HANDLER_OFF: u32 = offset_of!(Promise, has_handler) as u32;
const CALLBACKS_OFF: u32 = offset_of!(Promise, callbacks) as u32;
// the cell is written as four pairs, every field of it once
const _: () = assert!(offset_of!(Promise, state) == 8);
const _: () = assert!(offset_of!(Promise, value_repr) == 11);
const _: () = assert!(offset_of!(Promise, rx_kind) == 13);
const _: () = assert!(offset_of!(Promise, rx_ret_repr) == 14);
const _: () = assert!(offset_of!(Promise, rx_param_repr) == 15);
const _: () = assert!(offset_of!(Promise, value) == 16 && offset_of!(Promise, callbacks) == 24);
const _: () = assert!(offset_of!(Promise, props) == 32 && offset_of!(Promise, rx_next) == 40);
const _: () = assert!(offset_of!(Promise, rx_source) == 48 && offset_of!(Promise, rx_cb) == 56);
const _: () = assert!(PROMISE_SIZE == 64);

/// The module's plain-handler `then` / `catch` kernels.
#[derive(Debug, Clone, Copy)]
pub struct PromiseThen {
    funcs: [Option<FuncId>; 2],
    row_off: i32,
}

impl PromiseThen {
    /// `None` leaves every `then` a plain call: a module that declares
    /// neither kernel, a census compiler (whose allocator counters the
    /// row pop would bypass), a use-after-free detector compiler (whose
    /// rows stay empty), and `TORAJS_PROMISE_THEN_INLINE_OFF=1` (bisect
    /// gate).
    pub fn of_module(module: &Module) -> Option<PromiseThen> {
        if cfg!(torajs_census)
            || cfg!(torajs_uaf_detect)
            || std::env::var_os("TORAJS_PROMISE_THEN_INLINE_OFF").is_some_and(|v| v == "1")
        {
            return None;
        }
        let find = |(name, _): (&str, u8)| {
            module
                .funcs
                .iter()
                .position(|f| f.name == name)
                .map(|i| FuncId(i as u32))
        };
        let funcs = THEN_NAMES.map(find);
        let class = bucket_for(PROMISE_SIZE)?;
        (funcs.iter().any(Option::is_some)).then_some(PromiseThen {
            funcs,
            row_off: (class * TLAB_ROW_STRIDE) as i32,
        })
    }

    /// The reaction kind and the repr word of a call [`try_emit`]
    /// opens: a plain handler (no `any` parameter) on a source and a
    /// handler in registers or slots, with a result.
    fn site(self, inst: &Inst) -> Option<(u8, i64)> {
        let InstKind::Call(fid, args) = &inst.kind else {
            return None;
        };
        let at = self.funcs.iter().position(|f| *f == Some(*fid))?;
        match args.as_slice() {
            [
                Operand::Value(_),
                Operand::Value(_),
                Operand::ConstI64(word),
            ] if word & PARAM_ANY_FLAG == 0 && inst.result.is_some() => {
                Some((THEN_NAMES[at].1, *word))
            }
            _ => None,
        }
    }

    /// Whether [`try_emit`] opens the call `kind` inline.
    pub(crate) fn matches(self, kind: &InstKind) -> bool {
        let InstKind::Call(fid, args) = kind else {
            return false;
        };
        self.funcs.contains(&Some(*fid))
            && matches!(args.as_slice(),
                [Operand::Value(_), Operand::Value(_), Operand::ConstI64(w)] if w & PARAM_ANY_FLAG == 0)
    }
}

/// The derived cell's header word: count two, the promise tag, no flags.
const HEADER_WORD: i64 = 2 | (TAG_PROMISE as i64) << 32;

/// The derived cell's second word: pending, no heap value, unhandled,
/// stamped with the handler's return repr, and the reaction's kind and
/// reprs.
fn state_word(kind: u8, word: i64) -> i64 {
    let ret = word & 0xff;
    let param = (word >> PARAM_REPR_SHIFT) & 0xff;
    ret << 24 | (kind as i64) << 40 | ret << 48 | param << 56
}

/// The `then` at `inst`, opened inline; `false` when it is not one.
///
/// ```text
///     cbz   xS, slow ; cbz xF, slow
///     ldrb  w10, [xS, #state] ; cbnz w10, slow  ; settled: the reaction queues
///     adrp+add x9, tlab
///     ldp   x12, x10, [x9, #row]
///     cbz   x12, slow                           ; an empty row
///     ldr   x11, [x12, #link] ; sub x10, x10, #1
///     stp   x11, x10, [x9, #row]
///     mov   x9, #HEADER_WORD ; mov x10, #state_word
///     stp   x9, x10, [x12]
///     stp   xzr, xzr, [x12, #16]                ; value, callbacks
///     ldr   x11, [xS, #callbacks]
///     stp   xzr, x11, [x12, #32]                ; props, rx_next
///     stp   xS, xF, [x12, #48]                  ; rx_source, rx_cb
///     mov   w9, #1 ; strb w9, [xS, #has_handler]
///     orr   x9, x12, #INLINE_TAG ; str x9, [xS, #callbacks]
///     mov   xD, x12 ; b done
/// slow:
///     <store the call saves> ; <the call> ; <reload them>
/// done:
/// ```
///
/// Nothing is written before the last check passes, so the call meets
/// the source and the row as they were.
pub(crate) fn try_emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    alloc: &Assignment,
    saves: &[(Reg, u32)],
    facts: &ModuleFacts,
) -> bool {
    let Some(op) = facts.inline.promise_then else {
        return false;
    };
    let Some((kind, word)) = op.site(inst) else {
        return false;
    };
    let InstKind::Call(fid, args) = &inst.kind else {
        return false;
    };
    let result = inst.result.expect("site() admits a call with a result");
    let (tlab, t0, t1, cell) = (
        OP_SCRATCH_LHS,
        OP_SCRATCH_RHS,
        OP_SCRATCH_TMP,
        OP_SCRATCH_RESULT_GPR,
    );
    // cb first: a slot past the imm12 reach is read through an X16 base
    // (`slot::base_for`), which would overwrite src had it gone first
    let cb = materialize_operand_gpr(bytes, &args[1], Gpr::X17, alloc);
    let src = materialize_operand_gpr(bytes, &args[0], Gpr::X16, alloc);
    let mut slow = Vec::new();

    slow.push(place(bytes, Fix::Cbz(src)));
    slow.push(place(bytes, Fix::Cbz(cb)));
    write_u32(bytes, ldrb_w_imm12(t0, src, STATE_OFF));
    slow.push(place(bytes, Fix::Cbnz(t0)));
    emit_adrp_add_pair(bytes, relocs, tlab, TLAB_SYM.to_string());
    write_u32(bytes, ldp_x_off(cell, t0, tlab, op.row_off));
    slow.push(place(bytes, Fix::Cbz(cell)));
    write_u32(bytes, ldr_x_imm12(t1, cell, FREE_LINK_OFF as u32));
    write_u32(bytes, sub_imm(t0, t0, 1));
    write_u32(bytes, stp_x_off(t1, t0, tlab, op.row_off));

    materialize_const_i64(bytes, tlab, HEADER_WORD);
    materialize_const_i64(bytes, t0, state_word(kind, word));
    write_u32(bytes, stp_x_off(tlab, t0, cell, 0));
    write_u32(bytes, stp_x_off(Gpr::XZR, Gpr::XZR, cell, 16));
    write_u32(bytes, ldr_x_imm12(t1, src, CALLBACKS_OFF));
    write_u32(bytes, stp_x_off(Gpr::XZR, t1, cell, 32));
    write_u32(bytes, stp_x_off(src, cb, cell, 48));
    write_u32(bytes, movz_imm(tlab, 1, 0));
    write_u32(bytes, strb_w_imm12(tlab, src, HAS_HANDLER_OFF));
    let tag = logical_imm(CB_INLINE_TAG as u64).expect("the inline tag encodes");
    write_u32(bytes, orr_imm(tlab, cell, tag));
    write_u32(bytes, str_x_imm12(tlab, src, CALLBACKS_OFF));
    let (dst, spill_off) = alloc.def_gpr(result, OP_SCRATCH_RESULT_GPR);
    if dst != cell {
        write_u32(bytes, mov_x_reg(dst, cell));
    }
    write_def_spill_gpr(bytes, spill_off, dst);
    let to_done = place(bytes, Fix::B);

    let slow_at = bytes.len();
    for at in slow {
        patch(bytes, at, slow_at);
    }
    for &(reg, off) in saves {
        store(bytes, reg, off, true);
    }
    let param_types = facts.fn_sigs.get(fid.0 as usize).map(|v| v.as_slice());
    super::super::call::emit_call(bytes, relocs, inst, *fid, args, alloc, param_types);
    for &(reg, off) in saves {
        store(bytes, reg, off, false);
    }
    let done_at = bytes.len();
    patch(bytes, to_done, done_at);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_the_site_writes() {
        // count 2 (own + list stake), tag 8, no flags
        assert_eq!(HEADER_WORD, 0x0000_0008_0000_0002);
        // ret I64 (1), param I64 (1 << 16), RX_THEN
        let w = state_word(RX_THEN, 1 | 1 << 16);
        let bytes = w.to_le_bytes();
        assert_eq!(bytes, [0, 0, 0, 1, 0, RX_THEN, 1, 1]);
        assert_eq!(STATE_OFF, 8);
        assert_eq!(HAS_HANDLER_OFF, 10);
        assert_eq!(CALLBACKS_OFF, 24);
    }
}
