//! 628-05 — the own-key ORDER of runtime-computed INSTANCE fields.
//!
//! `class C { foo = 1; [x] = 42; bar = 2 }` — the layout has rows for
//! `foo` and `bar`, and the ctor prefix writes `this[__ccmk_C_0] = 42`
//! into the +24 expando dict in declaration order. But every own-key
//! walk lists layout rows first and expandos after, so `Object.keys`
//! answered `foo,bar,b` where §10.1.11.1 (definition order) says
//! `foo,b,bar`. The dict knows its insertion order; what nobody
//! recorded is where the computed field sits AMONG THE ROWS.
//!
//! The parser keeps that fact (`class_computed_instance_fields`: the
//! own named field declared just before it), and this pass hands it
//! to the runtime at the class-decl position, right after the key
//! global is let-bound:
//!
//! ```text
//! __torajs_class_computed_field_register("C", "foo", __ccmk_C_0)
//! ```
//!
//! The runtime resolves `"foo"` to its layout row once and the
//! enumeration walks (keys / values / entries / for-in / JSON) emit
//! the expando entry right behind that row. A field that LEADS its
//! class is anchored behind the last inherited field (`""` = before
//! every row when there is none). Inherited computed fields register
//! again under the subclass's tag — a `D extends C` instance carries
//! C's fields and C's anchors alike, root-most ancestor first, which
//! is also the order their ctor prefixes run.

use std::collections::HashMap;

use super::*;

/// Emit the register call for every computed instance field the
/// class `cname` carries — its ancestors' (root first) then its own.
/// `full_fields` is pass 3's flattened per-class field list (parent
/// fields first), which is where a LEADING computed field finds the
/// inherited row it follows.
pub(super) fn emit_instance_field_registers(
    ast: &mut Ast,
    cname: &str,
    full_fields: &HashMap<String, Vec<(PropKey, String)>>,
    patch: &mut Vec<Stmt>,
) {
    // Root-most ancestor first: the chain from `cname` upward,
    // reversed.
    let mut chain: Vec<String> = vec![cname.to_string()];
    let mut cur = cname.to_string();
    while let Some(Some(p)) = ast.class_parents.get(&cur) {
        chain.push(p.clone());
        cur = p.clone();
    }
    chain.reverse();
    let mut entries: Vec<(String, String, Option<PropKey>)> = Vec::new();
    for owner in &chain {
        entries.extend(
            ast.class_computed_instance_fields
                .iter()
                .filter(|(c, _, _)| c == owner)
                .cloned(),
        );
    }
    for (owner, sentinel, after) in entries {
        let Some(n) = sentinel
            .strip_prefix("__ccm_")
            .map(|r| r.trim_end_matches('_'))
        else {
            continue;
        };
        // A leading computed field anchors behind the last field its
        // OWNER inherits — the owner's parent's flattened list ends
        // with it.
        let anchor = after.or_else(|| inherited_last_of(ast, full_fields, &owner));
        let cname_e = ast.add_expr(Expr::String(cname.to_string().into()));
        let after_e = ast.add_expr(Expr::String(
            anchor.map(|k| k.into_wtf8buf()).unwrap_or_default(),
        ));
        let key_e = ast.add_expr(Expr::Ident(format!("__ccmk_{owner}_{n}")));
        let callee = ast.add_expr(Expr::Ident(
            "__torajs_class_computed_field_register".to_string(),
        ));
        let call = ast.add_expr(Expr::Call {
            callee,
            args: vec![cname_e, after_e, key_e],
        });
        patch.push(Stmt::Expr(call));
    }
}

/// The last named instance field `class` inherits — the row a
/// leading computed field of `class` follows. The parent's flattened
/// list already carries every ancestor's fields, so one hop answers;
/// `None` when nothing is inherited.
fn inherited_last_of(
    ast: &Ast,
    full_fields: &HashMap<String, Vec<(PropKey, String)>>,
    class: &str,
) -> Option<PropKey> {
    let Some(Some(parent)) = ast.class_parents.get(class) else {
        return None;
    };
    full_fields
        .get(parent)
        .and_then(|f| f.last())
        .map(|(k, _)| k.clone())
}

/// Does `class` — or any ancestor — declare a runtime-computed
/// instance field? Such a field is an expando the layout cannot
/// enumerate, so a typed own-value walk over the layout (the
/// `Object.values` unfold) would drop it: the checker answers
/// `Array<Any>` for the call and the lowering routes the runtime
/// own-walk, both through this one predicate (629-03).
pub(crate) fn class_has_computed_instance_fields(ast: &Ast, class: &str) -> bool {
    let mut cur = class.to_string();
    loop {
        if ast
            .class_computed_instance_fields
            .iter()
            .any(|(c, _, _)| *c == cur)
        {
            return true;
        }
        match ast.class_parents.get(&cur) {
            Some(Some(p)) => cur = p.clone(),
            _ => return false,
        }
    }
}
