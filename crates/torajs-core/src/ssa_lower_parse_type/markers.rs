//! `__struct(...)` / `__cls(...)->(R)` marker decoders of
//! [`super::parse_type`], split out 2026-07-03 (fn-debt decomp).
//! Bodies verbatim; recursion goes back through `super::parse_type`.

use crate::ast::PropKey;
use std::collections::HashMap;

use crate::ssa::{self, Type};
use crate::ssa_lower::intern_fn_sig;

use super::{parse_struct_field_type, parse_type};

/// `__struct(name:T|...)` — parse each field ann (field position:
/// `__nullable(number|boolean)` → Any, RFC 20260710 C4), dedup
/// against the existing struct_layouts pool, intern. `inner` is the
/// annotation between `__struct(` and the trailing `)`.
pub(super) fn parse_struct(
    inner: &str,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
) -> Type {
    let mut fields: Vec<(PropKey, Type)> = Vec::new();
    // One splitter for this encoding, shared with the checker
    // (`check_type_ann::split_top_pipe`). The hand-rolled copies this
    // family used to keep drifted apart twice: chunk 794 taught one of
    // them that the `>` of a return arrow is not a generic closer, and
    // r381 found two more still nesting parens only.
    for part in crate::check_type_ann::split_top_pipe(inner) {
        let (n, t) = part.split_once(':').unwrap_or((part, ""));
        let fty = parse_struct_field_type(
            t,
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
            inst_memo,
        );
        fields.push((PropKey::from(n), fty));
    }
    // Intern by structural equality.
    for (i, ex) in struct_layouts.iter().enumerate() {
        if *ex == fields {
            return Type::Obj(ssa::StructId(i as u32));
        }
    }
    let id = ssa::StructId(struct_layouts.len() as u32);
    struct_layouts.push(fields);
    Type::Obj(id)
}

/// `__cls(P1|...)->(R)` — struct-field closure slot; same parse shape
/// as `__fn` but interns as `Type::Closure` (env-first dispatch).
/// `rest` is the annotation past the `__cls(` prefix; `s` feeds panics.
#[allow(clippy::too_many_arguments)]
pub(super) fn parse_cls(
    s: &str,
    rest: &str,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(PropKey, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
) -> Type {
    let bytes = rest.as_bytes();
    let mut depth: i32 = 1;
    let mut close_idx = None;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    close_idx = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close_idx.unwrap_or_else(|| panic!("ssa-lower: malformed cls-type `{s}`"));
    let params_str = &rest[..close];
    let after = &rest[close + 1..];
    let ret_str = crate::type_ann_fnsig::ret_of_tail(after)
        .unwrap_or_else(|| panic!("ssa-lower: malformed cls-type ret `{s}`"));
    // RFC 20260708-variadic — a `__rest(E[])` segment (rest-tail
    // fn type) gets no static param slot: the fixed prefix is the
    // interned sig, and calls through the slot dispatch via the
    // boxed dual entry (`closure_call_variadic`) which never reads
    // the static params.
    let mut params: Vec<Type> = Vec::new();
    for seg in crate::check_type_ann::split_top_pipe(params_str) {
        if !seg.starts_with("__rest(") {
            params.push(parse_type(
                Some(seg),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
                inst_memo,
            ));
        }
    }
    let ret = parse_type(
        Some(ret_str),
        aliases,
        arr_layouts,
        fn_sigs,
        generic_struct_decls,
        struct_layouts,
        inst_memo,
    );
    let id = intern_fn_sig(fn_sigs, params, ret);
    return Type::Closure(id);
}
