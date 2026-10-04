//! The link judgment's evidence test — is a [`Guard`]'s runtime
//! text live in one reachability probe. The fix-point driver is
//! `dead_strip_elide`.

use crate::dead_strip_elide::Guard;
use crate::dead_strip_reach::{MemberReach, ReachResult};

/// Is the guard's evidence live? `Member`: any text atom of a
/// matching member, other than the atoms anchored by an ignored
/// symbol (members outside the closure are absent from the map, so
/// they answer `false`; a flag-rooted `all_live` text section
/// answers `true` without looking at names). `Symbols`: any live
/// text atom, in any member, anchored by one of the names.
///
/// Every defined row's address starts an atom and an atom's anchor
/// is a row at its own start (or none, for the leading gap), so both
/// arms look names up by address instead of scanning the nlist per
/// atom.
pub(crate) fn guard_live(reach: &ReachResult<'_>, guard: &Guard) -> bool {
    let mut at: Vec<(u8, u64)> = Vec::new();
    match guard {
        Guard::Member { prefix, ignore } => reach
            .members
            .values()
            .filter(|m| m.member_name.starts_with(prefix))
            .any(|m| {
                at.clear();
                for name in ignore {
                    name_positions(m, name, &mut at);
                }
                m.sects.iter().enumerate().any(|(si, s)| {
                    let sect = (si + 1) as u8;
                    s.is_text
                        && (s.all_live
                            || s.atoms.iter().zip(&s.live).any(|(&(start, _), &l)| {
                                l && !(at.contains(&(sect, start))
                                    && anchor_all_named(m, sect, start, ignore))
                            }))
                })
            }),
        Guard::Symbols(syms) => reach.members.values().any(|m| {
            at.clear();
            for name in syms {
                name_positions(m, name, &mut at);
            }
            at.iter().any(|&(sect, start)| {
                let Some(s) = (sect as usize).checked_sub(1).and_then(|i| m.sects.get(i)) else {
                    return false;
                };
                let i = s.atoms.partition_point(|&(a, _)| a < start);
                s.is_text && s.atoms.get(i).is_some_and(|&(a, _)| a == start) && s.live[i]
            })
        }),
    }
}

/// Push every `(n_sect, n_value)` a defined row named `name` sits at.
fn name_positions(m: &MemberReach<'_>, name: &str, out: &mut Vec<(u8, u64)>) {
    use torajs_obj::{N_SECT, N_TYPE};
    if m.dup_defined.binary_search(&name).is_ok() {
        out.extend(
            m.nlist
                .iter()
                .filter(|n| n.n_type & N_TYPE == N_SECT && n.name == name)
                .map(|n| (n.n_sect, n.n_value)),
        );
    } else if let Some(&p) = m.defined.get(name) {
        out.push(p);
    }
}

/// Every defined symbol anchored at `start`'s atom — the nearest
/// `n_value <= start` in this section, and ALL the names that share
/// it. One atom can carry several names: LLVM's MergeFunctions
/// aliases identical bodies onto one address (`__torajs_closure_
/// props_attach` / `__torajs_obj_props_attach` are both
/// `str x1, [x0, #24]; ret`), and rustc emits `ltmp` labels beside
/// the real name. A judgment that read only one of them answered
/// "dead" for the other writer while its atom was live — the
/// release-only `closure props drop stripped` reject (r506).
fn anchor_names<'m>(
    m: &'m MemberReach<'_>,
    n_sect: u8,
    start: u64,
) -> impl Iterator<Item = &'m str> {
    use torajs_obj::{N_SECT, N_TYPE};
    let anchor = m
        .nlist
        .iter()
        .filter(|n| n.n_type & N_TYPE == N_SECT && n.n_sect == n_sect && n.n_value <= start)
        .map(|n| n.n_value)
        .max();
    m.nlist
        .iter()
        .filter(move |n| {
            n.n_type & N_TYPE == N_SECT && n.n_sect == n_sect && Some(n.n_value) == anchor
        })
        .map(|n| n.name)
}

