//! The annotation-string face of the scalar type alias inline: where
//! in an annotation an alias name stands as a type.
//!
//! Annotations are the parser's canonical encoding, not source text:
//! `__inlobj(k:T|..)` for an object type, `__fn(P|..)->(R)` for a
//! function type (parameter names dropped), `__union(A|B)` /
//! `__nullable(T)` for unions, `Name<A|B>` for type arguments, `T[]`
//! for arrays, and a string literal type is already widened to
//! `string`. In that grammar a word is a type except where it is an
//! object type's field name — the word right before a `:` — or a part
//! of a qualified name. A union is canonical — members flattened,
//! deduplicated, sorted, nullish ones folded into `__nullable` — and
//! annotations compare as strings downstream, so a union a replacement
//! landed in is rebuilt the way the parser builds one.

use std::collections::HashMap;

/// `ann` with every type-position mention of a `table` name replaced by
/// its spelling; `None` when nothing changes.
pub(super) fn inline_ann(ann: &str, table: &HashMap<String, String>) -> Option<String> {
    let mut out: Option<String> = None;
    let mut copied = 0usize;
    for (start, end) in type_word_spans(ann) {
        if let Some(spelling) = table.get(&ann[start..end]) {
            let o = out.get_or_insert_with(|| String::with_capacity(ann.len() + 16));
            o.push_str(&ann[copied..start]);
            o.push_str(spelling);
            copied = end;
        }
    }
    let mut o = out?;
    o.push_str(&ann[copied..]);
    if o.contains(UNION) || o.contains(NULLABLE) {
        o = canonical_unions(&o);
    }
    if o.contains(INLOBJ) {
        o = closure_inlobj_fields(&o);
    }
    Some(o)
}

/// The words of `ann` that stand in a type position, in order.
pub(super) fn type_words(ann: &str) -> impl Iterator<Item = &str> {
    type_word_spans(ann).map(move |(s, e)| &ann[s..e])
}

