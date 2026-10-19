//! What the dead-strip passes parse out of a runtime member's bytes,
//! kept for one link. The elide fix-point, the strip pass, the
//! forced-table derivation and the layout each walk the member closure
//! again; the atom tables, nlist, defined-name map, sorted reloc tables,
//! decoded out-edges and undef names depend only on the member bytes, so
//! each is parsed once per distinct byte slice. Liveness is never cached: every
//! reachability pass starts from a fresh all-dead copy.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::archive::{ArMember, MemberSymtabError};
use crate::archives_merge::member_undef_extern_names;
use crate::dead_strip_reach::MemberReach;
use crate::dead_strip_reach_build::build_member_reach;
use crate::dead_strip_reach_edges::EdgeIndex;
use crate::dead_strip_reach_table::reach_from_table;

/// Member bytes compared by address and length. The runtime image
/// hands every copy of a deduplicated member the same slice, and a
/// rewritten member owns new bytes, so slice identity is parse-input
/// identity. Holding the borrow keeps the address from being reused
/// for other bytes while the entry lives.
#[derive(Clone, Copy)]
pub(crate) struct BytesKey<'a>(pub(crate) &'a [u8]);

impl PartialEq for BytesKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0.as_ptr(), other.0.as_ptr()) && self.0.len() == other.0.len()
    }
}

impl Eq for BytesKey<'_> {}

impl Hash for BytesKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.0.as_ptr() as usize).hash(state);
        self.0.len().hash(state);
    }
}

#[derive(Default)]
pub(crate) struct ReachCache<'a> {
    reach: HashMap<BytesKey<'a>, MemberReach<'a>>,
    undefs: HashMap<BytesKey<'a>, Rc<[&'a str]>>,
    pub(crate) edges: EdgeIndex<'a>,
}

impl<'a> ReachCache<'a> {
    /// The member's reachability tables with every atom dead, read
    /// from `table` (the runtime image's table of these bytes), or
    /// built from the bytes when `table` is empty.
    pub(crate) fn member_reach(
        &mut self,
        member: &ArMember<'a>,
        table: &'a [u8],
    ) -> Result<MemberReach<'a>, String> {
        let key = BytesKey(member.data);
        let mut r = match self.reach.get(&key) {
            Some(r) => r.clone(),
            None => {
                let r = if table.is_empty() {
                    build_member_reach(member)?
                } else {
                    reach_from_table(member, table)
                };
                self.reach.insert(key, r.clone());
                r
            }
        };
        // copies sharing the bytes keep their own name
        r.member_name = member.name;
        Ok(r)
    }

    /// The member's `N_UNDF | N_EXT` names, in nlist order.
    pub(crate) fn undef_externs(
        &mut self,
        member: &ArMember<'a>,
    ) -> Result<Rc<[&'a str]>, MemberSymtabError> {
        let key = BytesKey(member.data);
        if let Some(u) = self.undefs.get(&key) {
            return Ok(u.clone());
        }
        let u: Rc<[&'a str]> = member_undef_extern_names(member)?.into();
        self.undefs.insert(key, u.clone());
        Ok(u)
    }
}
