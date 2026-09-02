//! `desugar_classes` static-member rewrite table builder (chunk 177,
//! 2026-06-28) and the static-initializer emitter beside it.
//!
//! The two halves of a class's statics: this file's table answers
//! "what does `C.x` become", and `emit_static_inits` at the bottom
//! answers "where do `static x = …` and `static { … }` actually run".
//! The emitter moved here from `desugar_classes_pass3` (rotation 458)
//! when the return-override channel took that file past its size cap
//! — the statics are the one part of Pass 3 that already had a home.
//!
//! Extracted from `ast/desugar_classes.rs` (pre-extract was the
//! Pass 2.5 region between Pass 2 expr-arena rewrite and Pass 3
//! stmt-list rewrite). Pure function — consumes `class_index`, returns
//! a `HashMap<(ClassName, MemberName), ReplacementIdent>` consumed
//! later in Pass 3 to rewrite `Expr::Member { obj: Ident("ClassName"),
//! name }` into a plain `Expr::Ident(__sf_<C>__<n> | __sm_<C>__<m>)`.
//!
//! Two construction stages, original ordering preserved:
//!   1. Each class's own statics (StaticInit::Field + static method)
//!      register `__sf_<C>__<n>` / `__sm_<C>__<m>` keyed by
//!      `(C, name)`.
//!   2. V3-18 wedge — static inheritance per ES §15.7: walk every
//!      class's parent chain, alias inherited names to the parent's
//!      binding via `.entry().or_insert_with(...)` (sub's own statics
//!      already entered first, so they take precedence).
//!
//! Body verbatim from pre-extract; the shadowed-local `parent_map` at
//! the original construction site stays here (Pass 1 declared the
//! same name in outer scope at line 181; Pass 2.5 was already shadowing
//! it). No `&mut Ast` mutation — pure data computation.

use super::desugar_classes_super::ClassIndexEntry;
use super::*;
use std::collections::HashMap;

/// Per-(class, prop) static-accessor faces — `(getter fn, setter
/// fn)`, either absent for a one-sided pair (RFC
/// 20260718-accessor-reify 刀 3). Reads rewrite to a getter CALL,
/// writes to a setter call — never to a bare Ident.
pub(super) type StaticAccessorRewrites =
    HashMap<(String, PropKey), (Option<String>, Option<String>)>;

/// `(accessing class, member) → owning class`, for the static-method
/// entries the V3-18 inheritance walk aliased onto a PARENT's binding.
/// The value is what the flat rewrite loses: `Sub.make` collapses to
/// `Ident("__sm_Base__make")`, after which nothing downstream can tell
/// the call was written on `Sub`. §15.7.14 makes the receiver of a
/// `Sub.make()` call the `Sub` constructor object, so a body that reads
/// `this` needs that name carried to the twin (RFC 20260804 knife 3d).
pub(super) type InheritedStaticOwners = HashMap<(String, PropKey), String>;

