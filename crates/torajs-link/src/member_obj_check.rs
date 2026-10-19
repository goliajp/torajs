//! Test support: a member read through its cut against the object the
//! cut writes out.

use crate::archive::ArMember;
use crate::dead_strip_cut::Cut;
use crate::dead_strip_rewrite::DeadRowPlacement;
use crate::dead_strip_rewrite_sections::SectionCut;
use crate::member_obj::MemberObj;
use crate::member_sections::is_no_file_storage;

/// Cut `cuts` out of member `name` / `bytes` and check that reading
/// the member through the cut gives what parsing the written object
/// gives: every section header, reloc table, payload, row and name,
/// the defined and undef externs, the dead rows and the listed
/// symbols. Returns how many sections the cut shortened, `None`
/// when it changes nothing.
pub(crate) fn check_cut_reads_as_written(
    name: &str,
    bytes: &[u8],
    cuts: &[SectionCut<'_>],
    dead_at: DeadRowPlacement,
) -> Option<usize> {
    let cut = Cut::plan(bytes, cuts, dead_at, true).unwrap()?;
    let passes = cut.passes.len();
    let written = cut.write(bytes).unwrap();
    let mut dead_sorted = written.dead_rows.clone();
    dead_sorted.sort_unstable();
    assert_eq!(
        cut.dead_rows(bytes).unwrap(),
        dead_sorted,
        "{name}: dead rows"
    );
    let member = ArMember { name, data: bytes };
    let read = MemberObj::parse(member).unwrap().with_cut(cut);
    let out = ArMember {
        name,
        data: &written.bytes,
    };
    let want = MemberObj::parse(out).unwrap();
    assert_eq!(read.sections(), want.sections(), "{name}: sections");
    for s in want.sections() {
        let ord = s.section_index;
        assert_eq!(
            read.section_relocs(ord),
            want.section_relocs(ord),
            "{name} {ord}"
        );
        if !is_no_file_storage(s.flags) {
            assert!(
                read.payload(ord) == want.payload(ord),
                "{name} {ord}: payload"
            );
        }
    }
    check_rows(name, &read, &want);
    assert_eq!(read.defined_externs(), want.defined_externs(), "{name}");
    assert_eq!(read.undef_externs(), want.undef_externs(), "{name}");
    assert_eq!(
        read.list_symbols(&dead_sorted),
        want.list_symbols(&dead_sorted),
        "{name}: listed"
    );
    Some(passes)
}

/// Every nlist row and name of `read` against `want`.
fn check_rows(name: &str, read: &MemberObj<'_>, want: &MemberObj<'_>) {
    let (st_r, st_w) = (read.symtab().unwrap(), want.symtab().unwrap());
    assert_eq!(st_r.nsyms, st_w.nsyms);
    for i in 0..st_w.nsyms {
        assert_eq!(read.sym(&st_r, i), want.sym(&st_w, i), "{name}: row {i}");
        assert_eq!(read.sym_name(&st_r, i), want.sym_name(&st_w, i));
    }
}

/// Every member of the embedded runtime, as built and pre-stripped,
/// under a few made-up liveness patterns, read through its cut.
#[test]
fn runtime_members_read_through_a_cut_as_written() {
    use crate::archives_merge::merge_archive_indexes;
    use crate::dead_strip_reach_build::build_member_reach;
    use crate::dead_strip_repack::eligible_sections;
    let original = torajs_core::staticlibs::TORAJS_RUNTIME_IMAGE;
    let pre = crate::runtime_prestrip::prestrip_image(original).unwrap();
    let (mut checked, mut multi) = (0, 0);
    for (image, dead_at) in [
        (original, DeadRowPlacement::GapStart),
        (&pre[..], DeadRowPlacement::SectionBase),
    ] {
        let merged = merge_archive_indexes(image).unwrap();
        for a in 0..merged.archive_count() {
            for m in 0..merged.member_count(a) {
                let member = merged.member(a, m);
                let mut reach = build_member_reach(&member).unwrap();
                for pattern in 0..3usize {
                    for (si, sa) in reach.sects.iter_mut().enumerate() {
                        let keep_all = (si + pattern) % 4 == 3;
                        for (i, l) in sa.live.iter_mut().enumerate() {
                            *l = keep_all || (i * 5 + si + pattern) % 3 != 0;
                        }
                    }
                    let eligible = eligible_sections(&reach);
                    let cuts: Vec<SectionCut<'_>> = eligible
                        .iter()
                        .map(|(ord, atoms_rel, live)| SectionCut {
                            ord: *ord,
                            atoms_rel,
                            live,
                        })
                        .collect();
                    let name = member.name;
                    if let Some(passes) =
                        check_cut_reads_as_written(name, member.data, &cuts, dead_at)
                    {
                        checked += 1;
                        multi += usize::from(passes > 1);
                    }
                }
            }
        }
    }
    eprintln!("checked {checked}, several sections {multi}");
    assert!(checked > 1000, "{checked} cut members");
    assert!(multi > 100, "{multi} cuts of several sections");
}
