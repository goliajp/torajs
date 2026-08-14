//! W-J Phase A1 follow-up — anon-stamp coverage for ObjectLit sids
//! interned during Pass 2 (e.g. inline `{x:1, y:2}` literals + generic
//! mono spawning fresh shapes per specialization).
//!
//! Pass 1.5 (`ssa_lower::lower_module`) builds an immutable snapshot
//! covering every sid visible at that boundary. Pass 2 lowerers that
//! encounter a fresh anonymous ObjectLit alloc previously stamped
//! `class_tag = 0` and fell out of every reflection consumer:
//!
//! - `__torajs_struct_layout_lookup(0) → NULL`
//!   → `__torajs_struct_field_count(NULL) → 0`
//!   → `struct_print.rs` Tag::Obj walker emits `{}` instead of bun's
//!     `{\n  x: 1,\n  y: 2,\n}` form.
//!
//! [`AnonStampPool`] swaps that fall-through for in-flight tag
//! assignment: every Pass 2 stamp call routes through
//! [`AnonStampPool::assign_or_get`] (`RefCell<AnonStampPool>` lives in
//! [`LowerCtx`]), missing sids land in `fresh_sids` + receive
//! contiguous tags. `lower_module` walks `fresh_sids` after Pass 2 to
//! append matching `ClassLayoutMeta` rows to `module.class_layouts` —
//! same shape the Pass 1.5 anonymous-ObjectLit loop emits per sid, so
//! the cycle visitor / reflection helpers see a uniform table.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use crate::ssa;
use crate::ssa::Type;

/// Pool of anonymous-ObjectLit sid → class_tag mappings. Initial
/// contents come from the Pass 1.5 snapshot; Pass 2 stamp sites push
/// fresh assignments in declaration order.
pub struct AnonStampPool {
    /// Pass 1.5 snapshot mappings (named tags `[1..=n_named]` excluded)
    /// + Pass 2 fresh insertions. Lookup-on-miss falls into
    /// `assign_or_get` which allocates `next_tag` + records `fresh_sids`.
    sid_to_tag: HashMap<ssa::StructId, u32>,
    /// Sids that didn't appear in the snapshot — Pass 2 fresh stamps.
    /// `lower_module` walks this after Pass 2 to append matching
    /// `ClassLayoutMeta` rows to `module.class_layouts` so the runtime
    /// indexed view stays dense.
    fresh_sids: Vec<ssa::StructId>,
    /// Next tag to hand out. Starts at the snapshot's high-water mark
    /// (= `n_named + n_anon_snapshot + 1`) so fresh tags are disjoint
    /// from both named + snapshot anon tags + class_layouts is grown
    /// without renumbering.
    next_tag: u32,
    /// 404-01 / 405-03 — one row PER GENERIC MONO FACTORY:
    /// `(tag, layout sid, base class name)`, keyed for dedup by the
    /// factory's name. The alloc sid is the factory ObjectLit's
    /// STRUCTURAL intern, and two different generic classes with the
    /// same field shape share it (`G<number>` / `H<number>` both
    /// intern `{k: i64}`) — so a sid-keyed tag cannot carry class
    /// identity, the same lesson the named tags learned ("keyed by
    /// class name (not sid)"). Each factory therefore mints its OWN
    /// tag off the shared counter; the plain anon row of the sid
    /// stays untouched for plain object literals of that shape.
    generic_rows: Vec<(u32, ssa::StructId, String)>,
    generic_by_factory: HashMap<String, u32>,
}

impl AnonStampPool {
    /// Build a pool wrapping the Pass 1.5 snapshot. `next_tag_start`
    /// is the first tag value handed out for a Pass 2 fresh sid —
    /// callers compute it as `n_named + sid_to_tag.len() + 1`.
    pub fn from_snapshot(snapshot: HashMap<ssa::StructId, u32>, next_tag_start: u32) -> Self {
        Self {
            sid_to_tag: snapshot,
            fresh_sids: Vec::new(),
            next_tag: next_tag_start,
            generic_rows: Vec::new(),
            generic_by_factory: HashMap::new(),
        }
    }

