//! One reach closure's walk. The required members are numbered in key
//! order, so a node's member, visited bit and decoded edges are found
//! by index; an undefined symbol an edge names is answered once per
//! closure, by the row of its name in the runtime image's symbol index
//! where the index holds it, and the owner's definition of a row is
//! looked up by name once per link. A name is read only for a symbol
//! the index lacks and for the user-defined check of a newly reached
//! row.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use crate::archive_link_extra_syms::UserDefined;
use crate::archives_merge::{MergedArchives, RequiredMembers};
use crate::dead_strip_reach::{MemberReach, Node, Pred, ReachProbes, ReachResult, atom_is_cut};
use crate::dead_strip_reach_cache::ReachCache;
use crate::dead_strip_reach_edges::{Edge, EdgeIndex};
use crate::dead_strip_reach_resolve::{atom_at, local_node};
use crate::dead_strip_reach_seen::{Seen, UndefMemo};
use crate::dead_strip_reach_table::{Import, image_table};
use crate::dead_strip_reach_table_edges::image_edges;
use crate::sym_rows::SymSet;

mod roots;

/// The required members of one closure, in key order.
pub(crate) struct Walk<'a> {
    keys: Vec<(usize, usize)>,
    members: Vec<MemberReach<'a>>,
    /// Per member: its slot in the link's edge index, and its imports.
    slots: Vec<u32>,
    imports: Vec<Rc<[Import<'a>]>>,
    /// Per archive, per member: its position here + 1 (0: not walked).
    pos: Vec<Vec<u32>>,
}

impl<'a> Walk<'a> {
    /// The members `required` names, every atom dead.
    pub(crate) fn new(
        merged: &MergedArchives<'a>,
        required: &RequiredMembers,
        cache: &mut ReachCache<'a>,
    ) -> Result<Self, String> {
        let n = required.members.len();
        let mut w = Walk {
            keys: Vec::with_capacity(n),
            members: Vec::with_capacity(n),
            slots: Vec::with_capacity(n),
            imports: Vec::with_capacity(n),
            pos: Vec::new(),
        };
        for &(a, m) in &required.members {
            let member = merged.member(a, m);
            let r = cache.member_reach(&member, image_table(merged, (a, m)))?;
            let imports = cache
                .bytes_imports(merged, (a, m))
                .map_err(|e| format!("{}: imports: {e:?}", member.name))?;
            let edges = image_edges(merged, (a, m));
            w.slots.push(cache.edges.slot(&member, &r, edges, &imports));
            if w.pos.len() <= a {
                w.pos.resize(a + 1, Vec::new());
            }
            if w.pos[a].len() <= m {
                w.pos[a].resize(m + 1, 0);
            }
            w.keys.push((a, m));
            w.pos[a][m] = w.keys.len() as u32;
            w.members.push(r);
            w.imports.push(imports);
        }
        Ok(w)
    }

    /// The position of member `key` in the walk.
    fn pos(&self, (a, m): (usize, usize)) -> Option<usize> {
        let p = *self.pos.get(a)?.get(m)?;
        (p as usize).checked_sub(1)
    }
}

/// `sym_loc` of a row its owner does not define.
const KNOWN: u64 = 1 << 63;
/// `sym_loc` of a row its owner defines: section in bits 32..40, the
/// covering atom + 1 in the low 32 (0 = none, the whole section).
const DEFINED: u64 = 1 << 62;

/// One closure: the walk, its visited set and what it found.
pub(crate) struct Walker<'w, 'a> {
    merged: &'w MergedArchives<'a>,
    defined_in_user: &'w UserDefined<'w>,
    required: &'w RequiredMembers,
    walk: Walk<'a>,
    seen: Seen,
    unresolved: BTreeSet<String>,
    user_refs: BTreeSet<String>,
    preds: Option<BTreeMap<Node, Pred>>,
    names: UndefMemo,
    /// Rows asked about, and the ones a user definition answered.
    asked: SymSet,
    user_rows: SymSet,
}

