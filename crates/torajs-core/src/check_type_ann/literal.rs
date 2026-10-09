//! The annotation spelling of a literal type (TS §3.2.10): `"a"`,
//! `1`, `true` as `__lits(<hex bytes>)`, `__litn(<f64 bits>)`,
//! `__litb(true)`.
//!
//! The payload is spelled with word bytes only, so no decoder that
//! splits annotations on `|`, `,`, `:` or parentheses can cut a string
//! literal apart, and a literal is one annotation however its text
//! reads. Only a top-level type declaration's own record keeps the
//! markers (`Ast::literal_type_decls`), and only the checker reads it,
//! resolving each marker to [`Type::Literal`]; every annotation in the
//! AST spells a literal type as its base type, as it always has.

use crate::check::{Lit, Type};

const STR: &str = "__lits(";
const NUM: &str = "__litn(";
const BOOL: &str = "__litb(";

/// The annotation spelling of `lit`.
pub(crate) fn literal_marker(lit: &Lit) -> String {
    match lit {
        Lit::Str(s) => {
            let hex: String = s.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
            format!("{STR}{hex})")
        }
        Lit::Num(n) => format!("{NUM}{:016x})", n.to_bits()),
        Lit::Bool(b) => format!("{BOOL}{b})"),
    }
}

/// The literal `ann` spells, when it is exactly one literal marker.
pub(crate) fn parse_literal_marker(ann: &str) -> Option<Lit> {
    if let Some(hex) = payload(ann, STR) {
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
            .collect::<Option<Vec<u8>>>()?;
        return torajs_wtf8::Wtf8Buf::from_bytes(bytes).ok().map(Lit::Str);
    }
    if let Some(bits) = payload(ann, NUM) {
        return u64::from_str_radix(bits, 16)
            .ok()
            .map(|b| Lit::Num(f64::from_bits(b)));
    }
    match payload(ann, BOOL)? {
        "true" => Some(Lit::Bool(true)),
        "false" => Some(Lit::Bool(false)),
        _ => None,
    }
}

/// `ann` with every literal marker in it replaced by its base type's
/// keyword; `None` when it holds none.
fn erase_literal_markers(ann: &str) -> Option<String> {
    if !ann.contains("__lit") {
        return None;
    }
    let mut out = String::with_capacity(ann.len());
    let mut rest = ann;
    while let Some(at) = rest.find("__lit") {
        let tail = &rest[at..];
        let base = [(STR, "string"), (NUM, "number"), (BOOL, "boolean")]
            .into_iter()
            .find(|(m, _)| tail.starts_with(m));
        let Some((_, base)) = base else {
            out.push_str(&rest[..at + 5]);
            rest = &rest[at + 5..];
            continue;
        };
        let close = tail.find(')')?;
        out.push_str(&rest[..at]);
        out.push_str(base);
        rest = &tail[close + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// `ann` as the program's other readers see it: each literal type
/// widened to its base, and every union that touched rebuilt the way
/// the parser builds one — the spelling a literal type has always had.
pub(crate) fn widen_literal_types(ann: &str) -> String {
    match erase_literal_markers(ann) {
        Some(erased) => crate::ast::canonical_unions(&erased),
        None => ann.to_string(),
    }
}

/// The fields `ast` recorded, with their literal types, for the
/// top-level type declaration `name` whose AST fields are `fields`
/// (`Ast::literal_type_decls`). `None` when there is no such record or
/// it no longer widens to `fields` (a later pass rewrote them).
pub(crate) fn precise_decl_fields<'a>(
    ast: &'a crate::ast::Ast,
    name: &str,
    fields: &[(crate::ast::PropKey, String)],
) -> Option<&'a [(crate::ast::PropKey, String)]> {
    let precise = ast.literal_type_decls.get(name)?.as_deref()?;
    let same = precise.len() == fields.len()
        && precise
            .iter()
            .zip(fields)
            .all(|((pn, pt), (n, t))| pn == n && widen_literal_types(pt) == *t);
    same.then_some(precise)
}

fn payload<'a>(ann: &'a str, marker: &str) -> Option<&'a str> {
    let inner = ann.strip_prefix(marker)?.strip_suffix(')')?;
    inner
        .bytes()
        .all(|b| b.is_ascii_alphanumeric())
        .then_some(inner)
}

/// The type a literal marker denotes.
pub(super) fn resolve_literal_marker(ann: &str) -> Option<Type> {
    parse_literal_marker(ann).map(Type::Literal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_round_trip() {
        for lit in [
            Lit::Str(torajs_wtf8::Wtf8Buf::from("a|b(c)")),
            Lit::Num(1.5),
            Lit::Num(0.0),
            Lit::Bool(true),
        ] {
            assert_eq!(parse_literal_marker(&literal_marker(&lit)), Some(lit));
        }
        assert_eq!(parse_literal_marker("__lits(6)"), None);
        assert_eq!(parse_literal_marker("string"), None);
    }

    #[test]
    fn erase_spells_the_base() {
        let a = literal_marker(&Lit::Str(torajs_wtf8::Wtf8Buf::from("a")));
        let one = literal_marker(&Lit::Num(1.0));
        let ann = format!("__inlobj(kind:{a}|n:{one})[]");
        assert_eq!(
            erase_literal_markers(&ann).as_deref(),
            Some("__inlobj(kind:string|n:number)[]")
        );
        assert_eq!(
            erase_literal_markers("__litb(false)").as_deref(),
            Some("boolean")
        );
        assert_eq!(erase_literal_markers("__inlobj(a:number)"), None);
    }
}
