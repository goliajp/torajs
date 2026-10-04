//! Each expanded (member, section)'s reloc table for one reachability
//! pass, decoded once and kept sorted by address.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::archive::ArMember;
use crate::member_data_apply::parse_member_section_relocs;
use crate::member_reloc::MemberRelocEntry;

/// Each expanded (member, section)'s reloc table, decoded once and
/// sorted by `r_address` so an atom finds its relocs by binary search
/// instead of re-decoding and filtering the whole table.
#[derive(Default)]
pub(crate) struct RelocIndex {
    /// `(r_address, table position, entry)`, ascending.
    tables: BTreeMap<((usize, usize), u8), Vec<(u32, u32, MemberRelocEntry)>>,
    hits: Vec<(u32, MemberRelocEntry)>,
}

impl RelocIndex {
    /// The section's relocs with `lo <= r_address < hi`, in table
    /// order (the order a linear scan visits them).
    pub(crate) fn in_range(
        &mut self,
        member: &ArMember<'_>,
        key: (usize, usize),
        sect: u8,
        lo: u64,
        hi: u64,
    ) -> Result<impl Iterator<Item = &MemberRelocEntry>, String> {
        let table = match self.tables.entry((key, sect)) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => {
                let parsed = parse_member_section_relocs(member, sect)
                    .map_err(|e| format!("sect {sect} relocs: {e:?}"))?;
                let mut t: Vec<(u32, u32, MemberRelocEntry)> = parsed
                    .into_iter()
                    .enumerate()
                    .map(|(i, e)| (e.r_address, i as u32, e))
                    .collect();
                t.sort_unstable_by_key(|&(a, i, _)| (a, i));
                v.insert(t)
            }
        };
        let from = table.partition_point(|&(a, _, _)| u64::from(a) < lo);
        let to = table.partition_point(|&(a, _, _)| u64::from(a) < hi);
        self.hits.clear();
        self.hits
            .extend(table[from..to.max(from)].iter().map(|&(_, i, e)| (i, e)));
        self.hits.sort_unstable_by_key(|&(i, _)| i);
        Ok(self.hits.iter().map(|(_, e)| e))
    }
}