impl<'w, 'a> Walker<'w, 'a> {
    pub(crate) fn new(
        merged: &'w MergedArchives<'a>,
        defined_in_user: &'w UserDefined<'w>,
        required: &'w RequiredMembers,
        walk: Walk<'a>,
        record_preds: bool,
    ) -> Self {
        let n = merged.image.symbol_count();
        Walker {
            merged,
            defined_in_user,
            required,
            seen: Seen::new(&walk.members),
            walk,
            unresolved: BTreeSet::new(),
            user_refs: BTreeSet::new(),
            preds: record_preds.then(BTreeMap::new),
            names: UndefMemo::default(),
            asked: SymSet::new(n),
            user_rows: SymSet::new(n),
        }
    }

    /// Run the worklist to its fix-point.
    pub(crate) fn close(
        &mut self,
        mut worklist: Vec<Node>,
        probes: Option<&ReachProbes>,
        cache: &mut ReachCache<'a>,
    ) -> Result<(), String> {
        let ReachCache { edges, sym_loc, .. } = cache;
        while let Some(node) = worklist.pop() {
            let (Node::Atom { key, .. } | Node::AllSect { key, .. }) = node;
            let i = self.walk.pos(key).ok_or("member vanished")?;
            if !self.seen.insert(i, node) {
                continue;
            }
            // S2-5 pricing probes (diag-only; the strip path passes
            // None, so the what-if closures never shape bytes).
            if let Some(p) = probes {
                let r = &mut self.walk.members[i];
                // cut_in: the definition is treated as absent — not
                // live, no propagation.
                if !p.cut_ins.is_empty() && atom_is_cut(r, node, &p.cut_ins) {
                    continue;
                }
                // cut: live but stubbed — outgoing edges gone.
                if !p.cuts.is_empty() && atom_is_cut(r, node, &p.cuts) {
                    if let Node::Atom { sect, atom, .. } = node
                        && let Some(sa) = r.sects.get_mut(sect as usize - 1)
                    {
                        sa.live[atom] = true;
                    }
                    continue;
                }
            }
            self.expand(i, node, edges, sym_loc, &mut worklist)?;
        }
        Ok(())
    }

    /// Mark a node of walked member `i` live and enqueue everything it
    /// references.
    fn expand(
        &mut self,
        i: usize,
        node: Node,
        edges: &mut EdgeIndex<'a>,
        sym_loc: &mut Vec<u64>,
        worklist: &mut Vec<Node>,
    ) -> Result<(), String> {
        let r = &mut self.walk.members[i];
        let (key, sect_ord, atom) = match node {
            Node::Atom { key, sect, atom } => {
                let sa = r.sects.get_mut(sect as usize - 1).ok_or("bad sect")?;
                sa.live[atom] = true;
                (key, sect, Some(atom as u32))
            }
            Node::AllSect { key, sect } => {
                if sect == 0 {
                    return Ok(());
                }
                let Some(sa) = r.sects.get_mut(sect as usize - 1) else {
                    return Ok(());
                };
                sa.all_live = true;
                sa.live.fill(true);
                (key, sect, None)
            }
        };
        let r = &self.walk.members[i];
        if r.sects
            .get(sect_ord as usize - 1)
            .ok_or("bad sect")?
            .no_file_storage
        {
            return Ok(()); // zerofill: no payload, no relocs
        }
        let member = self.merged.member(key.0, key.1);
        let imports = &self.walk.imports[i];
        let list = edges.out_edges(self.walk.slots[i], &member, r, imports, sect_ord, atom)?;
        for k in list {
            let next = match edges.edge(k) {
                Edge::Local { sect, atom } => Some(local_node(key, sect, atom)),
                Edge::Sym(row) => self.ask_row(row, sym_loc),
                Edge::Name(id) => self.ask_name(id, edges.name(id), sym_loc),
            };
            let Some(next) = next else {
                continue;
            };
            if let Some(p) = self.preds.as_mut() {
                p.entry(next).or_insert(Pred::Node(node));
            }
            worklist.push(next);
        }
        Ok(())
    }

    /// The node an import's row makes live: none when a user fn
    /// defines it; recorded as unresolved when no walked member
    /// defines it. The rows of the index are never dyld imports — the
    /// member closure classifies only the names the index lacks.
    fn ask_row(&mut self, row: u32, sym_loc: &mut Vec<u64>) -> Option<Node> {
        if !self.asked.insert(row) {
            return if self.user_rows.contains(row) {
                None
            } else {
                self.sym_node(row, sym_loc)
            };
        }
        let image = &self.merged.image;
        let name = image.symbol_name(row as usize);
        if self.defined_in_user.contains_bytes(name) {
            self.user_rows.insert(row);
            self.user_refs
                .insert(image.symbol(row as usize).name.into());
            return None;
        }
        let node = self.sym_node(row, sym_loc);
        if node.is_none() {
            self.unresolved
                .insert(image.symbol(row as usize).name.into());
        }
        node
    }

