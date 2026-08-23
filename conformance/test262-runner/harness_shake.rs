//! Harness tree-shake — per-case minimal harness prepend (RFC
//! 20260724-test262-harness-preamble blade 1).
//!
//! The typed harness is ~700 lines; prepending all of it to every
//! case makes the tr front-end spend ~90% of its per-case work
//! re-compiling the same helpers 53k times per sweep. Split the
//! harness into top-level declaration segments once at startup, then
//! per case prepend only the segments whose declared names the
//! (transformed) case source references, closed over the segments'
//! own static references.
//!
//! Correctness posture: the scan is lexical (word-boundary substring
//! match), which can only over-include — a name mentioned in a string
//! or comment pulls its segment in, harmlessly. Under-inclusion is
//! impossible for statically referenced helpers and would be loud
//! anyway (tr AND bun both fail compiling the missing identifier;
//! the case flips to bug/harness-error, never a silent wrong pass).
//! Sweep counters (passTotal / bug / incompatible) are the
//! acceptance gate.

/// One top-level declaration segment of the harness: its declared
/// name, verbatim source slice (leading comment block included), and
/// the indices of other segments it references.
struct Segment {
    name: String,
    src: String,
    deps: Vec<usize>,
}

pub struct HarnessIndex {
    segments: Vec<Segment>,
}

impl HarnessIndex {
    /// Split the harness source into top-level declaration segments
    /// and wire their static dependency edges. Runs once at startup.
    pub fn build(harness: &str) -> Self {
        let mut segments: Vec<(String, Vec<String>)> = Vec::new();
        let mut pending: Vec<String> = Vec::new(); // comment/blank run preceding a decl
        let mut cur: Option<(String, Vec<String>)> = None;

        for line in harness.lines() {
            if let Some(name) = decl_name(line) {
                if let Some(seg) = cur.take() {
                    segments.push(seg);
                }
                let mut body = std::mem::take(&mut pending);
                body.push(line.to_string());
                cur = Some((name, body));
            } else if line.trim().is_empty() || line.trim_start().starts_with("//") {
                // Blank / comment run — could belong to the next
                // declaration; hold it out of the current segment so
                // a trailing doc block attaches forward.
                pending.push(line.to_string());
            } else {
                // Continuation of the current declaration (fn body,
                // multi-line const initialiser). Flush any held
                // blank/comment run back into the segment first.
                if let Some((_, body)) = cur.as_mut() {
                    body.append(&mut pending);
                    body.push(line.to_string());
                } else {
                    // Content before the first declaration that isn't
                    // comment/blank — file prologue; drop it with the
                    // pending run (the harness has none today).
                    pending.clear();
                }
            }
        }
        if let Some(seg) = cur.take() {
            segments.push(seg);
        }

        let names: Vec<String> = segments.iter().map(|(n, _)| n.clone()).collect();
        let segments: Vec<Segment> = segments
            .into_iter()
            .map(|(name, lines)| {
                let src = lines.join("\n");
                let deps = names
                    .iter()
                    .enumerate()
                    .filter(|(_, n)| **n != name && contains_word(&src, n))
                    .map(|(i, _)| i)
                    .collect();
                Segment { name, src, deps }
            })
            .collect();
        Self { segments }
    }

    /// The minimal harness prefix for one (already transformed) case
    /// source: every segment whose name the case references, closed
    /// over segment→segment references, in original harness order.
    pub fn minimal_for(&self, case_src: &str, gated_out: &[&str]) -> String {
        let mut selected = vec![false; self.segments.len()];
        let mut stack: Vec<usize> = Vec::new();
        for (i, seg) in self.segments.iter().enumerate() {
            // A segment that PORTS a harness include may only be
            // lexically selected by a case that DECLARED the
            // include: the shake matches names in comments and
            // strings too, and a case asserting that a helper does
            // NOT exist (`$DETACHBUFFER` throws a ReferenceError
            // when detachArrayBuffer.js was not included) spells
            // the very name it needs absent. Dep-closure below is
            // untouched — a harness-internal reference still pulls
            // the segment in.
            if gated_out.contains(&seg.name.as_str()) {
                continue;
            }
            if contains_word(case_src, &seg.name) {
                selected[i] = true;
                stack.push(i);
            }
        }
        while let Some(i) = stack.pop() {
            for &d in &self.segments[i].deps {
                if !selected[d] {
                    selected[d] = true;
                    stack.push(d);
                }
            }
        }
        let mut out = String::new();
        for (i, seg) in self.segments.iter().enumerate() {
            if selected[i] {
                out.push_str(&seg.src);
                out.push('\n');
            }
        }
        out
    }
}

