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

use crate::cmd_build_dispatch_judge_callee::CalleeMemo;
use crate::cmd_build_dispatch_judge_obj_world::COERCION_KEEP;
use crate::cmd_build_dispatch_judge_regexp_world::{RegexpWorld, enters_protocol, unplain_mid};
use crate::cmd_build_dispatch_judge_synth_error::is_synth_error_fn;

/// Mid-carrying dispatch intrinsics: (symbol, index of the mid in
/// the call's operand list). Closed set — every ssa_lower site that
/// hands a compile-time mid to the runtime dispatcher calls one of
/// these (`grep any_method_id crates/torajs-core/src` enumerates
/// the sites; checker-side interns route to these on lowering).
pub(crate) const MID_CARRIERS: [(&str, usize); 7] = [
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
pub(crate) const FALLBACK_PREFIXES: [&str; 5] = [
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

/// Emitted symbols that put a namespace / globalThis object (or a
/// reified static cell) into the any world — the only compiler-
/// emitted entrances to the ns-static mint face. The runtime has one
/// more: a `constructor` read answers a builtin ctor cell, whose
/// statics mint through the same seam
/// ([`crate::cmd_build_dispatch_judge_ctor_reach`]). A program with
/// neither never mints a namespace-static cell, so the mint seam
/// (`__torajs_ns_static_cell`) gets a loud-reject stub and the whole
/// ns-static dispatch universe dead-strips. (Builtin ctor values and
/// proxies can also reach ctor own-static reads, but both are
/// FALLBACK triggers — nothing is stubbed there at all.)
pub(crate) const NS_WORLD_PREFIXES: [&str; 4] = [
    "__torajs_ns_static_cell",
    "__torajs_ns_object_",
    "__torajs_globalthis_object",
    "__torajs_global_eval_value",
];

/// Family-usage prefixes: an emitted `__torajs_<prefix>*` call is
/// direct evidence the program works with that family, keeping its
/// arm (and its printers) alive. This is what covers the census's
/// fixed-mid channels without a symbol-exact table: collection
/// iterable-init adders ride `map_`/`set_` (SET/ADD + the iterable
/// walk), the iterator protocol rides `any_iter`/`*_iter_`, and
/// generator steps ride `genfn_`/`closure_install_gen_proto`.
pub(crate) const PREFIX_FAMILIES: [(&str, u16); 36] = [
    // the array toLocaleString kernels walk Invoke(elem,
    // "toLocaleString") over ANY element value — the any-lane walk
    // directly, the typed kernels once the receiver is exotic (an
    // accessor index answers any value; r500: that walk sits behind
    // a link seam, but this judgment runs before the link knows
    // whether the seam is stubbed). Every family.
    ("__torajs_arr_any_to_locale_string", fam::FAM_ALL),
    // the typed lane's own entry (G13 gate): the program emits this,
    // never the join kernels behind it — a holey number[] reaches the
    // exotic walk from here (`delete a[0]; a.toLocaleString()` died
    // on the stubbed num arm).
    ("__torajs_arr_typed_to_locale_string", fam::FAM_ALL),
    ("__torajs_arr_join_i64_locale", fam::FAM_ALL),
    ("__torajs_arr_join_f64_locale", fam::FAM_ALL),
    ("__torajs_map_", fam::FAM_MAPSET | fam::FAM_ITER),
    ("__torajs_set_", fam::FAM_MAPSET | fam::FAM_ITER),
    ("__torajs_arr_iter_", fam::FAM_ITER),
    // The user-facing iteration protocol re-dispatches `next` /
    // `return` / `throw` INSIDE the runtime against whatever the
    // receiver's `@@iterator` answered — an object literal's method,
    // a dynobj field, a closure — so the mids never appear in the
    // user `.o`. Rotation 634: `var [a, b] = iterable` over a
    // hand-written iterator object reached the struct arm's loud
    // reject because nothing else in the program named that
    // family. Same reason the destructuring drain keeps the obj
    // world.
    ("__torajs_any_iter", fam::FAM_ITER | fam::FAM_OBJ_WORLD),
    ("__torajs_iter_", fam::FAM_ITER | fam::FAM_OBJ_WORLD),
    ("__torajs_dstr_", fam::FAM_ITER | fam::FAM_OBJ_WORLD),
    ("__torajs_promise_", fam::FAM_PROMISE),
    ("__torajs_queue_microtask", fam::FAM_PROMISE),
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
    ("__torajs_weakref_new_checked", fam::FAM_WEAK),
    ("__torajs_weakref_deref", fam::FAM_WEAK),
    // No program emits a finreg symbol it does not use (drop is runtime-internal).
    ("__torajs_finreg_", fam::FAM_WEAK),
    ("__torajs_weakref_subclass", fam::FAM_WEAK),
    // primitive wrapper objects (`new Number(x)`) view-through to
    // their primitive's family arm on method / to-primitive
    // dispatch (BooleanWrapper reads [[BooleanData]] directly and
    // needs no arm — its gate fixtures pass fully stubbed).
    ("__torajs_number_wrapper", fam::FAM_NUM),
    ("__torajs_string_wrapper", fam::FAM_STR),
    // §7.1.18 ToObject of an any primitive (`Object(3)`, rotation
    // 629) mints the same wrappers INSIDE the runtime kernel, so the
    // two prefixes above never appear in the user `.o` — only this
    // one does; its coercion (`Object(3) * 2`, `-Object(3)`) views
    // through to the num / str arm like a spelled-out wrapper's.
    ("__torajs_any_to_object", fam::FAM_NUM | fam::FAM_STR),
    ("__torajs_genfn_", fam::FAM_CLOSURE | fam::FAM_ITER),
    (
        "__torajs_closure_install_gen_proto",
        fam::FAM_CLOSURE | fam::FAM_ITER,
    ),
    // a number dictionary's keyed read and write run the generic read
    // (getters included) and the member-set kernel inside them: what
    // `__torajs_any_accessor_get` and `__torajs_any_member_set` keep
    // in the coercion table, one exported hop deeper than the census
    // looks
    ("__torajs_dynobj_dict_", fam::FAM_OBJ_WORLD),
];

/// Print-world symbols whose emission proves heap values can reach
/// the per-tag print dispatch — any hit keeps every printer kernel.
/// Scalar prints (`print_i64`-shape, `str_print`, typed
/// `arr_print_<scalar>`) are NOT here: they answer without the
/// per-tag inspect world — except through the expando props face,
/// see [`ANY_WRITERS`].
pub(crate) const PRINT_WORLD: [&str; 6] = [
    "__torajs_print_anyv",
    "__torajs_fn_print_outer",
    "__torajs_map_print_outer",
    "__torajs_set_print_outer",
    "__torajs_symbol_print",
    "__torajs_anyv_struct_print",
];

/// The any-world writes that can put expando props on a typed array
/// (`a.x = 5` / `a[sym] = 3` on `let a = [1, 2]`). A typed
/// `arr_print_<scalar>` then prints the props face — keys and values
/// through the inspect world — so a program that has BOTH a writer
/// and a typed array printer keeps the printer kernels (rotation
/// 561: `[ 1, 2, g: { … } ]` threw "printer kernel stripped").
pub(crate) const ANY_WRITERS: [&str; 3] = [
    "__torajs_any_member_set",
    "__torajs_any_index_set",
    // a number dictionary's keyed write: any receiver but an own
    // entry's owner reaches the member-set kernel
    "__torajs_dynobj_dict_set",
];

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
    /// Stub the RegExp protocol seam, the protocol algorithms seam
    /// and the RegExp ctor-value build seam
    /// ([`crate::cmd_build_dispatch_judge_regexp_world`]).
    pub stub_regexp_protocol: bool,
    /// The protocol seam's stub answers 0 rather than rejecting
    /// ([`crate::cmd_build_dispatch_judge_regexp_world::RegexpVerdict`]).
    pub regexp_protocol_answers_zero: bool,
    pub stub_regexp_algorithms: bool,
    pub stub_ctor_regexp: bool,
}

/// A reified namespace static: the constant id resolves the cell
/// kernel's modelled re-dispatch surface (per-static table).
fn judge_ns_static(
    args: &[Operand],
    diag: bool,
    keep_bits: &mut u16,
    print_world: &mut bool,
    fallback: &mut bool,
) {
    match args.first() {
        Some(Operand::ConstI64(id)) => match torajs_rc::ns_static_judge::ns_static_judge(*id) {
            torajs_rc::ns_static_judge::NsStaticJudge::Keep(bits) => {
                if diag && *keep_bits & bits != bits {
                    eprintln!("[judge] ns-static keep: id {id} -> {bits:#b}");
                }
                *keep_bits |= bits;
            }
            torajs_rc::ns_static_judge::NsStaticJudge::Print => {
                *print_world = true;
            }
            torajs_rc::ns_static_judge::NsStaticJudge::Fallback => {
                if diag {
                    eprintln!("[judge] ns-static fallback: id {id}");
                }
                *fallback = true;
            }
        },
        // a non-constant id slot: a lowering changed shape under us —
        // unknowable, punt.
        _ => *fallback = true,
    }
}

/// Scan the lowered module and judge the stub set. Returns the
/// no-stub judgment (bits 0) on any conservative-fallback trigger.
pub(crate) fn judge(module: &Module) -> DispatchJudgment {
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
    let mut any_writer = false;
    let mut typed_arr_print = false;
    let mut symbol_gate = false;
    let (mut regexp_world, mut regexp_ctor) = (false, false);
    // (`@@match` reached directly, some symbol minted)
    let mut match_symbol = (false, false);

    let mut memo = CalleeMemo::new(module);
    for f in &module.funcs {
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
                let facts = memo.get(module, *fid);
                if !synth_error_fn && facts.synth_error {
                    // constructing a native error from user-visible
                    // code: its message coercion can reach user
                    // objects. (Synth-internal edges — the factory
                    // calling its own ctor — don't count.)
                    if diag && keep_bits & COERCION_KEEP != COERCION_KEEP {
                        eprintln!("[judge] error-ctor call keep: {} calls {name}", f.name);
                    }
                    keep_bits |= COERCION_KEEP;
                }
                if let Some(pos) = facts.mid_slot {
                    match args.get(pos) {
                        Some(Operand::ConstI64(mid)) => observed.push(*mid),
                        // a non-constant mid slot means a lowering
                        // changed shape under us — unknowable, punt.
                        _ => fallback = true,
                    }
                }
                if facts.ns_static_cell {
                    judge_ns_static(args, diag, &mut keep_bits, &mut print_world, &mut fallback);
                }
                symbol_gate |= facts.symbol_gate;
                regexp_ctor = regexp_ctor || facts.ctor_handout.hands_out(args);
                (facts.match_symbol).observe(facts.ctor_handout, args, &mut match_symbol);
                regexp_world |= facts.mints_regexp;
                ns_world |= facts.ns_world;
                if facts.fallback {
                    if diag {
                        eprintln!("[judge] fallback trigger: {name}");
                    }
                    fallback = true;
                }
                if !synth_error_fn && facts.coercion {
                    if diag && keep_bits & COERCION_KEEP != COERCION_KEEP {
                        eprintln!("[judge] coercion keep: {} calls {name}", f.name);
                    }
                    keep_bits |= COERCION_KEEP;
                }
                let bits = facts.family_bits;
                if diag && keep_bits & bits != bits {
                    eprintln!("[judge] prefix keep: {name} -> {bits:#b}");
                }
                keep_bits |= bits;
                print_world |= facts.print_world;
                any_writer |= facts.any_writer;
                typed_arr_print |= facts.typed_arr_print;
                // user code somehow calls an arm seam directly — not
                // a shape we emit today; refuse to stub it.
                printer_ref |= facts.arm_sym;
            }
        }
    }

    if !ns_world && !fallback {
        ns_world = crate::cmd_build_dispatch_judge_ctor_reach::ctor_cell_reachable(module, diag);
    }
    // computed just above unless a fallback trigger skipped it
    let ctor_read = (ns_world || !fallback).then_some(ns_world);
    if any_writer && typed_arr_print {
        if diag {
            eprintln!("[judge] props-face keep: any writer + typed array printer");
        }
        print_world = true;
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
        // decides entry, so any dispatch at all keeps them. The array
        // arm too: a method kernel coerces its ARGUMENTS inside the
        // runtime, and an array argument's ToString enters the arr arm
        // (`s.padStart(5, [7, 8])` with `s: any` reached its loud
        // reject, rotation 691).
        keep_bits |= fam::FAM_OBJ_WORLD;
    }

    if diag {
        observed.sort_unstable();
        observed.dedup();
        eprintln!(
            "[judge] observed={observed:?} keep={keep_bits:#017b} fallback={fallback} \
             printer_ref={printer_ref} print_world={print_world} ns_world={ns_world} \
             regexp_world={regexp_world} regexp_ctor={regexp_ctor}"
        );
    }
    // a fallback trigger hides protocol ids only behind a key the
    // program spells or cannot pin
    let unseen = fallback
        && (regexp_world || regexp_ctor)
        && crate::cmd_build_dispatch_judge_regexp_unseen::unseen_protocol_call(module, diag);
    let rx = RegexpWorld {
        world: regexp_world || regexp_ctor,
        ctor: regexp_ctor,
        ctor_read,
        protocol_entry: unseen || symbol_gate || observed.iter().any(|&m| enters_protocol(m)),
        plain_candidate: !unseen && !symbol_gate && !observed.iter().any(|&m| unplain_mid(m)),
        // a minted symbol's `constructor` read is the Symbol ctor
        match_symbol: match_symbol.0 || (match_symbol.1 && (ns_world || fallback)),
    }
    .verdict(module, diag);
    if fallback || printer_ref {
        return DispatchJudgment {
            stub_arm_bits: rx.arm_bits,
            stub_ns_static: false,
            stub_printers: false,
            stub_regexp_protocol: rx.protocol,
            regexp_protocol_answers_zero: rx.protocol_answers_zero,
            stub_regexp_algorithms: rx.algorithms,
            stub_ctor_regexp: rx.ctor,
        };
    }
    DispatchJudgment {
        stub_arm_bits: (fam::FAM_ALL & !keep_bits) | rx.arm_bits,
        stub_ns_static: !ns_world,
        // blade-2d granularity (per-family printer domains) comes
        // later; today printers stub only for the fully quiet shape
        // (nothing observed, no family usage, no print world).
        stub_printers: !print_world && observed.is_empty() && keep_bits == 0,
        stub_regexp_protocol: rx.protocol,
        regexp_protocol_answers_zero: rx.protocol_answers_zero,
        stub_regexp_algorithms: rx.algorithms,
        stub_ctor_regexp: rx.ctor,
    }
}
