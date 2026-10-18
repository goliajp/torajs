use crate::id_hash::HashMap;

use torajs_core::ssa::{FuncId, Module, ValueId};

use crate::{
    block_layout, concat_len, concat_licm, concat_num_fuse, const_copy_fwd, fdiv_pair, gated_pass,
    interval, iv_merge, loop_rotate, map_find_cse, pass_gates, pass_timer, rc_borrow,
    rc_dec_immediate, rc_peephole, scaled_addr, select_form, srem_fquot, str_append, strip_assumes,
    unroll_jam, unroll_pair,
};

mod global_ref_share;

/// Select formation and the inst-level fusions that read its output,
/// ahead of the rc and cfg-shape tail.
pub(crate) fn form_selects_and_fuse(module: &mut Module, names: Option<&[String]>) {
    // Select formation — if-convert pure CondBr diamonds into csel-
    // shaped `Select` defs (RFC 20260719-select-formation blade 2).
    // After ctpop so every arm-shaping rewrite (float_demote /
    // srem_parity) is done; before rc_peephole, which treats Select
    // as rc-transparent. `TORAJS_SELECT_FORM_OFF=1` skips (bisect
    // gate, mirrors the sibling passes).
    gated_pass("SELECT_FORM", module, select_form::form_selects);
    // A divisibility test the selects just formed decide on takes its
    // quotient from `fdiv` as an exit test does. Before cmp_sink, which
    // keeps the compare next to its select.
    // `TORAJS_SREM_FQUOT_SELECT_OFF=1` skips.
    gated_pass("SREM_FQUOT_SELECT", module, |m| {
        srem_fquot::float_quotient_select_srems(m, names)
    });
    // Compare sink — restore ICmp/Select adjacency that the hoisted
    // speculated arms just broke, so codegen's adjacency-gated NZCV
    // fuse fires (RFC 20260719-select-formation route ③). Right after
    // select_form: the Selects exist, and liveness is computed later
    // on the sunk order, which is what makes the delayed compare's
    // operands readable by construction.
    gated_pass("CMP_SINK", module, select_form::cmp_sink::sink_cmps);
    // TORAJS_SSA_DUMP=1 — pretty-print the post-egraph pre-peephole
    // SSA to stdout. Debug surface for attributing which pass shaped
    // a given inst stream (mirrors TORAJS_INLINER_STATS).
    if std::env::var("TORAJS_SSA_DUMP").as_deref() == Ok("1") {
        module.print();
    }
    // Concat length folding — a concat whose result is only measured
    // and dropped becomes the sum of its operands' lengths, the kernel
    // kept on a cold arm for the RangeError past the length bound.
    // Before the two rewrites below, which would claim the pair as an
    // append. `TORAJS_CONCAT_LEN_OFF=1` skips (bisect gate).
    gated_pass("CONCAT_LEN", module, concat_len::fold_concat_lens);
    // Number-to-string concat fusion — a strict `to_str` + `concat`
    // (right operand) + `drop` triple becomes one `concat_num` call
    // that formats the digits straight into the result allocation
    // (S1-A2 attack B1). Before str_append: this is the more
    // specific shape, and a number-on-the-left temp would otherwise
    // be claimed as str_append's drop-left operand.
    // `TORAJS_CONCAT_NUM_FUSE_OFF=1` skips (bisect gate).
    gated_pass("CONCAT_NUM_FUSE", module, concat_num_fuse::fuse_concat_nums);
    // String-append ownership forwarding — an adjacent `concat` +
    // `drop-left` pair becomes one `append`, which may then grow the
    // left cell in place instead of reallocating it. Last of the
    // inst-level rewrites so inlined-in and spliced pairs are already
    // adjacent; before rc_peephole, whose window a call closes anyway.
    // `TORAJS_STR_APPEND_OFF=1` skips (bisect gate).
    gated_pass("STR_APPEND", module, str_append::rewrite_str_appends);
    // Induction-variable merging — of two counters stepped together,
    // the one read only as a scaled access offset becomes the other
    // plus a preheader constant folded into the access base. Before
    // scaled_addr, which then folds the kept counter's shift into the
    // access. `TORAJS_IV_MERGE_OFF=1` skips (bisect gate).
    gated_pass("IV_MERGE", module, iv_merge::merge_ivs);
    // Scaled-addressing fold — `LoadDyn(_, base, Shl(idx, 3))` with a
    // single-use shift becomes `LoadDynScaled8` and the shift dies
    // (S7 knife c2): the AGU does the ×8, taking the shift off the
    // address dependency chain of every array-index loop iteration.
    // After the inst-level rewrites above so spliced/inlined shapes
    // are final; before regalloc by construction (SSA→SSA).
    // `TORAJS_SCALED_ADDR_OFF=1` skips (bisect gate).
    gated_pass("SCALED_ADDR", module, scaled_addr::fold_scaled_addresses);
}

