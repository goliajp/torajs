//! The canonical ANNOTATION an un-annotated top-level initializer
//! registers under — `__fn(` / `__cls(` for a closure and the names
//! that alias one, `__inlobj(` for an object literal, the class name
//! for a `new C(…)`. Split out of `shape.rs` (r602): that file's
//! subject is which primitive SLOT TYPE an initializer has, and these
//! answer a different question — what STRING both the checker's
//! pass_2 and the lowerer's K.3b resolve, so the two slots cannot
//! drift. Growing the two families in one file had taken it to 34
//! lines under the limit.

use crate::ast::{Ast, Expr, ExprId, Stmt};

use super::shape::{GlobalSlotShape, infer_slot_shape};

/// RFC 20260709-closure-global chunk 2 — the canonical `__fn(P|..)->(R)`
/// spelling for a lifted closure's FnDecl, read off its param /
/// return anns. By the time the checker / lowerer consult this,
/// `preinfer_closure_sigs` has backfilled missing param anns with
/// `any` and inferred the return ann (or left `None` for a body
/// without value returns, which spells `void` — same mapping that
/// pass uses when publishing sigs). `None` when the decl is absent or
/// a param ann is still missing (a non-`__closure_*` decl this pass
/// never touched).
pub fn lifted_closure_fn_canon(ast: &Ast, fn_name: &str) -> Option<String> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } if name == fn_name => {
            let mut anns: Vec<String> = Vec::with_capacity(params.len());
            // a promoted receiver stays out, as `closure_value_ty` keeps
            // it out of a local's type: the call lane adds the slot back
            let recv = ast.fnexpr_recv_fns.contains(fn_name);
            let skip = |p: &&crate::ast::Param| p.name == "__env" || (recv && p.name == "__this");
            for p in params.iter().filter(|p| !skip(p)) {
                // a rest parameter's own ann is the array spelling
                // (`number[]`) and the flag beside it makes it a rest:
                // spelled alone, `(...xs: number[]) => number` reads as
                // one array parameter. `__rest(` is how a rest reaches
                // a type ann everywhere else (`parser::type_ann_fn`).
                let a = p.type_ann.clone()?;
                anns.push(if p.is_rest { format!("__rest({a})") } else { a });
            }
            let ret = match return_type {
                Some(rt) => rt.clone(),
                None => "void".to_string(),
            };
            Some(crate::type_ann_fnsig::fn_type_ann(
                "__fn",
                &anns.join("|"),
                &ret,
            ))
        }
        _ => None,
    })
}

/// Rotation 592 — the same canonical spelling for a binding that
/// ALIASES a lifted-closure binding (`const c = k`). The alias
/// carries the identical value, so it must register under the
/// identical string: both the checker's pass_2 and the lowerer's
/// K.3b resolve what this answers, exactly as they do for the
/// closure init itself, and the two slots cannot drift.
///
/// Without it the alias registered as nothing, so a named-fn body
/// reading it answered "unknown identifier" — an ordinary
/// `const c = k; function give() { return c }` was a hard reject,
/// while the `any`-annotated spelling of the same program worked.
///
/// Follows a CHAIN of aliases (`const gc = gb; const gb = ga`),
/// making the same argument at every hop: the name must be declared
/// exactly once at top level, as a `let` / `const`. A second
/// declaration means the name at the alias site is not necessarily
/// the one found here; `var` is excluded for the reason every
/// sibling arm excludes it — its hoisted home is not this slot. A
/// name reached twice is a cycle and answers nothing.
pub fn closure_alias_fn_canon(ast: &Ast, init: ExprId) -> Option<String> {
    let Expr::Ident(src) = ast.get_expr(init) else {
        return None;
    };
    let mut name: &str = src;
    let mut seen: Vec<&str> = Vec::new();
    loop {
        if seen.contains(&name) {
            return None;
        }
        seen.push(name);
        let (init, ann) = unique_toplevel_binding(ast, name)?;
        // The source's own ANNOTATION is the string it registered
        // under, and it is not always its closure's decl canon: a
        // variadic FACE over a non-variadic closure (`const t:
        // (...xs: number[]) => R = (a: number) => …`) registers
        // rest-tail while the lifted decl spells one fixed
        // parameter. The alias holds that binding's value through
        // that binding's face, so the face is what it answers —
        // otherwise the two slots disagree about arity and the
        // alias's call takes a static dispatch the value cannot
        // serve. A non-fn-like ann (a named type alias) is not a
        // spelling this resolves, and falls through to the init.
        if let Some(a) = ann.filter(|a| crate::ast::is_fn_like_ann(a)) {
            return Some(a.to_string());
        }
        match ast.get_expr(init) {
            Expr::Closure { fn_name, .. } => return lifted_closure_fn_canon(ast, fn_name),
            Expr::Ident(next) => name = next,
            _ => return None,
        }
    }
}

