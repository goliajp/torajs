//! r499 — runtime hooks the artifact can prove idle (the policy half
//! of `torajs_link::dead_strip_elide`; that module is the mechanism).
//!
//! Two shapes, one judgment ("does the feeder member have live text
//! once the hook is assumed away"):
//!
//! **Elidable call sites** — `synthesize_main` closes every program
//! with three calls that are no-ops unless something else in the
//! artifact can feed them:
//!
//! | call | fed only by | guard member |
//! |---|---|---|
//! | `__torajs_microtask_run_until_idle` | `__torajs_microtask_enqueue` | `torajs_microtask-` |
//! | `__torajs_main_exit_code` (unhandled-rejection sweep) | a promise cell being rejected | `torajs_promise-` |
//! | `__torajs_cycle_at_exit_drain` | `__torajs_cycle_buffer` (a cyclic-shape rc_dec) | `torajs_cycle-` |
//!
//! The exit-code call's replacement is `mov w0, #0` so the value the
//! `ret` reads is the clean-run code; the two drains are `void` and
//! become `nop`. A promise cell's drop and printer reach the member
//! from the generic value-drop / inspect dispatch of any program that
//! prints or drops a value; neither can reject, so they are ignored.
//!
//! **Guarded stubs** — rc-hit-zero calls
//! `__torajs_weakref_target_dying` (torajs-weak) so a `WeakRef` /
//! `WeakMap` / `WeakSet` observing the dying cell learns of it. The
//! callee returns at once when no observer was ever registered (the
//! `__torajs_weakref_active` gate), but the reference alone roots the
//! registry, and the registry's observer-invalidation path roots the
//! generic value-drop world — a third of an empty program's runtime
//! text. Observers are only ever registered by torajs-weak's own
//! entry points, so "no text of `torajs_weak-` is live other than the
//! hook itself" is exactly "no observer can exist"; the stub is then
//! a bare `ret`, the hook's own fast path.
//!
//! **Symbol-guarded stubs** (r500, RFC 20260824-s2-5 刀 4 A2/A3) —
//! the typed array kernels keep one slow path each for a receiver
//! that stopped being dense: `arr_join_*`'s exotic branch and the
//! species guard's props-bag probe, both behind seams
//! (`torajs-arr/src/exotic_seam.rs`, defaults in `torajs-dispatch`).
//! Either alone roots the whole any world (a `[1,2,3].join(",")`
//! program links 348 KB, 264 KB of it through that one branch). An
//! array only becomes exotic through `__torajs_arr_flag_exotic` and
//! only grows a props bag through `__torajs_arr_props_attach` (or the
//! regex exec attach) — `#[inline(never)]` entries of the arr crate,
//! so the evidence is those symbols' own text liveness, not the
//! member's (the arr member is live in any program with an array).
//! The stub is the loud-reject landing pad (fam ids 40+), never a
//! silent answer: a writer added later without routing through the
//! entries would surface as a named TypeError under the gate.
//!
//! **Adapter mints** (r501, 刀 4 A1) — every closure mint stores its
//! `__boxed_` any-ABI adapter's address into the cell, and the
//! adapter's per-parameter unbox roots the whole any world (one
//! directly-called closure: 397 KB against 84 KB). The runtime reads
//! that slot only through torajs-rc's `#[inline(never)]`
//! `__torajs_closure_boxed_entry`, so each mint's `adrp/add` pair
//! (codegen's `__torajs_boxed_<i>` alias — the other address-takers
//! of the same adapter, the class registries, use `__torajs_fn_<i>`
//! and are never touched) is a `SiteShape::FnAddr` site guarded on
//! that symbol; assumed, it stores 0 and the link strips the orphaned
//! adapter (`dead_strip_elide::assume`).
//!
//! Every feeder lives in the guard member (or IS the guard symbol)
//! and is reached from any other crate through an extern symbol, so
//! a kept verdict is conservative and an assumed one is exact. Same
//! family as r498's argv-init judgment: evidence is reloc
//! reachability, the runtime is never asked to check at run time.

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};
use torajs_link::exec::{ElidableSite, Guard, GuardedStub, SiteShape};

use crate::cmd_build_synthesize::USER_MAIN_SYM;

const NOP: u32 = 0xD503_201F;
/// `movz w0, #0`.
const MOV_W0_ZERO: u32 = 0x5280_0000;
/// `ret` (x30).
const RET: u32 = 0xD65F_03C0;

/// (callee symbol as the reloc names it, guard member prefix, the
/// member's entry points that cannot feed the drain, replacement
/// word).
const SITES: [(&str, &str, &[&str], u32); 3] = [
    (
        "___torajs_microtask_run_until_idle",
        "torajs_microtask-",
        &[],
        NOP,
    ),
    (
        "___torajs_main_exit_code",
        "torajs_promise-",
        &["___torajs_promise_drop", "___torajs_promise_print"],
        MOV_W0_ZERO,
    ),
    ("___torajs_cycle_at_exit_drain", "torajs_cycle-", &[], NOP),
];