pub(super) fn build_static_member_rewrites(
    class_index: &[ClassIndexEntry],
) -> (
    HashMap<(String, PropKey), String>,
    StaticAccessorRewrites,
    InheritedStaticOwners,
) {
    // M-OO.4 — collect static-member rewrite tables: keys are
    // `(ClassName, member_name)` → flat replacement ident
    // (`__sf_<C>__<n>` for fields, `__sm_<C>__<m>` for methods). After
    // emitting the desugared decls, a second walk over `ast.exprs`
    // rewrites every `Expr::Member { obj: Ident("ClassName"), name }`
    // whose key is in the table to a plain `Expr::Ident(replacement)`.
    let mut static_member_rewrites: HashMap<(String, PropKey), String> = HashMap::new();
    let mut accessor_rewrites: StaticAccessorRewrites = HashMap::new();
    let mut inherited_owners: InheritedStaticOwners = HashMap::new();
    for (_, cname, _, _, _, sis, _, _, sms) in class_index {
        // P8.3-A3 — only StaticInit::Field entries are addressable as
        // `ClassName.member`; static blocks have no member name and are
        // emitted as `__sb_<C>__<idx>` named fns called at top level,
        // so they do not contribute to the static-member rewrite table.
        for si in sis {
            if let StaticInit::Field(sf) = si {
                static_member_rewrites.insert(
                    (cname.clone(), sf.name.clone()),
                    format!("__sf_{cname}__{}", mangle_key(&sf.name)),
                );
            }
        }
        for sm in sms {
            match sm.accessor_kind {
                // RFC 20260718-accessor-reify 刀 3 — a static
                // accessor's faces desugar with `_get` / `_set`
                // suffixes (mirror of the instance emit); reads /
                // writes rewrite to face CALLS, so the data table
                // never sees the name.
                Some(AccessorKind::Getter) => {
                    accessor_rewrites
                        .entry((cname.clone(), sm.name.clone()))
                        .or_insert((None, None))
                        .0 = Some(format!("__sm_{cname}__{}_get", mangle_key(&sm.name)));
                }
                Some(AccessorKind::Setter) => {
                    accessor_rewrites
                        .entry((cname.clone(), sm.name.clone()))
                        .or_insert((None, None))
                        .1 = Some(format!("__sm_{cname}__{}_set", mangle_key(&sm.name)));
                }
                None => {
                    static_member_rewrites.insert(
                        (cname.clone(), sm.name.clone()),
                        format!("__sm_{cname}__{}", mangle_key(&sm.name)),
                    );
                }
            }
        }
    }
    // V3-18 wedge — static inheritance per ES spec §15.7. When
    // `class Sub extends Base { ... }`, `Sub.greet` should resolve
    // to `Base.greet` (and `Sub.count` to `Base.count`), unless Sub
    // overrides them with its own static. Pre-fix
    // `Sub.<inherited_static>` failed at typecheck with
    // 'unknown identifier `Sub`' because the rewrite table only
    // recorded each class's own statics.
    //
    // Walk every class's parent chain, alias inherited static names
    // to the parent's __sf_/__sm_ binding. Sub's own statics already
    // take precedence (entered above). Multi-level chains (Sub →
    // Mid → Base) work transitively because the loop visits the
    // chain in order.
    let parent_map: HashMap<String, Option<String>> = class_index
        .iter()
        .map(|(_, c, _, p, _, _, _, _, _)| (c.clone(), p.clone()))
        .collect();
    let mut class_static_index: HashMap<String, (Vec<PropKey>, Vec<PropKey>)> = HashMap::new();
    for (_, cname, _, _, _, sis, _, _, sms) in class_index {
        class_static_index.insert(
            cname.clone(),
            (
                // P8.3-A3 — same Field-only filter as the rewrite table:
                // static blocks have no member name to inherit.
                sis.iter()
                    .filter_map(|si| match si {
                        StaticInit::Field(sf) => Some(sf.name.clone()),
                        StaticInit::Block(_) => None,
                    })
                    .collect(),
                // Accessor names stay out of the inheritance alias
                // walk (an inherited static accessor resolves through
                // the class-object proto chain — recorded boundary).
                sms.iter()
                    .filter(|sm| sm.accessor_kind.is_none())
                    .map(|sm| sm.name.clone())
                    .collect(),
            ),
        );
    }
    for (_, cname, _, parent, _, _, _, _, _) in class_index {
        let mut cur = parent.clone();
        // Cycle guard — same rationale as the collect_abstract_classes
        // walk: a mutual-extends cycle must not spin here (the checker
        // rejects it loudly right after this pass).
        let mut seen: Vec<String> = Vec::new();
        while let Some(p) = cur {
            if seen.contains(&p) {
                break;
            }
            seen.push(p.clone());
            if let Some((p_sfs, p_sms)) = class_static_index.get(&p) {
                for sf_name in p_sfs {
                    let key = (cname.clone(), sf_name.clone());
                    static_member_rewrites
                        .entry(key)
                        .or_insert_with(|| format!("__sf_{p}__{}", mangle_key(sf_name)));
                }
                for sm_name in p_sms {
                    let key = (cname.clone(), sm_name.clone());
                    // `or_insert_with` returning the fresh value means
                    // the alias really landed here (the sub does not
                    // shadow it, and no nearer ancestor already claimed
                    // it) — the one place the owner is still known.
                    let mut aliased = false;
                    static_member_rewrites
                        .entry(key.clone())
                        .or_insert_with(|| {
                            aliased = true;
                            format!("__sm_{p}__{}", mangle_key(sm_name))
                        });
                    if aliased {
                        inherited_owners.insert(key, p.clone());
                    }
                }
            }
            cur = parent_map.get(&p).cloned().flatten();
        }
    }
    (static_member_rewrites, accessor_rewrites, inherited_owners)
}

