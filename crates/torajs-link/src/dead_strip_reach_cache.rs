//! What the dead-strip passes parse out of a runtime member's bytes,
//! kept for one link. The elide fix-point, the strip pass, the
//! forced-table derivation and the layout each walk the member closure
//! again; the atom tables, nlist, defined-name map, sorted reloc tables,
//! decoded out-edges and undef names depend only on the member bytes, so
//! each is parsed once per distinct byte slice. Liveness is never cached: every
//! reachability pass starts from a fresh all-dead copy. A member's
//! imports are read off its table, each named by its row in the image's
//! symbol index.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use torajs_rtimage::archive_undef::member_undef_extern_rows;

use crate::archive::{ArMember, MemberSymtabError};
use crate::archives_merge::MergedArchives;
use crate::dead_strip_reach::MemberReach;
use crate::dead_strip_reach_build::build_member_reach;
use crate::dead_strip_reach_edges::EdgeIndex;
use crate::dead_strip_reach_table::{
    Import, ImportRef, image_table, reach_from_table, table_imports,
};

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
    imports: HashMap<BytesKey<'a>, Rc<[Import<'a>]>>,
    /// The imports of the members the dead-strip pass cut.
    cut_imports: HashMap<(usize, usize), Rc<[Import<'a>]>>,
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

    /// The `N_UNDF | N_EXT` rows of member `key` of `merged`, in nlist
    /// order: through its cut once the dead-strip pass cut it.
    pub(crate) fn member_imports(
        &mut self,
        merged: &MergedArchives<'a>,
        key: (usize, usize),
    ) -> Result<Rc<[Import<'a>]>, MemberSymtabError> {
        let Some(obj) = merged.cut_obj(key) else {
            return self.imports(merged, key);
        };
        if let Some(u) = self.cut_imports.get(&key) {
            return Ok(u.clone());
        }
        let all = self.imports(merged, key)?;
        let cut = obj.cut.as_ref().expect("cut_obj answers cut members only");
        let u: Rc<[Import<'a>]> = all
            .iter()
            .filter(|i| !cut.is_neutered(i.row))
            .copied()
            .collect();
        self.cut_imports.insert(key, u.clone());
        Ok(u)
    }

    /// The imports of member `key` as its bytes list them: off the
    /// image's table, or read from the bytes when the image holds none.
    fn imports(
        &mut self,
        merged: &MergedArchives<'a>,
        key: (usize, usize),
    ) -> Result<Rc<[Import<'a>]>, MemberSymtabError> {
        let member = merged.member(key.0, key.1);
        let bytes = BytesKey(member.data);
        if let Some(u) = self.imports.get(&bytes) {
            return Ok(u.clone());
        }
        let table = image_table(merged, key);
        let u: Rc<[Import<'a>]> = if table.is_empty() {
            let rows = member_undef_extern_rows(&member)?;
            rows.into_iter()
                .map(|(row, name)| Import {
                    row,
                    to: match merged.image.find_symbol(name) {
                        Some(i) => ImportRef::Sym(i as u32),
                        None => ImportRef::Foreign(name),
                    },
                })
                .collect()
        } else {
            table_imports(&member, table).into()
        };
        self.imports.insert(bytes, u.clone());
        Ok(u)
    }
}