/// The typed kernels' link-judged slow paths: (shadowed seam, writer
/// entries whose text liveness un-assumes the stub, landing-pad fam
/// id). The join seam's enabler is the exotic-flag writer; the
/// species probe's enablers are the two props-bag creators.
const SYMBOL_STUBS: [(&str, &[&str], u32); 4] = [
    (
        "___torajs_arr_join_exotic",
        &["___torajs_arr_flag_exotic"],
        40,
    ),
    (
        "___torajs_arr_species_guard_slow",
        &[
            "___torajs_arr_props_attach",
            "___torajs_arrprops_attach_exec3",
        ],
        41,
    ),
    // the scalar-array drop's two legs (r500 A4'): a props bag to
    // release — same enablers as the species probe — and a subclass
    // envelope to unwind — only `__torajs_arr_subclass_alloc` sets
    // FLAG_SUBCLASSED on an array.
    (
        "___torajs_arr_drop_props_slow",
        &[
            "___torajs_arr_props_attach",
            "___torajs_arrprops_attach_exec3",
        ],
        42,
    ),
    (
        "___torajs_arr_drop_subclass_slow",
        &["___torajs_arr_subclass_alloc"],
        43,
    ),
];

fn guard(prefix: &str, ignore: &[&str]) -> Guard {
    Guard::Member {
        prefix: prefix.to_string(),
        ignore: ignore.iter().map(|s| (*s).to_string()).collect(),
    }
}

/// The rc entry every boxed-entry read goes through
/// (`torajs_rc::closure_entry`).
const BOXED_ENTRY_READER: &str = "___torajs_closure_boxed_entry";

/// Every conditional site: the drain calls in the user main body,
/// then every adapter mint in any fn, each in reloc order.
pub(crate) fn collect_elidable_sites(funcs: &[CompiledFunction]) -> Vec<ElidableSite> {
    let mut out = drain_sites(funcs);
    out.extend(adapter_mint_sites(funcs));
    out
}

fn drain_sites(funcs: &[CompiledFunction]) -> Vec<ElidableSite> {
    let Some(main) = funcs.iter().find(|f| f.name == USER_MAIN_SYM) else {
        return Vec::new();
    };
    main.relocs
        .iter()
        .filter_map(|r| {
            let RelocKind::CallSite {
                target: CallTarget::Extern(name),
            } = &r.kind
            else {
                return None;
            };
            let (_, prefix, ignore, replacement) =
                SITES.iter().find(|(callee, _, _, _)| callee == name)?;
            Some(ElidableSite {
                func: USER_MAIN_SYM.to_string(),
                guard: guard(prefix, ignore),
                shape: SiteShape::Call {
                    byte_offset: r.byte_offset,
                    replacement: *replacement,
                },
            })
        })
        .collect()
}

fn adapter_mint_sites(funcs: &[CompiledFunction]) -> Vec<ElidableSite> {
    let guard = Guard::Symbols(vec![BOXED_ENTRY_READER.to_string()]);
    funcs
        .iter()
        .flat_map(|f| {
            f.relocs.iter().filter_map(|r| {
                let RelocKind::Page21 { target_sym } = &r.kind else {
                    return None;
                };
                target_sym
                    .starts_with("__torajs_boxed_")
                    .then(|| ElidableSite {
                        func: f.name.clone(),
                        guard: guard.clone(),
                        shape: SiteShape::FnAddr {
                            adrp_offset: r.byte_offset,
                            target: target_sym.clone(),
                        },
                    })
            })
        })
        .collect()
}

