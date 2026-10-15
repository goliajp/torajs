//! The seed each declared field of a class gets in its factory's
//! object literal (`__new_C`), before the field initializers and the
//! constructor run. Moved out of [`super`].

use crate::ast::{Ast, Expr, ExprId, PropKey, Stmt, mangle_key};

/// Build a default-initializer Expr for a type annotation string. Used by
/// `desugar_classes` to seed the factory's object-literal at the top of
/// `__new_C`. The constructor (if any) is responsible for overwriting
/// these defaults with caller-provided values; the defaults exist so the
/// object is well-typed even on fields a buggy constructor forgets to
/// touch.
/// Recursive default-initializer for a class field. Knows how to:
///   - hoist `T[]` into a typed prelude let returning the bound ident
///   - expand a class- or alias-typed field into an ObjectLit of
///     recursively-defaulted children (looked up in `class_layouts`
///     and `alias_layouts`)
///   - fall back to `default_init_for_type` for primitives / typevars
///
/// `seen` guards against direct cycles (a class transitively
/// containing itself by name); a hit panics rather than spinning.
#[allow(clippy::too_many_arguments)]
pub(crate) fn default_init_for_field(
    ast: &mut Ast,
    fty: &str,
    class_layouts: &std::collections::HashMap<String, Vec<(PropKey, String)>>,
    alias_layouts: &std::collections::HashMap<String, Vec<(PropKey, String)>>,
    prelude: &mut Vec<Stmt>,
    parent_cname: &str,
    parent_fname: &PropKey,
    seen: &mut std::collections::HashSet<String>,
) -> ExprId {
    if fty.ends_with("[]") {
        let local = format!("__def_arr_{parent_cname}_{}", mangle_key(parent_fname));
        let arr_lit = ast.add_expr(Expr::Array(Vec::new()));
        prelude.push(Stmt::LetDecl {
            mutable: false,
            name: local.clone(),
            type_ann: Some(fty.to_string()),
            init: arr_lit,
            is_var: false,
        });
        return ast.add_expr(Expr::Ident(local));
    }
    let sub_fields = class_layouts.get(fty).or_else(|| alias_layouts.get(fty));
    if let Some(sub_fields) = sub_fields {
        // V3-18 wedge — bare type alias `type X = T` is encoded
        // as a single field named "__alias__" carrying the
        // underlying ann. Recurse using the underlying ann
        // instead of treating the alias as a struct shape — the
        // alias name resolves to T at the type level, never a
        // struct with one __alias__ field.
        if sub_fields.len() == 1 && sub_fields[0].0 == "__alias__" {
            let underlying = sub_fields[0].1.clone();
            return default_init_for_field(
                ast,
                &underlying,
                class_layouts,
                alias_layouts,
                prelude,
                parent_cname,
                parent_fname,
                seen,
            );
        }
        if !seen.insert(fty.to_string()) {
            panic!(
                "default_init_for_field: cyclic struct/class layout via `{fty}` \
                 (parent `{parent_cname}.{}`)",
                parent_fname.lossy()
            );
        }
        let sub_fields = sub_fields.clone();
        let mut sub_pairs: Vec<(PropKey, ExprId)> = Vec::with_capacity(sub_fields.len());
        for (sfname, sfty) in &sub_fields {
            let sub_local = format!(
                "{parent_cname}_{}_{}",
                mangle_key(parent_fname),
                mangle_key(sfname)
            );
            let sub_id = default_init_for_field(
                ast,
                sfty,
                class_layouts,
                alias_layouts,
                prelude,
                &sub_local,
                sfname,
                seen,
            );
            sub_pairs.push((sfname.clone(), sub_id));
        }
        seen.remove(fty);
        // The cell this literal mints belongs to `fty`, not to the
        // factory that will lower it. Record the ExprId so the
        // cell-construction tail can ask which class it is building
        // instead of which factory it is inside — see the doc on
        // `Ast::field_default_objlits`.
        let lit = ast.add_expr(Expr::ObjectLit { fields: sub_pairs });
        ast.field_default_objlits.insert(lit, fty.to_string());
        return lit;
    }
    let init_expr = default_init_for_type(fty);
    ast.add_expr(init_expr)
}

