//! Internal marker-annotation decoders (`__inlobj(...)` /
//! `__fn(...)->(R)` / `__cls(...)->(R)`) plus the shared depth-aware
//! string scanners, split out of `resolve_type_ann_inner`
//! (2026-07-03, fn-debt decomp). Bodies verbatim; the four inline
//! copies of the depth-0 `|` splitter and the two copies of the
//! close-paren scanner collapse into `split_top_pipe` /
//! `find_close_paren`.

use crate::ast::PropKey;
use std::collections::{HashMap, HashSet};

use crate::check::{GenericAliasMap, Type};

/// The annotation inside a `__nullable(T)` wrapper, or the
/// annotation itself when it carries none.
///
/// For a lookup keyed by the annotation's own text — "is this the
/// name of a known class?" — the wrapper is noise: which class a
/// binding nominally IS does not change because it may also hold
/// undefined, and the type carries that second fact already. Two
/// such lookups (the `let` position and the parameter position) each
/// answered None on a wrapped name, and both of their consumers read
/// None as "no nominal info, allow it" — so a `readonly` write and a
/// `private` read from outside the class stopped being refused,
/// quietly. `f(c?: C)` is the shape that reaches it without anyone
/// writing a union: an optional parameter IS `__nullable(C)`.
pub(crate) fn strip_nullable(ann: &str) -> &str {
    ann.strip_prefix("__nullable(")
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(ann)
}

/// Split `s` at every depth-0 `|`, with both `(`/`)` and `<`/`>`
/// nesting. The `>` of a fn-type return arrow
/// (`Pair<__fn()->(number)|string>`) is not a generic closer: counting
/// it dropped the depth below zero and the depth-0 `|` between the
/// type args went unseen (`-` only precedes `>` in the return-arrow
/// spelling).
///
/// r381 — angle nesting used to be opt-in, and the `__inlobj(` /
/// `__fn(` decoders opted out. That was never a real distinction:
/// a `|` inside `<..>` separates GENERIC ARGUMENTS in every one of
/// these spellings, never the marker's own fields or params. Opting
/// out cut `__fn(Map<string|number>)->(void)` into `Map<string` and
/// `number>`, so any multi-argument generic in a fn-type or inline
/// object type was a loud "unknown type" (single-argument ones
/// carry no `|` and survived, which is why it stayed hidden).
pub(crate) fn split_top_pipe(s: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = Vec::new();
    let mut depth: i32 = 0;
    let mut last = 0usize;
    let mut prev: u8 = 0;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        match b {
            b'<' => depth += 1,
            b'>' if prev == b'-' => {}
            b'>' => depth -= 1,
            b'(' => depth += 1,
            b')' => depth -= 1,
            b'|' if depth == 0 => {
                parts.push(&s[last..i]);
                last = i + 1;
            }
            _ => {}
        }
        prev = b;
    }
    if !s.is_empty() {
        parts.push(&s[last..]);
    }
    parts
}

/// Index of the `)` closing the marker's already-consumed `(`
/// (depth starts at 1), or None when unbalanced.
pub(super) fn find_close_paren(rest: &str) -> Option<usize> {
    let mut depth: i32 = 1;
    for (i, &b) in rest.as_bytes().iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// V3-18 P2.4.c.2 — inline obj type `__inlobj(name1:T1|name2:T2|...)`.
/// Each field's type recurses through the resolver so nested inline
/// obj / generic types work. `rest` is the annotation past the
/// `__inlobj(` prefix.
pub(super) fn resolve_inlobj(
    rest: &str,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
    in_flight: &mut HashSet<String>,
) -> Option<Type> {
    let close = find_close_paren(rest)?;
    let fields_str = &rest[..close];
    let fields_split = split_top_pipe(fields_str);
    let mut fields_out: Vec<(PropKey, Type)> = Vec::with_capacity(fields_split.len());
    for f in fields_split {
        let colon = f.find(':')?;
        let fname = f[..colon].to_string();
        let fty_str = &f[colon + 1..];
        let fty = super::resolve_type_ann_inner(
            fty_str,
            aliases,
            type_params,
            generic_aliases,
            in_flight,
        )?;
        fields_out.push((PropKey::from(fname), fty));
    }
    Some(Type::Struct(fields_out))
}

/// `__fn(P1|P2|...)->(R)` (user-source fn type) and its
/// `tag_struct_field_closure_types`-tagged sibling `__cls(P1|...)->(R)`
/// (struct-field closure slot) share the same parse shape and both
/// resolve to `Type::Function(params, ret)` at the typecheck layer.
/// SSA `parse_type` is what actually distinguishes them: `__fn` →
/// `Type::FnSig` (direct dispatch), `__cls` → `Type::Closure`
/// (env-first dispatch). `rest` is the annotation past the prefix.
pub(super) fn resolve_fn_cls(
    rest: &str,
    aliases: &HashMap<String, Type>,
    type_params: &[String],
    generic_aliases: &GenericAliasMap,
    in_flight: &mut HashSet<String>,
) -> Option<Type> {
    let close = find_close_paren(rest)?;
    let params_str = &rest[..close];
    let after = &rest[close + 1..];
    let ret_str = crate::type_ann_fnsig::ret_of_tail(after)?;
    let params = split_top_pipe(params_str);
    let mut param_tys = Vec::with_capacity(params.len());
    for p in params {
        param_tys.push(super::resolve_type_ann_inner(
            p,
            aliases,
            type_params,
            generic_aliases,
            in_flight,
        )?);
    }
    let ret_ty =
        super::resolve_type_ann_inner(ret_str, aliases, type_params, generic_aliases, in_flight)?;
    Some(Type::Function(param_tys, Box::new(ret_ty)))
}
