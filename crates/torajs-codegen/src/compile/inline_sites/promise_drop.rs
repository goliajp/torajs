//! `__torajs_promise_drop(p)` opened at the call site.
//!
//! Most typed promise releases leave the cell alive with nothing more
//! to do: the count stays above zero and the cell cannot close a ring,
//! so the kernel would only decrement. The site does that inline. A
//! cell that may be a cycle root (the kernel's
//! `reaction::candidate::cycle_candidate`, never answered "no root"
//! here where the kernel answers "root") makes the plain call, which
//! sees the count untouched. The last reference (teardown, the cycle
//! buffer scrub, the WeakRef hook) calls the kernel's entry that skips
//! the tests the site already made.

use core::mem::offset_of;

use torajs_core::ssa::{FuncId, Inst, InstKind, Module, Operand};
use torajs_promise::layout::Promise;
use torajs_promise::reaction::candidate::{FLAG_LIST_EDGE, FLAG_RX_ENV};
use torajs_rc::HeapHeader;

use super::super::rc::STATIC_BIT;
use super::super::rc_dec::{Fix, patch, place, store};
use super::super::{
    OP_SCRATCH_LHS, OP_SCRATCH_RHS, OP_SCRATCH_TMP, materialize_operand_gpr, write_u32,
};
use crate::enc::{
    bl_imm26, cmp_w_imm, cond, ldr_x_imm12, ldrh_w_imm12, logical_imm, mov_x_reg, str_w_imm12,
    sub_w_imm, tst_imm,
};
use crate::reg::{Gpr, Reg};
use crate::regalloc::Assignment;
use crate::reloc::{CallTarget, Reloc, RelocKind};

const PROMISE_DROP_NAME: &str = "__torajs_promise_drop";
const DROP_LAST_SYM: &str = "___torajs_promise_drop_last";

const _: () = assert!(offset_of!(HeapHeader, refcount) == 0);
const FLAGS_SHIFT: u32 = offset_of!(HeapHeader, flags) as u32 * 8;
/// The header bits that mark a cell whose tree has an edge out, or
/// whose own reaction calls an env.
const EDGE_MASK: u64 = ((FLAG_LIST_EDGE | FLAG_RX_ENV) as u64) << FLAGS_SHIFT;
const PROPS_OFF: u32 = offset_of!(Promise, props) as u32;
// state and value_is_heap read as one half-word: state low, flag high
const STATE_OFF: u32 = offset_of!(Promise, state) as u32;
const _: () = assert!(offset_of!(Promise, value_is_heap) == offset_of!(Promise, state) + 1);

/// The module's typed promise release.
#[derive(Debug, Clone, Copy)]
pub struct PromiseDrop {
    func: FuncId,
}

impl PromiseDrop {
    /// `None` leaves every release a plain call: a module that never
    /// declares the kernel, and `TORAJS_PROMISE_DROP_INLINE_OFF=1`
    /// (bisect gate).
    pub fn of_module(module: &Module) -> Option<PromiseDrop> {
        if std::env::var_os("TORAJS_PROMISE_DROP_INLINE_OFF").is_some_and(|v| v == "1") {
            return None;
        }
        let at = module
            .funcs
            .iter()
            .position(|f| f.name == PROMISE_DROP_NAME)?;
        Some(PromiseDrop {
            func: FuncId(at as u32),
        })
    }

    /// Whether `kind` is a release of a value [`try_emit`] opens.
    pub(crate) fn matches(self, kind: &InstKind) -> bool {
        matches!(kind, InstKind::Call(fid, args)
            if *fid == self.func && matches!(args.as_slice(), [Operand::Value(_)]))
    }
}

