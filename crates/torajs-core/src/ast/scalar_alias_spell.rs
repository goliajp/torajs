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
    let b = ann.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0usize;
    let mut i = 0usize;
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
        if field_name || qualified {
            continue;
        }
        if let Some(spelling) = table.get(&ann[start..i]) {
            let o = out.get_or_insert_with(|| String::with_capacity(ann.len() + 16));
            o.push_str(&ann[copied..start]);
            o.push_str(spelling);
            copied = i;
        }
    }
    let mut o = out?;
    o.push_str(&ann[copied..]);
    if o.contains(UNION) || o.contains(NULLABLE) {
        o = canonical_unions(&o);
    }
    Some(o)
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
            inline_ann("__inlobj(Id:__nullable(Id)|N:__fn(N)->(N))", &t).as_deref(),
            Some("__inlobj(Id:__nullable(string)|N:__fn(number)->(number))")
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
}
