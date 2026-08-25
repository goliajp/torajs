//! r499 — main's end-of-program drains on demand (the policy half of
//! `torajs_link::dead_strip_elide`; that module is the mechanism).
//!
//! `synthesize_main` closes every program with three calls that are
//! no-ops unless something else in the artifact can feed them:
//!
//! | call | fed only by | guard member |
//! |---|---|---|
//! | `__torajs_microtask_run_until_idle` | `__torajs_microtask_enqueue` | `torajs_microtask-` |
//! | `__torajs_main_exit_code` (unhandled-rejection sweep) | a promise cell being rejected | `torajs_promise-` |
//! | `__torajs_cycle_at_exit_drain` | `__torajs_cycle_buffer` (a cyclic-shape rc_dec) | `torajs_cycle-` |
//!
//! Every feeder lives in the guard member and is reached from any
//! other crate through an extern symbol, so "that member has live
//! text once the drain's own edge is ignored" is exactly "someone
//! can feed the drain". A kept verdict is conservative (a promise
//! that can only resolve still keeps the sweep); an elided one is
//! exact. The exit-code call's replacement is `mov w0, #0` so the
//! value the `ret` reads is the clean-run code; the two drains are
//! `void` and become `nop`.
//!
//! Same family as r498's argv-init judgment: evidence is reloc
//! reachability, the runtime is never asked to check at run time.

use torajs_codegen::CompiledFunction;
use torajs_codegen::reloc::{CallTarget, RelocKind};
use torajs_link::exec::ElidableCall;

use crate::cmd_build_synthesize::USER_MAIN_SYM;

const NOP: u32 = 0xD503_201F;
/// `movz w0, #0`.
const MOV_W0_ZERO: u32 = 0x5280_0000;

/// (callee symbol as the reloc names it, guard member prefix, the
/// member's entry points that cannot feed the drain, replacement
/// word). A promise cell's drop and printer reach the member from
/// the generic value-drop / inspect dispatch of any program that
/// prints or drops a value; neither can reject a promise.
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
            let (_, guard, ignore, replacement) =
                SITES.iter().find(|(callee, _, _, _)| callee == name)?;
            Some(ElidableCall {
                func: USER_MAIN_SYM.to_string(),
                byte_offset: r.byte_offset,
                guard_member_prefix: (*guard).to_string(),
                guard_ignore: ignore.iter().map(|s| (*s).to_string()).collect(),
                replacement: *replacement,
            })
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
            .map(|s| (s.byte_offset, s.guard_member_prefix.as_str(), s.replacement))
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
            sites[1].guard_ignore,
            ["___torajs_promise_drop", "___torajs_promise_print"]
        );
        assert!(sites[0].guard_ignore.is_empty());
    }

    #[test]
    fn no_main_means_no_sites() {
        assert!(collect_elidable_calls(&[f("x", &["___torajs_main_exit_code"])]).is_empty());
    }
}