/// The release at `inst`, opened inline; `false` when it is not one.
///
/// ```text
///     cbz   xP, done
///     ldr   x10, [xP]
///     tbnz  x10, #STATIC_BIT, done        ; never counted, never a root
///     cmp   w10, #1 ; b.ls last           ; the last reference
///     tst   x10, #EDGE_MASK ; b.ne slow   ; an edge out of its tree, an env
///     ldr   x11, [xP, #props] ; cbnz x11, slow
///     ldrh  w11, [xP, #state]
///     tst   w11, #0xff00 ; b.eq dec       ; no heap value
///     tst   w11, #0xff ; b.ne slow        ; settled with a heap value
/// dec:
///     sub   w10, w10, #1 ; str w10, [xP] ; b done
/// slow:
///     <store the call saves> ; mov x0, xP ; bl promise_drop ; <reload> ; b done
/// last:
///     <store the call saves> ; mov x0, xP ; bl promise_drop_last ; <reload>
/// done:
/// ```
///
/// A settled heap value of 0 still takes the call, which answers it
/// alive the same way.
pub(crate) fn try_emit(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    inst: &Inst,
    alloc: &Assignment,
    saves: &[(Reg, u32)],
    op: PromiseDrop,
) -> bool {
    if !op.matches(&inst.kind) {
        return false;
    }
    let InstKind::Call(fid, args) = &inst.kind else {
        return false;
    };
    let p = materialize_operand_gpr(bytes, &args[0], OP_SCRATCH_LHS, alloc);
    let (hdr, t) = (OP_SCRATCH_RHS, OP_SCRATCH_TMP);
    let mut done = Vec::new();
    let mut slow = Vec::new();

    done.push(place(bytes, Fix::Cbz(p)));
    write_u32(bytes, ldr_x_imm12(hdr, p, 0));
    done.push(place(bytes, Fix::Tbnz(hdr, STATIC_BIT)));
    write_u32(bytes, cmp_w_imm(hdr, 1));
    let to_last = place(bytes, Fix::Cond(cond::LS));
    let (n, immr, imms) = logical_imm(EDGE_MASK).expect("edge bits encode as a logical immediate");
    write_u32(bytes, tst_imm(hdr, n, immr, imms));
    slow.push(place(bytes, Fix::Cond(cond::NE)));
    write_u32(bytes, ldr_x_imm12(t, p, PROPS_OFF));
    slow.push(place(bytes, Fix::Cbnz(t)));
    write_u32(bytes, ldrh_w_imm12(t, p, STATE_OFF));
    let (n, immr, imms) = logical_imm(0xff00).expect("0xff00 encodes");
    write_u32(bytes, tst_imm(t, n, immr, imms));
    let to_dec = place(bytes, Fix::Cond(cond::EQ));
    let (n, immr, imms) = logical_imm(0xff).expect("0xff encodes");
    write_u32(bytes, tst_imm(t, n, immr, imms));
    slow.push(place(bytes, Fix::Cond(cond::NE)));

    let dec_at = bytes.len();
    patch(bytes, to_dec, dec_at);
    write_u32(bytes, sub_w_imm(hdr, hdr, 1));
    write_u32(bytes, str_w_imm12(hdr, p, 0));
    done.push(place(bytes, Fix::B));

    let slow_at = bytes.len();
    call_out(bytes, relocs, p, saves, CallTarget::Func(*fid));
    done.push(place(bytes, Fix::B));
    let last_at = bytes.len();
    patch(bytes, to_last, last_at);
    call_out(
        bytes,
        relocs,
        p,
        saves,
        CallTarget::Extern(DROP_LAST_SYM.into()),
    );

    let done_at = bytes.len();
    for at in slow {
        patch(bytes, at, slow_at);
    }
    for at in done {
        patch(bytes, at, done_at);
    }
    true
}

/// A slow arm: the call on `p` with `saves` stored around it.
fn call_out(
    bytes: &mut Vec<u8>,
    relocs: &mut Vec<Reloc>,
    p: Gpr,
    saves: &[(Reg, u32)],
    target: CallTarget,
) {
    for &(reg, off) in saves {
        store(bytes, reg, off, true);
    }
    if p != Gpr::X0 {
        write_u32(bytes, mov_x_reg(Gpr::X0, p));
    }
    relocs.push(Reloc {
        byte_offset: bytes.len() as u32,
        kind: RelocKind::CallSite { target },
    });
    write_u32(bytes, bl_imm26(0));
    for &(reg, off) in saves {
        store(bytes, reg, off, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_the_site_reads() {
        assert_eq!(STATIC_BIT, 50);
        assert_eq!(EDGE_MASK, 0b11 << 58);
        assert_eq!(PROPS_OFF, 32);
        assert_eq!(STATE_OFF, 8);
        // the 64-bit tst of the header uses the two-bit run at 58
        assert!(logical_imm(EDGE_MASK).is_some());
    }
}
