//! The link judgment's evidence test — is a [`Guard`]'s runtime
//! text live in one reachability probe. The fix-point driver is
//! `dead_strip_elide`.

use crate::dead_strip_elide::Guard;
use crate::dead_strip_reach::{MemberReach, ReachResult};

/// Is the guard's evidence live? `Member`: any text atom of a
/// matching member, other than the atoms every name of which is an
/// ignored symbol (members outside the closure are absent from the
/// map, so they answer `false`; a flag-rooted `all_live` text section
/// answers `true` without looking at names). `Symbols`: any live
/// text atom, in any member, that holds one of the names.
///
/// A name belongs to the atom that covers its address. In an object
/// with `MH_SUBSECTIONS_VIA_SYMBOLS` every defined row starts an
/// atom; in any other a section is one atom and its rows sit
/// anywhere inside it. Both arms look the names up by address
/// instead of scanning the nlist per atom.
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
                            || s.atoms.iter().zip(&s.live).any(|(&atom, &l)| {
                                l && !(at.iter().any(|&(n, v)| n == sect && covers(atom, v))
                                    && atom_all_named(m, sect, atom, ignore))
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
                let i = s.atoms.partition_point(|&(a, _)| a <= start);
                s.is_text
                    && i.checked_sub(1)
                        .is_some_and(|i| covers(s.atoms[i], start) && s.live[i])
            })
        }),
    }
}

/// Push every `(n_sect, n_value)` a defined row named `name` sits at.
fn name_positions(m: &MemberReach<'_>, name: &str, out: &mut Vec<(u8, u64)>) {
    use torajs_obj::{N_SECT, N_TYPE};
    if m.is_dup_defined(name) {
        out.extend(
            m.nlist
                .iter()
                .filter(|n| n.n_type & N_TYPE == N_SECT && n.name == name)
                .map(|n| (n.n_sect, n.n_value)),
        );
    } else if let Some(p) = m.defined(name) {
        out.push(p);
    }
}

/// Does the atom hold an address? An empty atom (a row at the
/// section's end) holds the address it sits at.
fn covers((start, end): (u64, u64), v: u64) -> bool {
    start <= v && (v < end || v == start)
}

/// Every defined symbol inside the atom `[start, end)` of section
/// `n_sect`. One atom can carry several names at one address: LLVM's
/// MergeFunctions aliases identical bodies onto one address
/// (`__torajs_closure_props_attach` / `__torajs_obj_props_attach` are
/// both `str x1, [x0, #24]; ret`), and rustc emits `ltmp` labels
/// beside the real name. A judgment that read only one of them
/// answered "dead" for the other writer while its atom was live — the
/// release-only `closure props drop stripped` reject (r506). An atom
/// of an object without subsections-via-symbols is the whole section
/// and carries every name in it.
fn atom_names<'m>(
    m: &'m MemberReach<'_>,
    n_sect: u8,
    atom: (u64, u64),
) -> impl Iterator<Item = &'m str> {
    use torajs_obj::{N_SECT, N_TYPE};
    m.nlist
        .iter()
        .filter(move |n| {
            n.n_type & N_TYPE == N_SECT && n.n_sect == n_sect && covers(atom, n.n_value)
        })
        .map(|n| n.name)
}

/// Are ALL of the atom's names in `names`? The `Member` guard's
/// ignore test — an atom is only "just the writer entry" when every
/// name it carries is one; a merged non-writer body, or other code
/// sharing the writer's one-atom section, counts as member liveness.
fn atom_all_named(m: &MemberReach<'_>, n_sect: u8, atom: (u64, u64), names: &[String]) -> bool {
    let mut any = false;
    for a in atom_names(m, n_sect, atom) {
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
        use crate::dead_strip_reach_table::test_support::text_reach;
        let names = [
            "___torajs_closure_props_attach",
            "___torajs_obj_props_attach",
            "ltmp3",
        ];
        let rows: Vec<(&str, u64)> = names.iter().map(|&n| (n, 0x4800)).collect();
        let mut m = text_reach(0x4000, 0x1000, &rows);
        m.member_name = crate::archive::MemberName::new("rc.o");
        assert_eq!(m.sects[0].atoms.len(), 2);
        m.sects[0].live = vec![false, true];
        let closure = vec!["___torajs_closure_props_attach".to_string()];
        let obj = vec!["___torajs_obj_props_attach".to_string()];
        let all: Vec<String> = names.iter().map(|s| s.to_string()).collect();
        // Ignore-list semantics: the atom is "just the writer" only
        // when every name it carries is ignored.
        assert!(!atom_all_named(&m, 1, (0x4800, 0x5000), &closure));
        assert!(atom_all_named(&m, 1, (0x4800, 0x5000), &all));
        // An atom with no symbol in it is never "all named".
        assert!(!atom_all_named(&m, 1, (0x4000, 0x4800), &all));

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
        use crate::dead_strip_reach_table::test_support::text_reach;
        let mut m = text_reach(0, 16, &[("w", 0), ("w", 8)]);
        m.member_name = crate::archive::MemberName::new("x.o");
        assert_eq!(m.sects[0].atoms.len(), 2);
        m.sects[0].live = vec![true, false];
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

    /// An object without subsections-via-symbols is one atom per
    /// section, so a name past the section's start belongs to that
    /// atom, and an ignored name at the start does not make the whole
    /// section "just the writer".
    #[test]
    fn a_name_inside_a_whole_section_atom_answers_for_it() {
        use crate::archive::ArMember;
        use crate::dead_strip_reach_build::build_member_reach;
        use torajs_codegen::CompiledFunction;
        use torajs_codegen::frame::FrameLayout;
        let leaf = |name: &str| CompiledFunction {
            name: name.into(),
            bytes: vec![0xC0, 0x03, 0x5F, 0xD6],
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        };
        let obj = torajs_obj::write_object(&[leaf("_head"), leaf("_inner")]);
        let member = ArMember {
            name: crate::archive::MemberName::new("x.o"),
            data: &obj,
        };
        let reach_with = |live: bool| {
            let mut m = build_member_reach(&member).unwrap();
            let text = m.sects.iter_mut().find(|s| s.is_text).unwrap();
            assert_eq!(text.atoms.len(), 1);
            text.live[0] = live;
            ReachResult {
                members: [((0, 0), m)].into_iter().collect(),
                unresolved: Default::default(),
                user_refs: Default::default(),
                preds: Default::default(),
            }
        };
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let member_guard = |ignore: &[&str]| Guard::Member {
            prefix: "x".into(),
            ignore: names(ignore),
        };

        let live = reach_with(true);
        assert!(guard_live(&live, &Guard::Symbols(names(&["_inner"]))));
        assert!(guard_live(&live, &Guard::Symbols(names(&["_head"]))));
        assert!(guard_live(&live, &member_guard(&["_head"])));
        assert!(guard_live(&live, &member_guard(&["_inner"])));
        assert!(!guard_live(&live, &member_guard(&["_head", "_inner"])));

        let dead = reach_with(false);
        assert!(!guard_live(&dead, &Guard::Symbols(names(&["_inner"]))));
        assert!(!guard_live(&dead, &member_guard(&[])));
    }
}
