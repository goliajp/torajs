//! The members of object type literals (`{ c: C }`, spelled
//! `__inlobj(c:C)`) named like a field `super::obj_fields` widens.
//!
//! An object is handed as it is to a slot of any type its shape fits:
//! a class instance whose field `c` holds an `any` value can reach a
//! parameter `k: { c: C }`, whose reads would take the value at `C`'s
//! offsets. Every such member is respelled `any` wherever an
//! annotation spells it, as the declared types' fields are.

use std::collections::HashSet;

use super::retypable::ObjectTypes;
use crate::ast::ann_rewrite::{AnnRewrite, rewrite_anns, sub};
use crate::ast::{Ast, PropKey};

/// The members of the object type literal `ann` spells, when it spells
/// one: `(name, annotation)`.
pub(super) fn members(ann: &str) -> Option<Vec<(&str, &str)>> {
    let body = ann.strip_prefix("__inlobj(")?.strip_suffix(')')?;
    let parts = split_top(body);
    (parts.iter())
        .map(|p| p.split_once(':'))
        .collect::<Option<Vec<_>>>()
}

/// Add to `out` the names of the held members of every object type
/// literal in `ann`, nested ones included.
pub(super) fn held_names(objects: &ObjectTypes, ann: &str, out: &mut HashSet<PropKey>) {
    let mut rest = ann;
    while let Some(at) = rest.find("__inlobj(") {
        let open = at + "__inlobj(".len();
        let Some(close) = matching_close(&rest[open..]) else {
            return;
        };
        for p in split_top(&rest[open..open + close]) {
            if let Some((k, a)) = p.split_once(':') {
                if objects.held(a) {
                    out.insert(PropKey::from(k));
                }
                held_names(objects, a, out);
            }
        }
        rest = &rest[open + close + 1..];
    }
}

/// Respell `any` every object type literal member named in `names`
/// whose annotation is held, in every annotation of `r`.
pub(super) fn retype(r: &mut Ast, objects: &ObjectTypes, names: &HashSet<PropKey>) {
    if names.is_empty() {
        return;
    }
    let rw = Widen { objects, names };
    let mut stmts = std::mem::take(&mut r.stmts);
    rewrite_anns(r, &mut stmts, 0, &rw);
    r.stmts = stmts;
    for args in (r.call_type_args.values_mut()).chain(r.class_parent_type_args.values_mut()) {
        for a in args.iter_mut() {
            sub(a, &rw);
        }
    }
}

struct Widen<'a> {
    objects: &'a ObjectTypes,
    names: &'a HashSet<PropKey>,
}

impl AnnRewrite for Widen<'_> {
    fn skips(&self, _: &[String]) -> bool {
        false
    }

    fn rewrite(&self, ann: &str) -> Option<String> {
        if !ann.contains("__inlobj(") {
            return None;
        }
        let out = self.widen(ann);
        (out != ann).then_some(out)
    }
}

impl Widen<'_> {
    /// `ann` with each object type literal in it widened, nested ones
    /// first.
    fn widen(&self, ann: &str) -> String {
        let mut out = String::with_capacity(ann.len());
        let mut rest = ann;
        while let Some(at) = rest.find("__inlobj(") {
            out.push_str(&rest[..at]);
            let open = at + "__inlobj(".len();
            let Some(close) = matching_close(&rest[open..]) else {
                break;
            };
            let body = &rest[open..open + close];
            let fields: Vec<String> = (split_top(body).into_iter())
                .map(|p| match p.split_once(':') {
                    Some((k, a))
                        if self.names.contains(&PropKey::from(k)) && self.objects.held(a) =>
                    {
                        format!("{k}:any")
                    }
                    Some((k, a)) => format!("{k}:{}", self.widen(a)),
                    None => p.to_string(),
                })
                .collect();
            out.push_str("__inlobj(");
            out.push_str(&fields.join("|"));
            out.push(')');
            rest = &rest[open + close + 1..];
        }
        out.push_str(rest);
        out
    }
}

/// The offset of the `)` closing the group `s` opens into.
fn matching_close(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut prev = 0u8;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'(' | b'<' | b'[' | b'{' => depth += 1,
            b'>' if prev == b'-' || prev == b'=' => {}
            b')' if depth == 0 => return Some(i),
            b')' | b'>' | b']' | b'}' => depth -= 1,
            _ => {}
        }
        prev = b;
    }
    None
}

/// `s` split at its depth-0 `|`.
fn split_top(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut last, mut prev) = (0i32, 0usize, 0u8);
    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'(' | b'<' | b'[' | b'{' => depth += 1,
            b'>' if prev == b'-' || prev == b'=' => {}
            b')' | b'>' | b']' | b'}' => depth -= 1,
            b'|' if depth == 0 => {
                out.push(&s[last..i]);
                last = i + 1;
            }
            _ => {}
        }
        prev = b;
    }
    out.push(&s[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widen(ann: &str, names: &[&str]) -> String {
        let src = "class C { x = 1 }\nclass Box<T> { v: T }\n";
        let tokens = crate::lexer::tokenize(src).expect("lex");
        let mut ast = crate::parser::parse(src, &tokens).expect("parse");
        crate::ast::desugar_classes(&mut ast);
        let objects = ObjectTypes::of(&ast);
        let names: HashSet<PropKey> = names.iter().map(|n| PropKey::from(*n)).collect();
        Widen {
            objects: &objects,
            names: &names,
        }
        .widen(ann)
    }

    #[test]
    fn widens_only_held_members_named() {
        assert_eq!(
            widen("__inlobj(c:C|n:number)", &["c", "n"]),
            "__inlobj(c:any|n:number)"
        );
        assert_eq!(widen("__inlobj(c:C)[]", &["d"]), "__inlobj(c:C)[]");
        assert_eq!(
            widen("__inlobj(f:__cls(number)->(string)|c:C)", &["f"]),
            "__inlobj(f:any|c:C)"
        );
    }

    #[test]
    fn widens_nested_literals() {
        assert_eq!(
            widen("__inlobj(o:__inlobj(c:C)|c:C) | undefined", &["c"]),
            "__inlobj(o:__inlobj(c:any)|c:any) | undefined"
        );
        assert_eq!(widen("Box<__inlobj(c:C)>", &["c"]), "Box<__inlobj(c:any)>");
        assert_eq!(widen("__inlobj(b:Box<number>)", &["b"]), "__inlobj(b:any)");
    }

    #[test]
    fn keeps_members_of_no_declared_object_type() {
        for ann in [
            "T",
            "Map<string|number>",
            "TypeError",
            "Uint8Array",
            "number[]",
        ] {
            let lit = format!("__inlobj(c:{ann})");
            assert_eq!(widen(&lit, &["c"]), lit);
        }
    }

    #[test]
    fn reads_members() {
        assert_eq!(
            members("__inlobj(c:C|f:__fn(number)->(number))"),
            Some(vec![("c", "C"), ("f", "__fn(number)->(number)")])
        );
        assert_eq!(members("C"), None);
    }
}
