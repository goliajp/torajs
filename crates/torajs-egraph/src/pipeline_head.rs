use torajs_core::ssa::Module;

use crate::{devirt, fresh_cell, gated_pass, heap_to_stack, inliner, mem2reg, slot_forward};

/// What the egraph pass eats: callees inlined (a second round after
/// devirt turns fn-pointer calls direct), then the param round-trips
/// that splices leave forwarded and promoted.
pub(crate) fn inline_and_forward(module: &mut Module) {
    let inliner_stats = inliner::inline_module(module);
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
        let round2 = inliner::inline_module(module);
        if std::env::var("TORAJS_INLINER_STATS").as_deref() == Ok("1") {
            eprintln!("torajs-inliner-stats-round2: {round2:?}");
        }
    }
    // Block-local store→load forwarding — clears the alloca+store+load
    // param round-trips the inliner splices into hot loops (mem2reg's
    // degenerate in-block case). After inlining/devirt (the food),
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
    if gated_pass("HEAP_TO_STACK", module, heap_to_stack::promote_module)
        .is_some_and(|s| s.promoted > 0)
    {
        gated_pass("SLOTFWD", module, slot_forward::forward_slot_loads);
        gated_pass("MEM2REG", module, mem2reg::promote_slots);
    }
}
