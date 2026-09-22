//! Mechanical census that PRODUCES [`COERCION_PREFIXES`]: the
//! hand-written table drifted for weeks (rotation 690: 57
//! unregistered kernels, 8 of 31 probes a live `method family
//! stripped` on a legal program), because nothing told the author of
//! a new kernel that it re-enters the dispatcher with a receiver that
//! may be a user object. These tests are that signal, in both
//! directions: a kernel the table is missing fails
//! [`every_dispatching_kernel_is_in_the_table`], and an entry the
//! census no longer reaches fails [`no_stale_table_entry`].
//!
//! The census is a name-level call graph over the runtime crates'
//! sources (every crate that builds a staticlib), two layers deep.
//! Layer 1: exported kernels whose body reaches a dispatcher entry
//! ([`SEAMS`]) through private helpers. Layer 2: exported kernels
//! whose body reaches a seam or a layer-1 kernel the same way. Both
//! are flagged; propagation stops there. The full transitive closure
//! takes in ~300 kernels (the unhandled-rejection reporter in
//! `__torajs_main_exit_code` among them) and stops being a checklist,
//! and seeding from the table itself never reaches a fixpoint (every
//! entry added flags its callers). So this is a heuristic that errs
//! toward flagging on one axis (value-blind: a flagged kernel may
//! never see a user object) and toward missing on another (a kernel
//! three exported hops from a seam is not flagged).
//!
//! **The seam moved in rotation 693.** It was the private
//! OrdinaryToPrimitive machinery (`heap_to_primitive*` /
//! `ordinary_to_primitive`), because that is where a coercion used to
//! re-dispatch a user `toString` / `valueOf` mid. RFC
//! `20260922-to-primitive-get-call` made ToPrimitive spell §7.1.1.1
//! literally — Get, IsCallable, Call — so coercion no longer
//! re-dispatches anything and the question is no longer "who
//! coerces". It is "who re-enters the dispatcher", and the seam is
//! the dispatcher's own entries. 41 kernels left the table on that
//! measurement; what stayed, stayed because of the **Call** step (a
//! borrowed builtin method unit is dispatched, and the direct table
//! covers root props only).
//!
//! A flag is resolved by adding the kernel to the table (the sound
//! direction — it only keeps arms) or to [`EXEMPT`] with the reason.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::cmd_build_dispatch_judge::{FALLBACK_PREFIXES, MID_CARRIERS, PREFIX_FAMILIES};
use crate::cmd_build_dispatch_judge_census_parse::{
    calls, drop_test_mods, fns, read_lossy, rs_files, strip,
};
use crate::cmd_build_dispatch_judge_obj_world::COERCION_PREFIXES;
use torajs_rc::any_method_family as fam;

/// The dispatcher's entries — where a runtime kernel hands a
/// receiver that may be a user object back to the mid dispatcher.
const SEAMS: [&str; 3] = [
    "any_method_call_inner",
    "any_method_redispatch",
    "__torajs_any_method_dispatch",
];

/// Census hits that stay out of the table, each with the reason no
/// user object can reach its re-dispatch without the program emitting
/// a symbol that keeps the obj world anyway.
///
/// **Empty since rotation 693.** Both entries were arguments about
/// the OrdinaryToPrimitive seam and the new one does not reach
/// either kernel, so keeping them would be a claim the census
/// contradicts ([`no_stale_exemption`] is what says so). One of them
/// is worth remembering: `__torajs_str_match_regex` was exempted
/// because registering it cost every `str.match(re)` program its arm
/// strip (~49 KB across the nine bench regex cells, r690 A/B) — a
/// reasoned but uncomfortable carve-out, since the argument had to
/// enumerate which callers could put a user object in a boxed
/// `lastIndex`. Spelling §7.1.1.1 literally retired the question
/// instead of answering it: the whole regex any-argument lane left
/// the table on measurement.
const EXEMPT: [(&str, &str); 0] = [];

type Key = (String, String);

/// `(runtime crate, fn)` for private fns, `("*", name)` for the
/// exported `__torajs_*` kernels (one global namespace at link time).
fn key(krate: &str, name: &str) -> Key {
    if name.starts_with("__torajs_") {
        ("*".to_string(), name.to_string())
    } else {
        (krate.to_string(), name.to_string())
    }
}

pub(crate) struct Census {
    /// callee → callers.
    callers: HashMap<Key, HashSet<Key>>,
    pub fn_count: usize,
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

pub(crate) fn build() -> Census {
    let mut bodies: Vec<(Key, String, String)> = Vec::new();
    for e in std::fs::read_dir(crates_dir()).expect("crates/ is readable") {
        let dir = e.expect("dir entry is readable").path();
        let manifest = dir.join("Cargo.toml");
        if !manifest.exists() || !read_lossy(&manifest).contains("staticlib") {
            continue;
        }
        let krate = dir
            .file_name()
            .expect("crate dir")
            .to_string_lossy()
            .into_owned();
        let mut files = Vec::new();
        rs_files(&dir.join("src"), &mut files);
        for f in files {
            for (name, body) in fns(&drop_test_mods(strip(&read_lossy(&f)))) {
                bodies.push((key(&krate, &name), krate.clone(), body));
            }
        }
    }
    let defined: HashSet<Key> = bodies.iter().map(|(k, _, _)| k.clone()).collect();
    let mut callers: HashMap<Key, HashSet<Key>> = HashMap::new();
    for (caller, krate, body) in &bodies {
        for callee in calls(body) {
            let k = key(krate, callee);
            if k != *caller && defined.contains(&k) {
                callers.entry(k).or_default().insert(caller.clone());
            }
        }
    }
    Census {
        callers,
        fn_count: bodies.len(),
    }
}

fn starts_any(name: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| name.starts_with(p))
}