pub(crate) fn default_init_for_type(ann: &str) -> Expr {
    #[rustfmt::skip]
    fn ctor(name: &str) -> Expr { Expr::New { class_name: name.into(), args: vec![], type_args: vec![] } }
    match ann {
        "number" => Expr::Number(0.0),
        "string" => Expr::String(String::new().into()),
        "boolean" => Expr::Bool(false),
        // JS's zero value for an untyped slot IS undefined — the old
        // Number(0.0) catch-all leaked `0` where bun answers
        // `undefined` (exhausted generator step values, async
        // tail-safety defaults, any-field zero-init).
        // Same rationale one line down for the type that says there is
        // no value at all. `async function f(): Promise<void> {}` falls
        // off its end, and the tail-safety return took the catch-all
        // `0` — so a function promising nothing was caught returning a
        // number ("expects Promise(Void), got Promise(Number)") and
        // could not be written at all.
        "any" | "void" | "undefined" => Expr::Ident("undefined".into()),
        // T[] / __nullable(T) — typed zero / null (M5.2 inheritance follow-up).
        _ if ann.ends_with("[]") => Expr::Array(Vec::new()),
        // V3-05 — `T | null` field default null (parser flat `__nullable(T)`).
        _ if ann.starts_with("__nullable(") && ann.ends_with(')') => Expr::Null,
        // class W { m: Map<K,V>; } default → `new Map()` so SSA-lower intercepts
        // to __torajs_map_create; same for Set/WeakMap/WeakSet (bare or generic).
        _ if ann == "Map" || ann.starts_with("Map<") => ctor("Map"),
        _ if ann == "Set" || ann.starts_with("Set<") => ctor("Set"),
        _ if ann == "WeakMap" || ann.starts_with("WeakMap<") => ctor("WeakMap"),
        _ if ann == "WeakSet" || ann.starts_with("WeakSet<") => ctor("WeakSet"),
        // TypeVar (short all-uppercase T/U/K/V…) — monomorphizer-resolved marker.
        _ if is_likely_typevar(ann) => Expr::Ident(format!("__tvdefault__{ann}")),
        // Every type: the annotation's own undefined sentinel,
        // asked for by the marker [`crate::ast::UNDEF_SLOT_MARKER`]
        // that an async body's fall-through tail already uses. It
        // types as the annotation rather than as `Type::Undefined`,
        // which is the whole requirement here — the seed literal has
        // to agree with the class it is declared as.
        //
        // The catch-all used to be `Number(0.0)`, and a class simply
        // could not declare a field of any type outside the list
        // above: the factory died on its own synthesized `__this`
        // ("declared ClassRef(\"C\"), init has Struct([(\"d\", Number)])"),
        // taking every other field of that class down with it. A
        // function type, `Date`, `RegExp` and `bigint` were all
        // unusable as field types, however the field was initialized
        // — including from the constructor, since the seed is built
        // before the constructor runs.
        //
        // Fabricating a value of the type instead (an epoch `Date`,
        // an empty `RegExp`, `0n`) would answer a real-looking value
        // where the language answers `undefined`, and would make
        // every construction pay for it.
        //
        // A function type was held back on the wrong-typed zero one
        // release longer, because seeding it here SIGBUSed: the slot
        // took the Str-family oddball (fn types are Copy, so RFC
        // 20260710 C2a hands them that one) while the field it lands
        // in is refcounted, and the rc write hit the immortal cell's
        // read-only page. The repr is picked by the slot's own type
        // now — see [`crate::ssa_lower_ident`] — so a function type
        // seeds like every other type.
        _ => Expr::Ident(format!("{}{ann}", crate::ast::UNDEF_SLOT_MARKER)),
    }
}

fn is_likely_typevar(s: &str) -> bool {
    s.len() <= 2 && !s.is_empty() && s.chars().all(|c| c.is_ascii_uppercase())
}