/// The pipeline tail — rc-pair elision, then the cfg-shape passes whose
/// output is exactly what codegen lays out.
pub(crate) fn finish_pipeline(
    module: &mut Module,
    facts: Option<&[HashMap<ValueId, interval::NumFact>]>,
    clones: &[(FuncId, FuncId)],
) {
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
    // Array-element borrowing — an `any[]` element read whose box is
    // only inspected while the array stays untouched keeps no share
    // of its own: the retain after the read and the release at scope
    // end both go. Before the pair peephole, which pairs what is left.
    // `TORAJS_RC_BORROW_OFF=1` skips.
    gated_pass("RC_BORROW", module, rc_borrow::borrow_array_elems);
    // RC elide peephole — after the egraph pass so pure-inst dedup /
    // identity collapse has already tightened the windows between
    // retain/release pairs. `TORAJS_RC_PEEPHOLE_OFF=1` skips (bisect
    // gate, mirrors TORAJS_INLINER_OFF).
    gated_pass("RC_PEEPHOLE", module, rc_peephole::elide_rc_pairs);
    // A retain the two passes above left moves down to the paths that
    // have not proven its value a number (`anynum::inc_sink`), so the
    // number arms of a `+` carry no cell test. After the loop
    // versioning, whose copies have the shape the walk can pass.
    // `TORAJS_ANYNUM_INC_SINK_OFF=1` skips.
    gated_pass(
        "ANYNUM_INC_SINK",
        module,
        crate::anynum::inc_sink::sink_number_retains,
    );
    // A release opening a join moves onto the edges that did not prove
    // its value a number (`anynum::dec_split`), so the number arms of
    // the `+` that feeds it carry no cell test either. After the retain
    // sinking: a retain sunk to the head of a join keeps the releases
    // behind it in place. `TORAJS_ANYNUM_DEC_SPLIT_OFF=1` skips.
    gated_pass(
        "ANYNUM_DEC_SPLIT",
        module,
        crate::anynum::dec_split::split_join_releases,
    );
    // Map lookup sharing — a repeated string-key find on the same Map
    // with no Map write between reuses the earlier index; set_str takes
    // it as a checked hint. `TORAJS_MAP_FIND_CSE_OFF=1` skips.
    gated_pass("MAP_FIND_CSE", module, map_find_cse::share_map_finds);
    // Unroll-and-jam of counted loops around a latency-bound inner loop;
    // before loop_rotate, which then rotates the loops it builds.
    let mut jammed = Vec::new();
    gated_pass("UNROLL_JAM", module, |m| {
        let (per_func, stats) = unroll_jam::jam_functions(m);
        jammed = per_func;
        stats
    });
    // A divide-bound inner loop unrolled by two with its quotients in
    // one two-lane divide, in the functions the jam left alone (its
    // lane clones are entered for a few steps each, where the pair's
    // per-entry setup costs more than it saves); before loop_rotate.
    // The counter cells keep their ids and ranges from the interval
    // analysis to here, so their facts still hold.
    gated_pass("UNROLL_PAIR", module, |m| {
        unroll_pair::pair_module(m, facts, &jammed)
    });
    // Two quotients of one divisor (the jam's lanes dividing their own
    // `n` by a shared `d`) from one two-lane divide; after the passes
    // above, which make those blocks. `TORAJS_FDIV_PAIR_OFF=1` skips.
    gated_pass("FDIV_PAIR", module, |m| fdiv_pair::pair_module(m, facts));
    strip_clone_assumes(module, clones);
    // Loop rotation — copy each top-tested loop's exit test into its
    // latch so the loop runs one conditional branch per iteration.
    // Before block_layout, which lays out the rotated body.
    // `TORAJS_LOOP_ROTATE_OFF=1` skips (bisect gate).
    gated_pass("LOOP_ROTATE", module, loop_rotate::rotate_module);
    // A concat with loop-invariant operands out of the loop it sits
    // in. After loop_rotate, whose bottom-tested loops have the body
    // in the header; before block_layout, which lays out the preheader
    // and exit blocks it adds. `TORAJS_CONCAT_LICM_OFF=1` skips.
    gated_pass("CONCAT_LICM", module, concat_licm::hoist_concats);
    // One address per symbol in a call-free run of a block, so codegen
    // folds a read and a write of one module `let` through one page;
    // after every pass that adds or moves instructions.
    // `TORAJS_GLOBAL_REF_SHARE_OFF=1` skips.
    gated_pass(
        "GLOBAL_REF_SHARE",
        module,
        global_ref_share::share_global_refs,
    );
    // Loop-body contiguity layout — last, so the block order codegen
    // sees (fall-through chains, positional liveness/spill weights)
    // is the final one. Sinks cold blocks out of loop position
    // ranges; no inst-level rewrites. `TORAJS_BLOCK_LAYOUT_OFF=1`
    // skips (bisect gate).
    gated_pass("BLOCK_LAYOUT", module, block_layout::layout_module);
    // A constant set of a cell that only its own block reads, handed on
    // by a copy that tail_dup put next to it, goes straight into the
    // copy. After block_layout, whose tail_dup makes the shape.
    // `TORAJS_CONST_COPY_FWD_OFF=1` skips.
    gated_pass(
        "CONST_COPY_FWD",
        module,
        const_copy_fwd::forward_constant_copies,
    );
}

/// The integer clones' entry assumptions, read by every analysis
/// before it, go before loop_rotate copies their blocks.
fn strip_clone_assumes(module: &mut Module, clones: &[(FuncId, FuncId)]) {
    if clones.is_empty() {
        return;
    }
    let stats = pass_timer::timed("STRIP_ASSUMES", || {
        strip_assumes::strip_assumes(module, clones)
    });
    if std::env::var("TORAJS_FN_INT_CLONE_STATS").as_deref() == Ok("1") {
        eprintln!("torajs-fn-int-clone-stats: {stats:?}");
    }
    pass_gates::dump_after("STRIP_ASSUMES", module);
}