/// The one top-level `let` / `const` declaring `name`, with its init
/// and annotation — `None` when there is no such declaration or more
/// than one. `var` is not a candidate (its hoisted home is not this
/// slot), and a `var` of the same name does not make the `const`
/// ambiguous either: a program with both is not one this resolves.
pub fn unique_toplevel_binding<'a>(ast: &'a Ast, name: &str) -> Option<(ExprId, Option<&'a str>)> {
    let mut found: Option<(ExprId, Option<&'a str>)> = None;
    for s in crate::ast::toplevel_stmts_flat(ast) {
        let Stmt::LetDecl {
            name: decl,
            init,
            type_ann,
            is_var: false,
            ..
        } = s
        else {
            continue;
        };
        if decl != name {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some((*init, type_ann.as_deref()));
    }
    found
}

/// RFC 20260725 follow-up (un-annotated struct global) — the
/// canonical `__inlobj(f:T|...)` spelling for an all-literal-field
/// ObjectLit init (`let s = { a: 1, b: "x" }`). Both the checker's
/// pass_2 registration and the lowerer's K.3b slot inference resolve
/// the SAME string through their existing annotation pipelines, so
/// the two slots cannot drift (and the interned layout unifies with
/// an equivalent written annotation). `None` — keeping the binding
/// main-local — for any field that isn't a Number/String/Bool
/// literal or a lifted arrow, a computed-key / symbol / spread /
/// dunder sentinel name, a non-identifier name (the spelling's `:` /
/// `|` separators must stay unambiguous), or an empty literal (`{}`
/// is the dynobj-family idiom, left to the degrade passes).
///
/// A method-valued field carries its lifted fn's `__fn(...)->T`
/// spelling — the one a written `{ f: (x: number) => number }`
/// annotation resolves through too, and the `|` between its params
/// sits inside parens where `split_top_pipe`'s depth counter already
/// leaves it alone. Without this arm, ONE closure field kept the
/// whole binding main-local, so a named fn reading any field of it
/// got "unknown identifier" — which is what made `const thenable =
/// { then(res) {…} }` unreachable from every function in the file.
/// Variadic sigs stay out for the same reason the sibling arms keep
/// them out: their boxed-dual routing is a fn-local table.
pub fn objlit_literal_inlobj_ann(ast: &Ast, init: ExprId) -> Option<String> {
    let Expr::ObjectLit { fields } = ast.get_expr(init) else {
        return None;
    };
    if fields.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::with_capacity(fields.len());
    for (fname, val) in fields {
        let fname = fname.as_str()?;
        let mut chars = fname.chars();
        let head_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$');
        if !head_ok
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
            || fname.starts_with("__")
        {
            return None;
        }
        let ann: String = match ast.get_expr(*val) {
            Expr::Number(n) => {
                if n.fract() != 0.0 || n.abs() >= 9.223372036854776e18 {
                    "f64".to_string()
                } else {
                    "number".to_string()
                }
            }
            Expr::String(_) => "string".to_string(),
            Expr::Bool(_) => "boolean".to_string(),
            // A METHOD field (`{ next() { this.count } }`): the lifted
            // fn carries the hidden `__this` receiver as its first
            // declared param (`objlit_nominal::apply_patches`,
            // recorded in `fnexpr_recv_fns`), so the canon below
            // would spell the receiver INTO the signature and every
            // `o.next()` through the promoted slot failed arity
            // ("expected 1 argument(s), got 0" — rotation 546). The
            // field ann is the receiver-less `__mth(` spelling the
            // nominal lane already mints — the field-call arm is what
            // pushes the receiver back.
            Expr::Closure { fn_name, .. } if ast.fnexpr_recv_fns.contains(fn_name) => {
                mth_field_ann(ast, fn_name)?
            }
            Expr::Closure { fn_name, .. } => {
                let canon = lifted_closure_fn_canon(ast, fn_name)?;
                // A struct FIELD holding a fn is Closure-repr, not the
                // direct-dispatch FnSig a `__fn(` spelling names — the
                // same retag the parser applies where it mints
                // `__inlobj(` from written syntax. Leaving it untagged
                // resolves the field to a bare code pointer and calling
                // it through an env-first indirect call is a SIGBUS.
                crate::ast::retag_field_fn_ann(&canon)
            }
            // A computed field, typed by the same shape inference the
            // binding level uses: `{ msg: "a" + b }` is as certainly
            // a string as the literal arms above, and without this
            // one such field kept the WHOLE binding main-local — so a
            // named fn reading ANY field of it got "unknown
            // identifier".
            //
            // The numeric shapes used to stay out here, on the worry
            // that a struct FIELD is a different `num_width` key from
            // the binding, so answering `number` could park f64 bits
            // in an i64 field. It cannot: `widen_struct_fields` asks
            // `field_is_f64(key, fname)` for every field of the
            // resolved layout, and `num_width`'s ObjectLit walk seeds
            // that key with `width_of` of the field's INITIALIZER —
            // any expression, not just a literal. The literal number
            // arm above has ridden that correction all along. Probe
            // `{ v: 1 / 2 }`: the shape answers I64 (width is never
            // decided by shape), the ann spells `number`, and the
            // widen re-interns the field as F64 — `0.5`, where a
            // missing correction would print `0`.
            //
            // A field NAMING a top-level closure binding (`{ f: ga }`)
            // holds the same value the Closure arm above spells, one
            // hop away, so it spells the same string —
            // `closure_alias_fn_canon` carries the uniqueness argument
            // and answers the source binding's own face. The shape
            // table never resolved such a name (it only answers
            // primitives), so one such field kept the WHOLE literal
            // off this lane; the binding then landed on the boxed
            // any-promote lane when the name was non-variadic and
            // nowhere at all when it was variadic ("unknown identifier
            // `o`"). That the typed lane serves this field is not a
            // new claim: writing the annotation out
            // (`const o: { f: (...xs: number[]) => number } = { f: ga }`)
            // already takes it, variadic tail included.
            Expr::Ident(_) => match closure_alias_fn_canon(ast, *val) {
                Some(canon) => crate::ast::retag_field_fn_ann(&canon),
                None => shape_field_ann(ast, *val)?,
            },
            // A field holding a CAST to a fn type
            // (`{ f: c as (...xs: number[]) => number }`). The
            // written face IS the string, for the reason an alias
            // answers its source's face (r601): what the field call
            // goes through is the face, and the checker's rest test
            // on it is what routes a rest tail to the boxed dual
            // entry. Nothing about the promotion trusts the cast —
            // the literal is still checked against the layout this
            // spells, exactly as it is when the same annotation is
            // written on the binding instead. A non-fn cast
            // (`x as number`) is a different claim and keeps falling
            // through: the shape table answers from runtime
            // certainty, and an assertion is not that.
            Expr::As { ty_ann, .. } if crate::ast::is_fn_like_ann(ty_ann) => {
                crate::ast::retag_field_fn_ann(ty_ann)
            }
            // A class method read as a value (`{ f: d.n }`) is the
            // prototype's reified cell, an Any — see the predicate.
            Expr::Member { .. } if super::class_method_value_read(ast, *val) => "any".to_string(),
            _ => shape_field_ann(ast, *val)?,
        };
        parts.push(format!("{fname}:{ann}"));
    }
    Some(format!("__inlobj({})", parts.join("|")))
}

/// The field spelling of a value the shape table answers for — the
/// fall-back every non-callable field takes. `None` keeps the whole
/// literal off this lane, which is what the callers read.
fn shape_field_ann(ast: &Ast, val: ExprId) -> Option<String> {
    Some(match infer_slot_shape(ast, val, 0, None)? {
        GlobalSlotShape::Str => "string".to_string(),
        GlobalSlotShape::Bool => "boolean".to_string(),
        GlobalSlotShape::I64 => "number".to_string(),
        GlobalSlotShape::F64 => "f64".to_string(),
        GlobalSlotShape::BigInt => "bigint".to_string(),
        // A regex field keeps the literal off this lane like a symbol
        // does — the field lane was never asked to hold one.
        GlobalSlotShape::Symbol | GlobalSlotShape::RegExp => return None,
    })
}

/// The receiver-less `__mth(P|..)->(R)` spelling of a METHOD's lifted
/// FnDecl — the same string `objlit_nominal::apply_patches`
/// publishes into `fn_sigs`, reassembled from the decl (this module
/// only holds `&Ast`). Mirrors that fn's filter exactly: `__env` and
/// `__this` stay out of the signature; a missing param ann spells
/// `any`; a missing return spells `void`. Variadic sigs answer
/// `None` for the same reason the `__fn(` canon keeps them out.
fn mth_field_ann(ast: &Ast, fn_name: &str) -> Option<String> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } if name == fn_name => {
            let anns: Vec<String> = params
                .iter()
                .filter(|q| q.name != "__env" && q.name != "__this")
                .map(|q| q.type_ann.clone().unwrap_or_else(|| "any".to_string()))
                .collect();
            if anns.iter().any(|a| a.contains("__rest(")) {
                return None;
            }
            let ret = return_type.clone().unwrap_or_else(|| "void".to_string());
            Some(crate::type_ann_fnsig::fn_type_ann(
                "__mth",
                &anns.join("|"),
                &ret,
            ))
        }
        _ => None,
    })
}