    /// 404-01 / 405-03 — the tag a generic mono factory's instances
    /// wear (see the field doc). One tag per factory, minted off the
    /// shared counter on first ask.
    pub fn assign_generic(&mut self, factory: &str, sid: ssa::StructId, base: String) -> u32 {
        if let Some(&t) = self.generic_by_factory.get(factory) {
            return t;
        }
        let tag = self.next_tag;
        self.next_tag = self.next_tag.saturating_add(1);
        self.generic_by_factory.insert(factory.to_string(), tag);
        self.generic_rows.push((tag, sid, base));
        tag
    }

    /// The recorded generic factory rows, for the merged fresh-row
    /// append.
    pub fn generic_rows(&self) -> &[(u32, ssa::StructId, String)] {
        &self.generic_rows
    }

    /// Look up an existing tag for `sid` or allocate a fresh one. Fresh
    /// sids are recorded in `fresh_sids` so `lower_module` can append
    /// matching `ClassLayoutMeta` rows after Pass 2.
    pub fn assign_or_get(&mut self, sid: ssa::StructId) -> u32 {
        if let Some(&tag) = self.sid_to_tag.get(&sid) {
            return tag;
        }
        let tag = self.next_tag;
        self.next_tag = self.next_tag.saturating_add(1);
        self.sid_to_tag.insert(sid, tag);
        self.fresh_sids.push(sid);
        tag
    }

    /// Read-only access to the sids that received Pass 2 fresh tags —
    /// `lower_module` walks this to emit matching `ClassLayoutMeta`
    /// rows + grow `module.class_layouts`.
    pub fn fresh_sids(&self) -> &[ssa::StructId] {
        &self.fresh_sids
    }

    /// Read-only iterator over `(sid, class_tag)` pairs. S126-5 —
    /// `lower_any_member_read` enumerates this alongside
    /// `class_name_to_tag` so anon-stamped class instances dispatch
    /// through the same monomorphic IC arm as named classes.
    pub fn sid_to_tag_iter(&self) -> impl Iterator<Item = (ssa::StructId, u32)> + '_ {
        self.sid_to_tag.iter().map(|(&sid, &tag)| (sid, tag))
    }
}

/// Wrap the pool in a `RefCell` so the per-fn `LowerCtx` can borrow it
/// across multiple stamp sites without `&mut` lifetime grief.
pub type AnonStampPoolCell = RefCell<AnonStampPool>;

/// Build the Pass 1.5 snapshot pool from `class_name_to_tag` (named
/// classes hold tags `[1..=n_named]`) + the current `struct_layouts`
/// vec. Anonymous sids land in `[n_named+1..=n_named+n_anon]`; the
/// returned `next_tag_start` for Pass 2 fresh sids = `n_named +
/// n_anon + 1`.
pub fn build_snapshot_pool(
    class_name_to_tag: &HashMap<String, u32>,
    aliases: &HashMap<String, Type>,
    struct_layouts: &[Vec<(String, Type)>],
) -> AnonStampPoolCell {
    let named_sids: HashSet<ssa::StructId> = class_name_to_tag
        .keys()
        .filter_map(|cname| match aliases.get(cname) {
            Some(Type::Obj(sid)) => Some(*sid),
            _ => None,
        })
        .collect();
    let n_named = class_name_to_tag.len() as u32;
    let snapshot: HashMap<ssa::StructId, u32> = struct_layouts
        .iter()
        .enumerate()
        .filter_map(|(idx, _)| {
            let sid = ssa::StructId(idx as u32);
            if named_sids.contains(&sid) {
                None
            } else {
                Some(sid)
            }
        })
        .enumerate()
        .map(|(anon_idx, sid)| (sid, n_named + anon_idx as u32 + 1))
        .collect();
    let next_tag_start = n_named + snapshot.len() as u32 + 1;
    RefCell::new(AnonStampPool::from_snapshot(snapshot, next_tag_start))
}

