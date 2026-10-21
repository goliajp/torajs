//! Each expanded (member, section)'s reloc table, decoded once per
//! link.

use std::collections::hash_map::Entry;

use torajs_codegen::id_hash::HashMap;

use crate::archive::ArMember;
use crate::dead_strip_reach_cache::BytesKey;
use crate::member_data_apply::parse_member_section_relocs;
use crate::member_reloc::MemberRelocEntry;

/// One section's relocs, laid out so an atom finds its own by binary
/// search instead of filtering the whole table.
enum Table {
    /// In table order, `r_address` never rising — the order the
    /// object writer emits them in. An address range is one run.
    Descending(Vec<MemberRelocEntry>),
    /// `(r_address, table position, entry)`, ascending.
    Sorted(Vec<(u32, u32, MemberRelocEntry)>),
}

/// Keyed by the member bytes, so every reachability pass of one link
/// shares the tables of the members it did not rewrite.
#[derive(Default)]
pub(crate) struct RelocIndex<'a> {
    tables: HashMap<(BytesKey<'a>, u8), Table>,
    hits: Vec<(u32, MemberRelocEntry)>,
    out: Vec<MemberRelocEntry>,
}

impl<'a> RelocIndex<'a> {
    /// The section's relocs with `lo <= r_address < hi`, in table
    /// order (the order a linear scan visits them).
    pub(crate) fn in_range(
        &mut self,
        member: &ArMember<'a>,
        sect: u8,
        lo: u64,
        hi: u64,
    ) -> Result<&[MemberRelocEntry], String> {
        let table = match self.tables.entry((BytesKey(member.data), sect)) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => {
                let parsed = parse_member_section_relocs(member, sect)
                    .map_err(|e| format!("sect {sect} relocs: {e:?}"))?;
                v.insert(index(parsed))
            }
        };
        match table {
            Table::Descending(t) => {
                let from = t.partition_point(|e| u64::from(e.r_address) >= hi);
                let to = t.partition_point(|e| u64::from(e.r_address) >= lo);
                Ok(&t[from..to.max(from)])
            }
            Table::Sorted(t) => {
                let from = t.partition_point(|&(a, _, _)| u64::from(a) < lo);
                let to = t.partition_point(|&(a, _, _)| u64::from(a) < hi);
                self.hits.clear();
                self.hits
                    .extend(t[from..to.max(from)].iter().map(|&(_, i, e)| (i, e)));
                self.hits.sort_unstable_by_key(|&(i, _)| i);
                self.out.clear();
                self.out.extend(self.hits.iter().map(|&(_, e)| e));
                Ok(&self.out)
            }
        }
    }
}

fn index(parsed: Vec<MemberRelocEntry>) -> Table {
    if parsed.is_sorted_by(|a, b| a.r_address >= b.r_address) {
        return Table::Descending(parsed);
    }
    let mut t: Vec<_> = parsed
        .into_iter()
        .enumerate()
        .map(|(i, e)| (e.r_address, i as u32, e))
        .collect();
    t.sort_unstable_by_key(|&(a, i, _)| (a, i));
    Table::Sorted(t)
}

#[cfg(test)]
mod tests {
    use super::{RelocIndex, Table, index};
    use crate::archive::ArMember;
    use crate::member_reloc::MemberRelocEntry;

    fn at(r_address: u32, r_symbolnum: u32) -> MemberRelocEntry {
        MemberRelocEntry {
            r_address,
            r_symbolnum,
            r_pcrel: 0,
            r_length: 2,
            r_extern: 1,
            r_type: 2,
        }
    }

    /// Both layouts answer a range with the relocs inside it, in table
    /// order.
    #[test]
    fn a_range_reads_its_relocs_in_table_order() {
        let desc = vec![at(40, 0), at(24, 1), at(24, 2), at(8, 3), at(0, 4)];
        let mixed = vec![at(24, 1), at(40, 0), at(8, 3), at(24, 2), at(0, 4)];
        assert!(matches!(index(desc.clone()), Table::Descending(_)));
        assert!(matches!(index(mixed.clone()), Table::Sorted(_)));
        let bytes = [0u8; 1];
        let member = ArMember {
            name: crate::archive::MemberName::new("m.o"),
            data: &bytes,
        };
        for (table, want) in [(desc, [1, 2, 3]), (mixed, [1, 3, 2])] {
            let mut ix = RelocIndex::default();
            ix.tables.insert(
                (crate::dead_strip_reach_cache::BytesKey(member.data), 1),
                index(table),
            );
            let got: Vec<u32> = ix
                .in_range(&member, 1, 8, 40)
                .unwrap()
                .iter()
                .map(|e| e.r_symbolnum)
                .collect();
            assert_eq!(got, want);
            assert!(ix.in_range(&member, 1, 41, 48).unwrap().is_empty());
            assert!(ix.in_range(&member, 1, 9, 24).unwrap().is_empty());
        }
    }
}
