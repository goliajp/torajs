//! Each expanded (member, section)'s reloc table, decoded once per
//! link and kept sorted by address.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::archive::ArMember;
use crate::dead_strip_reach_cache::BytesKey;
use crate::member_data_apply::parse_member_section_relocs;
use crate::member_reloc::MemberRelocEntry;

/// Each expanded (member, section)'s reloc table, decoded once and
/// sorted by `r_address` so an atom finds its relocs by binary search
/// instead of re-decoding and filtering the whole table.
/// `(r_address, table position, entry)`, ascending.
type SortedRelocs = Vec<(u32, u32, MemberRelocEntry)>;

/// Keyed by the member bytes, so every reachability pass of one link
/// shares the tables of the members it did not rewrite.
#[derive(Default)]
pub(crate) struct RelocIndex<'a> {
    tables: HashMap<(BytesKey<'a>, u8), SortedRelocs>,
    hits: Vec<(u32, MemberRelocEntry)>,
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
    ) -> Result<impl Iterator<Item = &MemberRelocEntry>, String> {
        let table = match self.tables.entry((BytesKey(member.data), sect)) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => {
                let parsed = parse_member_section_relocs(member, sect)
                    .map_err(|e| format!("sect {sect} relocs: {e:?}"))?;
                let mut t: SortedRelocs = parsed
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
