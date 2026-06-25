//! Word-boundary substitution on a type-annotation string —
//! `ann_substitute` helper used by `resolve_type_ann_full` to plug
//! concrete type-arg strings into a generic alias body. Extracted
//! from `check_type_ann.rs` to keep that file under the 500-prod-LOC
//! file-size hard limit (`rules/common/file-size.md`). Pure
//! mechanical pull, no semantic change.

/// Word-boundary substitution on a type-annotation string. Same shape as
/// the SSA layer's `substitute_in_ann` (kept local to check.rs to avoid
/// a cross-module dep). Used by `resolve_type_ann_full` to substitute
/// generic-alias type-params (`A`, `B`, ...) with concrete arg ann strings
/// (`number`, `string`, `Pair<number|string>`, ...) during instantiation.
pub(crate) fn ann_substitute(ann: &str, subst: &[(String, String)]) -> String {
    let mut out = String::with_capacity(ann.len());
    let bytes = ann.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        let is_word_start = c.is_ascii_alphabetic() || c == b'_';
        if !is_word_start {
            out.push(c as char);
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() {
            let cc = bytes[i];
            if cc.is_ascii_alphanumeric() || cc == b'_' {
                i += 1;
            } else {
                break;
            }
        }
        let word = &ann[start..i];
        if let Some((_, replacement)) = subst.iter().find(|(from, _)| from == word) {
            out.push_str(replacement);
        } else {
            out.push_str(word);
        }
    }
    out
}
