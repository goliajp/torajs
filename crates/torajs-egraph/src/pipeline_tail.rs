use torajs_core::ssa::Module;

use crate::{block_layout, gated_pass, loop_rotate, map_find_cse, rc_dec_immediate, rc_peephole};

/// The pipeline tail — rc-pair elision, then the cfg-shape passes whose
/// output is exactly what codegen lays out.
pub(crate) fn finish_pipeline(module: &mut Module) {
    // Immediate-box release elision — `anyv_rc_dec` of a `box_from_
    // pair(<non-heap tag>, _)` is the kernel's own no-op (a `new C()`
    // site's undefined `__new_target` box). After inlining so mint
    // and release share a fn; before the pair elision, whose windows
    // such a release would otherwise close.
    // `TORAJS_RC_DEC_IMMEDIATE_OFF=1` skips.
    gated_pass(
        "RC_DEC_IMMEDIATE",
        module,
        rc_dec_immediate::elide_immediate_rc_decs,
    );
    // RC elide peephole — after the egraph pass so pure-inst dedup /
    // identity collapse has already tightened the windows between
    // retain/release pairs. `TORAJS_RC_PEEPHOLE_OFF=1` skips (bisect
    // gate, mirrors TORAJS_INLINER_OFF).
    gated_pass("RC_PEEPHOLE", module, rc_peephole::elide_rc_pairs);
    // Map lookup sharing — a repeated string-key find on the same Map
    // with no Map write between reuses the earlier index; set_str takes
    // it as a checked hint. `TORAJS_MAP_FIND_CSE_OFF=1` skips.
    gated_pass("MAP_FIND_CSE", module, map_find_cse::share_map_finds);
    // Loop rotation — copy each top-tested loop's exit test into its
    // latch so the loop runs one conditional branch per iteration.
    // Before block_layout, which lays out the rotated body.
    // `TORAJS_LOOP_ROTATE_OFF=1` skips (bisect gate).
    gated_pass("LOOP_ROTATE", module, loop_rotate::rotate_module);
    // Loop-body contiguity layout — last, so the block order codegen
    // sees (fall-through chains, positional liveness/spill weights)
    // is the final one. Sinks cold blocks out of loop position
    // ranges; no inst-level rewrites. `TORAJS_BLOCK_LAYOUT_OFF=1`
    // skips (bisect gate).
    gated_pass("BLOCK_LAYOUT", module, block_layout::layout_module);
}