/// M-OO.4 — emit `let __sf_<C>__<name>: T = init;` for each static
/// field (const-form, mutable=false, so K.4 refcount globals accept
/// it; the `init` ExprId is reused — desugar runs before any pass
/// that might mutate the expression referenced by it).
///
/// P8.3-A3 — `static { ... }` blocks share this walk with static
/// fields so spec §15.7.10 source-order interleaving is preserved.
/// Each Block desugars to a named-fn `__sb_<C>__<idx>` (mirrors
/// `__sm_<C>__<m>`, no `__this` param, void return) appended to
/// `ast.stmts`, plus a top-level `Stmt::Expr(Call(...))` at the
/// block's source-order position.
///
/// Static field LetDecls + static block Calls go into `own_statics`
/// (NOT `appended`), which the caller splices at the class's OWN
/// statement position — where §15.7.14 runs them, right after the
/// class's element names are evaluated.
///
/// 420-02 — they used to be prepended to the head of `ast.stmts`
/// instead, "so they init before the user's top-level code runs".
/// That answered one ordering question by asking a worse one: a
/// static initializer calling a named function then ran before every
/// `var` at the top level was initialized, so the function read
/// whatever the slot held before its own declaration. For a scalar
/// that is a silent wrong answer (`var n = 0; function bump(){ n =
/// n + 1 } class C { static s = bump() }` left `n` at 0, because the
/// `var n = 0` ran afterwards and overwrote the bump). For a heap
/// global it is a SEGV — `var log: string[] = []` had not allocated
/// yet, so `log.push(...)` dereferenced the slot's zero.
///
/// The shape the prepend was defending — a top-level `check()` call
/// ABOVE the class, reading `Counter.label` from inside — is the
/// spec's own TDZ: a class binding is not initialized until its
/// declaration is reached, and reaching through it early is a
/// ReferenceError, not a read of the eventual value.
pub(super) fn emit_static_inits(
    ast: &mut Ast,
    cname: &str,
    type_params: &[String],
    static_init: &[StaticInit],
    appended: &mut Vec<Stmt>,
    own_statics: &mut Vec<Stmt>,
) {
    for (block_idx, si) in static_init.iter().enumerate() {
        match si {
            StaticInit::Field(sf) => {
                // V3-18 m1.h.26 — static fields are mutable by
                // default (`Counter.value = 5` is valid TS). The
                // historical carve-out that kept refcount-typed
                // fields `mutable: false` (the K.6 globals path once
                // had no dec-old/inc-new for mutable refcount slots,
                // so ssa_lower skipped them and reads failed) is
                // retired — rotation 346: top-level `var g: any` /
                // `var s: string` reassignment has ridden K.6 for a
                // long while, and the false spelling made every
                // WRITE to an uninitialized `static x;` slot (typed
                // "any", init undefined) an unknown-ident reject
                // (the 68-case rs-static-privatename family).
                own_statics.push(Stmt::LetDecl {
                    mutable: true,
                    name: format!("__sf_{cname}__{}", mangle_key(&sf.name)),
                    type_ann: Some(sf.type_ann.clone()),
                    init: sf.init,
                    is_var: false,
                });
                // L3b static-field-reflect (2026-07-22) — mirror the
                // static-METHOD reify: the class object gets a real
                // own data entry so gOPD / any-lane member reads
                // answer (`verifyProperty(C, "<f>", ...)`). Emitted
                // right after the slot's LetDecl (value ready), and
                // the class registration ran earlier (class_globals
                // stmts prepend runs ahead of any class patch). The
                // value rides as the `__sf_` Ident so lowering reuses
                // the plain global-read path.
                let cname_str = ast.add_expr(Expr::String(cname.to_string().into()));
                let fname_str = ast.add_expr(Expr::String(sf.name.clone().into()));
                let val_ident = ast.add_expr(Expr::Ident(format!(
                    "__sf_{cname}__{}",
                    mangle_key(&sf.name)
                )));
                let callee = ast.add_expr(Expr::Ident("__torajs_static_field_reify".to_string()));
                let call = ast.add_expr(Expr::Call {
                    callee,
                    args: vec![cname_str, fname_str, val_ident],
                });
                own_statics.push(Stmt::Expr(call));
            }
            StaticInit::Block(stmts) => {
                let fn_name = format!("__sb_{cname}__{block_idx}");
                appended.push(Stmt::FnDecl {
                    name: fn_name.clone(),
                    type_params: type_params.to_vec(),
                    params: Vec::new(),
                    return_type: Some("void".into()),
                    body: stmts.clone(),
                    is_generator: false,
                    span: crate::lexer::Span { start: 0, end: 0 },
                });
                let callee_id = ast.add_expr(Expr::Ident(fn_name));
                let call_id = ast.add_expr(Expr::Call {
                    callee: callee_id,
                    args: Vec::new(),
                });
                own_statics.push(Stmt::Expr(call_id));
            }
        }
    }
}