/// The class spelling of an un-annotated `new C(...)` init — the
/// synthesized annotation a top-level binding promotes under, in the
/// [`objlit_literal_inlobj_ann`] / `arrlit_literal_elem_ann` family.
///
/// `desugar_classes` has already rewritten `new C(args)` into a call
/// to the synthesized `__new_C` factory by the time either consumer
/// asks, so the spelling is the name that factory carries. Requiring
/// the factory to EXIST is what keeps this to classes the program
/// declares: a `new` whose target the compiler could not resolve to
/// a class never got one (`NewDynamic`), and so never answers here.
///
/// The nominal type is the whole point. `any_promote_init` refuses
/// class instances deliberately — boxing one away demotes every
/// main-side method call to the any-lane (rotation 238) — but that
/// refusal left the binding with no home at all, so a named fn
/// reading it died with "unknown identifier": `let e = new Error(…)`
/// plus any `function f() { … e … }` did not compile, while the same
/// program with `let e: Error` written out did. Under its own
/// spelling the binding rides exactly the lane the written
/// annotation already rides.
pub fn new_class_ann(ast: &Ast, init: ExprId) -> Option<String> {
    let Expr::Call { callee, .. } = ast.get_expr(init) else {
        return None;
    };
    let Expr::Ident(fname) = ast.get_expr(*callee) else {
        return None;
    };
    let class = fname.strip_prefix("__new_")?;
    crate::ast::toplevel_stmts_flat(ast)
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name.as_str() == fname.as_str()))
        .then(|| class.to_string())
}

