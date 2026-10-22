//! W-ESC — closing the escape seeds over the container alias classes
//! (`escape.rs` collects them).

use std::collections::VecDeque;

use super::container::{UnionFind, canon_key_frozen};
use super::{KeySet, SlotKeyData, SlotKeyId, SlotKeys};

/// A key the desugar spelled, not the program: a synthetic binding,
/// class, field or return slot (`__cprologue`'s class registry, an
/// any-twin method's `__this`, a constructor's `__new_target`, a
/// generator state class). Their `any` faces are the compiler's own
/// slots — no program write reaches a container through them. A
/// program-named binding inside a synthetic fn (a hoisted local of an
/// async body, the parameter of an any-widened twin) is the program's.
pub(super) fn synthetic(keys: &SlotKeys, k: SlotKeyId) -> bool {
    match keys.data(k) {
        SlotKeyData::Global(n)
        | SlotKeyData::Captured(n)
        | SlotKeyData::Ret(n)
        | SlotKeyData::Class(n)
        | SlotKeyData::Local(_, n)
        | SlotKeyData::Param(_, n) => keys.dunder(n),
        SlotKeyData::Elem(x) => synthetic(keys, x),
        SlotKeyData::Field(x, name) => keys.dunder(name) || synthetic(keys, x),
        SlotKeyData::Anon(_) | SlotKeyData::Thrown => false,
    }
}

/// Push an escape down the Elem chain — a demoted outer array's inner
/// arrays must demote too (the any side reaches them through the
/// outer's NaN-box slots). Depth-capped: real TS nesting is shallow
/// and an unrepresented Elem key canonicalizes to itself, so the chain
/// would otherwise walk forever.
fn drain_elem_chain(
    work: &mut VecDeque<(SlotKeyId, usize)>,
    out: &mut KeySet,
    uf: &UnionFind,
    keys: &SlotKeys,
) {
    const ELEM_DEPTH_CAP: usize = 8;
    while let Some((k, d)) = work.pop_front() {
        if out.insert(k) && d < ELEM_DEPTH_CAP {
            let ek = canon_key_frozen(uf, keys, keys.elem(k));
            if !out.contains(&ek) {
                work.push_back((ek, d + 1));
            }
        }
    }
}

/// Post-canonicalize propagation: map every seed onto its frozen class
/// representative and push the escape down the Elem chain. An escaped
/// OBJECT also hands the any side every container it holds
/// (`(o as any).xs.push("s")` writes into `o.xs`'s block), so the walk
/// crosses the object's field points too — but only the fields that
/// hold a container (`containerish` evidence; a number field's width
/// or a fn-signature projection is not a block the any side can
/// write into), only program-spelled fields, and only from escapes
/// the program spelled: an any-twin method's `__this`, the class
/// registry's `any` locals and a generator state class's hoisted
/// locals are compiler slots, and crossing from them demoted every
/// class field array and every split product in the program.
pub(super) fn propagate(
    seeds: &[SlotKeyId],
    uf: &UnionFind,
    keys: &SlotKeys,
    containerish: &KeySet,
) -> KeySet {
    let mut out = KeySet::default();
    let mut work: VecDeque<(SlotKeyId, usize)> = (seeds.iter())
        .map(|k| (canon_key_frozen(uf, keys, *k), 0usize))
        .collect();
    drain_elem_chain(&mut work, &mut out, uf, keys);
    let held: KeySet = (containerish.iter())
        .map(|k| canon_key_frozen(uf, keys, *k))
        .collect();
    let projections: Vec<(SlotKeyId, SlotKeyId)> = (uf.keys())
        .filter_map(|k| match keys.data(k) {
            SlotKeyData::Field(base, name) if !keys.dunder(name) => Some((k, base)),
            _ => None,
        })
        .filter(|(k, _)| held.contains(&canon_key_frozen(uf, keys, *k)))
        .collect();
    let mut reach = KeySet::default();
    let mut work: VecDeque<(SlotKeyId, usize)> = (seeds.iter())
        .filter(|k| !synthetic(keys, **k))
        .map(|k| (canon_key_frozen(uf, keys, *k), 0usize))
        .collect();
    loop {
        drain_elem_chain(&mut work, &mut reach, uf, keys);
        for &(p, base) in &projections {
            let ck = canon_key_frozen(uf, keys, p);
            if !reach.contains(&ck) && reach.contains(&canon_key_frozen(uf, keys, base)) {
                work.push_back((ck, 0));
            }
        }
        if work.is_empty() {
            break;
        }
    }
    out.extend(reach);
    // TORAJS_NUM_WIDTH_STATS=1 — one line per escaped representative.
    if std::env::var_os("TORAJS_NUM_WIDTH_STATS").is_some() {
        let mut lines: Vec<String> = out.iter().map(|k| keys.debug(*k)).collect();
        lines.sort();
        for l in &lines {
            eprintln!("[num_width] any-escape class: {l}");
        }
    }
    out
}
