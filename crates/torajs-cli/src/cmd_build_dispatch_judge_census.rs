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
const EXEMPT: [(&str, &str); 1] = [(
    // keeping the obj world here costs every `str.match(re)` program
    // its arm strip (~49 KB on the nine bench regex cells, r690 A/B).
    "__torajs_str_match_regex",
    "its only coercion is ToNumber of a BOXED lastIndex, and a user \
     object reaches that slot only through `__torajs_any_member_set*` \
     or `__torajs_dynobj_define` (the `__torajs_regex_last_index_\
     store_boxed` / `_define` callers; member_get_value's call re-stores \
     a ShortStr already there) — both keep the obj world",
)];

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

/// Blank out comments, string / byte-string / raw-string literals and
/// char literals, so braces and identifiers inside them don't count.
fn strip(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while i < b.len() {
        let prev_ident = i > 0 && ident(b[i - 1]);
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < b.len() {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if !prev_ident && raw_string_len(&b[i..]).is_some() {
            i += raw_string_len(&b[i..]).unwrap_or(1);
            out.extend_from_slice(b"\"\"");
        } else if b[i] == b'"' || (!prev_ident && b[i..].starts_with(b"b\"")) {
            i += if b[i] == b'b' { 2 } else { 1 };
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
            out.extend_from_slice(b"\"\"");
        } else if b[i] == b'\'' {
            i += char_literal_len(&b[i..]).unwrap_or_else(|| {
                out.push(b'\'');
                1
            });
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Length of a raw (byte) string literal starting at `b`, if any.
fn raw_string_len(b: &[u8]) -> Option<usize> {
    let mut i = usize::from(b.first() == Some(&b'b'));
    if b.get(i) != Some(&b'r') {
        return None;
    }
    i += 1;
    let hashes = b[i..].iter().take_while(|&&c| c == b'#').count();
    i += hashes;
    if b.get(i) != Some(&b'"') {
        return None;
    }
    let close: Vec<u8> = std::iter::once(b'"')
        .chain(std::iter::repeat_n(b'#', hashes))
        .collect();
    let body = &b[i + 1..];
    let end = body
        .windows(close.len())
        .position(|w| w == close.as_slice())?;
    Some(i + 1 + end + close.len())
}

/// Length of a char literal at `b` (`'x'`, `'\n'`, `'é'`), or `None`
/// for a lifetime / label.
fn char_literal_len(b: &[u8]) -> Option<usize> {
    if b.get(1) == Some(&b'\\') {
        let end = b[2..].iter().position(|&c| c == b'\'')?;
        return Some(end + 3);
    }
    let width = match b.get(1)? {
        c if *c < 0x80 => 1,
        c if *c >= 0xF0 => 4,
        c if *c >= 0xE0 => 3,
        _ => 2,
    };
    (b.get(1 + width) == Some(&b'\'')).then_some(width + 2)
}

/// Cut every `#[cfg(test)] mod name { … }` block.
fn drop_test_mods(mut s: String) -> String {
    while let Some(at) = s.find("#[cfg(test)]") {
        let rest = &s[at + "#[cfg(test)]".len()..];
        let head = rest.trim_start();
        let head = head
            .strip_prefix("pub(crate)")
            .or_else(|| head.strip_prefix("pub"))
            .unwrap_or(head)
            .trim_start();
        let Some(open_rel) = head
            .strip_prefix("mod ")
            .and_then(|m| m.find(['{', ';']).filter(|&p| m.as_bytes()[p] == b'{'))
        else {
            // a cfg(test) fn / use — keep it (a test twin of a real fn
            // only widens the census), but step past the attribute.
            s.replace_range(at..at + "#[cfg(test)]".len(), "");
            continue;
        };
        let open = s.len() - head.len() + "mod ".len() + open_rel;
        let close = matching_brace(s.as_bytes(), open);
        s.replace_range(at..close, "");
    }
    s
}

/// Index one past the brace matching the `{` at `open`.
fn matching_brace(b: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    b.len()
}

/// Every `fn name(…) … { body }` in a stripped source: `(name, body)`.
fn fns(s: &str) -> Vec<(String, String)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = s[from..].find("fn ") {
        let at = from + rel;
        from = at + 3;
        if at > 0 && (b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_') {
            continue;
        }
        let name: String = s[from..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let Some(paren) = s[from..].find('(').map(|p| from + p) else {
            break;
        };
        let after = close_paren(b, paren);
        let Some(open) = s[after..].find(['{', ';']).map(|p| after + p) else {
            break;
        };
        if name.is_empty() || b[open] == b';' {
            continue;
        }
        let close = matching_brace(b, open);
        out.push((name, s[open..close].to_string()));
    }
    out
}

/// Index one past the `)` matching the `(` at `open`.
fn close_paren(b: &[u8], open: usize) -> usize {
    let mut depth = 0i32;
    for (i, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    b.len()
}

/// The names a body CALLS as free / path functions: an identifier not
/// preceded by `.` (a method on a std receiver would resolve by name to
/// an unrelated crate fn) and followed by `(` or a turbofish.
fn calls(body: &str) -> HashSet<&str> {
    let b = body.as_bytes();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = HashSet::new();
    let mut i = 0;
    while i < b.len() {
        if !(b[i].is_ascii_alphabetic() || b[i] == b'_') || (i > 0 && ident(b[i - 1])) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && ident(b[i]) {
            i += 1;
        }
        if start > 0 && b[start - 1] == b'.' {
            continue;
        }
        let rest = body[i..].trim_start();
        let rest = match rest.strip_prefix("::<") {
            Some(r) => r.find('>').map_or(r, |p| r[p + 1..].trim_start()),
            None => rest,
        };
        if rest.starts_with('(') {
            out.insert(&body[start..i]);
        }
    }
    out
}

pub(crate) struct Census {
    /// callee → callers.
    callers: HashMap<Key, HashSet<Key>>,
    pub fn_count: usize,
}

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).expect("crate src dir is readable") {
        let p = e.expect("dir entry is readable").path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Read as bytes: a stray NUL must not hide a file (rotation 678).
fn read_lossy(p: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(p).expect("source is readable")).into_owned()
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