/// The collection spelling of an un-annotated `new Map()` /
/// `new Set()` / `new WeakMap()` / `new WeakSet()` init (and of the
/// other builtins below) — the sibling of [`new_class_ann`] for the
/// builtins no factory claims.
///
/// `desugar_builtin_new` rewrites `new Date` / `new Array` /
/// `new RegExp` and friends into shapes the promote arms already
/// read, which is why a `const d = new Date(0)` has had a global
/// home all along. It does not rewrite the four collections, so they
/// reached neither the slot-shape table (which has no `Expr::New`
/// arm) nor the factory spelling above, and `const m = new Map()`
/// plus any `function f() { … m.get(k) … }` answered "unknown
/// identifier `m`" — while the identical program with
/// `const m: Map<string, number>` written out worked.
///
/// A user-declared `class Map` cannot reach here: `desugar_classes`
/// turns its `new` into a `__new_Map(...)` call, and
/// `route_non_class_new` has already run — an `Expr::New` still
/// holding a bare name is one nobody claimed. That is the same
/// argument [`new_class_ann`] makes from the other side.
///
/// The written type arguments are carried through when the program
/// gave any, in the parser's own canonical spelling — it joins
/// generic arguments with `|`, not `,` (`parser::type_ann`), so
/// `new Map<string, number>()` spells `Map<string|number>`. Joining
/// them the way the source reads left `Map<string, number>`
/// unresolvable and the binding main-local, while the single-argument
/// `Set<number>` promoted fine and hid it. An argument spelling that
/// still does not resolve leaves the binding main-local, the way
/// every sibling arm fails.
pub fn collection_new_ann(ast: &Ast, init: ExprId) -> Option<String> {
    let Expr::New {
        class_name,
        type_args,
        ..
    } = ast.get_expr(init)
    else {
        return None;
    };
    // A proxy impersonates its target, so the checker types `new
    // Proxy(…)` `any` and there is no `Proxy` instance type to spell.
    if class_name == "Proxy" {
        return Some("any".to_string());
    }
    // Each typed array (the §23.2 constructors plus Float16Array)
    // spells its own type, like the collections and buffers do: the
    // exact string `const a: Float64Array = …` writes, which is the
    // lane this function feeds.
    let spells_own_type = matches!(
        class_name.as_str(),
        "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "WeakRef"
            | "ArrayBuffer"
            | "DataView"
            | "FinalizationRegistry"
    ) || crate::ssa_lower_call_typedarray::kind_of_name(class_name).is_some();
    if !spells_own_type {
        return None;
    }
    Some(if type_args.is_empty() {
        class_name.clone()
    } else {
        format!("{class_name}<{}>", type_args.join("|"))
    })
}