/// The stubs every `tr build` offers the link judgment: the weak
/// hook (`ret`, member-guarded on `torajs_weak-` ignoring itself)
/// and the symbol-guarded exotic slow paths.
pub(crate) fn guarded_stubs() -> Vec<GuardedStub> {
    let weak_hook = "___torajs_weakref_target_dying";
    let mut out = vec![GuardedStub {
        sym: weak_hook.to_string(),
        bytes: RET.to_le_bytes().to_vec(),
        relocs: Vec::new(),
        guard: guard("torajs_weak-", &[weak_hook]),
    }];
    out.extend(SYMBOL_STUBS.iter().map(|(sym, writers, fam_id)| {
        let (bytes, relocs) = crate::cmd_build_dispatch_stubs::reject_stub_body(*fam_id);
        GuardedStub {
            sym: (*sym).to_string(),
            bytes,
            relocs,
            guard: Guard::Symbols(writers.iter().map(|w| (*w).to_string()).collect()),
        }
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::frame::FrameLayout;
    use torajs_codegen::reloc::Reloc;

    fn f(name: &str, callees: &[&str]) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: vec![0; 4 * callees.len()],
            relocs: callees
                .iter()
                .enumerate()
                .map(|(i, c)| Reloc {
                    byte_offset: (4 * i) as u32,
                    kind: RelocKind::CallSite {
                        target: CallTarget::Extern((*c).into()),
                    },
                })
                .collect(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    #[test]
    fn only_main_drain_sites_are_collected() {
        let funcs = vec![
            f("helper", &["___torajs_cycle_at_exit_drain"]),
            f(
                USER_MAIN_SYM,
                &[
                    "___torajs_print_i64",
                    "___torajs_microtask_run_until_idle",
                    "___torajs_main_exit_code",
                    "___torajs_cycle_at_exit_drain",
                ],
            ),
        ];
        let sites = collect_elidable_sites(&funcs);
        let got: Vec<(u32, String, u32)> = sites
            .iter()
            .map(|s| {
                let SiteShape::Call {
                    byte_offset,
                    replacement,
                } = s.shape
                else {
                    panic!("drain sites are calls");
                };
                (byte_offset, s.guard.to_string(), replacement)
            })
            .collect();
        assert_eq!(
            got,
            vec![
                (4, "member:torajs_microtask-".to_string(), NOP),
                (8, "member:torajs_promise-".to_string(), MOV_W0_ZERO),
                (12, "member:torajs_cycle-".to_string(), NOP),
            ]
        );
        assert!(sites.iter().all(|s| s.func == USER_MAIN_SYM));
        let Guard::Member { ignore, .. } = &sites[1].guard else {
            panic!("exit-code site is member-guarded");
        };
        assert_eq!(
            ignore,
            &["___torajs_promise_drop", "___torajs_promise_print"]
        );
        let Guard::Member { ignore, .. } = &sites[0].guard else {
            panic!("microtask site is member-guarded");
        };
        assert!(ignore.is_empty());
    }

    #[test]
    fn no_main_means_no_drain_sites() {
        assert!(collect_elidable_sites(&[f("x", &["___torajs_main_exit_code"])]).is_empty());
    }

    #[test]
    fn adapter_mints_are_symbol_guarded_fn_addr_sites_in_any_fn() {
        let pair = |off: u32, sym: &str| {
            [
                Reloc {
                    byte_offset: off,
                    kind: RelocKind::Page21 {
                        target_sym: sym.into(),
                    },
                },
                Reloc {
                    byte_offset: off + 4,
                    kind: RelocKind::PageOff12 {
                        target_sym: sym.into(),
                    },
                },
            ]
        };
        let mut helper = f("helper", &[]);
        helper.bytes = vec![0; 16];
        helper.relocs = pair(0, "__torajs_boxed_3")
            .into_iter()
            .chain(pair(8, "__torajs_fn_3"))
            .collect();
        let sites = collect_elidable_sites(&[f(USER_MAIN_SYM, &[]), helper]);
        assert_eq!(sites.len(), 1, "the plain fn_addr alias is never a site");
        assert_eq!(sites[0].func, "helper");
        assert_eq!(
            sites[0].guard.to_string(),
            "syms:___torajs_closure_boxed_entry"
        );
        let SiteShape::FnAddr {
            adrp_offset,
            ref target,
        } = sites[0].shape
        else {
            panic!("mint is a fn-addr site");
        };
        assert_eq!((adrp_offset, target.as_str()), (0, "__torajs_boxed_3"));
    }

    #[test]
    fn weak_hook_stub_ignores_only_itself() {
        let stubs = guarded_stubs();
        assert_eq!(stubs[0].sym, "___torajs_weakref_target_dying");
        assert_eq!(stubs[0].bytes, RET.to_le_bytes());
        assert!(stubs[0].relocs.is_empty());
        assert_eq!(stubs[0].guard.to_string(), "member:torajs_weak-");
        let Guard::Member { ignore, .. } = &stubs[0].guard else {
            panic!("weak hook is member-guarded");
        };
        assert_eq!(ignore, &["___torajs_weakref_target_dying"]);
    }

    #[test]
    fn exotic_slow_path_stubs_are_symbol_guarded_loud_rejects() {
        let stubs = guarded_stubs();
        assert_eq!(stubs.len(), 5);
        let join = &stubs[1];
        assert_eq!(join.sym, "___torajs_arr_join_exotic");
        assert_eq!(join.guard.to_string(), "syms:___torajs_arr_flag_exotic");
        // movz x7, #40 ; b <reject>
        assert_eq!(
            &join.bytes[..4],
            &(0xD280_0000u32 | (40 << 5) | 7).to_le_bytes()
        );
        assert_eq!(&join.bytes[4..], &[0x00, 0x00, 0x00, 0x14]);
        assert_eq!(join.relocs.len(), 1);
        assert_eq!(join.relocs[0].byte_offset, 4);
        let species = &stubs[2];
        assert_eq!(species.sym, "___torajs_arr_species_guard_slow");
        assert_eq!(
            species.guard.to_string(),
            "syms:___torajs_arr_props_attach|___torajs_arrprops_attach_exec3"
        );
        assert_eq!(
            &species.bytes[..4],
            &(0xD280_0000u32 | (41 << 5) | 7).to_le_bytes()
        );
        assert_eq!(stubs[3].sym, "___torajs_arr_drop_props_slow");
        assert_eq!(stubs[3].guard.to_string(), species.guard.to_string());
        assert_eq!(stubs[4].sym, "___torajs_arr_drop_subclass_slow");
        assert_eq!(
            stubs[4].guard.to_string(),
            "syms:___torajs_arr_subclass_alloc"
        );
        assert_eq!(
            &stubs[4].bytes[..4],
            &(0xD280_0000u32 | (43 << 5) | 7).to_le_bytes()
        );
    }
}
