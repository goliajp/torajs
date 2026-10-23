//! The symbol-guarded stub table of [`super`], moved out of it whole.

/// The typed kernels' link-judged slow paths: (shadowed seam, writer
/// entries whose text liveness un-assumes the stub, landing-pad fam
/// id). The join seam's enabler is the exotic-flag writer; the
/// species probe's enablers are the two props-bag creators.
pub(super) const SYMBOL_STUBS: [(&str, &[&str], u32); 20] = [
    (
        "___torajs_arr_join_exotic",
        &["___torajs_arr_flag_exotic"],
        40,
    ),
    ("___torajs_arr_species_guard_slow", &BAG_WRITERS, 41),
    // the scalar-array drop's two legs (r500 A4'): a props bag to
    // release — same enablers as the species probe — and a subclass
    // envelope to unwind — only `__torajs_arr_subclass_alloc` sets
    // FLAG_SUBCLASSED on an array.
    ("___torajs_arr_drop_props_slow", &BAG_WRITERS, 42),
    (
        "___torajs_arr_drop_subclass_slow",
        &["___torajs_arr_subclass_alloc"],
        43,
    ),
    // the closure env-drop's props leg (A5): a bag to release — only
    // `__torajs_closure_props_attach` gives a user closure one.
    (
        "___torajs_closure_drop_props_slow",
        &["___torajs_closure_props_attach"],
        44,
    ),
    // the uncaught reporter's Error-instance rendering (A6): FLAG_
    // ERROR is set only by Error-derived `__new_<C>` factories, and
    // every such class registers itself through
    // `__torajs_register_native_error` in the program prologue — the
    // runtime's own raisers go through those registered factories
    // and fall back to a bare Str when none is registered.
    (
        "___torajs_uncaught_error_render_slow",
        &["___torajs_register_native_error"],
        46,
    ),
    // a struct instance's expando legs (r502, the A5 shape on
    // `Tag::Obj`): a bag to release and — for a fields-all-scalar
    // instance — a bag to buffer a cycle through; only
    // `__torajs_obj_props_attach` gives an instance a bag.
    (
        "___torajs_obj_props_drop_slow",
        &["___torajs_obj_props_attach"],
        47,
    ),
    (
        "___torajs_obj_buffer_slow",
        &["___torajs_obj_props_attach"],
        48,
    ),
    // the typed field-write guard (r503): a struct cell's store is
    // anything but one store only under FLAG_FROZEN (written by
    // `__torajs_obj_freeze` alone) or FLAG_OBJ_EXOTIC_FIELD (written
    // by `__torajs_obj_flag_exotic_field` alone); the read guard asks
    // only the second bit.
    (
        "___torajs_obj_field_set_exotic",
        &["___torajs_obj_freeze", "___torajs_obj_flag_exotic_field"],
        50,
    ),
    (
        "___torajs_obj_field_get_exotic",
        &["___torajs_obj_flag_exotic_field"],
        57,
    ),
    // the typed sort lane's exotic divert: taken only under
    // FLAG_ARR_EXOTIC_INDEX (`__torajs_arr_flag_exotic` alone) or
    // FLAG_ARR_ANY | FLAG_ARR_ANY_HOLEY (`__torajs_arr_flag_any_holey`
    // alone); the kernel behind it is the whole any-world sort walk.
    (
        "___torajs_arr_sort_exotic",
        &["___torajs_arr_flag_exotic", "___torajs_arr_flag_any_holey"],
        51,
    ),
    // a typed array's read at a non-integral index, slow leg: taken
    // only for a cell with a props bag (the two bag creators), under
    // a minted builtin prototype (the one mint entry) or wearing
    // FLAG_SUBCLASSED (`__torajs_arr_subclass_alloc` alone).
    (
        "___torajs_arr_typed_nonint_f64_slow",
        &NAMED_PROP_WRITERS,
        52,
    ),
    (
        "___torajs_arr_typed_nonint_str_slow",
        &NAMED_PROP_WRITERS,
        53,
    ),
    // the static map lane's hole arm in the loop copy a holey source
    // runs: reached only after the header probe found the exotic,
    // slot-hole or sparse-tail bit, one writer entry each.
    (
        "___torajs_arr_map_hole_slow",
        &[
            "___torajs_arr_flag_exotic",
            "___torajs_arr_flag_any_holey",
            "___torajs_arr_flag_sparse_tail",
        ],
        54,
    ),
    // the `boolean[]` / pointer-element twin of the typed keyed read
    // (fam 52 / 53), same enablers.
    (
        "___torajs_arr_typed_nonint_any_slow",
        &NAMED_PROP_WRITERS,
        56,
    ),
    // a RegExp's lastIndex read in boxed form (a non-number stored
    // verbatim): only the any lane's store and the descriptor define
    // put a value there; numeric form never calls the seam.
    (
        "___torajs_regex_last_index_boxed_num",
        &[
            "___torajs_regex_last_index_store_boxed",
            "___torajs_regex_last_index_define",
        ],
        60,
    ),
    // `shift` on a non-writable length that is not frozen: the flag's
    // one writer outside a freeze is the descriptor define.
    (
        "___torajs_arr_shift_ro_length_slow",
        &["___torajs_arr_flag_length_ro"],
        61,
    ),
    // the unhandled-rejection reporter's Error-instance rendering, the
    // uncaught one's (fam 46) twin on the same guard: the exit sweep
    // that reaches it is live in every program that can reject.
    (
        "___torajs_unhandled_error_render_slow",
        &["___torajs_register_native_error"],
        62,
    ),
    // the thenable probe on a handler's or an async body's object
    // return: only a call site with an object or `any` return form
    // (the compiler roots the mark entry for one) or a handler's any
    // twin hands the settle an object.
    (
        "___torajs_promise_settle_thenable_slow",
        &[
            "___torajs_promise_obj_return_mark",
            "___torajs_then_twin_run",
        ],
        63,
    ),
    // `in` along a re-parented struct's user link: only the flag's one
    // writer makes a struct cell carry one.
    (
        "___torajs_struct_lacks_in_user_proto_slow",
        &["___torajs_struct_user_proto_raise"],
        64,
    ),
];

/// Every entry that turns an array's NULL props slot into a bag; the
/// index-only attach (holes) is a bag writer but never a named one.
const BAG_WRITERS: [&str; 3] = [
    "___torajs_arr_props_attach",
    "___torajs_arrprops_attach_exec3",
    "___torajs_arr_props_attach_index",
];

const NAMED_PROP_WRITERS: [&str; 4] = [
    "___torajs_arr_props_attach",
    "___torajs_arrprops_attach_exec3",
    "___torajs_get_builtin_prototype",
    "___torajs_arr_subclass_alloc",
];

/// Seams whose stub answers 0 instead of rejecting: the call is made on
/// every road through its kernel, and with the guard dead 0 is the
/// exact answer. (shadowed seam, entries whose text liveness
/// un-assumes the stub)
pub(super) const ZERO_STUBS: [(&str, &[&str]); 1] = [
    // the thenable probe on a combinator's result: any combinator entry
    // but `Promise.all`'s proved one, which stands on the `then` closure
    // proof, can settle a value that has a `then`. Every one of the
    // others runs §27.2.4.7 PromiseResolve over its elements through
    // the any lane's kernel (the sync entries on an any-shape input, the
    // dyn ones on every element), which the proved entry never calls, so
    // that one name stands for all eleven, and every `tr build` pays the
    // judgment for one guard name instead of eleven.
    (
        "___torajs_combinator_result_thenable_slow",
        &["___torajs_anyv_promise_resolve"],
    ),
];
