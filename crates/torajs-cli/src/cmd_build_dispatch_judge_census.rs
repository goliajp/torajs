//! Mechanical census for [`COERCION_PREFIXES`]: the hand-written table
//! drifted for weeks (rotation 690: 57 unregistered kernels, 8 of 31
//! probes a live `method family stripped` on a legal program), because
//! nothing told the author of a new kernel that it runs a coercion on a
//! value that may be a user object. This test is that signal.
//!
//! The census is a name-level call graph over the runtime crates'
//! sources (every crate that builds a staticlib), two layers deep.
//! Layer 1: exported kernels whose body reaches the private
//! OrdinaryToPrimitive machinery ([`SEAMS`]) through private helpers —
//! the coercion surface itself (`anyv_to_str`, `anyv_to_number`, the
//! pair kernels, the iterator steps). Layer 2: exported kernels whose
//! body reaches a seam or a layer-1 kernel the same way. Both are
//! flagged; propagation stops there. The full transitive closure takes
//! in ~300 kernels (the unhandled-rejection reporter in
//! `__torajs_main_exit_code` among them) and stops being a checklist,
//! and seeding from the table itself never reaches a fixpoint (every
//! entry added flags its callers). So this is a heuristic that errs
//! toward flagging on one axis (value-blind: a flagged kernel may never
//! see a user object) and toward missing on another (a kernel three
//! exported hops from the seam is not flagged). The structural fix is RFC
//! 20260922-to-primitive-get-call; until it lands, a new flagged kernel
//! fails this test at `cargo test` time instead of in a user program.
//!
//! A flag is resolved by adding the kernel to the table (the sound
//! direction — it only keeps arms) or to [`EXEMPT`] with the reason.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::cmd_build_dispatch_judge::{FALLBACK_PREFIXES, MID_CARRIERS, PREFIX_FAMILIES};
use crate::cmd_build_dispatch_judge_census_parse::{
    calls, drop_test_mods, fns, read_lossy, rs_files, strip,
};
use crate::cmd_build_dispatch_judge_obj_world::{COERCION_PREFIXES, KERNEL_PROBE_PREFIXES};
use torajs_rc::any_method_family as fam;

/// The private OrdinaryToPrimitive machinery (torajs-anyvalue
/// `to_primitive.rs`) — where a user object's `toString` / `valueOf`
/// is dispatched from inside the runtime.
const SEAMS: [&str; 3] = [
    "heap_to_primitive",
    "heap_to_primitive_default",
    "ordinary_to_primitive",
];

/// Census hits that stay out of the table, each with the reason no
/// user object can reach its coercion without the program emitting a
/// symbol that keeps the obj world anyway.
const EXEMPT: [(&str, &str); 2] = [
    (
        "__torajs_anyv_cast_str",
        "it calls the ToString seam only after testing that the value \
         already IS a string (short-str immediate or a Str cell), where \
         ToString is the identity and no user code can run",
    ),
    (
        // keeping the obj world here costs every `str.match(re)` program
        // its arm strip (~49 KB on the nine bench regex cells, r690 A/B).
        "__torajs_str_match_regex",
        "its only coercion is ToNumber of a BOXED lastIndex, and a user \
     object reaches that slot only through `__torajs_any_member_set*` \
     or `__torajs_dynobj_define` (the `__torajs_regex_last_index_\
     store_boxed` / `_define` callers; member_get_value's call re-stores \
     a ShortStr already there) — both keep the obj world",
    ),
];

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
    starts_any(name, &KERNEL_PROBE_PREFIXES)
        || starts_any(name, &FALLBACK_PREFIXES)
        || MID_CARRIERS.iter().any(|(m, _)| *m == name)
        || PREFIX_FAMILIES
            .iter()
            .any(|(p, bits)| name.starts_with(p) && bits & fam::FAM_OBJ_WORLD == fam::FAM_OBJ_WORLD)
}

/// Flagged kernels a lowering can emit that nothing keeps the obj
/// world for, sorted.
pub(crate) fn unregistered(census: &Census, table: &[&str], exempt: &[&str]) -> Vec<String> {
    let lowered = lowering_names();
    let mut out: Vec<String> = census
        .reach()
        .into_iter()
        .filter(|n| lowered.contains(n) && !starts_any(n, table) && !kept_otherwise(n))
        .filter(|n| !exempt.contains(&n.as_str()))
        .collect();
    out.sort();
    out
}

#[test]
fn every_coercing_kernel_is_in_the_table() {
    let census = build();
    let exempt: Vec<&str> = EXEMPT.iter().map(|(n, _)| *n).collect();
    let open = unregistered(&census, &COERCION_PREFIXES, &exempt);
    assert!(
        open.is_empty(),
        "kernels reach a coercion seam but are not in COERCION_PREFIXES \
         (add them, or to EXEMPT with the reason): {open:#?}"
    );
}

#[test]
fn census_turns_red_without_a_known_entry() {
    // the instrument must be able to fail: an empty scan would pass
    // the test above. `String.raw` and `new ArrayBuffer(q)` were live
    // loud-rejects before rotation 690 added them.
    let census = build();
    assert!(census.fn_count > 5000, "scan found {} fns", census.fn_count);
    let dropped = ["__torajs_string_raw", "__torajs_arraybuffer_create"];
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
fn every_exemption_is_still_a_census_hit() {
    let census = build();
    let open = unregistered(&census, &COERCION_PREFIXES, &[]);
    for (n, _) in EXEMPT {
        assert!(open.iter().any(|o| o == n), "stale exemption {n}");
    }
}