    /// [`Self::ask_row`] for a name the index may lack, which may also
    /// be a dyld import.
    fn ask_name(&mut self, id: u32, name: &str, sym_loc: &mut Vec<u64>) -> Option<Node> {
        if let Some(answer) = self.names.get(id) {
            return answer;
        }
        let user = self.defined_in_user.contains(name);
        if user {
            self.user_refs.insert(name.to_string());
        }
        let answer = if user || self.required.dyld_imports.contains_key(name) {
            None
        } else {
            let node = self
                .merged
                .image
                .find_symbol(name)
                .and_then(|row| self.sym_node(row as u32, sym_loc));
            if node.is_none() {
                self.unresolved.insert(name.to_string());
            }
            node
        };
        self.names.set(id, answer);
        answer
    }

    /// The node of row `row`'s definition in its owner, when the owner
    /// is walked and defines it.
    fn sym_node(&self, row: u32, sym_loc: &mut Vec<u64>) -> Option<Node> {
        let image = &self.merged.image;
        let key = image.symbol_owner(row as usize);
        let r = &self.walk.members[self.walk.pos(key)?];
        if sym_loc.is_empty() {
            *sym_loc = vec![0; image.symbol_count()];
        }
        let loc = &mut sym_loc[row as usize];
        if *loc == 0 {
            *loc = match r.defined_bytes(image.symbol_name(row as usize)) {
                None => KNOWN,
                Some((sect, value)) => {
                    let atom = atom_at(r, sect, value).map_or(0, |a| u64::from(a) + 1);
                    KNOWN | DEFINED | u64::from(sect) << 32 | atom
                }
            };
        }
        (*loc & DEFINED != 0)
            .then(|| local_node(key, (*loc >> 32) as u8, (*loc as u32).checked_sub(1)))
    }

    pub(crate) fn finish(self) -> ReachResult<'a> {
        ReachResult {
            members: self.walk.keys.into_iter().zip(self.walk.members).collect(),
            unresolved: self.unresolved,
            user_refs: self.user_refs,
            preds: self.preds.unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Walk, Walker};
    use crate::archive_link_extra_syms::ExtraDefined;
    use crate::archives_merge::{RequiredMembers, merge_archive_indexes};
    use crate::dead_strip_reach_cache::ReachCache;
    use crate::dead_strip_reach_resolve::node_for_global_name;
    use crate::runtime_prestrip::prestrip_image;

    /// The node a row of the runtime image's symbol index resolves to,
    /// first and when read back off the link's cache, is the node its
    /// name resolves to.
    #[test]
    fn every_row_lands_where_its_name_does() {
        let image = prestrip_image(torajs_core::staticlibs::TORAJS_RUNTIME_IMAGE).unwrap();
        let merged = merge_archive_indexes(&image).unwrap();
        let members = (0..merged.archive_count())
            .flat_map(|a| (0..merged.member_count(a)).map(move |m| (a, m)))
            .collect();
        let required = RequiredMembers {
            members,
            dyld_imports: BTreeMap::new(),
        };
        let mut cache = ReachCache::default();
        let walk = Walk::new(&merged, &required, &mut cache).unwrap();
        let extra = ExtraDefined::default();
        let user = extra.with_user_fns(&[]);
        let w = Walker::new(&merged, &user, &required, walk, false);
        let by_key: BTreeMap<_, _> = (w.walk.keys.iter().copied())
            .zip(w.walk.members.iter().cloned())
            .collect();
        let (mut sym_loc, mut defined) = (Vec::new(), 0);
        for row in 0..merged.image.symbol_count() {
            let name = merged.image.symbol(row).name;
            let want = node_for_global_name(name, &merged, &by_key);
            defined += usize::from(want.is_some());
            assert!(w.sym_node(row as u32, &mut sym_loc) == want, "{name}");
            assert!(w.sym_node(row as u32, &mut sym_loc) == want, "{name}");
        }
        assert!(defined > 1000, "{defined} defined rows");
    }
}
