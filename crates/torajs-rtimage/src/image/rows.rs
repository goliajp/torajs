//! The image's symbol index: the rows each kept name gets, and the
//! names blob they point into.

use std::collections::{BTreeMap, HashMap};

use super::{BuildError, SYM_LEN, u32v};
use crate::prune::Winners;

/// The image's symbol index while the image is built: the names it
/// will hold, sorted as [`super::Image::find_symbol`] searches them.
pub struct SymbolRows<'s> {
    pub(super) names: &'s [&'s str],
}

impl SymbolRows<'_> {
    /// The names [`sym_rows`] writes: those whose winner is kept.
    pub(super) fn kept_names<'a>(
        winners: &Winners<'a>,
        live: &[Vec<bool>],
    ) -> Result<Vec<&'a str>, BuildError> {
        let names: Vec<&str> = winners
            .iter()
            .filter(|(_, (a, m, _))| live[*a as usize][*m as usize])
            .map(|(name, _)| *name)
            .collect();
        u32v(names.len())?;
        Ok(names)
    }

    /// The row `name` will have in the image's symbol index.
    pub fn row(&self, name: &str) -> Option<u32> {
        let i = self.names.binary_search(&name).ok()?;
        // `build_members` refuses an index of more than u32::MAX rows
        Some(i as u32)
    }
}

/// The `syms` rows of the names whose winner is kept, sorted by name,
/// and their count.
pub(super) fn sym_rows<'a>(
    winners: Winners<'a>,
    renumber: &[Vec<Option<u32>>],
    names: &mut Names<'a>,
) -> Result<(usize, Vec<u8>), BuildError> {
    let index: BTreeMap<&str, (u32, u32, u64)> = winners
        .into_iter()
        .filter_map(|(name, (a, m, v))| renumber[a as usize][m as usize].map(|k| (name, (a, k, v))))
        .collect();
    let mut syms: Vec<u8> = Vec::with_capacity(index.len() * SYM_LEN);
    for (name, (a, m, n_value)) in &index {
        let (name_off, name_len) = names.intern(name)?;
        for v in [name_off, name_len, *a, *m] {
            syms.extend_from_slice(&v.to_le_bytes());
        }
        syms.extend_from_slice(&n_value.to_le_bytes());
    }
    Ok((index.len(), syms))
}

/// The names blob under construction, each distinct name written once.
#[derive(Default)]
pub(super) struct Names<'a> {
    pub(super) bytes: Vec<u8>,
    offs: HashMap<&'a str, (u32, u32)>,
}

impl<'a> Names<'a> {
    pub(super) fn intern(&mut self, s: &'a str) -> Result<(u32, u32), BuildError> {
        if let Some(&r) = self.offs.get(s) {
            return Ok(r);
        }
        let r = (u32v(self.bytes.len())?, u32v(s.len())?);
        self.bytes.extend_from_slice(s.as_bytes());
        self.offs.insert(s, r);
        Ok(r)
    }
}
