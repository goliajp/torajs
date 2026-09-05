//! The `Map` / `Set` VALUE-SLOT use shape — `m.set(key, k)` on a
//! `Map<_, any>`, `s.add(k)` on a `Set<any>`.
//!
//! Sibling of [`super::fnexpr_this_arrpush`], and the same proof with
//! one more step to reach the annotation. An `any[]` binding spells
//! its element type in the binding's own annotation; a keyed
//! collection spells it in a GENERIC ARGUMENT, which arrives by one
//! of two routes — on the binding (`const m: Map<string, any>`) or on
//! the constructor (`const m = new Map<string, any>()`). Both are read
//! here, because both are how the type is actually written.
//!
//! Once the slot is known to be `Any`, the rest is the push proof
//! verbatim: reading the value back yields an AnyValue however it is
//! spelled, and every any-lane call path shifts argv on
//! `FLAG_CLOSURE_RECV_FIRST`.
//!
//! Only the VALUE positions are admitted — `set`'s SECOND argument
//! and `add`'s only one. `set`'s first argument is a KEY, and the key
//! type is a different generic argument this proof says nothing
//! about; admitting arguments wholesale is the mistake
//! [`super::fnexpr_this_arrpush`] avoided with `splice`.
//!
//! The receiver must be a bare Ident naming such a binding, for the
//! push shape's reason: a member/index receiver would need the
//! field's type, which this pass cannot see.

use super::fnexpr_this_names::{peel_as, slot_value_idents};
use super::{Expr, ExprId, Stmt};

/// `(method name, index of the VALUE argument)`.
const VALUE_STORES: &[(&str, usize)] = &[("set", 1), ("add", 0)];

/// Split a flattened generic argument list at TOP-LEVEL separators.
///
/// The parser flattens `Map<string, any>` to the string
/// `Map<string|any>` — `|`, not `,`, mirroring the `__fn(P|Q)`
/// encoding (`parse_type_ann_inner`'s generic arm). Reading this
/// wrong is what made the annotated spelling refuse while
/// `Set<any>` — which has no separator to get wrong — worked.
///
/// Depth matters: `Map<string, Map<string, any>>` flattens to
/// `Map<string|Map<string|any>>` and must answer
/// `["string", "Map<string|any>"]`. A depth-blind "does it end in
/// `any>`" test would admit that binding, whose value slot is a Map.
fn split_top_level(args: &str) -> Vec<&str> {
    let (mut out, mut depth, mut start) = (Vec::new(), 0i32, 0usize);
    for (i, c) in args.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            '|' if depth == 0 => {
                out.push(args[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(args[start..].trim());
    out
}

/// True when `Map<…>` / `Set<…>` written as `ann` stores `any` in its
/// VALUE slot: the LAST generic argument for both spellings (a Map's
/// value, a Set's element), and only at the documented arity.
///
/// The arity test is what keeps a UNION type argument out. The
/// flattening uses `|` for both separators, so `Map<string|number,
/// any>` and `Map<string, number|any>` both arrive as three parts and
/// are indistinguishable here — so neither is admitted, and the
/// binding keeps its loud reject. Over-removal, the posture every
/// census in this family holds.
fn ann_value_is_any(ann: &str) -> bool {
    let ann = ann.trim();
    for (head, arity) in [("Map<", 2usize), ("Set<", 1)] {
        if let Some(rest) = ann.strip_prefix(head)
            && let Some(inner) = rest.strip_suffix('>')
        {
            let parts = split_top_level(inner);
            return parts.len() == arity && parts[arity - 1] == "any";
        }
    }
    false
}

/// The same question asked of a constructor's already-split type
/// arguments (`new Map<string, any>()`).
fn type_args_value_is_any(class_name: &str, type_args: &[String]) -> bool {
    match (class_name, type_args.len()) {
        ("Map", 2) | ("Set", 1) => type_args[type_args.len() - 1].trim() == "any",
        _ => false,
    }
}

/// Binding names whose Map/Set value slot is `any` at EVERY
/// declaration in the program.
///
/// Same over-removal posture as
/// [`super::fnexpr_this_recvs::collect_any_binding_names`]: a
/// same-name declaration anywhere that does not spell an `any` value
/// slot removes the name, which can only keep a promotion loud, never
/// mis-admit a typed slot. Walks the shared nested-list spine, so a
/// declaration inside a block / try / with is seen.
fn any_valued_names(stmts: &[Stmt], exprs: &[Expr]) -> std::collections::HashSet<String> {
    fn walk(
        stmts: &[Stmt],
        exprs: &[Expr],
        ok: &mut std::collections::HashSet<String>,
        other: &mut std::collections::HashSet<String>,
    ) {
        for s in stmts {
            if let Stmt::LetDecl {
                name,
                type_ann,
                init,
                ..
            } = s
            {
                // The annotation wins when present: it is what types
                // the slot, and a constructor's arguments have already
                // been checked against it.
                let admitted = match type_ann.as_deref() {
                    Some(ann) => ann_value_is_any(ann),
                    None => matches!(
                        &exprs[peel_as(exprs, *init).0 as usize],
                        Expr::New { class_name, type_args, .. }
                            if type_args_value_is_any(class_name, type_args)
                    ),
                };
                if admitted {
                    ok.insert(name.clone());
                } else {
                    other.insert(name.clone());
                }
            }
            super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| {
                walk(inner, exprs, ok, other)
            });
        }
    }
    let mut ok = std::collections::HashSet::new();
    let mut other = std::collections::HashSet::new();
    walk(stmts, exprs, &mut ok, &mut other);
    ok.retain(|n| !other.contains(n));
    ok
}

/// 591-06 — the bare-Ident VALUE argument of `set` / `add` on a
/// collection whose value slot is spelled `any`.
pub(super) fn any_valued_store_idents(
    stmts: &[Stmt],
    exprs: &[Expr],
) -> std::collections::HashSet<ExprId> {
    let names = any_valued_names(stmts, exprs);
    let mut out = std::collections::HashSet::new();
    if names.is_empty() {
        return out;
    }
    for e in exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = &exprs[callee.0 as usize] else {
            continue;
        };
        let Some((_, at)) = VALUE_STORES.iter().find(|(m, _)| *m == name.as_str()) else {
            continue;
        };
        // Exactly the documented arity: a `set` with one argument or
        // three is not the call this proof read.
        if args.len() != at + 1 {
            continue;
        }
        let obj = peel_as(exprs, *obj);
        let Expr::Ident(recv) = &exprs[obj.0 as usize] else {
            continue;
        };
        if !names.contains(recv.as_str()) {
            continue;
        }
        slot_value_idents(exprs, args[*at], &mut out);
    }
    out
}
