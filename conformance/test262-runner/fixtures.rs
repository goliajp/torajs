//! Sibling `*_FIXTURE.js` staging for module-import cases.
//!
//! test262 module cases import fixture files from their own corpus
//! directory (`import "./setup_FIXTURE.js"` — the `_FIXTURE` suffix
//! marks files the runner must never execute as tests). The runner
//! executes the assembled case from a temp file, so those relative
//! imports resolved against the temp dir and always missed. Staging
//! copies the case directory's fixtures next to the assembled
//! source: each worker slot owns a temp SUBDIRECTORY
//! (`torajs-test262-<pid>-<slot>/case.ts`), fixtures land beside it,
//! and both tr and the bun oracle resolve them naturally.
//!
//! The returned salt (fixture names + bytes) appends to the oracle
//! cache's case key: a fixture-referencing case's cached verdict was
//! recorded against the fixture-missing environment, and the key
//! must flip now that imports resolve (fixture-less cases keep their
//! existing entries — no oracle rebuild).

use std::path::Path;

/// Prepare `slot_dir` for one case: clear fixtures staged by the
/// previous case on this slot, then — when the case references a
/// fixture — copy every sibling `*_FIXTURE.js` in and answer the
/// cache salt. Chained fixture-to-fixture imports resolve because
/// the whole sibling set stages together. Best-effort on I/O errors:
/// a missing fixture keeps today's import-error verdict.
pub fn stage(slot_dir: &Path, case_path: &Path, case_src: &str) -> Vec<u8> {
    let _ = std::fs::create_dir_all(slot_dir);
    if let Ok(rd) = std::fs::read_dir(slot_dir) {
        for e in rd.flatten() {
            // Fixtures AND the previous case's self-import alias —
            // both are `.js`; the assembled case itself is `case.ts`
            // / `case.cts` and never matches.
            if e.file_name().to_string_lossy().ends_with(".js") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    // Two ways a case needs its directory staged, and only the first
    // was asked about. A case can reference a `_FIXTURE.js` sibling —
    // or it can reference ITSELF by corpus filename, which is what
    // §16.2 self-import cases do (`import f from
    // './instn-named-bndng-dflt-fun-anon.js'` IS that case). The
    // alias write below has handled the second kind since it was
    // added, but it sat behind this gate, so a case with no `_FIXTURE`
    // string returned here and never reached it: 32 of the 75
    // `import error` verdicts under `language/module-code` were this
    // gate, not a gap in tr.
    let self_name = case_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned());
    let refs_self = self_name.as_deref().is_some_and(|n| case_src.contains(n));
    if !case_src.contains("_FIXTURE") && !refs_self {
        return Vec::new();
    }
    let Some(dir) = case_path.parent() else {
        return Vec::new();
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with("_FIXTURE.js"))
        .collect();
    // Deterministic salt across sweeps regardless of read_dir order.
    names.sort();
    let mut salt = Vec::new();
    for n in &names {
        if let Ok(bytes) = std::fs::read(dir.join(n)) {
            let _ = std::fs::write(slot_dir.join(n), &bytes);
            salt.extend_from_slice(n.as_bytes());
            salt.push(0xff);
            salt.extend_from_slice(&bytes);
            salt.push(0xff);
        }
    }
    // Self-import alias — the assembled case runs as `case.ts`, but
    // several module-code fixtures import the case back by its
    // CORPUS filename (`export { x } from './instn-iee-err-circular
    // .js'` where that file IS the case). Stage the raw case source
    // under its original name too so those chains resolve instead of
    // dying on path-not-found. The alias is the UNTRANSFORMED source
    // (both runtimes see the same module text); it rides the oracle
    // salt so cached verdicts from the alias-less staging era can't
    // score this one.
    if let Some(name) = case_path.file_name() {
        let _ = std::fs::write(slot_dir.join(name), case_src.as_bytes());
        salt.extend_from_slice(name.to_string_lossy().as_bytes());
        salt.push(0xfe);
        salt.extend_from_slice(case_src.as_bytes());
        salt.push(0xfe);
    }
    // Fixture chains can also reference SIBLING CASE files (the -as
    // variant's fixture points at the base variant: `export { x } from
    // './instn-iee-err-circular.js'` inside circular-as's chain).
    // Pull every same-directory `.js` a staged fixture mentions, to a
    // fixpoint — chains are short, and anything already staged (the
    // fixtures, the case alias) is skipped by the existence check.
    // The case's OWN alias seeds the scan alongside the fixtures: a
    // case that names a sibling directly is the same chain one link
    // shorter, and seeding only from `_FIXTURE` files could not see
    // it.
    let mut scan: Vec<String> = names;
    if let Some(n) = self_name {
        scan.push(n);
    }
    while !scan.is_empty() {
        let mut next: Vec<String> = Vec::new();
        for n in &scan {
            let Ok(bytes) = std::fs::read(dir.join(n)) else {
                continue;
            };
            for r in sibling_js_refs(&bytes) {
                if slot_dir.join(&r).is_file() || !dir.join(&r).is_file() {
                    continue;
                }
                if let Ok(rb) = std::fs::read(dir.join(&r)) {
                    let _ = std::fs::write(slot_dir.join(&r), &rb);
                    salt.extend_from_slice(r.as_bytes());
                    salt.push(0xfd);
                    salt.extend_from_slice(&rb);
                    salt.push(0xfd);
                    next.push(r);
                }
            }
        }
        next.sort();
        scan = next;
    }
    salt
}

/// `./<name>.js` references inside one staged module's source —
/// byte-scan, no string-literal awareness needed: a false positive
/// only stages an extra sibling file nobody imports.
fn sibling_js_refs(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'.' && bytes[i + 1] == b'/' {
            let start = i + 2;
            let mut j = start;
            while j < bytes.len() && !matches!(bytes[j], b'\'' | b'"' | b'\n' | b'`') {
                j += 1;
            }
            if j > start && bytes[start..j].ends_with(b".js") {
                out.push(String::from_utf8_lossy(&bytes[start..j]).into_owned());
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}
