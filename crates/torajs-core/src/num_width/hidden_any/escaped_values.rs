//! The promise value points that settle with a value of any kind.
//!
//! An array that reached the `any` world lowers as an `Arr<Any>` and
//! holds whatever the `any` side stored in it: a promise, or any other
//! value (`["s", Promise.resolve(1)]` handed to a `Promise<number>[]`
//! slot). Awaiting an element runs PromiseResolve on it (§27.7.5.3), so
//! it settles with that value as is, a string or an object included;
//! `Promise.race` / `Promise.any` over the array settle with one of the
//! elements' values (§27.2.4.5.3, §27.2.4.3.1), and the analysis joins
//! their result's value point with the elements' (`combinator_wiring`).
//! An await of such a point answers a value of any kind: it reads it on
//! the any lane (`widen_promise_inner_ty`), and a binding that stores
//! it holds it as is, so the await seeds `hidden_any`
//! (`hidden_any_containers`).
//!
//! The set holds every class of an element's `value` point under an
//! escaped array, so that an await of `a[0]` reads the same way whether
//! or not a combinator also read `a`. Only an await seeds: the elements
//! of an escaped array of records with a `value` field (the request
//! queue of an async generator) share the spelling, and a plain read
//! of that field keeps its kind.

use super::super::container::canon_key_frozen;
use super::super::{Analysis, KeySet, SlotKey, SlotKeyData, SlotKeyId, WidthTable};

/// The classes of `Field(Elem(r), "value")` for every key `r` whose
/// class is in `any_escaped`.
pub(super) fn of(a: &Analysis<'_>, any_escaped: &KeySet) -> KeySet {
    if any_escaped.is_empty() {
        return KeySet::default();
    }
    let canon = |k| canon_key_frozen(&a.uf, &a.keys, k);
    (0..a.keys.len())
        .map(SlotKeyId::from_index)
        .filter(|&k| match a.keys.data(k) {
            SlotKeyData::Field(h, n) if a.keys.is(n, "value") => matches!(
                a.keys.data(h),
                SlotKeyData::Elem(r) if any_escaped.contains(&canon(r))
            ),
            _ => false,
        })
        .map(canon)
        .collect()
}

impl WidthTable {
    /// Does the promise value point `k` settle with a value of any kind
    /// ([`of`])?
    pub(crate) fn promise_value_escapes(&self, k: &SlotKey) -> bool {
        let vs = &self.hidden_any.escaped_values;
        !vs.is_empty() && self.class_of(k).is_some_and(|c| vs.contains(&c))
    }
}
