//! The blade-2b stub JUDGMENT (RFC 20260824-s2-5 Phase B): decide,
//! from the lowered SSA module alone, which dispatch families the
//! program can never enter — those families' arm seams (and, for a
//! provably print-quiet program, the per-family printer kernels)
//! get loud-reject stubs in the user `.o`, the default arms lose
//! their references, and the family kernels dead-strip.
//!
//! Soundness shape: `stub F ⟺ (observed mids ∪ channel-implied
//! mids) ∩ domain(F) = ∅`, computed only when no conservative-
//! fallback trigger fired. Every input errs toward KEEPING:
//!
//! - observed mids come from the closed set of mid-carrying
//!   intrinsics ([`MID_CARRIERS`] — every lowering that hands a mid
//!   to the runtime dispatcher goes through one of them);
//! - the family-domain table (`torajs_rc::any_method_family`) errs
//!   large, universal mids answering all families;
//! - runtime-internal re-dispatch (the r491 census) is covered by
//!   the PREFIX map: any emitted `__torajs_<family>_*` symbol keeps
//!   its family (collection-init adders, iterator protocol, gen
//!   steps ride the same prefixes);
//! - anything that can re-enter the dispatcher with mids the scan
//!   cannot see (dynamic-name index calls, proxies, call/apply/bind
//!   re-dispatch, any→primitive coercion's toString/valueOf world,
//!   an unknown-name mid) triggers the FALLBACK: no stubs at all.
//!
//! A wrong judgment is LOUD by construction — the stub throws a
//! named TypeError, caught by the conformance gate and the test262
//! sweep — never a silent wrong answer.

use torajs_core::ssa::{InstKind, Module, Operand};
use torajs_rc::any_method_family as fam;

/// Mid-carrying dispatch intrinsics: (symbol, index of the mid in
/// the call's operand list). Closed set — every ssa_lower site that
/// hands a compile-time mid to the runtime dispatcher calls one of
/// these (`grep any_method_id crates/torajs-core/src` enumerates
/// the sites; checker-side interns route to these on lowering).
const MID_CARRIERS: [(&str, usize); 7] = [
    ("__torajs_any_method_call", 1),
    ("__torajs_any_method_call_opt", 1),
    ("__torajs_any_method_probe", 1),
    ("__torajs_any_method_call_spread", 1),
    ("__torajs_builtin_method_cell_tagged", 1),
    ("__torajs_super_builtin_method", 1),
    ("__torajs_super_builtin_method_spread", 1),
];

/// Emitted symbols that make the observed-mid set unknowable —
/// runtime paths that re-dispatch arbitrary or runtime-interned
/// mids. Prefix match; any hit = conservative fallback (no stubs).
/// (`__torajs_ns_static_cell` left this list when the per-static
/// table landed — its constant id resolves through
/// [`torajs_rc::ns_static_judge`] instead.)
const FALLBACK_PREFIXES: [&str; 5] = [
    // `recv[key](args…)` — the key is a runtime value, the mid
    // interns at runtime (ToPropertyKey dispatch).
    "__torajs_any_index_method_call",
    // the proxy world re-enters the dispatcher with pass-through
    // mids of its own once traps get involved.
    "__torajs_proxy",
    // builtin constructors handled as VALUES (bind/call surfaces)
    // and dynamic `new (anyCtor)()` — arbitrary family entry.
    "__torajs_builtin_ctor_value",
    "__torajs_builtin_proto_method_value",
    "__torajs_anyv_construct",
];

