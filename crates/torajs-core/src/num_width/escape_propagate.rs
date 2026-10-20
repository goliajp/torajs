//! W-ESC — closing the escape seeds over the container alias classes
//! (`escape.rs` collects them).

use std::collections::{HashSet, VecDeque};

use super::SlotKey;
use super::container::{UnionFind, canon_key_frozen};

/// A key the desugar spelled, not the program: a synthetic binding,
/// class, field or return slot (`__cprologue`'s class registry, an
/// any-twin method's `__this`, a constructor's `__new_target`, a
/// generator state class). Their `any` faces are the compiler's own
/// slots — no program write reaches a container through them. A
/// program-named binding inside a synthetic fn (a hoisted local of an
/// async body, the parameter of an any-widened twin) is the program's.
pub(super) fn synthetic(k: &SlotKey) -> bool {
    let dunder = |s: &str| s.starts_with("__");
    match k {
        SlotKey::Global(n)
        | SlotKey::Captured(n)
        | SlotKey::Ret(n)
        | SlotKey::Class(n)
        | SlotKey::Local(_, n)
        | SlotKey::Param(_, n) => dunder(n),
        SlotKey::Elem(x) => synthetic(x),
        SlotKey::Field(x, name) => dunder(&name.lossy()) || synthetic(x),
        SlotKey::Anon(_) | SlotKey::Thrown => false,
    }
}

/// Push an escape down the Elem chain — a demoted outer array's inner
/// arrays must demote too (the any side reaches them through the
/// outer's NaN-box slots). Depth-capped: real TS nesting is shallow
/// and an unrepresented Elem key canonicalizes to itself, so the chain
/// would otherwise walk forever.
fn drain_elem_chain(
    work: &mut VecDeque<(SlotKey, usize)>,
    out: &mut HashSet<SlotKey>,
    uf: &UnionFind,
) {
    const ELEM_DEPTH_CAP: usize = 8;
    while let Some((k, d)) = work.pop_front() {
        if out.insert(k.clone()) && d < ELEM_DEPTH_CAP {
            let ek = canon_key_frozen(uf, &SlotKey::Elem(Box::new(k)));
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
    seeds: &[SlotKey],
    uf: &UnionFind,
    containerish: &HashSet<SlotKey>,
) -> HashSet<SlotKey> {
    let mut out: HashSet<SlotKey> = HashSet::new();
    let mut work: VecDeque<(SlotKey, usize)> = seeds
        .iter()
        .map(|k| (canon_key_frozen(uf, k), 0usize))
        .collect();
    drain_elem_chain(&mut work, &mut out, uf);
    let held: HashSet<SlotKey> = containerish
        .iter()
        .map(|k| canon_key_frozen(uf, k))
        .collect();
    let projections: Vec<&SlotKey> = uf
        .keys()
        .filter(|k| {
            matches!(k, SlotKey::Field(_, name) if !name.lossy().starts_with("__"))
                && held.contains(&canon_key_frozen(uf, k))
        })
        .collect();
    let mut reach: HashSet<SlotKey> = HashSet::new();
    let mut work: VecDeque<(SlotKey, usize)> = seeds
        .iter()
        .filter(|k| !synthetic(k))
        .map(|k| (canon_key_frozen(uf, k), 0usize))
        .collect();
    loop {
        drain_elem_chain(&mut work, &mut reach, uf);
        for p in &projections {
            let SlotKey::Field(base, _) = p else {
                continue;
            };
            let ck = canon_key_frozen(uf, p);
            if !reach.contains(&ck) && reach.contains(&canon_key_frozen(uf, base)) {
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
        let mut lines: Vec<String> = out.iter().map(|k| format!("{k:?}")).collect();
        lines.sort();
        for l in &lines {
            eprintln!("[num_width] any-escape class: {l}");
        }
    }
    out
}
