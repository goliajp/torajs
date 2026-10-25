//! Which call holds the module needs ([`settle`], run once every body
//! is in place).
//!
//! A call holds its own reference on a receiver or argument it read out
//! of a place (`ssa_lower::call_hold`), so that code which runs during
//! the call and overwrites the place cannot free the cell under the
//! callee. The hold is needless when nothing that runs while it is
//! taken may release a reference to a cell of the value's type: then
//! the reference the place had when the value was read is still there
//! when the call is done, and the cell lives through the call.
//!
//! What runs while a hold is taken is its *span*: the blocks on a path
//! from the retain to the normal path's release, which lower the later
//! arguments, make the call and check it for a throw, together with
//! every function those blocks reach by direct calls. A throw path
//! leaves the span: after a check fails the callee has returned and
//! nothing reads the value again. A hold is kept when its span has
//!
//! - a call of a runtime function or a class's release fn handed a
//!   value whose type may hold such a cell (`cell_release::MayHold`),
//!   but for the runtime functions that only read their cells
//!   (`BORROWS`);
//! - a call of a runtime function that may run user code: any not in
//!   the lists known to run none (`pin_facts::grow::READ_ONLY`, the
//!   allocations [`runtime::ALLOCS`] and the array releases), since
//!   user code may reach the cell through a global or a callback no
//!   argument shows. A read that runs user code only through a getter
//!   or a conversion hook, and releases only what it made itself
//!   ([`runtime::REENTRANT_READS`]), is kept out of that when nothing
//!   the runtime can enter ([`runtime::entered`]) may free the cell;
//! - an indirect call, an untyped release (but of a boxed immediate)
//!   or a suspension.
//!
//! A hold whose release its retain does not reach is kept.
//!
//! The pieces of every hold, needed or not, are left out of the scan:
//! a hold's release follows its own retain, so it can drop the count
//! to zero only after some other release in the scan already did.
//!
//! The retain-release pairing the e-graph does afterwards
//! (`rc_peephole`, `rc_obj_pair`) removes a pair only across
//! instructions that cannot release anything, never across a call; a
//! hold always spans one, so the two never judge the same pair.
//! `TORAJS_HOLD_STATS=1` prints why each hold is kept.

mod guards;
mod judge;
mod runtime;
mod span;

use std::cell::RefCell;
use std::collections::HashMap;

use crate::ssa::{FuncId, Module};

use super::cell_release::Slots;

use judge::{Body, Judge};

/// Keeps the holds the module needs and removes the rest, emptying
/// `module.call_holds`. `slots` says what the slots of each layout may
/// hold besides their own cells, over every function
/// (`cell_release::Slots`); `asleep`, when given,
/// the functions the runtime never enters (`drop_proofs::asleep`, read
/// while no class instance reaches the runtime).
pub(crate) fn settle(module: &mut Module, slots: Slots<'_>, asleep: Option<&[bool]>) {
    let holds = std::mem::take(&mut module.call_holds);
    if holds.is_empty() {
        return;
    }
    let pieces: HashMap<FuncId, Vec<bool>> = (holds.iter())
        .map(|(fid, hs)| {
            let f = &module.funcs[fid.0 as usize];
            (*fid, guards::piece_blocks(f, hs))
        })
        .collect();
    let judge = Judge {
        module,
        slots,
        pieces: &pieces,
        drops: module.class_drop_fns.iter().copied().collect(),
        asleep,
        entered: RefCell::new(HashMap::new()),
        walks: RefCell::new(HashMap::new()),
        scans: RefCell::new(HashMap::new()),
    };
    let stats = std::env::var_os("TORAJS_HOLD_STATS").is_some();
    let verdicts: Vec<Vec<bool>> = (holds.iter())
        .map(|(fid, hs)| {
            let body = Body::of(module, *fid);
            (hs.iter())
                .map(|h| {
                    let why = judge.needed(&body, h);
                    if stats && let Some(why) = &why {
                        let f = &module.funcs[fid.0 as usize].name;
                        eprintln!("[hold] {f} b{}: kept: {why}", h.take.at.0);
                    }
                    why.is_some()
                })
                .collect()
        })
        .collect();
    drop(judge);
    for ((fid, hs), keep) in holds.iter().zip(verdicts) {
        guards::settle(&mut module.funcs[fid.0 as usize], hs, &keep);
    }
}