/// Any→primitive coercion surface: these kernels run the spec
/// OrdinaryToPrimitive machinery (toString / valueOf mids) against
/// arbitrary heap receivers, `await` resolves thenables, and
/// JSON.stringify consults toJSON. The mids they re-dispatch enter
/// a family ARM only for: user objects' own valueOf/toString
/// (dynobj / struct / closure expandos), Array.prototype.toString
/// (= join, in the arr arm), and the per-family exotic coercions
/// (Date valueOf, RegExp toString, TypedArray join, Symbol/BigInt
/// throw faces) — the latter group is covered by the PREFIX map,
/// because a value of those families cannot exist without its
/// construction/usage symbols being emitted. So a coercion hit
/// keeps the OBJ-world four, not everything. (The prologue's error
/// constructors emit `anyv_to_str_pair` in every program — the
/// spec ToString(message) — which is why this must not be a
/// fallback trigger: it would kill the judgment for the empty
/// program too.)
const COERCION_PREFIXES: [&str; 11] = [
    "__torajs_anyv_to_number",
    "__torajs_anyv_number_ctor",
    "__torajs_anyv_to_str",
    "__torajs_anyv_to_display_str",
    "__torajs_any_to_bigint",
    "__torajs_any_to_object",
    "__torajs_anyv_add_pair",
    "__torajs_anyv_arith_pair",
    "__torajs_anyv_loose_eq",
    "__torajs_anyv_json_stringify",
    "__torajs_anyv_await",
];

/// Kernels that PROBE user objects internally — spec machinery that
/// reads a constructor / species / descriptor off an arbitrary
/// object and so re-enters the dispatcher with obj-world receivers
/// (defineProperty's ToNumber(value) runs a user valueOf; the
/// array species guard reads `.constructor` through the expando
/// probe). Emitting one keeps the obj-world four, same as a
/// coercion hit. (The `class_*_define` registration faces are NOT
/// here — their descriptors are compiler-built and the prologue
/// emits them in every program.)
const KERNEL_PROBE_PREFIXES: [&str; 4] = [
    "__torajs_arr_species_guard",
    "__torajs_dynobj_define",
    "__torajs_arr_define",
    "__torajs_anyv_define_props_source_gate",
];

/// What a coercion hit keeps — see [`COERCION_PREFIXES`]. Shared
/// truth with the per-static table (`torajs_rc::ns_static_judge`).
const COERCION_KEEP: u16 = fam::FAM_OBJ_WORLD;

/// Emitted symbols that put a namespace / globalThis object (or a
/// reified static cell) into the any world — the only compiler-
/// emitted entrances to the ns-static mint face. A program with
/// none of these provably never mints a namespace-static cell, so
/// the mint seam (`__torajs_ns_static_cell`) gets a loud-reject
/// stub and the whole ns-static dispatch universe dead-strips.
/// (Builtin ctor values and proxies can also reach ctor own-static
/// reads, but both are FALLBACK triggers — nothing is stubbed
/// there at all.)
const NS_WORLD_PREFIXES: [&str; 4] = [
    "__torajs_ns_static_cell",
    "__torajs_ns_object_",
    "__torajs_globalthis_object",
    "__torajs_global_eval_value",
];

/// The prologue-synthesized native-error constructors
/// (`inject_builtin_classes` — Error plus the §20.5.5/.7/.8
/// family). Their bodies carry the spec ToString(message) coercion
/// in EVERY program, so scanning them would tax the empty program
/// with the obj-world keep. They only run when something CALLS
/// them: a runtime throw passes a constant str message (no arm
/// entry), and a user-code call site is a visible `Call` to one of
/// these names — which re-applies the coercion keep. A user class
/// SHADOWING one of these names makes the ctor user-authored; its
/// call sites still hit the Call rule, so the exemption stays
/// sound (only its internal coercion observation is skipped).
const SYNTH_ERROR_FNS: [&str; 16] = [
    "__cm_Error__ctor",
    "__cm_TypeError__ctor",
    "__cm_RangeError__ctor",
    "__cm_ReferenceError__ctor",
    "__cm_SyntaxError__ctor",
    "__cm_EvalError__ctor",
    "__cm_URIError__ctor",
    "__cm_AggregateError__ctor",
    "__new_Error",
    "__new_TypeError",
    "__new_RangeError",
    "__new_ReferenceError",
    "__new_SyntaxError",
    "__new_EvalError",
    "__new_URIError",
    "__new_AggregateError",
];

