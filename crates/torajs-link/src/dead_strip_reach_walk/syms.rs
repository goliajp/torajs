//! A closure's answer per symbol: the node an import's row or an
//! undefined name makes live, and where each row of the runtime
//! image's symbol index is defined in its owner (kept for the link in
//! `sym_loc`).

use super::Walker;
use crate::dead_strip_reach::Node;
use crate::dead_strip_reach_resolve::{atom_at, local_node};

/// `sym_loc` of a row its owner does not define.
const KNOWN: u64 = 1 << 63;
/// `sym_loc` of a row its owner defines: section in bits 32..40, the
/// covering atom + 1 in the low 32 (0 = none, the whole section).
const DEFINED: u64 = 1 << 62;

impl Walker<'_, '_> {
    /// The node an import's row makes live: none when a user fn
    /// defines it; recorded as unresolved when no walked member
    /// defines it. The rows of the index are never dyld imports — the
    /// member closure classifies only the names the index lacks.
    pub(super) fn ask_row(&mut self, row: u32, sym_loc: &mut Vec<u64>) -> Option<Node> {
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
    pub(super) fn ask_name(&mut self, id: u32, name: &str, sym_loc: &mut Vec<u64>) -> Option<Node> {
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
    pub(super) fn sym_node(&self, row: u32, sym_loc: &mut Vec<u64>) -> Option<Node> {
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
}
