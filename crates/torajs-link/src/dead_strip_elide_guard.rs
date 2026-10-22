//! The link judgment's evidence test — is a [`Guard`]'s runtime
//! text live in one reachability probe. The fix-point driver is
//! `dead_strip_elide`.

use torajs_codegen::id_hash::HashSet;

use crate::dead_strip_elide::Guard;
use crate::dead_strip_reach::{MemberReach, ReachResult};

/// A member key of the closure: `(archive_idx, member_idx)`.
type Key = (usize, usize);

/// Every defined row of every `Symbols` guard name still asked, in
/// every member some probe of one link has walked. A member's rows
/// depend only on its bytes, so each member is read once per link
/// however many probes see it, and a guard asks only the members that
/// define its names instead of searching every live member for each
/// name. A guard found live is never asked again, so a member is read
/// only for the names of the guards still open when it is first seen:
/// the open set only shrinks, so every name still asked has its rows
/// in every member read so far.
pub(crate) struct GuardIndex<'n> {
    /// Every name of every `Symbols` guard, sorted, without repeats.
    names: Vec<&'n str>,
    /// Indices into `names` of the names some open guard carries.
    open: Vec<usize>,
    seen: HashSet<Key>,
    /// Per name of `names`: `(member, n_sect, n_value)` of each row.
    rows: Vec<Vec<(Key, u8, u64)>>,
}

impl<'n> GuardIndex<'n> {
    pub(crate) fn new(guards: impl IntoIterator<Item = &'n Guard>) -> Self {
        let mut names: Vec<&str> = guards
            .into_iter()
            .flat_map(|g| match g {
                Guard::Symbols(syms) => syms.as_slice(),
                Guard::Member { .. } => &[],
            })
            .map(String::as_str)
            .collect();
        names.sort_unstable();
        names.dedup();
        let rows = vec![Vec::new(); names.len()];
        GuardIndex {
            names,
            open: Vec::new(),
            seen: HashSet::default(),
            rows,
        }
    }

    /// Read the members of `reach` no earlier probe walked, for the
    /// names of `open`: the guards still to be asked, a subset of the
    /// guards open at every earlier call.
    pub(crate) fn observe<'g>(
        &mut self,
        reach: &ReachResult<'_>,
        open: impl IntoIterator<Item = &'g Guard>,
    ) {
        self.open.clear();
        for g in open {
            if let Guard::Symbols(syms) = g {
                for n in syms {
                    let i = self.index_of(n);
                    self.open.push(i);
                }
            }
        }
        self.open.sort_unstable();
        self.open.dedup();
        let (Some(&lo), Some(&hi)) = (self.open.first(), self.open.last()) else {
            return;
        };
        let (lo, hi) = (self.names[lo], self.names[hi]);
        let (mut at, mut hit) = (Vec::new(), Vec::new());
        for (&key, m) in &reach.members {
            if !self.seen.insert(key) {
                continue;
            }
            // `defined` is sorted by name: only the rows between the
            // least and the greatest open name can carry one, and the
            // shorter of the two sorted lists drives the search
            let d = &m.defined;
            let mut d = &d[d.partition_point(|e| e.0 < lo)..d.partition_point(|e| e.0 <= hi)];
            let (open, names) = (&self.open, &self.names);
            hit.clear();
            if d.len() < open.len() {
                for (j, e) in d.iter().enumerate() {
                    let first = j == 0 || d[j - 1].0 != e.0;
                    if first && let Ok(k) = open.binary_search_by(|&i| names[i].cmp(e.0)) {
                        hit.push(open[k]);
                    }
                }
            } else {
                for &i in open {
                    d = &d[d.partition_point(|e| e.0 < names[i])..];
                    let Some(e) = d.first() else { break };
                    if e.0 == names[i] {
                        hit.push(i);
                    }
                }
            }
            for &i in &hit {
                at.clear();
                name_positions(m, self.names[i], &mut at);
                self.rows[i].extend(at.iter().map(|&(s, v)| (key, s, v)));
            }
        }
    }

    /// The rows of `name`, which a guard the index was built from
    /// carries.
    fn rows_of(&self, name: &str) -> &[(Key, u8, u64)] {
        &self.rows[self.index_of(name)]
    }

    fn index_of(&self, name: &str) -> usize {
        self.names
            .binary_search(&name)
            .expect("the index holds every Symbols guard name")
    }
}

/// Is the guard's evidence live? `Member`: any text atom of a
/// matching member, other than the atoms every name of which is an
/// ignored symbol (members outside the closure are absent from the
/// map, so they answer `false`; a flag-rooted `all_live` text section
/// answers `true` without looking at names). `Symbols`: any live
/// text atom, in any member, that holds one of the names — read off
/// `idx`, which has observed `reach`.
///
/// A name belongs to the atom that covers its address. In an object
/// with `MH_SUBSECTIONS_VIA_SYMBOLS` every defined row starts an
/// atom; in any other a section is one atom and its rows sit
/// anywhere inside it. Both arms look the names up by address
/// instead of scanning the nlist per atom.
pub(crate) fn guard_live(reach: &ReachResult<'_>, guard: &Guard, idx: &GuardIndex<'_>) -> bool {
    match guard {
        Guard::Member { prefix, ignore } => {
            let mut at: Vec<(u8, u64)> = Vec::new();
            reach
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
                })
        }
        Guard::Symbols(syms) => syms.iter().any(|name| {
            idx.rows_of(name).iter().any(|&(key, sect, start)| {
                reach
                    .members
                    .get(&key)
                    .is_some_and(|m| row_live(m, sect, start))
            })
        }),
    }
}

