//! The decoding of an inline object-type annotation (`__inlobj(..)`).
//! Moved out of [`super`].

use std::collections::HashMap;

/// Chunk 793 — decode an inline object-type annotation
/// (`__inlobj(f:ann|...)`, optionally `__nullable(...)`-wrapped)
/// into its field→annotation map; `None` for any other ann shape.
/// Splits on the checker's own `check_type_ann::split_top_pipe`, so
/// the two readers of this encoding cannot drift apart. The private
/// copy this replaced nested parens only, which cut a field spelled
/// with a multi-argument generic (`{ m: Map<string, number> }` →
/// `__inlobj(m:Map<string|number>)`) in half.
pub(crate) fn parse_inlobj_field_anns(ann: &str) -> Option<HashMap<String, String>> {
    let fields = inlobj_fields(ann)?;
    Some(fields.into_iter().map(|(k, _, a)| (k, a)).collect())
}

/// [`parse_inlobj_field_anns`] with each field's optional mark (`m?`
/// or `m??` in the encoding: a member written `m?: T`).
pub(crate) fn inlobj_fields(ann: &str) -> Option<Vec<(String, bool, String)>> {
    let t = ann.trim();
    let t = t
        .strip_prefix("__nullable(")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(t);
    let body = t.strip_prefix("__inlobj(")?.strip_suffix(')')?;
    let mut out = Vec::new();
    if body.trim().is_empty() {
        return Some(out);
    }
    for seg in crate::check_type_ann::split_top_pipe(body) {
        let colon = seg.find(':')?;
        let name = seg[..colon].trim();
        out.push((
            name.trim_end_matches('?').to_string(),
            name.ends_with('?'),
            seg[colon + 1..].trim().to_string(),
        ));
    }
    Some(out)
}