/// Family-usage prefixes: an emitted `__torajs_<prefix>*` call is
/// direct evidence the program works with that family, keeping its
/// arm (and its printers) alive. This is what covers the census's
/// fixed-mid channels without a symbol-exact table: collection
/// iterable-init adders ride `map_`/`set_` (SET/ADD + the iterable
/// walk), the iterator protocol rides `any_iter`/`*_iter_`, and
/// generator steps ride `genfn_`/`closure_install_gen_proto`.
const PREFIX_FAMILIES: [(&str, u16); 30] = [
    // the array toLocaleString kernels walk Invoke(elem,
    // "toLocaleString") over ANY element value — the any-lane walk
    // directly, the typed kernels once the receiver is exotic (an
    // accessor index answers any value; r500: that walk sits behind
    // a link seam, but this judgment runs before the link knows
    // whether the seam is stubbed). Every family.
    ("__torajs_arr_any_to_locale_string", fam::FAM_ALL),
    ("__torajs_arr_join_i64_locale", fam::FAM_ALL),
    ("__torajs_arr_join_f64_locale", fam::FAM_ALL),
    ("__torajs_map_", fam::FAM_MAPSET | fam::FAM_ITER),
    ("__torajs_set_", fam::FAM_MAPSET | fam::FAM_ITER),
    ("__torajs_arr_iter_", fam::FAM_ITER),
    ("__torajs_any_iter", fam::FAM_ITER),
    ("__torajs_iter_", fam::FAM_ITER),
    ("__torajs_promise_", fam::FAM_PROMISE),
    ("__torajs_queue_microtask", fam::FAM_PROMISE),
    (
        "__torajs_array_from_async",
        fam::FAM_PROMISE | fam::FAM_ITER,
    ),
    ("__torajs_regex", fam::FAM_REGEXP),
    ("__torajs_str_match", fam::FAM_REGEXP),
    ("__torajs_str_replace", fam::FAM_REGEXP),
    ("__torajs_str_split_regex", fam::FAM_REGEXP),
    ("__torajs_str_search_regex", fam::FAM_REGEXP),
    ("__torajs_date_", fam::FAM_DATE),
    ("__torajs_arraybuffer_", fam::FAM_BUFFER),
    ("__torajs_typedarray_", fam::FAM_BUFFER),
    ("__torajs_dataview_", fam::FAM_BUFFER),
    ("__torajs_bigint_", fam::FAM_BIGINT),
    ("__torajs_symbol_", fam::FAM_SYMBOL),
    ("__torajs_weakmap_", fam::FAM_WEAK),
    ("__torajs_weakset_", fam::FAM_WEAK),
    // NOT plain `__torajs_weakref_`: `weakref_target_dying` is the
    // rc drop path's registry notification, emitted in every
    // program — only the mint/read faces mean the family is used.
    ("__torajs_weakref_create", fam::FAM_WEAK),
    ("__torajs_weakref_deref", fam::FAM_WEAK),
    // primitive wrapper objects (`new Number(x)`) view-through to
    // their primitive's family arm on method / to-primitive
    // dispatch (BooleanWrapper reads [[BooleanData]] directly and
    // needs no arm — its gate fixtures pass fully stubbed).
    ("__torajs_number_wrapper", fam::FAM_NUM),
    ("__torajs_string_wrapper", fam::FAM_STR),
    ("__torajs_genfn_", fam::FAM_CLOSURE | fam::FAM_ITER),
    (
        "__torajs_closure_install_gen_proto",
        fam::FAM_CLOSURE | fam::FAM_ITER,
    ),
];

/// Print-world symbols whose emission proves heap values can reach
/// the per-tag print dispatch — any hit keeps every printer kernel.
/// Scalar prints (`print_i64`-shape, `str_print`, typed
/// `arr_print_<scalar>`) are NOT here: they answer without the
/// per-tag inspect world.
const PRINT_WORLD: [&str; 6] = [
    "__torajs_print_anyv",
    "__torajs_fn_print_outer",
    "__torajs_map_print_outer",
    "__torajs_set_print_outer",
    "__torajs_symbol_print",
    "__torajs_anyv_struct_print",
];