/// `function NAME(` / `class NAME` / `const NAME` / `let NAME` /
/// `var NAME` / `type NAME` at column 0 → declared top-level name.
/// (`type` matters: r208 acceptance caught `type
/// __T262BuildStringArgs = ...` gluing onto the previous segment,
/// which stranded `buildString`'s parameter type whenever that
/// neighbor wasn't referenced — 560 RegExp cases type-errored.)
fn decl_name(line: &str) -> Option<String> {
    for kw in ["function ", "class ", "const ", "let ", "var ", "type "] {
        if let Some(rest) = line.strip_prefix(kw) {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
                .collect();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// Word-boundary substring search: `needle` occurs in `hay` not
/// flanked by identifier characters.
fn contains_word(hay: &str, needle: &str) -> bool {
    let hb = hay.as_bytes();
    let mut from = 0;
    while let Some(pos) = hay[from..].find(needle) {
        let start = from + pos;
        let end = start + needle.len();
        let pre_ok = start == 0 || !is_ident_byte(hb[start - 1]);
        let post_ok = end >= hb.len() || !is_ident_byte(hb[end]);
        if pre_ok && post_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

#[cfg(test)]
mod tests {
    use super::*;

    const HARNESS: &str = "\
// prologue comment dropped
class T262Err extends Error {
  constructor(m: string) { super(m); }
}

// helper one — depends on nothing
function __t262_assert(a: boolean): void {
  if (!a) { throw new T262Err(\"no\"); }
}

// helper two — depends on helper one
function __t262_sameValue<T>(a: T, b: T): void {
  __t262_assert(a === b);
}

const NaNs: number[] = [
  0 / 0,
];
";

    #[test]
    fn type_decl_is_its_own_segment_and_closes_into_users() {
        let harness = "\
const unrelated: number = 1;

type Args = { a: number };

function build(args: Args): number {
  return args.a;
}
";
        let idx = HarnessIndex::build(harness);
        let names: Vec<&str> = idx.segments.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["unrelated", "Args", "build"]);
        let min = idx.minimal_for("build({ a: 1 });", &[]);
        assert!(min.contains("type Args"), "param type closes in");
        assert!(!min.contains("unrelated"));
    }

    #[test]
    fn segments_split_on_top_level_decls() {
        let idx = HarnessIndex::build(HARNESS);
        let names: Vec<&str> = idx.segments.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            ["T262Err", "__t262_assert", "__t262_sameValue", "NaNs"]
        );
    }

    #[test]
    fn minimal_closes_over_deps() {
        let idx = HarnessIndex::build(HARNESS);
        let min = idx.minimal_for("__t262_sameValue(1, 1);", &[]);
        // sameValue pulls assert pulls the error class; NaNs stays out.
        assert!(min.contains("__t262_sameValue"));
        assert!(min.contains("__t262_assert"));
        assert!(min.contains("class T262Err"));
        assert!(!min.contains("NaNs"));
    }

    #[test]
    fn unreferenced_case_gets_empty_prefix() {
        let idx = HarnessIndex::build(HARNESS);
        assert_eq!(idx.minimal_for("console.log(1);", &[]), "");
    }

    #[test]
    fn word_boundary_rejects_substrings() {
        assert!(!contains_word("const NaNsplus = 1;", "NaNs"));
        assert!(contains_word("use(NaNs);", "NaNs"));
    }

    #[test]
    fn segments_keep_original_order() {
        let idx = HarnessIndex::build(HARNESS);
        let min = idx.minimal_for("__t262_sameValue(1, 1); NaNs;", &[]);
        let a = min.find("class T262Err").unwrap();
        let b = min.find("function __t262_assert").unwrap();
        let c = min.find("function __t262_sameValue").unwrap();
        let d = min.find("const NaNs").unwrap();
        assert!(a < b && b < c && c < d);
    }
}
