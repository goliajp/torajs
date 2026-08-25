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
//! Every feeder lives in the guard member and is reached from any
//! other crate through an extern symbol, so a kept verdict is
//! conservative and an assumed one is exact. Same family as r498's
//! argv-init judgment: evidence is reloc reachability, the runtime is
//! never asked to check at run time.

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};
use torajs_link::exec::{ElidableCall, GuardedStub, MemberGuard};

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

/// (shadowed symbol, guard member prefix): the stub body is `ret`.
const STUBS: [(&str, &str); 1] = [("___torajs_weakref_target_dying", "torajs_weak-")];

fn guard(prefix: &str, ignore: &[&str]) -> MemberGuard {
    MemberGuard {
        member_prefix: prefix.to_string(),
        ignore: ignore.iter().map(|s| (*s).to_string()).collect(),
    }
}

/// Every drain site in the user main body, in reloc order.
pub(crate) fn collect_elidable_calls(funcs: &[CompiledFunction]) -> Vec<ElidableCall> {
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
            Some(ElidableCall {
                func: USER_MAIN_SYM.to_string(),
                byte_offset: r.byte_offset,
                guard: guard(prefix, ignore),
                replacement: *replacement,
            })
        })
        .collect()
}

/// The hook stubs every `tr build` offers the link judgment.
pub(crate) fn guarded_stubs() -> Vec<GuardedStub> {
    STUBS
        .iter()
        .map(|(sym, prefix)| GuardedStub {
            sym: (*sym).to_string(),
            bytes: RET.to_le_bytes().to_vec(),
            guard: guard(prefix, &[sym]),
        })
        .collect()
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
        let sites = collect_elidable_calls(&funcs);
        let got: Vec<(u32, &str, u32)> = sites
            .iter()
            .map(|s| (s.byte_offset, s.guard.member_prefix.as_str(), s.replacement))
            .collect();
        assert_eq!(
            got,
            vec![
                (4, "torajs_microtask-", NOP),
                (8, "torajs_promise-", MOV_W0_ZERO),
                (12, "torajs_cycle-", NOP),
            ]
        );
        assert!(sites.iter().all(|s| s.func == USER_MAIN_SYM));
        assert_eq!(
            sites[1].guard.ignore,
            ["___torajs_promise_drop", "___torajs_promise_print"]
        );
        assert!(sites[0].guard.ignore.is_empty());
    }

    #[test]
    fn no_main_means_no_sites() {
        assert!(collect_elidable_calls(&[f("x", &["___torajs_main_exit_code"])]).is_empty());
    }

    #[test]
    fn weak_hook_stub_ignores_only_itself() {
        let stubs = guarded_stubs();
        assert_eq!(stubs.len(), 1);
        assert_eq!(stubs[0].sym, "___torajs_weakref_target_dying");
        assert_eq!(stubs[0].bytes, RET.to_le_bytes());
        assert_eq!(stubs[0].guard.member_prefix, "torajs_weak-");
        assert_eq!(stubs[0].guard.ignore, ["___torajs_weakref_target_dying"]);
    }
}