/// A prologue-synthesized error-world fn: one of the base names or
/// a synthesized wrapper of one (`__boxed___cm_Error__ctor` — the
/// boxed-entry adapters end with the wrapped name). A user class
/// whose NAME ends in a base spelling (`class Foo__cm_Error`) would
/// be mis-exempted here; the cost is a loud stub TypeError caught
/// by the gate, never a silent wrong answer.
fn is_synth_error_fn(name: &str) -> bool {
    SYNTH_ERROR_FNS
        .iter()
        .any(|b| name == *b || (b.starts_with("__cm_") && name.ends_with(b)))
}

pub(crate) struct DispatchJudgment {
    /// Families whose arm seam gets a loud-reject stub (bit order =
    /// the arm roster).
    pub stub_arm_bits: u16,
    /// Stub the ns-static mint seam — the program provably never
    /// puts a namespace / ctor / globalThis object into the any
    /// world (see [`NS_WORLD_PREFIXES`]).
    pub stub_ns_static: bool,
    /// Stub the per-family printer kernels too — only for programs
    /// proven print-quiet (no path from user code into the per-tag
    /// inspect dispatch).
    pub stub_printers: bool,
}

/// Scan the lowered module and judge the stub set. Returns the
/// no-stub judgment (bits 0) on any conservative-fallback trigger.
/// `live[i]` (FuncId index; a short slice reads as all-live) marks
/// the fns the artifact carries after user-gc — a stripped fn's
/// calls are not evidence of anything (r500 A0).
pub(crate) fn judge(module: &Module, live: &[bool]) -> DispatchJudgment {
    let diag_env = std::env::var_os("TORAJS_DISPATCH_JUDGE_DIAG");
    let diag = diag_env.as_deref().is_some_and(|v| v != "0");
    // level 2: dump every distinct runtime symbol the module calls.
    let diag_syms = diag_env.as_deref().is_some_and(|v| v == "2");
    let mut seen_syms: Vec<String> = Vec::new();
    let mut observed: Vec<i64> = Vec::new();
    let mut keep_bits: u16 = 0;
    let mut fallback = false;
    let mut ns_world = false;
    let mut print_world = false;
    let mut printer_ref = false;

    for (i, f) in module.funcs.iter().enumerate() {
        if live.get(i).is_some_and(|&l| !l) {
            continue;
        }
        let synth_error_fn = is_synth_error_fn(&f.name);
        for b in &f.blocks {
            for inst in &b.insts {
                let InstKind::Call(fid, args) = &inst.kind else {
                    continue;
                };
                let name = module.funcs[fid.0 as usize].name.as_str();
                if diag_syms && name.starts_with("__torajs") && !seen_syms.iter().any(|x| x == name)
                {
                    eprintln!("[judge] sym: {} calls {name}", f.name);
                    seen_syms.push(name.to_string());
                }
                if !synth_error_fn && is_synth_error_fn(name) {
                    // constructing a native error from user-visible
                    // code: its message coercion can reach user
                    // objects. (Synth-internal edges — the factory
                    // calling its own ctor — don't count.)
                    if diag && keep_bits & COERCION_KEEP != COERCION_KEEP {
                        eprintln!("[judge] error-ctor call keep: {} calls {name}", f.name);
                    }
                    keep_bits |= COERCION_KEEP;
                }
                if let Some((_, pos)) = MID_CARRIERS.iter().find(|(n, _)| *n == name) {
                    match args.get(*pos) {
                        Some(Operand::ConstI64(mid)) => observed.push(*mid),
                        // a non-constant mid slot means a lowering
                        // changed shape under us — unknowable, punt.
                        _ => fallback = true,
                    }
                }
                if name == "__torajs_ns_static_cell" {
                    // a reified namespace static: the constant id
                    // resolves the cell kernel's modelled
                    // re-dispatch surface (per-static table).
                    match args.first() {
                        Some(Operand::ConstI64(id)) => {
                            match torajs_rc::ns_static_judge::ns_static_judge(*id) {
                                torajs_rc::ns_static_judge::NsStaticJudge::Keep(bits) => {
                                    if diag && keep_bits & bits != bits {
                                        eprintln!("[judge] ns-static keep: id {id} -> {bits:#b}");
                                    }
                                    keep_bits |= bits;
                                }
                                torajs_rc::ns_static_judge::NsStaticJudge::Print => {
                                    print_world = true;
                                }
                                torajs_rc::ns_static_judge::NsStaticJudge::Fallback => {
                                    if diag {
                                        eprintln!("[judge] ns-static fallback: id {id}");
                                    }
                                    fallback = true;
                                }
                            }
                        }
                        // a non-constant id slot: a lowering changed
                        // shape under us — unknowable, punt.
                        _ => fallback = true,
                    }
                }
                if NS_WORLD_PREFIXES.iter().any(|p| name.starts_with(p)) {
                    ns_world = true;
                }
                if FALLBACK_PREFIXES.iter().any(|p| name.starts_with(p)) {
                    if diag {
                        eprintln!("[judge] fallback trigger: {name}");
                    }
                    fallback = true;
                }
                if KERNEL_PROBE_PREFIXES.iter().any(|p| name.starts_with(p)) {
                    if diag && keep_bits & COERCION_KEEP != COERCION_KEEP {
                        eprintln!("[judge] kernel-probe keep: {} calls {name}", f.name);
                    }
                    keep_bits |= COERCION_KEEP;
                }
                if !synth_error_fn && COERCION_PREFIXES.iter().any(|p| name.starts_with(p)) {
                    if diag && keep_bits & COERCION_KEEP != COERCION_KEEP {
                        eprintln!("[judge] coercion keep: {} calls {name}", f.name);
                    }
                    keep_bits |= COERCION_KEEP;
                }
                for (p, bits) in PREFIX_FAMILIES {
                    if name.starts_with(p) {
                        if diag && keep_bits & bits != bits {
                            eprintln!("[judge] prefix keep: {name} -> {bits:#b}");
                        }
                        keep_bits |= bits;
                    }
                }
                if PRINT_WORLD.iter().any(|p| name.starts_with(p))
                    || crate::cmd_build_dispatch_stubs::is_printer_sym(name)
                {
                    print_world = true;
                }
                if crate::cmd_build_dispatch_stubs::is_arm_sym(name) {
                    // user code somehow calls an arm seam directly —
                    // not a shape we emit today; refuse to stub it.
                    printer_ref = true;
                }
            }
        }
    }

    for &mid in &observed {
        if mid == torajs_rc::ANY_METHOD_UNKNOWN
            || mid == torajs_rc::ANY_METHOD_CALL
            || mid == torajs_rc::ANY_METHOD_APPLY
            || mid == torajs_rc::ANY_METHOD_BIND
        {
            // runtime-interned names / receiver re-dispatch: the
            // effective mid set is unknowable at compile time.
            fallback = true;
        }
        keep_bits |= fam::any_method_families(mid);
    }
    if !observed.is_empty() {
        // the dynobj / struct / closure expando worlds probe USER
        // properties under any mid — receiver shape, not mid value,
        // decides entry, so any dispatch at all keeps them.
        keep_bits |= fam::FAM_DYNOBJ | fam::FAM_STRUCT | fam::FAM_CLOSURE;
    }

    if diag {
        observed.sort_unstable();
        observed.dedup();
        eprintln!(
            "[judge] observed={observed:?} keep={keep_bits:#017b} fallback={fallback} \
             printer_ref={printer_ref} print_world={print_world} ns_world={ns_world}"
        );
    }
    if fallback || printer_ref {
        return DispatchJudgment {
            stub_arm_bits: 0,
            stub_ns_static: false,
            stub_printers: false,
        };
    }
    DispatchJudgment {
        stub_arm_bits: fam::FAM_ALL & !keep_bits,
        stub_ns_static: !ns_world,
        // blade-2d granularity (per-family printer domains) comes
        // later; today printers stub only for the fully quiet shape
        // (nothing observed, no family usage, no print world).
        stub_printers: !print_world && observed.is_empty() && keep_bits == 0,
    }
}