impl Census {
    /// Layer 1 ∪ layer 2 (see the module doc).
    pub(crate) fn reach(&self) -> HashSet<String> {
        let seams: Vec<Key> = SEAMS.iter().map(|s| key("torajs-anyvalue", s)).collect();
        let layer1 = self.reach_from(seams.clone());
        let mut seeds = seams;
        seeds.extend(layer1.iter().map(|n| key("*", n)));
        let mut out = self.reach_from(seeds);
        out.extend(layer1);
        out
    }

    /// Exported kernels reaching a seed through private helpers only.
    fn reach_from(&self, seeds: Vec<Key>) -> HashSet<String> {
        let mut seen: HashSet<Key> = seeds.iter().cloned().collect();
        let mut stack = seeds;
        let mut hits = HashSet::new();
        while let Some(k) = stack.pop() {
            for c in self.callers.get(&k).into_iter().flatten() {
                if !seen.insert(c.clone()) {
                    continue;
                }
                if c.0 == "*" {
                    hits.insert(c.1.clone());
                } else {
                    stack.push(c.clone());
                }
            }
        }
        hits
    }
}

/// Every `__torajs_*` spelling in torajs-core — the kernels a lowering
/// can put into a user `.o` (the only symbols the judgment sees).
fn lowering_names() -> HashSet<String> {
    let mut files = Vec::new();
    rs_files(&crates_dir().join("torajs-core/src"), &mut files);
    let mut out = HashSet::new();
    for f in files {
        let s = read_lossy(&f);
        let mut from = 0;
        while let Some(rel) = s[from..].find("__torajs_") {
            let at = from + rel;
            let end = s[at..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map_or(s.len(), |p| at + p);
            out.insert(s[at..end].to_string());
            from = end;
        }
    }
    out
}

/// Whether the judgment already keeps the obj world when `name` is
/// emitted, by any rule other than the coercion table.
fn kept_otherwise(name: &str) -> bool {
    starts_any(name, &FALLBACK_PREFIXES)
        || MID_CARRIERS.iter().any(|(m, _)| *m == name)
        || PREFIX_FAMILIES
            .iter()
            .any(|(p, bits)| name.starts_with(p) && bits & fam::FAM_OBJ_WORLD == fam::FAM_OBJ_WORLD)
}

/// Flagged kernels a lowering can actually emit — the only names the
/// judgment ever sees in a user `.o`. Sorted.
pub(crate) fn emittable_hits(census: &Census) -> Vec<String> {
    let lowered = lowering_names();
    let mut out: Vec<String> = census
        .reach()
        .into_iter()
        .filter(|n| lowered.contains(n))
        .collect();
    out.sort();
    out
}

/// Flagged kernels a lowering can emit that nothing keeps the obj
/// world for, sorted.
pub(crate) fn unregistered(census: &Census, table: &[&str], exempt: &[&str]) -> Vec<String> {
    emittable_hits(census)
        .into_iter()
        .filter(|n| !starts_any(n, table) && !kept_otherwise(n))
        .filter(|n| !exempt.contains(&n.as_str()))
        .collect()
}

#[test]
fn every_dispatching_kernel_is_in_the_table() {
    let census = build();
    let exempt: Vec<&str> = EXEMPT.iter().map(|(n, _)| *n).collect();
    let open = unregistered(&census, &COERCION_PREFIXES, &exempt);
    assert!(
        open.is_empty(),
        "kernels reach a dispatcher entry but are not in \
         COERCION_PREFIXES (add them, or to EXEMPT with the reason): \
         {open:#?}"
    );
}

/// The other direction. Without it the table only ever grows: an
/// entry whose seam edge is gone keeps arms alive in every program
/// that emits it, and nothing says so. Rotation 693 found 41 such
/// entries at once, because the seam had moved under the whole table.
#[test]
fn no_stale_table_entry() {
    let hits = emittable_hits(&build());
    let stale: Vec<&str> = COERCION_PREFIXES
        .iter()
        .copied()
        .filter(|p| !hits.iter().any(|h| h.starts_with(p)))
        .collect();
    assert!(
        stale.is_empty(),
        "table entries the census no longer reaches — drop them (they \
         cost every program that emits one its arm strip): {stale:#?}"
    );
}

#[test]
fn census_turns_red_without_a_known_entry() {
    // the instrument must be able to fail: an empty scan would pass
    // the test above. Both of these are live census hits under the
    // rotation-693 seam — `o[k]` runs ToPropertyKey in the kernel,
    // and `a.length = obj` runs ArraySetLength inside member_set.
    let census = build();
    assert!(census.fn_count > 5000, "scan found {} fns", census.fn_count);
    let dropped = ["__torajs_any_index_get", "__torajs_any_member_set"];
    let table: Vec<&str> = COERCION_PREFIXES
        .iter()
        .copied()
        .filter(|p| !dropped.contains(p))
        .collect();
    let open = unregistered(&census, &table, &[]);
    for d in dropped {
        assert!(open.iter().any(|n| n == d), "{d} not flagged: {open:?}");
    }
}

#[test]
fn no_stale_exemption() {
    let census = build();
    let open = unregistered(&census, &COERCION_PREFIXES, &[]);
    for (n, _) in EXEMPT {
        assert!(open.iter().any(|o| o == n), "stale exemption {n}");
    }
}