/// Byte spans of the words in a type position: not an object type's
/// field name (the word right before a `:` or `?`), not a part of a
/// qualified name.
fn type_word_spans(ann: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let b = ann.as_bytes();
    let mut i = 0usize;
    std::iter::from_fn(move || {
        while i < b.len() {
            if !is_word_byte(b[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < b.len() && is_word_byte(b[i]) {
                i += 1;
            }
            let field_name = b.get(i) == Some(&b':') || b.get(i) == Some(&b'?');
            let qualified = b.get(i) == Some(&b'.') || (start > 0 && b[start - 1] == b'.');
            if !(field_name || qualified) {
                return Some((start, i));
            }
        }
        None
    })
}

const UNION: &str = "__union(";
const NULLABLE: &str = "__nullable(";

/// `ann` with every union marker rebuilt by the parser's own combine.
pub(crate) fn canonical_unions(ann: &str) -> String {
    let at = [UNION, NULLABLE]
        .iter()
        .filter_map(|m| ann.find(m).map(|p| (p, *m)))
        .min();
    let Some((p, marker)) = at else {
        return ann.to_string();
    };
    let open = p + marker.len();
    let close = matching_paren(ann, open);
    let inner = &ann[open..close];
    let combined = if marker == UNION {
        let members: Vec<String> = crate::check_type_ann::split_top_pipe(inner)
            .into_iter()
            .map(canonical_unions)
            .collect();
        let parts: Vec<&str> = members.iter().map(String::as_str).collect();
        crate::parser::union_combine(&parts)
    } else {
        crate::parser::union_combine(&[canonical_unions(inner).as_str(), "undefined"])
    };
    let rest = canonical_unions(&ann[(close + 1).min(ann.len())..]);
    format!("{}{combined}{rest}", &ann[..p])
}

const INLOBJ: &str = "__inlobj(";

/// `ann` with every object-type field whose type is a function type
/// spelled closure-shaped, the way the parser spells such a field when
/// it reads the function type in place: a field is a mutable slot that
/// may hold a capturing closure.
fn closure_inlobj_fields(ann: &str) -> String {
    let Some(p) = ann.find(INLOBJ) else {
        return ann.to_string();
    };
    let open = p + INLOBJ.len();
    let close = matching_paren(ann, open);
    let fields: Vec<String> = crate::check_type_ann::split_top_pipe(&ann[open..close])
        .into_iter()
        .map(|field| match field.split_once(':') {
            Some((name, ty)) => {
                let ty = crate::ast::retag_field_fn_ann(&closure_inlobj_fields(ty));
                format!("{name}:{ty}")
            }
            None => field.to_string(),
        })
        .collect();
    let rest = closure_inlobj_fields(&ann[(close + 1).min(ann.len())..]);
    format!("{}{INLOBJ}{}){rest}", &ann[..p], fields.join("|"))
}

/// The index of the `)` closing the group whose contents start at
/// `open`.
fn matching_paren(s: &str, open: usize) -> usize {
    let mut depth = 1usize;
    for (i, c) in s.bytes().enumerate().skip(open) {
        match c {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    s.len()
}

pub(super) fn is_word_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'$'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn field_names_and_qualified_parts_are_not_types() {
        let t = table(&[("N", "number"), ("Id", "string")]);
        assert_eq!(
            inline_ann("__inlobj(N:N)", &t).as_deref(),
            Some("__inlobj(N:number)")
        );
        assert_eq!(
            inline_ann("__inlobj(Id:__nullable(Id)|N:__cls(N)->(N))", &t).as_deref(),
            Some("__inlobj(Id:__nullable(string)|N:__cls(number)->(number))")
        );
        assert_eq!(inline_ann("ns.N", &t), None);
        assert_eq!(inline_ann("N.x", &t), None);
    }

    #[test]
    fn a_union_a_replacement_lands_in_stays_canonical() {
        let t = table(&[("N", "number"), ("Nil", "undefined")]);
        assert_eq!(
            inline_ann("__union(N|number)", &t).as_deref(),
            Some("number")
        );
        assert_eq!(
            inline_ann("__union(Nil|string)", &t).as_deref(),
            Some("__nullable(string)")
        );
        assert_eq!(
            inline_ann("__nullable(__union(N|boolean))[]", &t).as_deref(),
            Some("__nullable(__union(boolean|number))[]")
        );
    }

    #[test]
    fn every_type_position_is_replaced() {
        let t = table(&[("N", "number"), ("Id", "string")]);
        assert_eq!(inline_ann("N[][]", &t).as_deref(), Some("number[][]"));
        assert_eq!(
            inline_ann("Map<Id|N[]>", &t).as_deref(),
            Some("Map<string|number[]>")
        );
        assert_eq!(
            inline_ann("__union(Id|N)", &t).as_deref(),
            Some("__union(number|string)")
        );
        assert_eq!(
            inline_ann("__dict(Id|__rest(N))", &t).as_deref(),
            Some("__dict(string|__rest(number))")
        );
        assert_eq!(inline_ann("Nope[]", &t), None);
        assert_eq!(
            inline_ann("Map<__union(N|boolean)|__nullable(Id)>", &t).as_deref(),
            Some("Map<__union(boolean|number)|__nullable(string)>")
        );
        assert_eq!(inline_ann("string", &t), None);
    }

    #[test]
    fn a_function_type_landing_in_an_object_field_is_closure_shaped() {
        let t = table(&[("G", "__fn(number)->(number)")]);
        assert_eq!(
            inline_ann("__inlobj(f:G|g:__nullable(G)|h:G[])", &t).as_deref(),
            Some(
                "__inlobj(f:__cls(number)->(number)|g:__nullable(__cls(number)->(number))\
                 |h:__cls(number)->(number)[])"
            )
        );
        assert_eq!(
            inline_ann("__inlobj(o:__inlobj(f:G))", &t).as_deref(),
            Some("__inlobj(o:__inlobj(f:__cls(number)->(number)))")
        );
        assert_eq!(
            inline_ann("__fn(G)->(G)", &t).as_deref(),
            Some("__fn(__fn(number)->(number))->(__fn(number)->(number))")
        );
    }
}