/// Append a `ClassLayoutMeta` row per Pass 2 fresh tag recorded in
/// `pool` to `class_layouts` — plain anon sids and generic-factory
/// rows (405-03) share one tag counter, so the two lists merge
/// sorted by tag to keep the runtime indexed view dense
/// (`class_tag - 1` keeps holding).
///
/// `struct_layouts` is the post-Pass-2 vec — every fresh sid index
/// is now in-range. `generic_methods` maps a generic class to its
/// re-targeted method table (built by `populate_class_layouts`).
pub fn append_fresh_class_layouts(
    pool: &AnonStampPoolCell,
    struct_layouts: &[Vec<(String, crate::ssa::Type)>],
    generic_methods: &HashMap<String, Vec<crate::ssa::MethodMetaSpec>>,
    class_layouts: &mut Vec<crate::ssa::ClassLayoutMeta>,
) {
    let p = pool.borrow();
    let mut rows: Vec<(u32, ssa::StructId, Option<&String>)> = p
        .fresh_sids()
        .iter()
        .map(|sid| (p.sid_to_tag[sid], *sid, None))
        .chain(
            p.generic_rows()
                .iter()
                .map(|(tag, sid, base)| (*tag, *sid, Some(base))),
        )
        .collect();
    rows.sort_by_key(|(tag, _, _)| *tag);
    for (_, sid, base) in rows {
        let sid_idx = sid.0 as usize;
        let layout = &struct_layouts[sid_idx];
        let mut child_offsets: Vec<u32> = Vec::new();
        let mut field_metadata: Vec<crate::ssa::FieldMetaSpec> = Vec::new();
        for (i, (fname, fty)) in layout.iter().enumerate() {
            let off = crate::ssa_lower::OBJ_HEADER_SIZE as u32 + (i as u32) * 8;
            if fty.is_refcounted() {
                child_offsets.push(off);
            }
            field_metadata.push(crate::ssa::FieldMetaSpec {
                name: fname.clone(),
                offset: off,
                type_tag: crate::ssa::field_type_tag_of(*fty),
            });
        }
        // 404-01 / 405-03 — a generic factory's row keeps its
        // per-specialization LAYOUT but wears the class identity
        // (name + re-targeted method table).
        let (class_name, methods) = match base {
            Some(base) => (
                base.clone(),
                generic_methods.get(base).cloned().unwrap_or_default(),
            ),
            None => (format!("__anon_struct_{sid_idx}"), Vec::new()),
        };
        class_layouts.push(crate::ssa::ClassLayoutMeta {
            class_name,
            child_offsets,
            field_metadata,
            is_named: false,
            methods,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_hits_return_existing_tag() {
        let mut snap = HashMap::new();
        snap.insert(ssa::StructId(0), 1u32);
        snap.insert(ssa::StructId(1), 2u32);
        let mut pool = AnonStampPool::from_snapshot(snap, 3);
        assert_eq!(pool.assign_or_get(ssa::StructId(0)), 1);
        assert_eq!(pool.assign_or_get(ssa::StructId(1)), 2);
        assert!(pool.fresh_sids().is_empty());
    }

    #[test]
    fn fresh_sids_get_contiguous_tags_from_next_start() {
        let mut pool = AnonStampPool::from_snapshot(HashMap::new(), 5);
        assert_eq!(pool.assign_or_get(ssa::StructId(7)), 5);
        assert_eq!(pool.assign_or_get(ssa::StructId(8)), 6);
        assert_eq!(pool.assign_or_get(ssa::StructId(9)), 7);
        assert_eq!(pool.fresh_sids().len(), 3);
        // Re-querying a fresh sid returns the cached tag, no new alloc.
        assert_eq!(pool.assign_or_get(ssa::StructId(7)), 5);
        assert_eq!(pool.fresh_sids().len(), 3);
    }

    #[test]
    fn snapshot_and_fresh_disjoint_tag_space() {
        let mut snap = HashMap::new();
        snap.insert(ssa::StructId(0), 1u32);
        let mut pool = AnonStampPool::from_snapshot(snap, 2);
        let t_fresh = pool.assign_or_get(ssa::StructId(5));
        assert_eq!(t_fresh, 2);
        assert_eq!(pool.assign_or_get(ssa::StructId(0)), 1);
    }
}