/// Is the text atom covering `start` in section `sect` of `m` live?
fn row_live(m: &MemberReach<'_>, sect: u8, start: u64) -> bool {
    let Some(s) = (sect as usize).checked_sub(1).and_then(|i| m.sects.get(i)) else {
        return false;
    };
    let i = s.atoms.partition_point(|&(a, _)| a <= start);
    s.is_text
        && i.checked_sub(1)
            .is_some_and(|i| covers(s.atoms[i], start) && s.live[i])
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
    use crate::dead_strip_reach_table::test_support::text_reach;

    /// The `Symbols` judgment as it read before the index: every
    /// live member searched for every name.
    fn scan_symbols(reach: &ReachResult<'_>, syms: &[String]) -> bool {
        let mut at: Vec<(u8, u64)> = Vec::new();
        reach.members.values().any(|m| {
            at.clear();
            for name in syms {
                name_positions(m, name, &mut at);
            }
            at.iter().any(|&(sect, start)| row_live(m, sect, start))
        })
    }

    /// [`guard_live`] through an index that has observed `reach`;
    /// a `Symbols` answer must match [`scan_symbols`].
    fn live(reach: &ReachResult<'_>, guard: &Guard) -> bool {
        let mut idx = GuardIndex::new([guard]);
        idx.observe(reach, [guard]);
        let got = guard_live(reach, guard, &idx);
        if let Guard::Symbols(syms) = guard {
            assert_eq!(got, scan_symbols(reach, syms), "{guard}");
        }
        got
    }

    fn reach_of<'a>(members: Vec<((usize, usize), MemberReach<'a>)>) -> ReachResult<'a> {
        ReachResult {
            members: members.into_iter().collect(),
            unresolved: Default::default(),
            user_refs: Default::default(),
            preds: Default::default(),
        }
    }

    fn syms(n: &[&str]) -> Guard {
        Guard::Symbols(n.iter().map(|s| s.to_string()).collect())
    }

    /// One text member defining `rows`, its two atoms live as given.
    fn member(rows: &[(&str, u64)], live: [bool; 2]) -> MemberReach<'static> {
        let mut m = text_reach(0, 16, rows);
        assert_eq!(m.sects[0].atoms.len(), 2);
        m.sects[0].live = live.to_vec();
        m
    }

    /// A name two members define answers for whichever copy is live,
    /// and a name only a dead copy defines answers dead.
    #[test]
    fn a_name_defined_in_two_members_answers_for_each_copy() {
        let rows = [("_w", 0), ("_v", 8)];
        let both = |a, b| reach_of(vec![((0, 0), member(&rows, a)), ((1, 0), member(&rows, b))]);
        assert!(live(&both([true, false], [false, false]), &syms(&["_w"])));
        assert!(live(&both([false, false], [true, false]), &syms(&["_w"])));
        assert!(!live(&both([false, true], [false, true]), &syms(&["_w"])));
        assert!(live(
            &both([false, true], [false, false]),
            &syms(&["_w", "_v"])
        ));
        assert!(!live(
            &both([false, false], [false, false]),
            &syms(&["_w", "_v"])
        ));
    }

    /// A name no member defines is never evidence, and does not hide
    /// the other names of its guard.
    #[test]
    fn a_name_no_member_defines_answers_dead() {
        let reach = reach_of(vec![(
            (0, 0),
            member(&[("_w", 0), ("_v", 8)], [true, true]),
        )]);
        assert!(!live(&reach, &syms(&["_absent"])));
        assert!(!live(&reach, &syms(&["_a", "_z"])));
        assert!(live(&reach, &syms(&["_a", "_v", "_z"])));
        assert!(!live(&reach_of(Vec::new()), &syms(&["_w"])));
    }

    /// The index outlives probes: a member an earlier probe walked and
    /// a later one dropped, or walked again with its atoms dead, is no
    /// evidence in the later probe.
    #[test]
    fn a_member_dead_in_a_later_probe_answers_dead() {
        let rows = [("_w", 0), ("_v", 8)];
        let g = syms(&["_w"]);
        let mut idx = GuardIndex::new([&g]);
        let first = reach_of(vec![((0, 0), member(&rows, [true, false]))]);
        idx.observe(&first, [&g]);
        assert!(guard_live(&first, &g, &idx));
        let gone = reach_of(vec![(
            (1, 0),
            member(&[("_x", 0), ("_y", 8)], [true, true]),
        )]);
        idx.observe(&gone, [&g]);
        assert!(!guard_live(&gone, &g, &idx));
        let dead = reach_of(vec![((0, 0), member(&rows, [false, true]))]);
        idx.observe(&dead, [&g]);
        assert!(!guard_live(&dead, &g, &idx));
        // a member first walked in a later probe is read then
        let late = reach_of(vec![((2, 0), member(&rows, [true, false]))]);
        idx.observe(&late, [&g]);
        assert!(guard_live(&late, &g, &idx));
    }

    /// Over a run of probes and a shrinking open set, every guard
    /// answers as the scan does, whether a member's rows or the open
    /// names drive the read, and with a name a member defines twice.
    #[test]
    fn every_probe_answers_as_the_scan() {
        let rows = [("_w", 0), ("_v", 8)];
        let (gw, gv, gaz) = (syms(&["_w"]), syms(&["_v"]), syms(&["_a", "_z"]));
        let twice = |l| member(&[("_w", 0), ("_w", 8)], l);
        let probe = |a, b, c| {
            let ms = vec![((0, 0), member(&rows, a)), ((1, 0), member(&rows, b))];
            reach_of(ms.into_iter().chain([((2, 0), twice(c))]).collect())
        };
        let probes = [
            probe([false, false], [false, false], [false, false]),
            probe([false, true], [false, false], [false, true]),
            probe([false, false], [true, false], [false, false]),
            probe([true, true], [false, false], [false, false]),
        ];
        let guards = [&gw, &gv, &gaz];
        let mut idx = GuardIndex::new(guards);
        for (n, p) in probes.iter().enumerate() {
            // the open set shrinks: probe n leaves out the first n - 1
            let open = &guards[n.saturating_sub(1).min(2)..];
            idx.observe(p, open.iter().copied());
            for g in open {
                let Guard::Symbols(names) = g else {
                    unreachable!()
                };
                assert_eq!(guard_live(p, g, &idx), scan_symbols(p, names), "{n} {g}");
            }
        }
        let mut one = GuardIndex::new([&gw]);
        for p in &probes {
            one.observe(p, [&gw]);
            assert_eq!(guard_live(p, &gw, &one), scan_symbols(p, &["_w".into()]));
        }
    }

    /// A member first walked after a guard closed is read only for the
    /// guards still open, and those still answer as the scan does.
    #[test]
    fn a_guard_still_open_answers_after_another_closed() {
        let rows = [("_w", 0), ("_v", 8)];
        let (gw, gv) = (syms(&["_w"]), syms(&["_v"]));
        let mut idx = GuardIndex::new([&gw, &gv]);
        let first = reach_of(vec![((0, 0), member(&rows, [true, false]))]);
        idx.observe(&first, [&gw, &gv]);
        assert!(guard_live(&first, &gw, &idx));
        assert!(!guard_live(&first, &gv, &idx));
        let second = reach_of(vec![
            ((0, 0), member(&rows, [false, false])),
            ((1, 0), member(&rows, [false, true])),
        ]);
        idx.observe(&second, [&gv]);
        assert!(guard_live(&second, &gv, &idx));
        assert_eq!(
            guard_live(&second, &gv, &idx),
            scan_symbols(&second, &["_v".into()])
        );
        assert!(idx.rows_of("_w").iter().all(|r| r.0 == (0, 0)));
    }

    /// r506 — an atom carrying two names (MergeFunctions aliases)
    /// is evidence for BOTH writers; reading one name answered
    /// "dead" for the other and stubbed a slow path the program
    /// reached.
    #[test]
    fn aliased_anchor_answers_to_every_name() {
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
        assert!(live(&reach, &Guard::Symbols(closure.clone())));
        assert!(live(&reach, &Guard::Symbols(obj)));
        assert!(!live(&reach, &Guard::Symbols(vec!["other".into()])));
        let member = |ignore: Vec<String>| Guard::Member {
            prefix: "rc".into(),
            ignore,
        };
        assert!(live(&reach, &member(closure)));
        assert!(!live(&reach, &member(all)));
    }

    /// A name carried by two defined rows (rustc's local labels can
    /// repeat) answers at every row, not only the one the by-name
    /// table kept.
    #[test]
    fn repeated_name_answers_at_every_row() {
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
        assert!(live(&reach, &Guard::Symbols(vec!["w".into()])));
        let member = Guard::Member {
            prefix: "x".into(),
            ignore: vec!["w".into()],
        };
        assert!(!live(&reach, &member));
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

        let on = reach_with(true);
        assert!(live(&on, &Guard::Symbols(names(&["_inner"]))));
        assert!(live(&on, &Guard::Symbols(names(&["_head"]))));
        assert!(live(&on, &member_guard(&["_head"])));
        assert!(live(&on, &member_guard(&["_inner"])));
        assert!(!live(&on, &member_guard(&["_head", "_inner"])));

        let off = reach_with(false);
        assert!(!live(&off, &Guard::Symbols(names(&["_inner"]))));
        assert!(!live(&off, &member_guard(&[])));
    }
}
