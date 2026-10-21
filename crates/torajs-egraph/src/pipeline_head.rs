use torajs_core::ssa::{FuncId, InstKind, Module};

use crate::frame_cell_clean::FrameCellCleanStats;
use crate::fresh_cell::FreshCellStats;
use crate::mem2reg::Mem2RegStats;
use crate::slot_forward::SlotForwardStats;
use crate::{
    devirt, field_pin, frame_cell_clean, fresh_cell, gated_pass, heap_to_stack, inliner, mem2reg,
    pass_timer, slot_forward,
};

/// What the egraph pass eats: callees inlined (a second round after
/// devirt turns fn-pointer calls direct), then the param round-trips
/// that splices leave forwarded and promoted.
pub(crate) fn inline_and_forward(module: &mut Module) {
    let inliner_stats = pass_timer::timed("INLINER", || inliner::inline_module(module));
    // TORAJS_INLINER_STATS=1 — dump per-compile inliner decision
    // counters to stderr for fire-rate attribution (which SkipReason
    // bucket holds the production call sites a given corpus exposes).
    if std::env::var("TORAJS_INLINER_STATS").as_deref() == Ok("1") {
        eprintln!("torajs-inliner-stats: {inliner_stats:?}");
    }
    // Indirect-call promotion — after the first inliner round (which
    // exposes fn-pointer slots by splicing fn-typed params into
    // callers), before a second round that inlines the promoted
    // direct calls. `TORAJS_DEVIRT_OFF=1` skips (bisect gate).
    if let Some(devirt_stats) = gated_pass("DEVIRT", module, devirt::devirtualize_module)
        && devirt_stats.rewritten > 0
    {
        let round2 = pass_timer::timed("INLINER", || inliner::inline_module(module));
        if std::env::var("TORAJS_INLINER_STATS").as_deref() == Ok("1") {
            eprintln!("torajs-inliner-stats-round2: {round2:?}");
        }
    }
    // Extended-basic-block store→load forwarding — clears the
    // alloca+store+load param round-trips the inliner splices into hot
    // loops (mem2reg's degenerate straight-line case). After inlining/devirt (the food),
    // before the egraph pass (forwarded constants feed const-fold /
    // GVN). `TORAJS_SLOTFWD_OFF=1` skips (bisect gate).
    gated_pass("SLOTFWD", module, slot_forward::forward_slot_loads);
    // Cross-block single-def-block slot promotion — the dominance
    // fast path of LLVM mem2reg (param spills whose loads live in
    // branch arms). After slot_forward (in-block round-trips already
    // cleared, slots possibly DSE'd), before the egraph pass.
    // `TORAJS_MEM2REG_OFF=1` skips (bisect gate).
    gated_pass("MEM2REG", module, mem2reg::promote_slots);
    // after the slot passes (the ctor's `this` reaches the cell through
    // a param spill), before heap_to_stack; `TORAJS_FRESH_CELL_OFF=1` skips
    gated_pass("FRESH_CELL", module, fresh_cell::forward_fresh_cells);
    // Heap-to-stack — a header cell whose every use now sits in this
    // function becomes an alloca. After the slot passes above (the
    // cell pointer often reaches its uses through an inlined local's
    // spill slot), then those passes again to forward its fields.
    // `TORAJS_HEAP_TO_STACK_OFF=1` skips (bisect gate).
    let frames_before = frame_cells(module);
    if gated_pass("HEAP_TO_STACK", module, heap_to_stack::promote_module)
        .is_some_and(|s| s.promoted > 0)
    {
        let promoted: Vec<bool> = frame_cells(module)
            .iter()
            .zip(&frames_before)
            .map(|(after, before)| after > before)
            .collect();
        // `TORAJS_FRAME_CELL_ROUNDS_OFF=1` skips (bisect gate)
        gated_pass("FRAME_CELL_ROUNDS", module, |m| {
            forward_frame_cells(m, &promoted)
        });
        gated_pass("SLOTFWD", module, slot_forward::forward_slot_loads);
        gated_pass("MEM2REG", module, mem2reg::promote_slots);
    }
    // a pinned array field's header words, read once per member call:
    // after the slot passes (the receiver reaches the loads directly),
    // before the egraph pass, which keeps a load where it stands.
    // `TORAJS_FIELD_PIN_OFF=1` skips (bisect gate).
    if !module.pins.is_empty() {
        gated_pass("FIELD_PIN", module, field_pin::pin_field_loads);
    }
}

/// `alloca_bytes` count per function: heap_to_stack's promotions are
/// the ones it adds.
fn frame_cells(module: &Module) -> Vec<usize> {
    module
        .funcs
        .iter()
        .map(|f| {
            f.blocks
                .iter()
                .flat_map(|b| &b.insts)
                .filter(|i| matches!(i.kind, InstKind::AllocaBytes(_)))
                .count()
        })
        .collect()
}

/// Rounds over a function with promoted cells until nothing changes:
/// each round's folds and forwards hand the next one a cell with fewer
/// uses (the release reached through a `let` slot is decided only once
/// that slot's load forwards to the cell). Afterwards every function
/// gets the slot passes once more, as before the rounds existed.
const FRAME_CELL_ROUNDS: u32 = 4;

#[derive(Debug, Default)]
struct FrameCellRoundsStats {
    /// Functions that ran the rounds.
    functions: u32,
    /// Rounds run, summed over those functions.
    rounds: u32,
    /// Rounds that changed their function.
    rounds_changed: u32,
}

fn forward_frame_cells(module: &mut Module, promoted: &[bool]) -> FrameCellRoundsStats {
    let on = |key: &str| std::env::var(format!("TORAJS_{key}_OFF")).as_deref() != Ok("1");
    let (clean_on, slotfwd_on) = (on("FRAME_CELL_CLEAN"), on("SLOTFWD"));
    let (mem2reg_on, fresh_on) = (on("MEM2REG"), on("FRESH_CELL"));
    let alloc = module
        .funcs
        .iter()
        .position(|f| f.name == "__torajs_obj_alloc")
        .map(|i| FuncId(i as u32));
    let mut st = FrameCellRoundsStats::default();
    for (func, &promoted) in module.funcs.iter_mut().zip(promoted) {
        if func.is_declaration() || !promoted {
            continue;
        }
        st.functions += 1;
        for _ in 0..FRAME_CELL_ROUNDS {
            st.rounds += 1;
            let mut changed = false;
            if clean_on {
                let mut s = FrameCellCleanStats::default();
                frame_cell_clean::clean_in_function(func, &mut s);
                changed |= s != Default::default();
            }
            if slotfwd_on {
                let mut s = SlotForwardStats::default();
                slot_forward::forward_in_function(func, &mut s);
                changed |= s != Default::default();
            }
            if mem2reg_on {
                let mut s = Mem2RegStats::default();
                mem2reg::promote_in_function(func, &mut s);
                changed |= s != Default::default();
            }
            if fresh_on {
                let mut s = FreshCellStats::default();
                fresh_cell::forward_in_function(func, alloc, &mut s);
                changed |= s != Default::default();
            }
            if !changed {
                break;
            }
            st.rounds_changed += 1;
        }
    }
    st
}