/// Are ALL of the atom's names in `names`? The `Member` guard's
/// ignore test — an atom is only "just the writer entry" when every
/// name it carries is one; a merged non-writer body counts as
/// member liveness.
fn anchor_all_named(m: &MemberReach<'_>, n_sect: u8, start: u64, names: &[String]) -> bool {
    let mut any = false;
    for a in anchor_names(m, n_sect, start) {
        any = true;
        if !names.iter().any(|i| i == a) {
            return false;
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r506 — an atom carrying two names (MergeFunctions aliases)
    /// is evidence for BOTH writers; reading one name answered
    /// "dead" for the other and stubbed a slow path the program
    /// reached.
    #[test]
    fn aliased_anchor_answers_to_every_name() {
        use crate::dead_strip_reach::{MemberReach, NlEntry, SectAtoms};
        use torajs_obj::N_SECT;
        let names = [
            "___torajs_closure_props_attach",
            "___torajs_obj_props_attach",
            "ltmp3",
        ];
        let nlist: Vec<NlEntry<'_>> = names
            .iter()
            .copied()
            .map(|n| NlEntry {
                name: n,
                n_type: N_SECT,
                n_sect: 1,
                n_desc: 0,
                n_value: 0x4800,
            })
            .collect();
        let m = MemberReach {
            member_name: "rc.o",
            sects: vec![SectAtoms {
                addr: 0x4000,
                size: 0x1000,
                file_off: 0,
                no_file_storage: false,
                flags: 0,
                sectname: *b"__text\0\0\0\0\0\0\0\0\0\0",
                is_text: true,
                atoms: vec![(0x4000, 0x4800), (0x4800, 0x5000)],
                live: vec![false, true],
                all_live: false,
            }],
            defined: nlist
                .iter()
                .map(|n| (n.name, (n.n_sect, n.n_value)))
                .collect(),
            nlist,
            dup_defined: Vec::new(),
        };
        let closure = vec!["___torajs_closure_props_attach".to_string()];
        let obj = vec!["___torajs_obj_props_attach".to_string()];
        let all: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        // Ignore-list semantics: the atom is "just the writer" only
        // when every name it carries is ignored.
        assert!(!anchor_all_named(&m, 1, 0x4800, &closure));
        assert!(anchor_all_named(&m, 1, 0x4800, &all));
        // An atom with no anchoring symbol is never "all named".
        assert!(!anchor_all_named(&m, 1, 0x4000, &all));

        let reach = ReachResult {
            members: [((0, 0), m)].into_iter().collect(),
            unresolved: Default::default(),
            user_refs: Default::default(),
            preds: Default::default(),
        };
        assert!(guard_live(&reach, &Guard::Symbols(closure.clone())));
        assert!(guard_live(&reach, &Guard::Symbols(obj)));
        assert!(!guard_live(&reach, &Guard::Symbols(vec!["other".into()])));
        let member = |ignore: Vec<String>| Guard::Member {
            prefix: "rc".into(),
            ignore,
        };
        assert!(guard_live(&reach, &member(closure)));
        assert!(!guard_live(&reach, &member(all)));
    }

    /// A name carried by two defined rows (rustc's local labels can
    /// repeat) answers at every row, not only the one the by-name
    /// table kept.
    #[test]
    fn repeated_name_answers_at_every_row() {
        use crate::dead_strip_reach::{MemberReach, NlEntry, SectAtoms};
        use torajs_obj::N_SECT;
        let row = |n_value| NlEntry {
            name: "w",
            n_type: N_SECT,
            n_sect: 1,
            n_desc: 0,
            n_value,
        };
        let m = MemberReach {
            member_name: "x.o",
            sects: vec![SectAtoms {
                addr: 0,
                size: 16,
                file_off: 0,
                no_file_storage: false,
                flags: 0,
                sectname: *b"__text\0\0\0\0\0\0\0\0\0\0",
                is_text: true,
                atoms: vec![(0, 8), (8, 16)],
                live: vec![true, false],
                all_live: false,
            }],
            nlist: vec![row(0), row(8)],
            defined: [("w", (1, 8))].into_iter().collect(),
            dup_defined: vec!["w"],
        };
        let reach = ReachResult {
            members: [((0, 0), m)].into_iter().collect(),
            unresolved: Default::default(),
            user_refs: Default::default(),
            preds: Default::default(),
        };
        assert!(guard_live(&reach, &Guard::Symbols(vec!["w".into()])));
        let member = Guard::Member {
            prefix: "x".into(),
            ignore: vec!["w".into()],
        };
        assert!(!guard_live(&reach, &member));
    }
}
